use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{ReadingStatus, User};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/users/{user_id}/library", get(user_library))
}

#[derive(Debug, Deserialize)]
pub struct LibraryParams {
    /// `reading` (currently_reading), `read` (finished or did_not_finish), or
    /// `want_to_read`. Omit for the whole library.
    pub shelf: Option<String>,
}

/// SQL predicate over `bs.status` for a shelf name. Static strings only — no
/// user input reaches the query text.
fn shelf_filter(shelf: &str) -> Option<&'static str> {
    match shelf {
        "reading" => Some("bs.status = 'currently_reading'"),
        "read" => Some("bs.status in ('finished', 'did_not_finish')"),
        "want_to_read" => Some("bs.status = 'want_to_read'"),
        "did_not_finish" => Some("bs.status = 'did_not_finish'"),
        "all" => Some("true"),
        _ => None,
    }
}

/// You can read your own library, or a friend's if they opted in
/// (`users.share_shelves`). Non-friends and unknown ids get the same 403 so
/// this doesn't reveal who exists.
async fn ensure_can_view_library(pool: &PgPool, me: &User, owner_id: Uuid) -> ApiResult<()> {
    if me.id == owner_id {
        return Ok(());
    }
    let (low, high) = if me.id < owner_id {
        (me.id, owner_id)
    } else {
        (owner_id, me.id)
    };
    let shared = sqlx::query_scalar::<_, bool>(
        "select u.share_shelves from friendships f \
         join users u on u.id = $3 \
         where f.user_a_id = $1 and f.user_b_id = $2",
    )
    .bind(low)
    .bind(high)
    .bind(owner_id)
    .fetch_optional(pool)
    .await?;

    match shared {
        Some(true) => Ok(()),
        Some(false) => Err(ApiError::Forbidden(
            "this friend hasn't shared their shelves".into(),
        )),
        None => Err(ApiError::Forbidden(
            "you can only view your own or a friend's shelves".into(),
        )),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LibraryRow {
    status: ReadingStatus,
    rating: Option<i16>,
    progress_percent: Option<i16>,
    updated_at: DateTime<Utc>,
    book_id: Uuid,
    book_title: String,
    book_author: Option<String>,
    book_cover_image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LibraryBook {
    pub id: Uuid,
    pub title: String,
    pub author: Option<String>,
    pub cover_image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LibraryEntry {
    pub status: ReadingStatus,
    pub rating: Option<i16>,
    pub progress_percent: Option<i16>,
    pub updated_at: DateTime<Utc>,
    pub book: LibraryBook,
}

/// A user's shelves: always your own; a friend's only if they've turned on
/// `share_shelves`.
///
/// `?shelf=reading` is the in-progress view; `?shelf=read`
/// is everything they've finished or abandoned (with ratings). To add a book
/// they read off-platform, resolve it first (`/books/search` + `/books/resolve`,
/// or a manual `/books/resolve` body) then `PUT /book-statuses` with
/// `status: "finished"` and an optional `rating`.
async fn user_library(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(user_id): Path<Uuid>,
    Query(params): Query<LibraryParams>,
) -> ApiResult<Json<Vec<LibraryEntry>>> {
    ensure_can_view_library(&pool, &me, user_id).await?;

    let filter = match params.shelf.as_deref() {
        None => "true",
        Some(name) => shelf_filter(name).ok_or_else(|| {
            ApiError::BadRequest(
                "shelf must be one of: reading, read, want_to_read, did_not_finish, all".into(),
            )
        })?,
    };

    let sql = format!(
        r#"
        select
            bs.status              as status,
            bs.rating              as rating,
            bs.progress_percent    as progress_percent,
            bs.updated_at          as updated_at,
            b.id                   as book_id,
            b.canonical_title      as book_title,
            b.primary_author       as book_author,
            b.cover_image_url      as book_cover_image_url
        from book_statuses bs
        join books b on b.id = bs.book_id
        where bs.user_id = $1 and ({filter})
        order by bs.updated_at desc
        "#
    );

    // `sql` is dynamic (the `where` clause varies by shelf), but every piece
    // that goes into it comes from `shelf_filter`'s fixed, static match arms
    // above — never straight from `params.shelf` — so this is safe despite
    // not being a `&'static str` itself. See `sqlx::AssertSqlSafe`'s docs.
    let rows = sqlx::query_as::<_, LibraryRow>(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(&pool)
        .await?;

    let entries = rows
        .into_iter()
        .map(|r| LibraryEntry {
            status: r.status,
            rating: r.rating,
            progress_percent: r.progress_percent,
            updated_at: r.updated_at,
            book: LibraryBook {
                id: r.book_id,
                title: r.book_title,
                author: r.book_author,
                cover_image_url: r.book_cover_image_url,
            },
        })
        .collect();

    Ok(Json(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_documented_shelf_name() {
        for shelf in ["reading", "read", "want_to_read", "did_not_finish", "all"] {
            assert!(
                shelf_filter(shelf).is_some(),
                "{shelf:?} should be a recognized shelf"
            );
        }
    }

    #[test]
    fn rejects_unknown_shelf_names() {
        assert!(
            shelf_filter("finished").is_none(),
            "not the actual enum spelling"
        );
        assert!(shelf_filter("").is_none());
        assert!(shelf_filter("Reading").is_none(), "case-sensitive");
    }
}
