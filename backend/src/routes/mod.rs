mod books;
mod feed;
mod friendships;
mod invites;
mod library;
mod me;
mod recommendations;
mod statuses;
mod users;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(me::router())
        .merge(users::router())
        .merge(friendships::router())
        .merge(invites::router())
        .merge(books::router())
        .merge(statuses::router())
        .merge(recommendations::router())
        .merge(feed::router())
        .merge(library::router())
}
