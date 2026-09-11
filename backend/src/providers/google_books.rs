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
    let url = format!("{BASE}/volumes");
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
    let url = format!("{BASE}/volumes/{volume_id}");
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
