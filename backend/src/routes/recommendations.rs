use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{ensure_self, CurrentUser};
use crate::error::ApiResult;
use crate::models::{CreateRecommendation, Recommendation};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", post(create_recommendation))
        .route(
            "/users/{user_id}/recommendations/inbox",
            get(inbox_for_user),
        )
}

/// The sender is the authenticated caller; the recipient and book come from the
/// body.
async fn create_recommendation(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<CreateRecommendation>,
) -> ApiResult<Json<Recommendation>> {
    let rec = sqlx::query_as::<_, Recommendation>(
        r#"
        insert into recommendations (from_user_id, to_user_id, book_id, note)
        values ($1, $2, $3, $4)
        returning *
        "#,
    )
    .bind(me.id)
    .bind(input.to_user_id)
    .bind(input.book_id)
    .bind(&input.note)
    .fetch_one(&pool)
    .await?;

    Ok(Json(rec))
}

/// "X recommended a book to you" feed — this is the core loop of the app.
async fn inbox_for_user(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<Vec<Recommendation>>> {
    ensure_self(&me, user_id)?;

    let recs = sqlx::query_as::<_, Recommendation>(
        "select * from recommendations where to_user_id = $1 order by created_at desc",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(recs))
}
