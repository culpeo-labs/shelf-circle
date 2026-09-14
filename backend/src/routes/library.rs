use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{ensure_self, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::models::ReadingStatus;
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

/// A user's own shelves. `?shelf=reading` is the in-progress view; `?shelf=read`
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
    ensure_self(&me, user_id)?;

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
