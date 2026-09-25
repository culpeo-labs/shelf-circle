use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{Friend, FriendProfile, Friendship};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/friends", get(list_my_friends))
        .route("/friends/{friendship_id}", get(get_friend))
}

/// Everyone the caller is friends with, from either side of the canonicalized
/// row — so an invite's creator sees the person who scanned their code, not
/// only the scanner seeing the creator. Alphabetical by display name. Friends
/// are identified by friendship, not user id (see `Friend`).
async fn list_my_friends(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<Vec<Friend>>> {
    let friends = sqlx::query_as::<_, Friend>(
        "select f.id as friendship_id, u.handle, u.display_name, u.avatar_url \
         from friendships f \
         join users u on u.id = case when f.user_a_id = $1 then f.user_b_id else f.user_a_id end \
         where f.user_a_id = $1 or f.user_b_id = $1 \
         order by lower(u.display_name), f.id",
    )
    .bind(me.id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(friends))
}

/// One friend's profile. A friendship that isn't yours is a 404.
async fn get_friend(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
    Path(friendship_id): Path<Uuid>,
) -> ApiResult<Json<FriendProfile>> {
    let profile = sqlx::query_as::<_, FriendProfile>(
        "select f.id as friendship_id, u.handle, u.display_name, u.avatar_url, u.share_shelves \
         from friendships f \
         join users u on u.id = case when f.user_a_id = $2 then f.user_b_id else f.user_a_id end \
         where f.id = $1 and (f.user_a_id = $2 or f.user_b_id = $2)",
    )
    .bind(friendship_id)
    .bind(me.id)
    .fetch_optional(&pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(profile))
}

/// The other person's user id for one of the caller's friendships — server-side
/// only, so handlers can act on a friend without that id ever reaching a client.
/// `NotFound` if the friendship doesn't exist or isn't the caller's.
pub async fn friend_user_id(
    executor: impl sqlx::PgExecutor<'_>,
    me: Uuid,
    friendship_id: Uuid,
) -> ApiResult<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        "select case when user_a_id = $2 then user_b_id else user_a_id end \
         from friendships where id = $1 and (user_a_id = $2 or user_b_id = $2)",
    )
    .bind(friendship_id)
    .bind(me)
    .fetch_optional(executor)
    .await?
    .ok_or(ApiError::NotFound)
}

/// The one place friendships are created: accepting a single-use invite or
/// approving a friend request. There is deliberately no add-by-handle — a
/// friendship is mutual and gives access to someone's timeline (and shelves, if
/// shared), so it only ever forms with both people's involvement.
///
/// Rows are canonicalized so user_a_id < user_b_id (see migration). Generic over
/// the executor so a caller can run it inside a transaction (`&mut *tx`).
/// Idempotent.
pub async fn upsert_friendship(
    executor: impl sqlx::PgExecutor<'_>,
    a: Uuid,
    b: Uuid,
) -> ApiResult<Json<Friendship>> {
    let (low, high) = if a < b { (a, b) } else { (b, a) };

    let friendship = sqlx::query_as::<_, Friendship>(
        r#"
        insert into friendships (user_a_id, user_b_id)
        values ($1, $2)
        on conflict (user_a_id, user_b_id) do update set user_a_id = excluded.user_a_id
        returning id, user_a_id, user_b_id, created_at
        "#,
    )
    .bind(low)
    .bind(high)
    .fetch_one(executor)
    .await?;

    Ok(Json(friendship))
}
