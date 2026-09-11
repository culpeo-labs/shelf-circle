use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use sqlx::PgPool;

use crate::auth::AuthClaims;
use crate::error::{ApiError, ApiResult};
use crate::models::User;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(get_me))
}

/// The profile for the current token. `404` means the token is valid but the
/// user hasn't onboarded yet — the client should send them through `POST /users`.
async fn get_me(
    State(pool): State<PgPool>,
    AuthClaims(claims): AuthClaims,
) -> ApiResult<Json<User>> {
    let user = sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, created_at \
         from users where hanko_user_id = $1",
    )
    .bind(&claims.sub)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(user))
}
