use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{ensure_self, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::models::{BookStatus, ReadingStatus, SetBookStatus};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/book-statuses", put(set_status))
        .route(
            "/users/{user_id}/book-statuses",
            get(list_statuses_for_user),
        )
}

/// Upsert: a user has at most one status per book (see unique constraint).
/// Setting a new status overwrites the previous one with a fresh updated_at.
/// The acting user is taken from the auth token.
///
/// `rating` (1-5) is only accepted alongside `finished` / `did_not_finish`; it
/// is cleared whenever the book moves to any other status.
/// `rating` is only meaningful once reading has ended, and must be 1-5 when
/// given. Returns the rating to persist (possibly `None`, e.g. to clear a
/// stale rating when the status moves away from finished/did-not-finish), or
/// the `BadRequest` to reject the update with. Split out of `set_status` so
/// this validation is testable without a database.
fn resolve_rating(status: ReadingStatus, rating: Option<i16>) -> ApiResult<Option<i16>> {
    match status {
        ReadingStatus::Finished | ReadingStatus::DidNotFinish => {
            if let Some(r) = rating {
                if !(1..=5).contains(&r) {
                    return Err(ApiError::BadRequest(
                        "rating must be between 1 and 5".into(),
                    ));
                }
            }
            Ok(rating)
        }
        _ if rating.is_some() => Err(ApiError::BadRequest(
            "rating can only be set with status 'finished' or 'did_not_finish'".into(),
        )),
        _ => Ok(None),
    }
}

async fn set_status(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<SetBookStatus>,
) -> ApiResult<Json<BookStatus>> {
    let rating = resolve_rating(input.status, input.rating)?;

    let mut tx = pool.begin().await?;

    if input.backdated {
        // Not a bind parameter: `SET` doesn't take query parameters, and
        // the value here is a fixed literal, not user input, so there's
        // nothing to inject. `SET LOCAL` is transaction-scoped, so this
        // can't leak into another request's use of the same pooled
        // connection once this transaction commits below.
        sqlx::query("set local shelf_circle.suppress_activity = 'true'")
            .execute(&mut *tx)
            .await?;
    }

    let status = sqlx::query_as::<_, BookStatus>(
        r#"
        insert into book_statuses (user_id, book_id, status, progress_percent, rating)
        values ($1, $2, $3, $4, $5)
        on conflict (user_id, book_id) do update
            set status = excluded.status,
                progress_percent = excluded.progress_percent,
                rating = excluded.rating,
                updated_at = now()
        returning *
        "#,
    )
    .bind(me.id)
    .bind(input.book_id)
    .bind(input.status)
    .bind(input.progress_percent)
    .bind(rating)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(status))
}

async fn list_statuses_for_user(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<Vec<BookStatus>>> {
    ensure_self(&me, user_id)?;

    let statuses = sqlx::query_as::<_, BookStatus>(
        "select * from book_statuses where user_id = $1 order by updated_at desc",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(statuses))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_required_range_is_1_to_5_for_finished() {
        assert!(resolve_rating(ReadingStatus::Finished, Some(0)).is_err());
        assert!(resolve_rating(ReadingStatus::Finished, Some(6)).is_err());
        assert_eq!(
            resolve_rating(ReadingStatus::Finished, Some(1)).unwrap(),
            Some(1)
        );
        assert_eq!(
            resolve_rating(ReadingStatus::Finished, Some(5)).unwrap(),
            Some(5)
        );
    }

    #[test]
    fn rating_is_optional_for_finished_and_did_not_finish() {
        assert_eq!(resolve_rating(ReadingStatus::Finished, None).unwrap(), None);
        assert_eq!(
            resolve_rating(ReadingStatus::DidNotFinish, Some(3)).unwrap(),
            Some(3)
        );
    }

    #[test]
    fn rating_is_rejected_for_every_other_status() {
        for status in [ReadingStatus::WantToRead, ReadingStatus::CurrentlyReading] {
            assert!(
                resolve_rating(status, Some(4)).is_err(),
                "{status:?} should reject a rating"
            );
            assert_eq!(
                resolve_rating(status, None).unwrap(),
                None,
                "{status:?} with no rating is fine"
            );
        }
    }
}
