//! "Get it at your library": pick a library system, then link a book to that
//! library's catalog. Not to be confused with `library.rs`, which is a user's
//! own bookshelves.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::catalogs::{self, BookQuery, Catalogs, LibrarySystem, LibrarySystemSummary};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Cap on ISBNs considered per lookup (a book can accumulate many editions).
const MAX_ISBNS: usize = 8;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library-systems", get(list_systems))
        .route("/me/library-system", get(get_my_system).put(set_my_system))
        .route("/books/{id}/library-link", get(book_library_link))
}

async fn list_systems(_caller: CurrentUser) -> Json<Vec<LibrarySystemSummary>> {
    Json(catalogs::SYSTEMS.iter().map(Into::into).collect())
}

#[derive(Debug, Serialize)]
struct MySystem {
    library_system: Option<LibrarySystemSummary>,
}

/// The stored id → registry entry. An id that's since been removed from the
/// registry reads as "not chosen" rather than an error.
async fn my_system(pool: &PgPool, user_id: Uuid) -> ApiResult<Option<&'static LibrarySystem>> {
    let id =
        sqlx::query_scalar::<_, Option<String>>("select library_system from users where id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(id.as_deref().and_then(catalogs::system))
}

async fn get_my_system(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<MySystem>> {
    let system = my_system(&pool, me.id).await?;
    Ok(Json(MySystem {
        library_system: system.map(Into::into),
    }))
}

#[derive(Debug, Deserialize)]
struct SetSystem {
    /// A `GET /library-systems` id, or null to clear.
    library_system: Option<String>,
}

async fn set_my_system(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<SetSystem>,
) -> ApiResult<Json<MySystem>> {
    let system = match input.library_system.as_deref() {
        None => None,
        Some(id) => Some(
            catalogs::system(id)
                .ok_or_else(|| ApiError::BadRequest(format!("unknown library system '{id}'")))?,
        ),
    };

    sqlx::query("update users set library_system = $2 where id = $1")
        .bind(me.id)
        .bind(system.map(|s| s.id))
        .execute(&pool)
        .await?;

    Ok(Json(MySystem {
        library_system: system.map(Into::into),
    }))
}

#[derive(Debug, Serialize)]
struct LibraryLink {
    library: LibrarySystemSummary,
    /// True when the catalog has this exact edition (by ISBN) and `url` is its
    /// record page. False → `url` is a catalog search for the title instead.
    found: bool,
    /// True when the catalog couldn't be reached: `found: false` then means
    /// "unknown", not "they don't have it".
    lookup_failed: bool,
    url: String,
}

/// A link to this book in the caller's chosen library: the record page when
/// the catalog has this work (matched by title + author, preferring one of the
/// book's ISBNs), otherwise a title search. Always
/// answers 200 with a usable `url` — the catalog being down/slow shouldn't
/// break the book page.
async fn book_library_link(
    State(pool): State<PgPool>,
    State(catalogs): State<Arc<Catalogs>>,
    CurrentUser(me): CurrentUser,
    Path(book_id): Path<Uuid>,
) -> ApiResult<Json<LibraryLink>> {
    let system = my_system(&pool, me.id)
        .await?
        .ok_or_else(|| ApiError::BadRequest("choose a library first".into()))?;

    let (title, author) = sqlx::query_as::<_, (String, Option<String>)>(
        "select canonical_title, primary_author from books where id = $1",
    )
    .bind(book_id)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    // Editions on file: ISBN-13s first (what catalogs mostly index), then
    // ISBN-10s. Used to prefer the exact edition, not to identify the book.
    let editions = sqlx::query_as::<_, (Option<String>, Option<String>, String, String)>(
        "select isbn_13, isbn_10, language, title from book_editions where book_id = $1 order by created_at",
    )
    .bind(book_id)
    .fetch_all(&pool)
    .await?;
    let mut isbns: Vec<String> = Vec::new();
    for isbn in editions
        .iter()
        .filter_map(|(a, ..)| a.as_deref())
        .chain(editions.iter().filter_map(|(_, b, ..)| b.as_deref()))
    {
        let isbn = catalogs::normalize_isbn(isbn);
        if !isbn.is_empty() && !isbns.contains(&isbn) {
            isbns.push(isbn);
        }
    }
    isbns.truncate(MAX_ISBNS);

    // Edition titles that differ from the work's (translations) — the catalog
    // may list the book under one of those instead.
    let alt_titles: Vec<String> = editions.iter().map(|(.., t)| t.clone()).collect();

    let query = BookQuery {
        title: &title,
        alt_titles: &alt_titles,
        author: author.as_deref(),
        language: editions.first().map(|(_, _, l, _)| l.as_str()),
        isbns: &isbns,
    };

    let mut lookup_failed = false;
    match catalogs.find_book(system, &query).await {
        Ok(Some(found)) => {
            return Ok(Json(LibraryLink {
                library: system.into(),
                found: true,
                lookup_failed: false,
                url: found.url,
            }));
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("library lookup ({}, {title:?}) failed: {e}", system.id);
            lookup_failed = true;
        }
    }

    Ok(Json(LibraryLink {
        library: system.into(),
        found: false,
        lookup_failed,
        url: catalogs::search_url(system, &title, author.as_deref()),
    }))
}
