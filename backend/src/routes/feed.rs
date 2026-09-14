use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{ensure_self, CurrentUser};
use crate::error::ApiResult;
use crate::models::ReadingStatus;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/users/{user_id}/feed", get(user_feed))
}

#[derive(Debug, Deserialize)]
pub struct FeedParams {
    /// Page size, 1..=100, default 50.
    pub limit: Option<i64>,
    /// Keyset cursor: return only events strictly older than this timestamp
    /// (RFC 3339). Pass the `created_at` of the last item from the previous page.
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct FeedRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    status: ReadingStatus,
    actor_id: Uuid,
    actor_handle: String,
    actor_display_name: String,
    actor_avatar_url: Option<String>,
    book_id: Uuid,
    book_title: String,
    book_author: Option<String>,
    book_cover_image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedActor {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedBook {
    pub id: Uuid,
    pub title: String,
    pub author: Option<String>,
    pub cover_image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub status: ReadingStatus,
    /// Human-friendly phrasing for the timeline, e.g. "started reading".
    pub verb: &'static str,
    pub actor: FeedActor,
    pub book: FeedBook,
}

fn verb_for(status: ReadingStatus) -> &'static str {
    match status {
        ReadingStatus::WantToRead => "wants to read",
        ReadingStatus::CurrentlyReading => "started reading",
        ReadingStatus::Finished => "finished",
        ReadingStatus::DidNotFinish => "did not finish",
    }
}

/// The friend timeline: reading-status activity from the user's friends plus
/// their own, newest first. Keyset-paginated via `?before=<rfc3339>&limit=`.
async fn user_feed(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(user_id): Path<Uuid>,
    Query(params): Query<FeedParams>,
) -> ApiResult<Json<Vec<FeedItem>>> {
    ensure_self(&me, user_id)?;

    let limit = params.limit.unwrap_or(50).clamp(1, 100);

    let rows = sqlx::query_as::<_, FeedRow>(
        r#"
        select
            ae.id                  as id,
            ae.created_at          as created_at,
            ae.status              as status,
            u.id                   as actor_id,
            u.handle               as actor_handle,
            u.display_name         as actor_display_name,
            u.avatar_url           as actor_avatar_url,
            b.id                   as book_id,
            b.canonical_title      as book_title,
            b.primary_author       as book_author,
            b.cover_image_url      as book_cover_image_url
        from activity_events ae
        join users u on u.id = ae.actor_user_id
        join books b on b.id = ae.book_id
        where (
                ae.actor_user_id = $1
                or ae.actor_user_id in (
                    select case when f.user_a_id = $1 then f.user_b_id else f.user_a_id end
                    from friendships f
                    where f.user_a_id = $1 or f.user_b_id = $1
                )
            )
            and ($2::timestamptz is null or ae.created_at < $2)
        order by ae.created_at desc, ae.id desc
        limit $3
        "#,
    )
    .bind(user_id)
    .bind(params.before)
    .bind(limit)
    .fetch_all(&pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| FeedItem {
            id: r.id,
            created_at: r.created_at,
            status: r.status,
            verb: verb_for(r.status),
            actor: FeedActor {
                id: r.actor_id,
                handle: r.actor_handle,
                display_name: r.actor_display_name,
                avatar_url: r.actor_avatar_url,
            },
            book: FeedBook {
                id: r.book_id,
                title: r.book_title,
                author: r.book_author,
                cover_image_url: r.book_cover_image_url,
            },
        })
        .collect();

    Ok(Json(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reading_status_has_distinct_feed_phrasing() {
        let statuses = [
            ReadingStatus::WantToRead,
            ReadingStatus::CurrentlyReading,
            ReadingStatus::Finished,
            ReadingStatus::DidNotFinish,
        ];
        let verbs: Vec<&str> = statuses.iter().copied().map(verb_for).collect();

        assert_eq!(verbs[0], "wants to read");
        assert_eq!(verbs[1], "started reading");
        assert_eq!(verbs[2], "finished");
        assert_eq!(verbs[3], "did not finish");

        let unique: std::collections::HashSet<_> = verbs.iter().collect();
        assert_eq!(unique.len(), verbs.len(), "no two statuses share phrasing");
    }
}
