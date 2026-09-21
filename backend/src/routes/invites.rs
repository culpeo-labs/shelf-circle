use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{Friendship, Invite, InvitePreview, User};
use crate::state::AppState;

/// How long an invite token stays redeemable. Not single-use of the whole
/// feature — a user can have several outstanding invites (e.g. one generated
/// fresh per friend they're adding) — but each individual token is single-use
/// (see `accept_invite`).
const INVITE_TTL: Duration = Duration::days(7);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/invites", post(create_invite))
        .route("/invites/{token}", get(get_invite))
        .route("/invites/{token}/accept", post(accept_invite))
}

async fn create_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<Invite>> {
    let token = Uuid::new_v4().simple().to_string();
    let expires_at = Utc::now() + INVITE_TTL;

    sqlx::query(
        "insert into invite_tokens (token, created_by_user_id, expires_at) values ($1, $2, $3)",
    )
    .bind(&token)
    .bind(me.id)
    .bind(expires_at)
    .execute(&pool)
    .await?;

    Ok(Json(Invite { token, expires_at }))
}

/// Public — no `CurrentUser`/`AuthClaims` extractor, so no token is required.
async fn get_invite(
    State(pool): State<PgPool>,
    Path(token): Path<String>,
) -> ApiResult<Json<InvitePreview>> {
    let preview = sqlx::query_as::<_, InvitePreview>(
        "select u.display_name, u.avatar_url from invite_tokens it \
         join users u on u.id = it.created_by_user_id \
         where it.token = $1 and it.used_at is null and it.expires_at > now()",
    )
    .bind(&token)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(preview))
}

/// Creates the (canonicalized) friendship and marks the token used. A token
/// already used, expired, or unknown is a 400, same as accepting your own
/// invite — both are just "this invite doesn't work", not a 404/403
/// distinction worth exposing.
async fn accept_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<Friendship>> {
    let inviter = sqlx::query_as::<_, User>(
        "select u.id, u.handle, u.display_name, u.avatar_url, u.locale, u.created_at \
         from invite_tokens it join users u on u.id = it.created_by_user_id \
         where it.token = $1 and it.used_at is null and it.expires_at > now()",
    )
    .bind(&token)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::BadRequest("invite is invalid, expired, or already used".into()))?;

    if inviter.id == me.id {
        return Err(ApiError::BadRequest("cannot accept your own invite".into()));
    }

    let (low, high) = if me.id < inviter.id {
        (me.id, inviter.id)
    } else {
        (inviter.id, me.id)
    };

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
    .fetch_one(&pool)
    .await?;

    sqlx::query("update invite_tokens set used_at = now(), used_by_user_id = $1 where token = $2")
        .bind(me.id)
        .bind(&token)
        .execute(&pool)
        .await?;

    Ok(Json(friendship))
}
