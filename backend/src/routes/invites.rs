use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::friendships::upsert_friendship;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{Friendship, Invite, InvitePreview};
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
    // A CSPRNG-backed token, kept URL-friendly-short per friends.md rather
    // than using the full 32-char UUID hex string; 64 bits of randomness is
    // ample for a single-use, 7-day-lived token at this app's scale.
    let token = Uuid::new_v4().simple().to_string()[..16].to_string();
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
///
/// The claim (`update ... where used_at is null ... returning`) and the
/// friendship insert run in one transaction so two concurrent accepts of the
/// same token can't both win: a plain check-then-act (select, then insert,
/// then a separate unconditional update) would let two callers both pass the
/// `used_at is null` check before either update commits, since the update
/// carried no such guard — silently accepting a single-use token twice.
async fn accept_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<Friendship>> {
    let mut tx = pool.begin().await?;

    let inviter_id = sqlx::query_scalar::<_, Uuid>(
        "update invite_tokens set used_at = now(), used_by_user_id = $1 \
         where token = $2 and used_at is null and expires_at > now() \
         returning created_by_user_id",
    )
    .bind(me.id)
    .bind(&token)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::BadRequest("invite is invalid, expired, or already used".into()))?;

    if inviter_id == me.id {
        return Err(ApiError::BadRequest("cannot accept your own invite".into()));
    }

    let friendship = upsert_friendship(&mut *tx, me.id, inviter_id).await?;
    tx.commit().await?;

    Ok(friendship)
}
