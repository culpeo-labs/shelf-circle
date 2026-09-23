pub mod auth;
pub mod db;
pub mod error;
pub mod models;
pub mod providers;
pub mod routes;
pub mod state;
pub mod storage;

use axum::routing::get;
use axum::Router;

use crate::state::AppState;

/// Builds the full application router (health check + every route module) over
/// an already-constructed [`AppState`]. Split out of `main` so integration
/// tests can exercise the real router — including axum's extractor wiring —
/// without a network listener.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(routes::router())
        .with_state(state)
}
