use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{CreateFriendship, Friendship, User};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/friendships", post(create_friendship))
}

/// Friendship rows are canonicalized so user_a_id < user_b_id (see migration).
/// The caller is one side (from the auth token); the body carries the other
/// person's handle. Idempotent.
async fn create_friendship(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<CreateFriendship>,
) -> ApiResult<Json<Friendship>> {
    let other = fetch_user_by_handle(&pool, &input.user_handle).await?;

    if other.id == me.id {
        return Err(ApiError::BadRequest("cannot friend yourself".into()));
    }

    upsert_friendship(&pool, me.id, other.id).await
}

/// Shared by `create_friendship` and `invites::accept_invite` (per friends.md:
/// "reuse the existing canonicalized friendship insert logic"). Generic over
/// the executor so a caller that needs this inside a transaction (accept_invite
/// does, to close a check-then-act race on the invite token) can pass `&mut
/// *tx` instead of a bare pool.
pub async fn upsert_friendship(
    executor: impl sqlx::PgExecutor<'_>,
    a: Uuid,
    b: Uuid,
) -> ApiResult<Json<Friendship>> {
    let (low, high) = if a < b { (a, b) } else { (b, a) };

    let friendship = sqlx::query_as::<_, Friendship>(
        r#"
        insert into friendships (user_a_id, user_b_id)
        values ($1, $2)
        on conflict (user_a_id, user_b_id) do update set user_a_id = excluded.user_a_id
        returning id, user_a_id, user_b_id, created_at
        "#,
    )
    .bind(low)
    .bind(high)
    .fetch_one(executor)
    .await?;

    Ok(Json(friendship))
}

async fn fetch_user_by_handle(pool: &PgPool, handle: &str) -> ApiResult<User> {
    sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, created_at from users where handle = $1",
    )
    .bind(handle)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::BadRequest(format!("no user with handle '{handle}'")))
}
