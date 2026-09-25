//! Avatar uploads to Azure Blob Storage.
//!
//! The app never sends image bytes through this API. `POST /me/avatar-upload`
//! mints a short-lived, write-only SAS URL for one blob under the caller's own
//! prefix; the app `PUT`s the (already resized) JPEG straight to Blob Storage,
//! then saves the resulting public URL with `PATCH /me { avatar_url }`, which
//! re-checks that the URL points at the caller's own prefix (see
//! [`AvatarStorage::owns_avatar_url`]).
//!
//! SAS tokens are signed locally with the storage account key (HMAC-SHA256);
//! no Azure SDK involved. Format: "Create a service SAS" in the Azure Storage
//! REST docs, `sv=2022-11-02`.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

const SAS_VERSION: &str = "2022-11-02";
/// How long the app has to complete the `PUT` after asking for the URL.
const UPLOAD_TTL: Duration = Duration::minutes(10);

#[derive(Debug, Clone)]
pub struct AvatarStorage {
    account: String,
    key: Vec<u8>,
    container: String,
    /// Where blobs are served from, no trailing slash — normally
    /// `https://<account>.blob.core.windows.net`; Azurite's path-style
    /// `http://127.0.0.1:10000/devstoreaccount1` for local dev.
    endpoint: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AvatarUpload {
    /// SAS URL to `PUT` the image to (needs `x-ms-blob-type: BlockBlob`).
    pub upload_url: String,
    /// Public URL the image will be served from once uploaded.
    pub avatar_url: String,
    pub expires_at: DateTime<Utc>,
}

impl AvatarStorage {
    pub fn new(
        account: impl Into<String>,
        key_base64: &str,
        container: impl Into<String>,
        endpoint: Option<String>,
    ) -> anyhow::Result<Self> {
        let account = account.into();
        let endpoint = endpoint
            .map(|e| e.trim_end_matches('/').to_string())
            .unwrap_or_else(|| format!("https://{account}.blob.core.windows.net"));
        Ok(Self {
            key: B64.decode(key_base64.trim())?,
            container: container.into(),
            account,
            endpoint,
        })
    }

    /// `None` when `AZURE_STORAGE_ACCOUNT` / `AZURE_STORAGE_KEY` aren't set
    /// (local dev without storage: the upload route answers 503).
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let (Ok(account), Ok(key)) = (
            std::env::var("AZURE_STORAGE_ACCOUNT"),
            std::env::var("AZURE_STORAGE_KEY"),
        ) else {
            return Ok(None);
        };
        let container =
            std::env::var("AZURE_STORAGE_CONTAINER").unwrap_or_else(|_| "avatars".into());
        let endpoint = std::env::var("AZURE_STORAGE_BLOB_ENDPOINT")
            .ok()
            .filter(|e| !e.is_empty());
        Self::new(account, &key, container, endpoint).map(Some)
    }

    /// Public URL prefix under which one user's avatars live. `key` is the user's
    /// random `avatar_key`, deliberately **not** their user id: the photo URL is
    /// shown to friends and in the public invite preview, so the id must not be
    /// derivable from it.
    fn user_prefix(&self, key: Uuid) -> String {
        format!("{}/{}/{}/", self.endpoint, self.container, key)
    }

    /// True only for `<prefix>/<uuid>.jpg` under this user's own prefix — the
    /// exact shape `create_upload` hands out, so a client can't point their
    /// profile at someone else's blob or an arbitrary external URL.
    pub fn owns_avatar_url(&self, key: Uuid, url: &str) -> bool {
        url.strip_prefix(&self.user_prefix(key))
            .and_then(|rest| rest.strip_suffix(".jpg"))
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
    }

    pub fn create_upload(&self, key: Uuid) -> AvatarUpload {
        self.create_upload_at(key, Uuid::new_v4(), Utc::now())
    }

    fn create_upload_at(&self, key: Uuid, blob_id: Uuid, now: DateTime<Utc>) -> AvatarUpload {
        let blob = format!("{key}/{blob_id}.jpg");
        let avatar_url = format!("{}/{}/{}", self.endpoint, self.container, blob);
        let expires_at = now + UPLOAD_TTL;
        let se = expires_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        // Force https on the real service; Azurite (local dev) is plain http.
        let spr = if self.endpoint.starts_with("https://") {
            "https"
        } else {
            "https,http"
        };

        // Order matters: this is the exact string Azure recomputes.
        // permissions (create+write), start, expiry, resource, identifier, IP,
        // protocol, version, resource type (blob), snapshot time, encryption
        // scope, then the five response-header overrides (all empty).
        let string_to_sign = format!(
            "cw\n\n{se}\n/blob/{}/{}/{blob}\n\n\n{spr}\n{SAS_VERSION}\nb\n\n\n\n\n\n\n",
            self.account, self.container
        );
        let sig = self.sign(&string_to_sign);

        let mut url = reqwest::Url::parse(&avatar_url).expect("endpoint is a valid URL");
        url.query_pairs_mut()
            .append_pair("sv", SAS_VERSION)
            .append_pair("spr", spr)
            .append_pair("se", &se)
            .append_pair("sr", "b")
            .append_pair("sp", "cw")
            .append_pair("sig", &sig);

        AvatarUpload {
            upload_url: url.into(),
            avatar_url,
            expires_at,
        }
    }

    fn sign(&self, string_to_sign: &str) -> String {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key)
            .expect("HMAC accepts any key length");
        mac.update(string_to_sign.as_bytes());
        B64.encode(mac.finalize().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> AvatarStorage {
        AvatarStorage::new(
            "acct",
            &B64.encode(b"not-a-real-key"),
            "avatars",
            Some("https://acct.blob.core.windows.net/".into()),
        )
        .unwrap()
    }

    #[test]
    fn upload_url_is_scoped_to_the_users_prefix_and_write_only() {
        let (user, blob) = (Uuid::new_v4(), Uuid::new_v4());
        let up = storage().create_upload_at(user, blob, Utc::now());

        assert_eq!(
            up.avatar_url,
            format!("https://acct.blob.core.windows.net/avatars/{user}/{blob}.jpg")
        );
        assert!(up.upload_url.starts_with(&up.avatar_url));
        assert!(up.upload_url.contains("sp=cw"), "create+write only");
        assert!(up.upload_url.contains("sr=b"));
        assert!(
            up.upload_url.contains("spr=https&"),
            "https only on real Azure"
        );
        assert!(up.upload_url.contains("sig="));
    }

    #[test]
    fn signature_is_deterministic_for_fixed_inputs() {
        let (user, blob) = (Uuid::nil(), Uuid::nil());
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let a = storage().create_upload_at(user, blob, now);
        let b = storage().create_upload_at(user, blob, now);
        assert_eq!(a, b);
        assert!(a.upload_url.contains("se=2026-01-01T00%3A10%3A00Z"));
    }

    #[test]
    fn only_own_generated_urls_are_accepted() {
        let s = storage();
        let (me, other) = (Uuid::new_v4(), Uuid::new_v4());
        let mine = s.create_upload(me).avatar_url;

        assert!(s.owns_avatar_url(me, &mine));
        assert!(!s.owns_avatar_url(other, &mine), "someone else's prefix");
        assert!(!s.owns_avatar_url(me, "https://evil.example/x.jpg"));
        assert!(
            !s.owns_avatar_url(me, &format!("{mine}?sig=x")),
            "no query string"
        );
        assert!(!s.owns_avatar_url(me, &mine.replace(".jpg", ".html")));
        assert!(!s.owns_avatar_url(
            me,
            &format!("https://acct.blob.core.windows.net/avatars/{me}/../{other}/x.jpg")
        ));
    }
}
