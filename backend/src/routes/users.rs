use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{AuthClaims, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::models::{CreateUser, User};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", post(create_user))
        .route("/users/{id}", get(get_user))
        .route("/users/by-handle/{handle}", get(get_user_by_handle))
}

/// Create the profile for the authenticated token (onboarding). The Hanko user
/// id (`sub`) and email come from the token; the client supplies the handle and
/// display name. A duplicate `handle` or a second call for the same token both
/// surface as `409` (see `ApiError::Database` unique-violation mapping).
async fn create_user(
    State(pool): State<PgPool>,
    AuthClaims(claims): AuthClaims,
    Json(input): Json<CreateUser>,
) -> ApiResult<Json<User>> {
    if input.handle.trim().is_empty() || input.display_name.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "handle and display_name are required".into(),
        ));
    }

    let user = sqlx::query_as::<_, User>(
        r#"
        insert into users (handle, display_name, locale, hanko_user_id, email)
        values ($1, $2, coalesce($3, 'en'), $4, $5)
        returning id, handle, display_name, avatar_url, locale, share_shelves, created_at
        "#,
    )
    .bind(&input.handle)
    .bind(&input.display_name)
    .bind(&input.locale)
    .bind(&claims.sub)
    .bind(claims.email())
    .fetch_one(&pool)
    .await?;

    Ok(Json(user))
}

async fn get_user(
    State(pool): State<PgPool>,
    _caller: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<User>> {
    let user = sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, share_shelves, created_at from users where id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(user))
}

async fn get_user_by_handle(
    State(pool): State<PgPool>,
    _caller: CurrentUser,
    Path(handle): Path<String>,
) -> ApiResult<Json<User>> {
    let user = sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, share_shelves, created_at from users where handle = $1",
    )
    .bind(handle)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(user))
}
