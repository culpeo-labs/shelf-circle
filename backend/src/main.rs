mod auth;
mod db;
mod error;
mod models;
mod providers;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

use crate::auth::HankoAuth;
use crate::providers::BookProviders;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env before anything reads the environment. Real environment
    // variables win over .env entries; a missing file is fine (production sets
    // the environment directly).
    let dotenv_path = dotenvy::dotenv();

    tracing_subscriber::fmt::init();

    match &dotenv_path {
        Ok(path) => tracing::info!("loaded environment from {}", path.display()),
        Err(e) if e.not_found() => tracing::debug!("no .env file, using process environment"),
        Err(e) => tracing::warn!("failed to load .env: {e}"),
    }

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://shelfcircle:shelfcircle@localhost:5432/shelfcircle".into());

    let pool = db::connect(&database_url).await?;

    let auth = Arc::new(HankoAuth::from_env()?);
    auth.warm().await;

    let state = AppState {
        pool,
        providers: Arc::new(BookProviders::from_env()),
        auth,
    };

    let app: Router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(routes::router())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("shelf-circle backend listening on {addr}");

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
