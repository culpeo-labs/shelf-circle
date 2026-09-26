use std::sync::Arc;

use axum::extract::FromRef;
use sqlx::PgPool;

use crate::auth::HankoAuth;
use crate::catalogs::Catalogs;
use crate::hanko_admin::HankoAdmin;
use crate::providers::BookProviders;
use crate::storage::AvatarStorage;

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
    /// `None` when Azure Blob Storage isn't configured (avatar upload → 503).
    pub storage: Option<Arc<AvatarStorage>>,
    pub catalogs: Arc<Catalogs>,
    /// `None` unless `HANKO_API_KEY` is set (account deletion answers 503).
    pub hanko_admin: Option<Arc<HankoAdmin>>,
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

impl FromRef<AppState> for Option<Arc<AvatarStorage>> {
    fn from_ref(state: &AppState) -> Self {
        state.storage.clone()
    }
}

impl FromRef<AppState> for Arc<Catalogs> {
    fn from_ref(state: &AppState) -> Self {
        state.catalogs.clone()
    }
}

impl FromRef<AppState> for Option<Arc<HankoAdmin>> {
    fn from_ref(state: &AppState) -> Self {
        state.hanko_admin.clone()
    }
}
