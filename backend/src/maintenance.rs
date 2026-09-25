//! Background housekeeping.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::storage::AvatarStorage;

/// How long an uploaded-but-unsaved profile photo lives before it's deleted.
/// The upload URL itself is good for 10 minutes; the rest is slack for someone
/// who uploads a photo, keeps editing their name, and saves later. Saving after
/// it expired asks them to choose the photo again.
pub const UNSAVED_PHOTO_TTL: Duration = Duration::from_secs(60 * 60);
const SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);
const SWEEP_BATCH: i64 = 100;

/// Delete photos that were uploaded (`POST /me/avatar-upload`) but never saved
/// to a profile, once they're older than `older_than`: the file in blob storage,
/// then its `avatar_uploads` row. Returns how many were removed.
///
/// Safe to run from several backend replicas at once: rows are claimed with
/// `for update skip locked`, so each is handled by one sweeper. A file that
/// can't be deleted keeps its row and is retried on the next sweep.
pub async fn sweep_unclaimed_avatars(
    pool: &PgPool,
    storage: &AvatarStorage,
    older_than: Duration,
) -> anyhow::Result<usize> {
    let mut tx = pool.begin().await?;
    let expired = sqlx::query_as::<_, (Uuid, String)>(
        "select id, blob_path from avatar_uploads \
         where claimed_at is null and created_at < now() - make_interval(secs => $1::float8) \
         order by created_at limit $2 for update skip locked",
    )
    .bind(older_than.as_secs_f64())
    .bind(SWEEP_BATCH)
    .fetch_all(&mut *tx)
    .await?;

    let mut removed = Vec::new();
    for (id, blob_path) in expired {
        match storage.delete_blob(&blob_path).await {
            Ok(()) => removed.push(id),
            Err(e) => tracing::warn!("couldn't delete unsaved photo {blob_path}: {e}"),
        }
    }
    if !removed.is_empty() {
        sqlx::query("delete from avatar_uploads where id = any($1)")
            .bind(&removed)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(removed.len())
}

/// Run [`sweep_unclaimed_avatars`] every few minutes for the life of the process
/// (the first pass is immediate, which also cleans up after a restart).
pub fn spawn_avatar_sweeper(pool: PgPool, storage: Arc<AvatarStorage>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            match sweep_unclaimed_avatars(&pool, &storage, UNSAVED_PHOTO_TTL).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("deleted {n} expired unsaved profile photo(s)"),
                Err(e) => tracing::warn!("unsaved-photo sweep failed: {e}"),
            }
        }
    });
}
