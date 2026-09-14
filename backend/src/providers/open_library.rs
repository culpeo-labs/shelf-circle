//! Open Library provider. Search: <https://openlibrary.org/dev/docs/api/search>.
//! No API key required.

use serde::Deserialize;

use super::{normalize_language, BookSearchResult, ProviderError};
use crate::models::ResolvedBook;

const BASE: &str = "https://openlibrary.org";
const SOURCE: &str = "open_library";

fn cover_url(cover_id: i64) -> String {
    format!("https://covers.openlibrary.org/b/id/{cover_id}-L.jpg")
}

fn strip_prefix<'a>(key: &'a str, prefix: &str) -> &'a str {
    key.strip_prefix(prefix).unwrap_or(key)
}

fn dedupe(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

// ---------- search ----------

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    docs: Vec<SearchDoc>,
}

#[derive(Debug, Deserialize)]
struct SearchDoc {
    key: String, // "/works/OL...W"
    #[serde(default)]
    title: String,
    #[serde(default)]
    author_name: Vec<String>,
    first_publish_year: Option<i32>,
    cover_i: Option<i64>,
    #[serde(default)]
    language: Vec<String>,
}

pub async fn search(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<BookSearchResult>, ProviderError> {
    search_at(http, BASE, query, limit).await
}

/// `search`'s actual implementation, taking the base URL as a parameter so
/// tests can point it at a mock server instead of the real Open Library.
async fn search_at(
    http: &reqwest::Client,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<BookSearchResult>, ProviderError> {
    let url = format!("{base}/search.json");
    let resp = http
        .get(&url)
        .query(&[
            ("q", query),
            ("limit", &limit.to_string()),
            (
                "fields",
                "key,title,author_name,first_publish_year,cover_i,language",
            ),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(ProviderError::Status {
            status: resp.status(),
            url,
        });
    }

    let body: SearchResponse = resp.json().await?;

    Ok(body
        .docs
        .into_iter()
        .filter(|d| !d.title.is_empty())
        .map(|d| {
            let work_id = strip_prefix(&d.key, "/works/").to_string();
            BookSearchResult {
                source: SOURCE,
                source_id: work_id.clone(),
                title: d.title,
                authors: d.author_name,
                first_publish_year: d.first_publish_year,
                cover_image_url: d.cover_i.map(cover_url),
                languages: dedupe(
                    d.language
                        .iter()
                        .map(|c| normalize_language(c))
                        .filter(|l| !l.is_empty() && l != "und")
                        .collect(),
                ),
                open_library_work_id: Some(work_id),
                google_books_volume_id: None,
            }
        })
        .collect())
}

// ---------- resolve by work id ----------

#[derive(Debug, Deserialize)]
struct Work {
    #[serde(default)]
    title: String,
    #[serde(default)]
    authors: Vec<WorkAuthor>,
    #[serde(default)]
    covers: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct WorkAuthor {
    author: KeyRef,
}

#[derive(Debug, Clone, Deserialize)]
struct KeyRef {
    key: String,
}

#[derive(Debug, Deserialize)]
struct AuthorDoc {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EditionsResponse {
    #[serde(default)]
    entries: Vec<Edition>,
}

#[derive(Debug, Clone, Deserialize)]
struct Edition {
    title: Option<String>,
    #[serde(default)]
    isbn_13: Vec<String>,
    #[serde(default)]
    isbn_10: Vec<String>,
    #[serde(default)]
    publishers: Vec<String>,
    #[serde(default)]
    languages: Vec<KeyRef>,
    #[serde(default)]
    covers: Vec<i64>,
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
) -> Result<Option<T>, ProviderError> {
    let resp = http.get(url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(ProviderError::Status {
            status: resp.status(),
            url: url.to_string(),
        });
    }
    Ok(Some(resp.json().await?))
}

pub async fn resolve(http: &reqwest::Client, work_id: &str) -> Result<ResolvedBook, ProviderError> {
    resolve_at(http, BASE, work_id).await
}

/// `resolve`'s actual implementation, taking the base URL as a parameter so
/// tests can point it at a mock server instead of the real Open Library.
async fn resolve_at(
    http: &reqwest::Client,
    base: &str,
    work_id: &str,
) -> Result<ResolvedBook, ProviderError> {
    let work_id = strip_prefix(work_id, "/works/").trim();

    let work: Work = get_json(http, &format!("{base}/works/{work_id}.json"))
        .await?
        .ok_or_else(|| ProviderError::NotFound {
            provider: SOURCE,
            source_id: work_id.to_string(),
        })?;

    // First author's display name (one extra call).
    let primary_author = match work.authors.first() {
        Some(a) => {
            let key = strip_prefix(&a.author.key, "/authors/");
            get_json::<AuthorDoc>(http, &format!("{base}/authors/{key}.json"))
                .await
                .ok()
                .flatten()
                .and_then(|d| d.name)
        }
        None => None,
    };

    // Representative edition for language / ISBN / publisher. Open Library
    // returns editions in no useful order, so pull a batch and prefer an
    // English one, then any with an ISBN-13, then whatever's first.
    let editions = get_json::<EditionsResponse>(
        http,
        &format!("{base}/works/{work_id}/editions.json?limit=50"),
    )
    .await?
    .map(|r| r.entries)
    .unwrap_or_default();

    fn is_english(e: &Edition) -> bool {
        e.languages
            .iter()
            .any(|l| matches!(strip_prefix(&l.key, "/languages/"), "eng" | "en"))
    }

    let edition = editions
        .iter()
        .find(|e| is_english(e))
        .or_else(|| editions.iter().find(|e| !e.isbn_13.is_empty()))
        .or_else(|| editions.first())
        .cloned();

    let language = edition
        .as_ref()
        .and_then(|e| e.languages.first())
        .map(|l| normalize_language(strip_prefix(&l.key, "/languages/")))
        .unwrap_or_else(|| "en".to_string());

    let cover_id = work
        .covers
        .iter()
        .chain(edition.as_ref().map(|e| e.covers.as_slice()).unwrap_or(&[]))
        .find(|id| **id > 0)
        .copied();

    let edition_title = edition
        .as_ref()
        .and_then(|e| e.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| work.title.clone());

    Ok(ResolvedBook {
        canonical_title: work.title,
        primary_author,
        language,
        isbn_13: edition.as_ref().and_then(|e| e.isbn_13.first().cloned()),
        isbn_10: edition.as_ref().and_then(|e| e.isbn_10.first().cloned()),
        edition_title,
        publisher: edition.as_ref().and_then(|e| e.publishers.first().cloned()),
        cover_image_url: cover_id.map(cover_url),
        source: SOURCE.to_string(),
        source_id: work_id.to_string(),
        open_library_work_id: Some(work_id.to_string()),
        google_books_volume_id: None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn search_normalizes_and_dedupes_languages_and_drops_untitled_docs() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "docs": [
                    {
                        "key": "/works/OL1W",
                        "title": "Dune",
                        "author_name": ["Frank Herbert"],
                        "first_publish_year": 1965,
                        "cover_i": 123,
                        "language": ["eng", "eng", "spa"]
                    },
                    // No title -> Open Library sometimes returns stub docs;
                    // these aren't real search hits and must be dropped.
                    { "key": "/works/OL2W", "title": "" }
                ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let results = search_at(&http, &server.uri(), "dune", 20).await.unwrap();

        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.source, "open_library");
        assert_eq!(r.source_id, "OL1W", "the /works/ prefix is stripped");
        assert_eq!(r.open_library_work_id.as_deref(), Some("OL1W"));
        assert_eq!(r.title, "Dune");
        assert_eq!(r.authors, vec!["Frank Herbert"]);
        assert_eq!(r.first_publish_year, Some(1965));
        assert_eq!(
            r.cover_image_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/123-L.jpg")
        );
        assert_eq!(r.languages, vec!["en", "es"], "normalized and deduped");
    }

    #[tokio::test]
    async fn search_surfaces_non_success_status_as_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = search_at(&http, &server.uri(), "dune", 20)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProviderError::Status { status, .. } if status == 503
        ));
    }

    #[tokio::test]
    async fn resolve_prefers_the_english_edition_and_fills_in_author_and_cover() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/works/OL1W.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "title": "Dune",
                "authors": [{ "author": { "key": "/authors/OL1A" } }],
                "covers": []
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/authors/OL1A.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "name": "Frank Herbert" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/works/OL1W/editions.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "entries": [
                    {
                        "title": "Dune (French)",
                        "isbn_13": ["9782000000000"],
                        "languages": [{ "key": "/languages/fre" }]
                    },
                    {
                        "title": "Dune",
                        "isbn_13": ["9780000000001"],
                        "isbn_10": ["0000000001"],
                        "publishers": ["Ace Books"],
                        "languages": [{ "key": "/languages/eng" }],
                        "covers": [111]
                    }
                ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let book = resolve_at(&http, &server.uri(), "/works/OL1W")
            .await
            .unwrap();

        assert_eq!(book.canonical_title, "Dune");
        assert_eq!(book.primary_author.as_deref(), Some("Frank Herbert"));
        assert_eq!(
            book.language, "en",
            "the English edition is preferred over the first one"
        );
        assert_eq!(book.edition_title, "Dune");
        assert_eq!(book.isbn_13.as_deref(), Some("9780000000001"));
        assert_eq!(book.publisher.as_deref(), Some("Ace Books"));
        assert_eq!(
            book.cover_image_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/111-L.jpg"),
            "falls back to the edition's cover when the work has none"
        );
        assert_eq!(book.source, "open_library");
        assert_eq!(book.source_id, "OL1W");
    }

    #[tokio::test]
    async fn resolve_returns_not_found_for_a_missing_work() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/works/OL404W.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = resolve_at(&http, &server.uri(), "OL404W")
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProviderError::NotFound { provider: "open_library", source_id } if source_id == "OL404W"
        ));
    }
}
