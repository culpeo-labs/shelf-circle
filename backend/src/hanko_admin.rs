//! Hanko's Admin API — used only to delete a user's sign-in identity when they
//! delete their account. The user's session token can't do that; it takes the
//! project's admin API key, so this is configured server-side
//! (`HANKO_API_URL` + `HANKO_API_KEY`).
//!
//! `DELETE {HANKO_API_URL}/admin/users/{id}` with `Authorization: Bearer <key>`;
//! 204 on success, 404 if the user is already gone. Docs:
//! <https://docs.hanko.io/api-reference/admin/user-management/delete-a-user-by-id>

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HankoAdmin {
    http: reqwest::Client,
    /// `<HANKO_API_URL>/admin`, no trailing slash.
    base: String,
    api_key: String,
}

impl HankoAdmin {
    pub fn new(api_url: &str, api_key: impl Into<String>) -> anyhow::Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            base: format!("{}/admin", api_url.trim_end_matches('/')),
            api_key: api_key.into(),
        })
    }

    /// `None` unless both `HANKO_API_URL` and `HANKO_API_KEY` are set. Without it,
    /// account deletion answers 503 rather than deleting our data and leaving the
    /// person's sign-in record (and email) behind at Hanko.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        match (get("HANKO_API_URL"), get("HANKO_API_KEY")) {
            (Some(url), Some(key)) => Self::new(&url, key).map(Some),
            _ => Ok(None),
        }
    }

    /// Permanently delete a user by their Hanko id (`sub`). Already gone (404)
    /// counts as success, so a retry after a partial failure completes.
    pub async fn delete_user(&self, hanko_user_id: &str) -> anyhow::Result<()> {
        let mut url = reqwest::Url::parse(&self.base)?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Hanko base URL can't hold path segments"))?
            .extend(["users", hanko_user_id]);
        let response = self
            .http
            .delete(url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let status = response.status();
        if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            anyhow::bail!("Hanko returned {status} deleting the user")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn deletes_with_the_bearer_key_and_treats_404_as_done() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/admin/users/u-1"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/admin/users/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/admin/users/boom"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let admin = HankoAdmin::new(&format!("{}/", server.uri()), "secret").unwrap();
        assert!(admin.delete_user("u-1").await.is_ok());
        assert!(
            admin.delete_user("gone").await.is_ok(),
            "404 = already deleted"
        );
        assert!(admin.delete_user("boom").await.is_err());
    }

    #[tokio::test]
    async fn ids_are_path_encoded_not_interpreted() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/admin/users/a%2Fb%3Fc|d"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let admin = HankoAdmin::new(&server.uri(), "k").unwrap();
        assert!(admin.delete_user("a/b?c|d").await.is_ok());
    }
}
