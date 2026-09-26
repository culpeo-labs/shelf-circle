//! Account deletion (`DELETE /me`): removes everything we hold about the caller —
//! and their sign-in identity at Hanko.
//!
//! Order matters, because the steps live in different systems and can't share a
//! transaction:
//!  1. **Photo files** are deleted first (both the current key's folder and the
//!     legacy `<user id>/` one). If that fails we stop: nothing else has changed
//!     and the user can retry.
//!  2. Then one **database transaction**: tombstone the Hanko id, delete the
//!     `users` row (every user table cascades from it), and — still inside the
//!     transaction — delete the user at **Hanko**. Only if Hanko succeeds do we
//!     commit; if it fails the transaction rolls back and the account is intact.
//!     Hanko treats an already-deleted user as success, so retries converge.
//!
//! Refused with 503 when Hanko's admin key isn't configured (unless auth is
//! disabled for local dev), rather than leaving someone's email at Hanko.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{CurrentUser, HankoAuth};
use crate::error::{ApiError, ApiResult};
use crate::hanko_admin::HankoAdmin;
use crate::storage::AvatarStorage;

pub async fn delete_account(
    State(pool): State<PgPool>,
    State(auth): State<Arc<HankoAuth>>,
    State(storage): State<Option<Arc<AvatarStorage>>>,
    State(hanko): State<Option<Arc<HankoAdmin>>>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<StatusCode> {
    let (hanko_user_id, avatar_key) = sqlx::query_as::<_, (Option<String>, Uuid)>(
        "select hanko_user_id, avatar_key from users where id = $1",
    )
    .bind(me.id)
    .fetch_one(&pool)
    .await?;

    // Whether we must also delete at Hanko: not for local-dev users who have no
    // Hanko identity (auth disabled), but never silently skipped in production.
    let hanko_step = match (&hanko_user_id, auth.disabled) {
        (Some(id), false) => {
            let admin = hanko.as_ref().ok_or_else(|| {
                ApiError::Unavailable("account deletion isn't configured on this server".into())
            })?;
            Some((id.clone(), admin.clone()))
        }
        _ => None,
    };

    // 1. Photos.
    if let Some(storage) = storage.as_ref() {
        for folder in [avatar_key, me.id] {
            storage.delete_folder(folder).await.map_err(|e| {
                tracing::error!("account deletion: couldn't delete photos: {e:#}");
                ApiError::BadGateway("couldn't delete your photos — please try again".into())
            })?;
        }
    }

    // 2. Database + Hanko, committed together or not at all.
    let mut tx = pool.begin().await?;
    if let Some(sub) = &hanko_user_id {
        sqlx::query(
            "insert into deleted_accounts (hanko_user_id) values ($1) \
             on conflict (hanko_user_id) do update set deleted_at = now()",
        )
        .bind(sub)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("delete from users where id = $1")
        .bind(me.id)
        .execute(&mut *tx)
        .await?;
    if let Some((sub, admin)) = hanko_step {
        admin.delete_user(&sub).await.map_err(|e| {
            tracing::error!("account deletion: Hanko refused: {e:#}");
            ApiError::BadGateway(
                "couldn't delete your sign-in account — nothing was deleted, please try again"
                    .into(),
            )
        })?;
    }
    tx.commit().await?;

    tracing::info!("account deleted");
    Ok(StatusCode::NO_CONTENT)
}
