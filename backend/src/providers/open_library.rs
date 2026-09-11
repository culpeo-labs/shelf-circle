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
    let url = format!("{BASE}/search.json");
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
    let work_id = strip_prefix(work_id, "/works/").trim();

    let work: Work = get_json(http, &format!("{BASE}/works/{work_id}.json"))
        .await?
        .ok_or_else(|| ProviderError::NotFound {
            provider: SOURCE,
            source_id: work_id.to_string(),
        })?;

    // First author's display name (one extra call).
    let primary_author = match work.authors.first() {
        Some(a) => {
            let key = strip_prefix(&a.author.key, "/authors/");
            get_json::<AuthorDoc>(http, &format!("{BASE}/authors/{key}.json"))
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
        &format!("{BASE}/works/{work_id}/editions.json?limit=50"),
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
