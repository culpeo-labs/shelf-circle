use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::models::{Friendship, User};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/me/friends", get(list_my_friends))
}

/// Everyone the caller is friends with, from either side of the canonicalized
/// row — so an invite's creator sees the person who scanned their code, not
/// only the scanner seeing the creator. Alphabetical by display name.
async fn list_my_friends(
    State(pool): State<PgPool>,
    CurrentUser(me): CurrentUser,
) -> ApiResult<Json<Vec<User>>> {
    let friends = sqlx::query_as::<_, User>(
        "select u.id, u.handle, u.display_name, u.avatar_url, u.locale, u.share_shelves, u.created_at \
         from friendships f \
         join users u on u.id = case when f.user_a_id = $1 then f.user_b_id else f.user_a_id end \
         where f.user_a_id = $1 or f.user_b_id = $1 \
         order by lower(u.display_name), u.id",
    )
    .bind(me.id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(friends))
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
