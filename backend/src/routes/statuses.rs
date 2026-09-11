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
async fn set_status(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<SetBookStatus>,
) -> ApiResult<Json<BookStatus>> {
    let rating = match input.status {
        ReadingStatus::Finished | ReadingStatus::DidNotFinish => {
            if let Some(r) = input.rating {
                if !(1..=5).contains(&r) {
                    return Err(ApiError::BadRequest(
                        "rating must be between 1 and 5".into(),
                    ));
                }
            }
            input.rating
        }
        _ if input.rating.is_some() => {
            return Err(ApiError::BadRequest(
                "rating can only be set with status 'finished' or 'did_not_finish'".into(),
            ));
        }
        _ => None,
    };

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
    .fetch_one(&pool)
    .await?;

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
