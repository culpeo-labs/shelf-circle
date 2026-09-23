use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use sqlx::PgPool;

use crate::auth::{AuthClaims, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::models::{UpdateMe, User};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(get_me).patch(update_me))
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

/// Edit the caller's own profile. Only fields present in the body change.
async fn update_me(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Json(input): Json<UpdateMe>,
) -> ApiResult<Json<User>> {
    let user = sqlx::query_as::<_, User>(
        "update users set share_shelves = coalesce($2, share_shelves) where id = $1 \
         returning id, handle, display_name, avatar_url, locale, share_shelves, created_at",
    )
    .bind(me.id)
    .bind(input.share_shelves)
    .fetch_one(&pool)
    .await?;

    Ok(Json(user))
}
