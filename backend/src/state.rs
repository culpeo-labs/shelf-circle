use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::auth::HankoAuth;
use crate::providers::BookProviders;

/// Shared application state. Handlers extract the piece they need via
/// `State<PgPool>`, `State<Arc<BookProviders>>`, or `State<Arc<HankoAuth>>` (all
/// resolved through `FromRef`), so route modules don't all have to name the
/// whole struct. The auth extractors (`AuthClaims`, `CurrentUser`) pull
/// `Arc<HankoAuth>` and `PgPool` the same way.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub providers: Arc<BookProviders>,
    pub auth: Arc<HankoAuth>,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Arc<BookProviders> {
    fn from_ref(state: &AppState) -> Self {
        state.providers.clone()
    }
}

impl FromRef<AppState> for Arc<HankoAuth> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}
