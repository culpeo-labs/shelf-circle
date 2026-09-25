use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::friendships::upsert_friendship;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{Invite, InvitePreview};
use crate::state::AppState;

/// Two modes (see migration 0013):
///  * single-use (default): valid 7 days, the first person to accept becomes a
///    friend immediately — the issuer chose to hand that link/QR to someone.
///  * reusable ("anyone with the link"): valid 30 days until revoked; every
///    accept creates a *pending request* the issuer must approve, so a link
///    that spreads can't let strangers in on its own.
const SINGLE_USE_TTL: Duration = Duration::days(7);
const REUSABLE_TTL: Duration = Duration::days(30);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/invites", post(create_invite).get(list_my_invites))
        .route("/invites/{token}", get(get_invite).delete(revoke_invite))
        .route("/invites/{token}/accept", post(accept_invite))
        .route("/me/friend-requests", get(list_friend_requests))
        .route("/friend-requests/{id}/approve", post(approve_request))
        .route("/friend-requests/{id}/decline", post(decline_request))
}

/// SQL fragment: the invite can still be used (not revoked, expired, or spent).
const USABLE: &str = "it.revoked_at is null and it.expires_at > now() \
                      and (it.max_uses is null or it.use_count < it.max_uses)";

#[derive(Debug, Deserialize)]
struct CreateParams {
    #[serde(default)]
    reusable: bool,
}

/// `POST /invites?reusable=true` for an "anyone with the link" invite.
async fn create_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Query(params): Query<CreateParams>,
) -> ApiResult<Json<Invite>> {
    // The token is the only secret protecting the invite, so a link that can be
    // passed around (reusable) gets the full 122 random bits of a v4 UUID; a
    // single-use one is kept short (~60 bits) for a friendlier QR code.
    let simple = Uuid::new_v4().simple().to_string();
    let (token, ttl, max_uses) = if params.reusable {
        (simple, REUSABLE_TTL, None)
    } else {
        (simple[..16].to_string(), SINGLE_USE_TTL, Some(1))
    };
    let expires_at = Utc::now() + ttl;

    sqlx::query(
        "insert into invite_tokens (token, created_by_user_id, expires_at, max_uses, requires_approval) \
         values ($1, $2, $3, $4, $5)",
    )
    .bind(&token)
    .bind(me.id)
    .bind(expires_at)
    .bind(max_uses)
    .bind(params.reusable)
    .execute(&pool)
    .await?;

    Ok(Json(Invite {
        token,
        expires_at,
        reusable: params.reusable,
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct MyInvite {
    token: String,
    reusable: bool,
    expires_at: DateTime<Utc>,
    /// How many people have become friends through it.
    use_count: i32,
    /// Requests waiting for your approval (reusable invites).
    pending_requests: i64,
}

/// The caller's own invites that can still be used, newest first.
async fn list_my_invites(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<Vec<MyInvite>>> {
    let sql = format!(
        "select it.token, it.requires_approval as reusable, it.expires_at, it.use_count, \
                (select count(*) from friend_requests fr \
                  where fr.invite_id = it.id and fr.status = 'pending') as pending_requests \
         from invite_tokens it \
         where it.created_by_user_id = $1 and {USABLE} \
         order by it.created_at desc"
    );
    // `USABLE` is a fixed literal above, not user input.
    let invites = sqlx::query_as::<_, MyInvite>(sqlx::AssertSqlSafe(sql))
        .bind(me.id)
        .fetch_all(&pool)
        .await?;
    Ok(Json(invites))
}

/// Stop an invite from being used. Only its owner can; idempotent. Requests
/// already sent stay pending so you can still decide on them.
async fn revoke_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<StatusCode> {
    let owned = sqlx::query_scalar::<_, Uuid>(
        "update invite_tokens set revoked_at = coalesce(revoked_at, now()) \
         where token = $1 and created_by_user_id = $2 returning id",
    )
    .bind(&token)
    .bind(me.id)
    .fetch_optional(&pool)
    .await?;
    owned
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}

/// Public — no `CurrentUser`/`AuthClaims` extractor, so no token is required.
/// Only the inviter's display name and photo: never a handle or an id.
async fn get_invite(
    State(pool): State<PgPool>,
    Path(token): Path<String>,
) -> ApiResult<Json<InvitePreview>> {
    let sql = format!(
        "select u.display_name, u.avatar_url, it.requires_approval \
         from invite_tokens it join users u on u.id = it.created_by_user_id \
         where it.token = $1 and {USABLE}"
    );
    let preview = sqlx::query_as::<_, InvitePreview>(sqlx::AssertSqlSafe(sql))
        .bind(&token)
        .fetch_optional(&pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(preview))
}

#[derive(Debug, Serialize)]
struct AcceptResult {
    /// `friends`: you're connected now. `pending`: the inviter has to approve.
    status: &'static str,
    /// Set when `status` is `friends`. An opaque id for the friendship, not a
    /// user id.
    friendship_id: Option<Uuid>,
}

const UNUSABLE: &str = "invite is invalid, expired, or already used";

/// Accept an invite. Runs in one transaction that first locks the invite row,
/// so concurrent accepts of a single-use token can't both win: with the lock
/// held, the capacity check and the `use_count` bump are one atomic step (a
/// plain check-then-act would let two callers both pass the check).
///
/// A bad/expired/revoked/spent invite is a 400 (as is your own invite) — all
/// just "this invite doesn't work". Already being friends is a success and
/// doesn't use up the invite.
async fn accept_invite(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<AcceptResult>> {
    let mut tx = pool.begin().await?;

    let invite = sqlx::query_as::<_, (Uuid, Uuid, bool)>(
        "select it.id, it.created_by_user_id, it.requires_approval \
         from invite_tokens it where it.token = $1 and it.revoked_at is null \
           and it.expires_at > now() \
           and (it.max_uses is null or it.use_count < it.max_uses) \
         for update",
    )
    .bind(&token)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::BadRequest(UNUSABLE.into()))?;
    let (invite_id, inviter_id, requires_approval) = invite;

    if inviter_id == me.id {
        return Err(ApiError::BadRequest("cannot accept your own invite".into()));
    }

    let (low, high) = if me.id < inviter_id {
        (me.id, inviter_id)
    } else {
        (inviter_id, me.id)
    };
    let existing = sqlx::query_scalar::<_, Uuid>(
        "select id from friendships where user_a_id = $1 and user_b_id = $2",
    )
    .bind(low)
    .bind(high)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(friendship_id) = existing {
        return Ok(Json(AcceptResult {
            status: "friends",
            friendship_id: Some(friendship_id),
        }));
    }

    if requires_approval {
        // Idempotent per (invite, requester); a declined request stays declined
        // and looks the same as a pending one to the requester.
        sqlx::query(
            "insert into friend_requests (invite_id, requester_user_id, inviter_user_id) \
             values ($1, $2, $3) on conflict (invite_id, requester_user_id) do nothing",
        )
        .bind(invite_id)
        .bind(me.id)
        .bind(inviter_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Json(AcceptResult {
            status: "pending",
            friendship_id: None,
        }));
    }

    sqlx::query(
        "update invite_tokens set use_count = use_count + 1, used_at = now(), \
             used_by_user_id = $2 where id = $1",
    )
    .bind(invite_id)
    .bind(me.id)
    .execute(&mut *tx)
    .await?;
    let friendship = upsert_friendship(&mut *tx, me.id, inviter_id).await?;
    tx.commit().await?;

    Ok(Json(AcceptResult {
        status: "friends",
        friendship_id: Some(friendship.0.id),
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct FriendRequestView {
    id: Uuid,
    /// Only what the invite preview already shows: they aren't your friend yet,
    /// so no handle and no user id.
    display_name: String,
    avatar_url: Option<String>,
    created_at: DateTime<Utc>,
}

async fn list_friend_requests(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<Vec<FriendRequestView>>> {
    let requests = sqlx::query_as::<_, FriendRequestView>(
        "select fr.id, u.display_name, u.avatar_url, fr.created_at \
         from friend_requests fr join users u on u.id = fr.requester_user_id \
         where fr.inviter_user_id = $1 and fr.status = 'pending' \
         order by fr.created_at",
    )
    .bind(me.id)
    .fetch_all(&pool)
    .await?;
    Ok(Json(requests))
}

/// Approve a pending request: the requester becomes a friend. Only the invite's
/// owner can; a request that isn't yours or isn't pending is a 404.
async fn approve_request(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AcceptResult>> {
    let mut tx = pool.begin().await?;
    let (requester_id, invite_id) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "update friend_requests set status = 'approved', decided_at = now() \
         where id = $1 and inviter_user_id = $2 and status = 'pending' \
         returning requester_user_id, invite_id",
    )
    .bind(id)
    .bind(me.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    sqlx::query("update invite_tokens set use_count = use_count + 1 where id = $1")
        .bind(invite_id)
        .execute(&mut *tx)
        .await?;
    let friendship = upsert_friendship(&mut *tx, me.id, requester_id).await?;
    tx.commit().await?;

    Ok(Json(AcceptResult {
        status: "friends",
        friendship_id: Some(friendship.0.id),
    }))
}

async fn decline_request(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let declined = sqlx::query(
        "update friend_requests set status = 'declined', decided_at = now() \
         where id = $1 and inviter_user_id = $2 and status = 'pending'",
    )
    .bind(id)
    .bind(me.id)
    .execute(&pool)
    .await?;
    if declined.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
