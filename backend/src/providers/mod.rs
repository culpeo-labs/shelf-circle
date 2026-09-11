//! Book data providers: wrap external catalog APIs (Open Library, Google Books),
//! normalize their responses into our own shapes, and merge across sources.
//!
//! Open Library is always on (no key needed). Google Books is only queried when
//! `GOOGLE_BOOKS_API_KEY` is set — keyless access now returns HTTP 429.

mod google_books;
mod open_library;

use serde::Serialize;

use crate::models::ResolvedBook;

/// A single hit from a book search, normalized across providers. Work-level:
/// one entry per logical book, not per edition/translation. Not persisted —
/// the client turns one of these into a saved `Book` via `POST /books/resolve`.
#[derive(Debug, Clone, Serialize)]
pub struct BookSearchResult {
    pub source: &'static str,
    /// Provider id to hand back to `/books/resolve` (Open Library work id, or
    /// Google Books volume id).
    pub source_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub first_publish_year: Option<i32>,
    pub cover_image_url: Option<String>,
    /// Best-effort BCP-47 language tags known for this work.
    pub languages: Vec<String>,
    pub open_library_work_id: Option<String>,
    pub google_books_volume_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("book provider request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("book provider returned {status} for {url}")]
    Status {
        status: reqwest::StatusCode,
        url: String,
    },
    #[error("no book found for {provider} id '{source_id}'")]
    NotFound {
        provider: &'static str,
        source_id: String,
    },
    #[error("unknown book source '{0}' (expected 'open_library' or 'google_books')")]
    UnknownSource(String),
}

/// Fans search / resolve-by-reference out to the configured providers.
#[derive(Clone)]
pub struct BookProviders {
    http: reqwest::Client,
    google_books_key: Option<String>,
}

impl BookProviders {
    pub fn from_env() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("shelf-circle-backend/0.1 (+https://github.com/culpeo-labs/shelf-circle)")
            .build()
            .expect("failed to build reqwest client");

        let google_books_key = std::env::var("GOOGLE_BOOKS_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());

        if google_books_key.is_some() {
            tracing::info!("book providers: Open Library + Google Books");
        } else {
            tracing::info!("book providers: Open Library only (set GOOGLE_BOOKS_API_KEY to enable Google Books)");
        }

        Self {
            http,
            google_books_key,
        }
    }

    /// Search every configured provider and merge the results. Open Library is
    /// authoritative for ordering; Google Books hits that don't dedupe against
    /// an Open Library entry are appended.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<BookSearchResult>, ProviderError> {
        let mut results = open_library::search(&self.http, query, limit).await?;

        if let Some(key) = &self.google_books_key {
            match google_books::search(&self.http, key, query, limit).await {
                Ok(more) => merge(&mut results, more),
                // A second-source failure shouldn't fail the whole search.
                Err(e) => tracing::warn!(
                    "google books search failed, continuing with open library only: {e}"
                ),
            }
        }

        results.truncate(limit);
        Ok(results)
    }

    /// Fetch one book by provider reference and normalize it into the shape
    /// `/books/resolve`'s upsert logic already understands.
    pub async fn resolve(
        &self,
        source: &str,
        source_id: &str,
    ) -> Result<ResolvedBook, ProviderError> {
        match source {
            "open_library" => open_library::resolve(&self.http, source_id).await,
            "google_books" => {
                let key = self.google_books_key.as_deref().ok_or_else(|| {
                    ProviderError::UnknownSource(
                        "google_books (GOOGLE_BOOKS_API_KEY not set)".to_string(),
                    )
                })?;
                google_books::resolve(&self.http, key, source_id).await
            }
            other => Err(ProviderError::UnknownSource(other.to_string())),
        }
    }
}

/// Append `extra` entries that don't already appear in `into`, matched loosely
/// on (lowercased title, lowercased first author).
fn merge(into: &mut Vec<BookSearchResult>, extra: Vec<BookSearchResult>) {
    fn key(r: &BookSearchResult) -> (String, String) {
        (
            r.title.to_lowercase(),
            r.authors
                .first()
                .map(|a| a.to_lowercase())
                .unwrap_or_default(),
        )
    }

    let seen: std::collections::HashSet<(String, String)> = into.iter().map(key).collect();
    for r in extra {
        if !seen.contains(&key(&r)) {
            into.push(r);
        }
    }
}

/// MARC / ISO-639-2 language codes (as Open Library reports them) to BCP-47.
/// Unrecognized codes pass through unchanged; `und` ("undetermined") is dropped
/// by callers.
pub(crate) fn normalize_language(code: &str) -> String {
    match code.to_lowercase().as_str() {
        "eng" | "en" => "en",
        "spa" | "es" => "es",
        "fre" | "fra" | "fr" => "fr",
        "ger" | "deu" | "de" => "de",
        "por" | "pt" => "pt",
        "ita" | "it" => "it",
        "dut" | "nld" | "nl" => "nl",
        "rus" | "ru" => "ru",
        "jpn" | "ja" => "ja",
        "chi" | "zho" | "zh" => "zh",
        "gre" | "ell" | "el" => "el",
        "rum" | "ron" | "ro" => "ro",
        "ukr" | "uk" => "uk",
        "pol" | "pl" => "pl",
        "vie" | "vi" => "vi",
        "heb" | "he" => "he",
        "ara" | "ar" => "ar",
        "swe" | "sv" => "sv",
        "nor" | "nob" | "no" => "no",
        "dan" | "da" => "da",
        "fin" | "fi" => "fi",
        "cze" | "ces" | "cs" => "cs",
        "tur" | "tr" => "tr",
        "kor" | "ko" => "ko",
        other => return other.to_string(),
    }
    .to_string()
}
