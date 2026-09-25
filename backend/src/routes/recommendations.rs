use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::friendships::friend_user_id;
use crate::auth::{ensure_self, CurrentUser};
use crate::error::ApiResult;
use crate::models::CreateRecommendation;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", post(create_recommendation))
        .route(
            "/users/{user_id}/recommendations/inbox",
            get(inbox_for_user),
        )
}

#[derive(Debug, Serialize)]
struct RecommendationSent {
    id: Uuid,
    book_id: Uuid,
    note: Option<String>,
    created_at: DateTime<Utc>,
    to_friendship_id: Uuid,
}

/// The sender is the authenticated caller; the recipient is one of their friends,
/// named by friendship (so it's also impossible to recommend to a non-friend).
async fn create_recommendation(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<CreateRecommendation>,
) -> ApiResult<Json<RecommendationSent>> {
    let to_user_id = friend_user_id(&pool, me.id, input.to_friendship_id).await?;

    let (id, created_at) = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
        "insert into recommendations (from_user_id, to_user_id, book_id, note) \
         values ($1, $2, $3, $4) returning id, created_at",
    )
    .bind(me.id)
    .bind(to_user_id)
    .bind(input.book_id)
    .bind(&input.note)
    .fetch_one(&pool)
    .await?;

    Ok(Json(RecommendationSent {
        id,
        book_id: input.book_id,
        note: input.note,
        created_at,
        to_friendship_id: input.to_friendship_id,
    }))
}

#[derive(Debug, sqlx::FromRow)]
struct InboxRow {
    id: Uuid,
    book_id: Uuid,
    note: Option<String>,
    created_at: DateTime<Utc>,
    from_friendship_id: Option<Uuid>,
    from_handle: String,
    from_display_name: String,
    from_avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct Sender {
    /// The friendship with the sender; never their user id.
    friendship_id: Option<Uuid>,
    handle: String,
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct InboxItem {
    id: Uuid,
    book_id: Uuid,
    note: Option<String>,
    created_at: DateTime<Utc>,
    from: Sender,
}

/// "X recommended a book to you" feed — this is the core loop of the app.
async fn inbox_for_user(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<Vec<InboxItem>>> {
    ensure_self(&me, user_id)?;

    let rows = sqlx::query_as::<_, InboxRow>(
        "select r.id, r.book_id, r.note, r.created_at, \
                f.id as from_friendship_id, u.handle as from_handle, \
                u.display_name as from_display_name, u.avatar_url as from_avatar_url \
         from recommendations r \
         join users u on u.id = r.from_user_id \
         left join friendships f \
             on (f.user_a_id = r.from_user_id and f.user_b_id = r.to_user_id) \
             or (f.user_b_id = r.from_user_id and f.user_a_id = r.to_user_id) \
         where r.to_user_id = $1 \
         order by r.created_at desc",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| InboxItem {
            id: r.id,
            book_id: r.book_id,
            note: r.note,
            created_at: r.created_at,
            from: Sender {
                friendship_id: r.from_friendship_id,
                handle: r.from_handle,
                display_name: r.from_display_name,
                avatar_url: r.from_avatar_url,
            },
        })
        .collect();

    Ok(Json(items))
}
