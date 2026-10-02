//! Deletes the caller's *own* account at Hanko by driving the Profile flow's
//! `account_delete` action — the same generic Flow API our frontend already
//! speaks for login/registration (see `frontend/src/auth/hankoFlowClient.ts`),
//! just called server-to-server with the user's own session token.
//!
//! No admin API key: `POST {HANKO_API_URL}/profile` accepts a plain
//! `Authorization: Bearer <jwt>` header — confirmed in Hanko's source
//! (`backend/flow_api/handler.go`'s `validateSession`, which builds its token
//! lookup as `header:Authorization:Bearer,cookie:<name>`) — so this acts *as*
//! the user, never with elevated access, and needs no extra secret beyond the
//! `HANKO_API_URL` already configured for JWKS.
//!
//! Docs: <https://docs.hanko.io/using-the-api/understanding-the-flow-api>

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Hanko's action name for deleting the account (`profile` flow,
/// `shared.ActionAccountDelete` in Hanko's source).
const ACCOUNT_DELETE_ACTION: &str = "account_delete";
/// The state Hanko returns once the account is actually gone
/// (`shared.StateProfileAccountDeleted`).
const ACCOUNT_DELETED_STATE: &str = "account_deleted";

#[derive(Debug, Clone)]
pub struct HankoFlow {
    http: reqwest::Client,
    /// `HANKO_API_URL`, no trailing slash.
    base: String,
}

#[derive(Debug, Default, Deserialize)]
struct FlowState {
    #[serde(default)]
    name: String,
    csrf_token: Option<String>,
    #[serde(default)]
    actions: HashMap<String, FlowAction>,
}

#[derive(Debug, Deserialize)]
struct FlowAction {
    href: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteAccountError {
    /// Hanko rejected the token as not naming a live session (`validateSession`
    /// returns 401/403 *before* the flow even runs). Since our own backend just
    /// independently verified this same JWT's signature and expiry moments
    /// earlier (`CurrentUser`), the only plausible reason Hanko would then
    /// reject it is that the account (and its session row) is already gone —
    /// e.g. a previous delete attempt that got this far but failed afterward.
    /// Callers should treat this the same as success.
    #[error("Hanko no longer recognizes this session")]
    AlreadyGone,
    /// A valid session, but no `account_delete` action on offer — the Hanko
    /// project has self-service deletion turned off, or returned some other
    /// unexpected flow state.
    #[error("account deletion isn't available: {0}")]
    NotAvailable(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

impl HankoFlow {
    pub fn new(hanko_api_url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()?,
            base: hanko_api_url.trim_end_matches('/').to_string(),
        })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let url =
            std::env::var("HANKO_API_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
        Self::new(&url)
    }

    /// Delete the account behind `token` (its own current Hanko session JWT,
    /// exactly as received on our `Authorization` header) by driving the
    /// Profile flow. `Ok(())` covers both "deleted just now" and "was already
    /// gone" — either way there's nothing left for the caller to retry at Hanko.
    pub async fn delete_own_account(&self, token: &str) -> Result<(), DeleteAccountError> {
        let init = match self
            .post(&format!("{}/profile", self.base), token, &json!({}))
            .await
        {
            Ok(state) => state,
            Err(e) if is_auth_rejection(&e) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let action = init.actions.get(ACCOUNT_DELETE_ACTION).ok_or_else(|| {
            DeleteAccountError::NotAvailable(
                "the Hanko project doesn't have self-service account deletion enabled".into(),
            )
        })?;
        let href = resolve_href(&self.base, &action.href);

        let body = json!({ "input_data": {}, "csrf_token": init.csrf_token });
        let result = match self.post(&href, token, &body).await {
            Ok(state) => state,
            Err(e) if is_auth_rejection(&e) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        if result.name == ACCOUNT_DELETED_STATE {
            Ok(())
        } else {
            Err(DeleteAccountError::NotAvailable(format!(
                "Hanko returned unexpected state {:?}",
                result.name
            )))
        }
    }

    async fn post(
        &self,
        url: &str,
        token: &str,
        body: &serde_json::Value,
    ) -> reqwest::Result<FlowState> {
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        // A state Hanko doesn't recognize (network hiccup mid-response, an
        // upgrade) parses as an empty/default state rather than panicking;
        // `delete_own_account` then reports it as `NotAvailable`.
        Ok(response.json().await.unwrap_or_default())
    }
}

fn is_auth_rejection(e: &reqwest::Error) -> bool {
    matches!(
        e.status(),
        Some(reqwest::StatusCode::UNAUTHORIZED) | Some(reqwest::StatusCode::FORBIDDEN)
    )
}

/// Mirrors `hankoFlowClient.ts`'s `resolveHref`: an action's `href` may be
/// absolute or relative to the Hanko API root.
fn resolve_href(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else {
        format!("{base}/{}", href.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn account_delete_flow(server: &MockServer) -> serde_json::Value {
        json!({
            "name": "profile_init",
            "csrf_token": "test-csrf",
            "actions": {
                "account_delete": { "href": format!("{}/profile-actions/account_delete", server.uri()) }
            }
        })
    }

    #[tokio::test]
    async fn deletes_via_the_profile_flow_with_the_users_own_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .and(header("authorization", "Bearer user-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(account_delete_flow(&server)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/profile-actions/account_delete"))
            .and(header("authorization", "Bearer user-token"))
            .and(body_json(
                json!({ "input_data": {}, "csrf_token": "test-csrf" }),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "name": "account_deleted", "actions": {} })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(flow.delete_own_account("user-token").await.is_ok());
    }

    #[tokio::test]
    async fn an_absolute_or_relative_href_both_resolve() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "profile_init",
                "csrf_token": "c",
                "actions": { "account_delete": { "href": "/profile-actions/account_delete" } }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/profile-actions/account_delete"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "name": "account_deleted", "actions": {} })),
            )
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(flow.delete_own_account("t").await.is_ok());
    }

    #[tokio::test]
    async fn a_401_on_the_initial_request_means_already_gone() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "name": "unauthorized",
                "error": { "code": "unauthorized", "message": "no valid session" }
            })))
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(flow.delete_own_account("stale-token").await.is_ok());
    }

    #[tokio::test]
    async fn a_403_on_the_action_step_also_means_already_gone() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(account_delete_flow(&server)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/profile-actions/account_delete"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(flow.delete_own_account("t").await.is_ok());
    }

    #[tokio::test]
    async fn a_server_error_is_a_real_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(matches!(
            flow.delete_own_account("t").await,
            Err(DeleteAccountError::Http(_))
        ));
    }

    #[tokio::test]
    async fn no_account_delete_action_is_not_available() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/profile"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "profile_init",
                "csrf_token": "c",
                "actions": { "logout": { "href": "/logout" } }
            })))
            .mount(&server)
            .await;

        let flow = HankoFlow::new(&server.uri()).unwrap();
        assert!(matches!(
            flow.delete_own_account("t").await,
            Err(DeleteAccountError::NotAvailable(_))
        ));
    }
}
