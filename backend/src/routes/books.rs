use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{Book, BookEdition, ResolvedBook};
use crate::providers::{BookProviders, BookSearchResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/books/search", get(search_books))
        .route("/books/resolve", post(resolve_book))
        .route("/books/{id}", get(get_book))
}

// ---------- search ----------

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// Free-text query (title, author, ISBN, …).
    pub q: String,
    /// Max results, 1..=40, default 20.
    pub limit: Option<usize>,
}

/// Search external book catalogs (Open Library, plus Google Books when
/// `GOOGLE_BOOKS_API_KEY` is set). Results are work-level and NOT persisted —
/// POST a chosen result's `{source, source_id}` to `/books/resolve` to save it.
async fn search_books(
    State(providers): State<Arc<BookProviders>>,
    _caller: CurrentUser,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<Vec<BookSearchResult>>> {
    let query = params.q.trim();
    if query.is_empty() {
        return Err(ApiError::BadRequest(
            "query parameter 'q' is required".into(),
        ));
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 40);
    let results = providers.search(query, limit).await?;
    Ok(Json(results))
}

// ---------- resolve ----------

#[derive(Debug, Serialize)]
pub struct BookWithEdition {
    #[serde(flatten)]
    pub book: Book,
    pub edition: BookEdition,
}

/// A `/books/resolve` body is either a fully-normalized book (from a caller
/// that already did its own normalization) or a bare provider reference that
/// the backend fetches and normalizes itself. `Normalized` is tried first, so
/// a `{source, source_id}`-only body — which can't satisfy `ResolvedBook`'s
/// required fields — falls through to `Reference`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ResolveRequest {
    Normalized(Box<ResolvedBook>),
    Reference(ResolveRef),
}

#[derive(Debug, Deserialize)]
pub struct ResolveRef {
    pub source: String,
    pub source_id: String,
}

/// Upserts a book from either a normalized payload or a provider reference.
/// Matching strategy (unchanged):
/// 1. If an edition with this (source, source_id) already exists, return its book.
/// 2. Else if the book already exists by open_library_work_id / google_books_volume_id,
///    attach a new edition to it (this is how translations/editions merge into one work).
/// 3. Else create a new book + edition.
async fn resolve_book(
    State(pool): State<PgPool>,
    State(providers): State<Arc<BookProviders>>,
    _caller: CurrentUser,
    Json(request): Json<ResolveRequest>,
) -> ApiResult<Json<BookWithEdition>> {
    let input = match request {
        ResolveRequest::Normalized(book) => *book,
        ResolveRequest::Reference(r) => providers.resolve(&r.source, &r.source_id).await?,
    };

    upsert_resolved_book(&pool, input).await.map(Json)
}

async fn upsert_resolved_book(pool: &PgPool, input: ResolvedBook) -> ApiResult<BookWithEdition> {
    let mut tx = pool.begin().await?;

    if let Some(existing) = sqlx::query_as::<_, BookEdition>(
        "select * from book_editions where source = $1 and source_id = $2",
    )
    .bind(&input.source)
    .bind(&input.source_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        let book =
            fill_description(&mut tx, existing.book_id, input.description.as_deref()).await?;
        tx.commit().await?;
        return Ok(BookWithEdition {
            book,
            edition: existing,
        });
    }

    let existing_book = match (&input.open_library_work_id, &input.google_books_volume_id) {
        (Some(olid), _) => {
            sqlx::query_as::<_, Book>("select * from books where open_library_work_id = $1")
                .bind(olid)
                .fetch_optional(&mut *tx)
                .await?
        }
        (None, Some(gbid)) => {
            sqlx::query_as::<_, Book>("select * from books where google_books_volume_id = $1")
                .bind(gbid)
                .fetch_optional(&mut *tx)
                .await?
        }
        (None, None) => None,
    };

    let book = match existing_book {
        Some(b) => fill_description(&mut tx, b.id, input.description.as_deref()).await?,
        None => {
            sqlx::query_as::<_, Book>(
                r#"
                insert into books (canonical_title, primary_author, open_library_work_id, google_books_volume_id, cover_image_url, description, description_checked_at)
                values ($1, $2, $3, $4, $5, $6, case when $6 is not null then now() end)
                returning *
                "#,
            )
            .bind(&input.canonical_title)
            .bind(&input.primary_author)
            .bind(&input.open_library_work_id)
            .bind(&input.google_books_volume_id)
            .bind(&input.cover_image_url)
            .bind(&input.description)
            .fetch_one(&mut *tx)
            .await?
        }
    };

    let edition = sqlx::query_as::<_, BookEdition>(
        r#"
        insert into book_editions (book_id, language, isbn_13, isbn_10, title, publisher, cover_image_url, source, source_id)
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        returning *
        "#,
    )
    .bind(book.id)
    .bind(&input.language)
    .bind(&input.isbn_13)
    .bind(&input.isbn_10)
    .bind(&input.edition_title)
    .bind(&input.publisher)
    .bind(&input.cover_image_url)
    .bind(&input.source)
    .bind(&input.source_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(BookWithEdition { book, edition })
}

/// Re-resolving a book we already have: keep the row, but take this chance to
/// fill in a description it doesn't have yet (never overwrites one).
async fn fill_description(
    tx: &mut sqlx::PgConnection,
    book_id: Uuid,
    description: Option<&str>,
) -> ApiResult<Book> {
    Ok(sqlx::query_as::<_, Book>(
        "update books set \
             description = coalesce(description, $2), \
             description_checked_at = case when description is null and $2 is not null \
                                           then now() else description_checked_at end \
         where id = $1 returning *",
    )
    .bind(book_id)
    .bind(description)
    .fetch_one(tx)
    .await?)
}

/// How long to wait on a provider while serving a book page, and how long a
/// "no description found" answer stays valid before we look again.
const BACKFILL_TIMEOUT: Duration = Duration::from_secs(4);
const RECHECK_AFTER_DAYS: i64 = 30;

async fn get_book(
    State(pool): State<PgPool>,
    State(providers): State<Arc<BookProviders>>,
    _caller: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Book>> {
    let mut book = sqlx::query_as::<_, Book>("select * from books where id = $1")
        .bind(id)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Books saved before we kept descriptions: look one up once, on first view.
    // Best-effort — a slow/failed provider must never break the book page, and
    // a failure isn't recorded, so the next view tries again.
    let has_source = book.open_library_work_id.is_some() || book.google_books_volume_id.is_some();
    if book.description.is_none() && has_source {
        let checked_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            "select description_checked_at from books where id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await?;
        let stale =
            checked_at.is_none_or(|t| Utc::now() - t > chrono::Duration::days(RECHECK_AFTER_DAYS));
        if stale {
            let lookup = tokio::time::timeout(
                BACKFILL_TIMEOUT,
                providers.description(
                    book.open_library_work_id.as_deref(),
                    book.google_books_volume_id.as_deref(),
                ),
            )
            .await;
            match lookup {
                Ok(Ok(found)) => {
                    sqlx::query(
                        "update books set description = $2, description_checked_at = now() where id = $1",
                    )
                    .bind(id)
                    .bind(&found)
                    .execute(&pool)
                    .await?;
                    book.description = found;
                }
                Ok(Err(e)) => tracing::warn!("description lookup for book {id} failed: {e}"),
                Err(_) => tracing::warn!("description lookup for book {id} timed out"),
            }
        }
    }

    Ok(Json(book))
}
