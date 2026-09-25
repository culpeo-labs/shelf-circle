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

/// A profile: your own, or a friend's. Anyone else is a 404 — there's no
/// looking people up by id (or by handle: that endpoint no longer exists), so
/// being signed in isn't enough to see who someone is.
async fn get_user(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<User>> {
    if id != me.id {
        let (low, high) = if me.id < id { (me.id, id) } else { (id, me.id) };
        let friends = sqlx::query_scalar::<_, bool>(
            "select exists(select 1 from friendships where user_a_id = $1 and user_b_id = $2)",
        )
        .bind(low)
        .bind(high)
        .fetch_one(&pool)
        .await?;
        if !friends {
            return Err(ApiError::NotFound);
        }
    }

    let user = sqlx::query_as::<_, User>(
        "select id, handle, display_name, avatar_url, locale, share_shelves, created_at from users where id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(user))
}
