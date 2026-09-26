use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use std::sync::Arc;

use crate::auth::{AuthClaims, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::maintenance::discard_pending_avatar_uploads;
use crate::models::{AvatarUploadTicket, UpdateMe, User};
use crate::state::AppState;
use crate::storage::AvatarStorage;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me).patch(update_me))
        .route("/me/avatar-upload", post(avatar_upload))
}

/// The profile for the current token. `404` means the token is valid but the
/// user hasn't onboarded yet — the client should send them through `POST /users`.
async fn get_me(
    State(pool): State<PgPool>,
    AuthClaims(claims): AuthClaims,
) -> ApiResult<Json<User>> {
    let user = sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, share_shelves, created_at \
         from users where hanko_user_id = $1",
    )
    .bind(&claims.sub)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(user))
}

const MAX_DISPLAY_NAME_CHARS: usize = 50;
const AVATAR_DELETE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Edit the caller's own profile. Only fields present in the body change.
async fn update_me(
    State(pool): State<PgPool>,
    State(storage): State<Option<Arc<AvatarStorage>>>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<UpdateMe>,
) -> ApiResult<Json<User>> {
    let display_name = match input.display_name.as_deref().map(str::trim) {
        Some(n) if n.is_empty() || n.chars().count() > MAX_DISPLAY_NAME_CHARS => {
            return Err(ApiError::BadRequest(format!(
                "display_name must be 1-{MAX_DISPLAY_NAME_CHARS} characters"
            )));
        }
        other => other,
    };

    // Only URLs this API minted for the caller (see `AvatarStorage`) — never an
    // arbitrary external URL, and never another user's blob. `null` clears it.
    if let Some(Some(url)) = &input.avatar_url {
        let key = avatar_key(&pool, me.id).await?;
        let ok = storage
            .as_ref()
            .is_some_and(|s| s.owns_avatar_url(key, url));
        if !ok {
            return Err(ApiError::BadRequest(
                "avatar_url must come from POST /me/avatar-upload".into(),
            ));
        }
        // ...and one that hasn't expired: an upload that was never saved is
        // deleted after `UNSAVED_PHOTO_TTL` (see maintenance.rs), so a URL
        // whose record is gone points at a file that no longer exists.
        let blob = storage.as_ref().and_then(|s| s.blob_path(url));
        let live = sqlx::query_scalar::<_, bool>(
            "select exists(select 1 from avatar_uploads where user_id = $1 and blob_path = $2)",
        )
        .bind(me.id)
        .bind(blob)
        .fetch_one(&pool)
        .await?;
        if !live {
            return Err(ApiError::BadRequest(
                "that photo upload has expired — please choose the photo again".into(),
            ));
        }
    }
    let (set_avatar, avatar_url) = match input.avatar_url {
        Some(url) => (true, url),
        None => (false, None),
    };
    // The photo we're about to replace or remove, so its file can be deleted.
    let previous_avatar = if set_avatar {
        sqlx::query_scalar::<_, Option<String>>("select avatar_url from users where id = $1")
            .bind(me.id)
            .fetch_one(&pool)
            .await?
    } else {
        None
    };

    let user = sqlx::query_as::<_, User>(
        "update users set \
             share_shelves = coalesce($2, share_shelves), \
             display_name = coalesce($3, display_name), \
             avatar_url = case when $4 then $5 else avatar_url end \
         where id = $1 \
         returning id, handle, display_name, avatar_url, locale, share_shelves, created_at",
    )
    .bind(me.id)
    .bind(input.share_shelves)
    .bind(display_name)
    .bind(set_avatar)
    .bind(avatar_url)
    .fetch_one(&pool)
    .await?;

    if let (true, Some(storage)) = (set_avatar, storage.as_ref()) {
        // The new photo is now in use, so it must no longer expire.
        if let Some(blob) = user
            .avatar_url
            .as_deref()
            .and_then(|u| storage.blob_path(u))
        {
            sqlx::query(
                "update avatar_uploads set claimed_at = coalesce(claimed_at, now()) \
                 where user_id = $1 and blob_path = $2",
            )
            .bind(me.id)
            .bind(blob)
            .execute(&pool)
            .await?;
        }

        // Replacing or removing a photo deletes the old file, so a discarded photo
        // doesn't stay reachable at its old URL. Best-effort: the profile is already
        // saved, so a storage hiccup is logged rather than failing the request.
        if let Some(old) = previous_avatar.filter(|o| user.avatar_url.as_deref() != Some(o)) {
            let deleted = match tokio::time::timeout(
                AVATAR_DELETE_TIMEOUT,
                storage.delete_avatar(&old),
            )
            .await
            {
                Ok(Ok(())) => true,
                Ok(Err(e)) => {
                    tracing::warn!("couldn't delete replaced avatar: {e}");
                    false
                }
                Err(_) => {
                    tracing::warn!("deleting replaced avatar timed out");
                    false
                }
            };
            if let Some(blob) = storage.blob_path(&old) {
                if deleted {
                    // Gone: forget its record.
                    sqlx::query("delete from avatar_uploads where user_id = $1 and blob_path = $2")
                        .bind(me.id)
                        .bind(blob)
                        .execute(&pool)
                        .await?;
                } else {
                    // Still there: hand it back to the expiry sweep, which retries.
                    sqlx::query(
                        "update avatar_uploads set claimed_at = null \
                         where user_id = $1 and blob_path = $2",
                    )
                    .bind(me.id)
                    .bind(blob)
                    .execute(&pool)
                    .await?;
                }
            }
        }
    }

    Ok(Json(user))
}

/// Step 1 of changing the avatar: a short-lived, write-only URL to `PUT` a JPEG
/// to. 503 when the server has no Blob Storage configured.
async fn avatar_upload(
    State(pool): State<PgPool>,
    State(storage): State<Option<Arc<AvatarStorage>>>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<AvatarUploadTicket>> {
    let storage =
        storage.ok_or_else(|| ApiError::Unavailable("avatar uploads aren't configured".into()))?;
    // One pending upload per user: starting a new one discards the last unsaved
    // photo. Best-effort — failing to tidy up must not stop the user uploading.
    if let Err(e) = discard_pending_avatar_uploads(&pool, &storage, me.id).await {
        tracing::warn!("couldn't discard previous pending photo: {e}");
    }

    let up = storage.create_upload(avatar_key(&pool, me.id).await?);
    // Recorded so that if it's never saved to the profile it can expire.
    if let Some(blob) = storage.blob_path(&up.avatar_url) {
        sqlx::query("insert into avatar_uploads (user_id, blob_path) values ($1, $2)")
            .bind(me.id)
            .bind(blob)
            .execute(&pool)
            .await?;
    }
    Ok(Json(AvatarUploadTicket {
        upload_url: up.upload_url,
        avatar_url: up.avatar_url,
        expires_at: up.expires_at,
    }))
}

/// The random key photos are stored under — not the user id (see migration 0014).
async fn avatar_key(pool: &PgPool, user_id: uuid::Uuid) -> ApiResult<uuid::Uuid> {
    Ok(
        sqlx::query_scalar::<_, uuid::Uuid>("select avatar_key from users where id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?,
    )
}
