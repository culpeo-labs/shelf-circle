//! Account deletion (`DELETE /me`): removes everything we hold about the caller
//! and their sign-in identity at Hanko — as *them*, using their own session
//! token, not an admin key (see `hanko_flow.rs`).
//!
//! Order, because the steps live in different systems and can't share a
//! transaction:
//!  1. **Photo files** — both the current key's folder and the legacy
//!     `<user id>/` one. A failure stops everything here: nothing else has
//!     changed and the user can retry.
//!  2. **Hanko**, via the caller's own session (skipped under `AUTH_DISABLED`,
//!     where there's no real Hanko token). A rejected/failed call stops here
//!     too — except when Hanko says the session is no longer valid, which
//!     means the account is already gone there (see
//!     `HankoFlow::delete_own_account`), so we proceed.
//!  3. One **database transaction**: tombstone the Hanko id, then delete the
//!     `users` row (every user table cascades from it).

use axum::extract::State;
use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{CurrentUser, HankoSessionToken};
use crate::error::{ApiError, ApiResult};
use crate::hanko_flow::{DeleteAccountError, HankoFlow};
use crate::storage::AvatarStorage;
use std::sync::Arc;

pub async fn delete_account(
    State(pool): State<PgPool>,
    State(storage): State<Option<Arc<AvatarStorage>>>,
    State(hanko_flow): State<Arc<HankoFlow>>,
    CurrentUser(me): CurrentUser,
    HankoSessionToken(token): HankoSessionToken,
) -> ApiResult<StatusCode> {
    let (hanko_user_id, avatar_key) = sqlx::query_as::<_, (Option<String>, Uuid)>(
        "select hanko_user_id, avatar_key from users where id = $1",
    )
    .bind(me.id)
    .fetch_one(&pool)
    .await?;

    // 1. Photos.
    if let Some(storage) = storage.as_ref() {
        for folder in [avatar_key, me.id] {
            storage.delete_folder(folder).await.map_err(|e| {
                tracing::error!("account deletion: couldn't delete photos: {e:#}");
                ApiError::BadGateway("couldn't delete your photos — please try again".into())
            })?;
        }
    }

    // 2. Hanko, as the user themselves.
    if let Some(token) = token {
        match hanko_flow.delete_own_account(&token).await {
            Ok(()) => {}
            Err(DeleteAccountError::NotAvailable(msg)) => {
                tracing::error!("account deletion: Hanko says it isn't available: {msg}");
                return Err(ApiError::Unavailable(
                    "account deletion isn't available right now — please try again later".into(),
                ));
            }
            Err(e) => {
                tracing::error!("account deletion: Hanko refused: {e:#}");
                return Err(ApiError::BadGateway(
                    "couldn't delete your sign-in account — nothing was deleted, please try again"
                        .into(),
                ));
            }
        }
    }

    // 3. Our data.
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
    tx.commit().await?;

    tracing::info!("account deleted");
    Ok(StatusCode::NO_CONTENT)
}
