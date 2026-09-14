//! Google Books provider. <https://developers.google.com/books/docs/v1/using>.
//! Requires an API key — keyless access returns HTTP 429 (quota 0/day).

use serde::Deserialize;

use super::{normalize_language, BookSearchResult, ProviderError};
use crate::models::ResolvedBook;

const BASE: &str = "https://www.googleapis.com/books/v1";
const SOURCE: &str = "google_books";

#[derive(Debug, Deserialize)]
struct VolumeList {
    #[serde(default)]
    items: Vec<Volume>,
}

#[derive(Debug, Deserialize)]
struct Volume {
    id: String,
    #[serde(rename = "volumeInfo", default)]
    volume_info: VolumeInfo,
}

#[derive(Debug, Default, Deserialize)]
struct VolumeInfo {
    #[serde(default)]
    title: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(rename = "publishedDate")]
    published_date: Option<String>,
    publisher: Option<String>,
    language: Option<String>,
    #[serde(rename = "imageLinks", default)]
    image_links: ImageLinks,
    #[serde(rename = "industryIdentifiers", default)]
    industry_identifiers: Vec<IndustryIdentifier>,
}

#[derive(Debug, Default, Deserialize)]
struct ImageLinks {
    thumbnail: Option<String>,
    #[serde(rename = "smallThumbnail")]
    small_thumbnail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IndustryIdentifier {
    #[serde(rename = "type")]
    kind: String,
    identifier: String,
}

fn year_of(date: &Option<String>) -> Option<i32> {
    date.as_deref()
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse().ok())
}

fn https(url: Option<String>) -> Option<String> {
    url.map(|u| u.replacen("http://", "https://", 1))
}

impl VolumeInfo {
    fn isbn(&self, kind: &str) -> Option<String> {
        self.industry_identifiers
            .iter()
            .find(|i| i.kind == kind)
            .map(|i| i.identifier.clone())
    }

    fn cover(&self) -> Option<String> {
        https(
            self.image_links
                .thumbnail
                .clone()
                .or_else(|| self.image_links.small_thumbnail.clone()),
        )
    }
}

pub async fn search(
    http: &reqwest::Client,
    key: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<BookSearchResult>, ProviderError> {
    search_at(http, BASE, key, query, limit).await
}

/// `search`'s actual implementation, taking the base URL as a parameter so
/// tests can point it at a mock server instead of the real Google Books.
async fn search_at(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<BookSearchResult>, ProviderError> {
    let url = format!("{base}/volumes");
    let max_results = limit.clamp(1, 40).to_string();
    let resp = http
        .get(&url)
        .query(&[
            ("q", query),
            ("maxResults", &max_results),
            ("printType", "books"),
            ("key", key),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(ProviderError::Status {
            status: resp.status(),
            url,
        });
    }

    let body: VolumeList = resp.json().await?;

    Ok(body
        .items
        .into_iter()
        .filter(|v| !v.volume_info.title.is_empty())
        .map(|v| {
            let info = v.volume_info;
            BookSearchResult {
                source: SOURCE,
                source_id: v.id.clone(),
                first_publish_year: year_of(&info.published_date),
                cover_image_url: info.cover(),
                languages: info
                    .language
                    .as_deref()
                    .map(|l| vec![normalize_language(l)])
                    .unwrap_or_default(),
                title: info.title,
                authors: info.authors,
                open_library_work_id: None,
                google_books_volume_id: Some(v.id),
            }
        })
        .collect())
}

pub async fn resolve(
    http: &reqwest::Client,
    key: &str,
    volume_id: &str,
) -> Result<ResolvedBook, ProviderError> {
    resolve_at(http, BASE, key, volume_id).await
}

/// `resolve`'s actual implementation, taking the base URL as a parameter so
/// tests can point it at a mock server instead of the real Google Books.
async fn resolve_at(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    volume_id: &str,
) -> Result<ResolvedBook, ProviderError> {
    let url = format!("{base}/volumes/{volume_id}");
    let resp = http.get(&url).query(&[("key", key)]).send().await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ProviderError::NotFound {
            provider: SOURCE,
            source_id: volume_id.to_string(),
        });
    }
    if !resp.status().is_success() {
        return Err(ProviderError::Status {
            status: resp.status(),
            url,
        });
    }

    let volume: Volume = resp.json().await?;
    let info = volume.volume_info;

    Ok(ResolvedBook {
        language: info
            .language
            .as_deref()
            .map(normalize_language)
            .unwrap_or_else(|| "en".to_string()),
        isbn_13: info.isbn("ISBN_13"),
        isbn_10: info.isbn("ISBN_10"),
        edition_title: info.title.clone(),
        publisher: info.publisher.clone(),
        cover_image_url: info.cover(),
        canonical_title: info.title,
        primary_author: info.authors.into_iter().next(),
        source: SOURCE.to_string(),
        source_id: volume.id.clone(),
        open_library_work_id: None,
        google_books_volume_id: Some(volume.id),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn search_maps_fields_and_upgrades_thumbnails_to_https() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [
                    {
                        "id": "abc123",
                        "volumeInfo": {
                            "title": "Dune",
                            "authors": ["Frank Herbert"],
                            "publishedDate": "1965-08-01",
                            "language": "en",
                            "imageLinks": { "thumbnail": "http://books.google.com/thumb.jpg" }
                        }
                    },
                    // No title -> not a usable hit, must be dropped.
                    { "id": "notitle", "volumeInfo": {} }
                ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let results = search_at(&http, &server.uri(), "test-key", "dune", 20)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.source, "google_books");
        assert_eq!(r.source_id, "abc123");
        assert_eq!(r.google_books_volume_id.as_deref(), Some("abc123"));
        assert_eq!(r.title, "Dune");
        assert_eq!(r.first_publish_year, Some(1965));
        assert_eq!(r.languages, vec!["en"]);
        assert_eq!(
            r.cover_image_url.as_deref(),
            Some("https://books.google.com/thumb.jpg"),
            "http thumbnail links are upgraded to https"
        );
    }

    #[tokio::test]
    async fn resolve_prefers_isbn_13_and_falls_back_to_small_thumbnail() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "abc123",
                "volumeInfo": {
                    "title": "Dune",
                    "authors": ["Frank Herbert"],
                    "publisher": "Ace Books",
                    "language": "en",
                    "imageLinks": { "smallThumbnail": "http://books.google.com/small.jpg" },
                    "industryIdentifiers": [
                        { "type": "ISBN_10", "identifier": "0000000001" },
                        { "type": "ISBN_13", "identifier": "9780000000001" }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let book = resolve_at(&http, &server.uri(), "test-key", "abc123")
            .await
            .unwrap();

        assert_eq!(book.canonical_title, "Dune");
        assert_eq!(book.primary_author.as_deref(), Some("Frank Herbert"));
        assert_eq!(book.isbn_13.as_deref(), Some("9780000000001"));
        assert_eq!(book.isbn_10.as_deref(), Some("0000000001"));
        assert_eq!(
            book.cover_image_url.as_deref(),
            Some("https://books.google.com/small.jpg")
        );
        assert_eq!(book.google_books_volume_id.as_deref(), Some("abc123"));
    }

    #[tokio::test]
    async fn resolve_returns_not_found_for_a_missing_volume() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = resolve_at(&http, &server.uri(), "test-key", "missing")
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProviderError::NotFound { provider: "google_books", source_id } if source_id == "missing"
        ));
    }

    #[tokio::test]
    async fn search_surfaces_non_success_status_as_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = search_at(&http, &server.uri(), "test-key", "dune", 20)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProviderError::Status { status, .. } if status == 429
        ));
    }
}
