//! Library catalogs: "is this book in my library, and where do I get it?"
//!
//! A [`LibrarySystem`] is a registry entry the user can pick (Seattle Public
//! Library, King County Library System, …) that names *how* to talk to its
//! catalog ([`Kind`]) plus that kind's config. Adding a library that runs on
//! an existing kind is one line in [`SYSTEMS`]; adding a new kind of catalog
//! (Libby/OverDrive, Sierra, …) is a new module here + a `Kind` variant + an
//! arm in [`Catalogs::lookup_isbn`]. Each kind only has to answer one
//! question — "record link for this ISBN, if the catalog has it".
//!
//! The endpoints behind these (BiblioCommons' gateway) are the ones the
//! libraries' own websites use; they're unauthenticated but unofficial, so
//! callers must treat every lookup as allowed to fail (see the route).

mod biblio_commons;
mod matching;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// How a system's catalog is reached, with that kind's config.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// A BiblioCommons-hosted catalog: `slug` selects the library on the
    /// gateway API, `host` is its public site (record/search links).
    BiblioCommons {
        slug: &'static str,
        host: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct LibrarySystem {
    /// Stable id stored on `users.library_system` — never rename.
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
}

/// Every library a user can choose. Order is display order.
pub const SYSTEMS: &[LibrarySystem] = &[
    LibrarySystem {
        id: "seattle",
        name: "Seattle Public Library",
        kind: Kind::BiblioCommons {
            slug: "seattle",
            host: "seattle.bibliocommons.com",
        },
    },
    LibrarySystem {
        id: "kcls",
        name: "King County Library System",
        kind: Kind::BiblioCommons {
            slug: "kcls",
            host: "kcls.bibliocommons.com",
        },
    },
];

pub fn system(id: &str) -> Option<&'static LibrarySystem> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// What we know about a book, for finding it in a catalog. Title + author
/// identify the *work*; ISBNs (of the editions we have on file) only help pick
/// the exact edition or catch retitled ones.
#[derive(Debug, Clone, Copy)]
pub struct BookQuery<'a> {
    pub title: &'a str,
    pub author: Option<&'a str>,
    /// BCP-47-ish tag ("en"); records in other languages are skipped.
    pub language: Option<&'a str>,
    /// Normalized (see [`normalize_isbn`]), ISBN-13s first.
    pub isbns: &'a [String],
}

impl BookQuery<'_> {
    fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.title.trim().to_lowercase(),
            self.author.unwrap_or("").trim().to_lowercase(),
            self.language.unwrap_or(""),
            self.isbns.join(",")
        )
    }
}

/// Where a catalog says the book lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogMatch {
    pub url: String,
    /// The catalog record's title (for logs/tests; not shown to users).
    pub title: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("catalog returned {0}")]
    Status(reqwest::StatusCode),
}

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const CACHE_MAX_ENTRIES: usize = 5_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const DEFAULT_BIBLIOCOMMONS_GATEWAY: &str = "https://gateway.bibliocommons.com";

type CacheKey = (&'static str, String);

/// Catalog lookups with a small in-process cache. A holdings record rarely
/// moves, and every book-page view would otherwise hit the library's servers.
/// Only successful lookups are cached (including "not in the catalog"); a
/// failed one is retried next time.
pub struct Catalogs {
    http: reqwest::Client,
    biblio_commons_gateway: String,
    cache: Mutex<HashMap<CacheKey, (Instant, Option<CatalogMatch>)>>,
}

impl Catalogs {
    pub fn from_env() -> Self {
        Self::with_biblio_commons_gateway(DEFAULT_BIBLIOCOMMONS_GATEWAY)
    }

    /// Point the BiblioCommons plugin at another gateway (tests use a mock).
    pub fn with_biblio_commons_gateway(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("shelf-circle-backend/0.1 (+https://github.com/culpeo-labs/shelf-circle)")
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client");
        Self {
            http,
            biblio_commons_gateway: base.into().trim_end_matches('/').to_string(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The catalog's record page for this book, or `None` if the catalog
    /// doesn't appear to have it. Cached per (system, book).
    pub async fn find_book(
        &self,
        system: &'static LibrarySystem,
        book: &BookQuery<'_>,
    ) -> Result<Option<CatalogMatch>, CatalogError> {
        let key = (system.id, book.cache_key());

        if let Some((at, hit)) = self.cache.lock().unwrap().get(&key) {
            if at.elapsed() < CACHE_TTL {
                return Ok(hit.clone());
            }
        }

        let found = match system.kind {
            Kind::BiblioCommons { slug, host } => {
                biblio_commons::find(&self.http, &self.biblio_commons_gateway, slug, host, book)
                    .await?
            }
        };

        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), found.clone()));
        Ok(found)
    }
}

/// A catalog-search link for when there's no exact record (no ISBN on file, no
/// match, or the lookup failed) — still lands the user on the right catalog.
pub fn search_url(system: &LibrarySystem, title: &str, author: Option<&str>) -> String {
    let query = match author {
        Some(a) if !a.trim().is_empty() => format!("{title} {a}"),
        _ => title.to_string(),
    };
    match system.kind {
        Kind::BiblioCommons { host, .. } => biblio_commons::search_url(host, &query),
    }
}

/// Digits and a trailing `X` only, upper-cased ("978-0-593-13520-4" → digits).
pub fn normalize_isbn(isbn: &str) -> String {
    isbn.chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, 'x' | 'X'))
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

#[derive(Debug, Serialize)]
pub struct LibrarySystemSummary {
    pub id: &'static str,
    pub name: &'static str,
}

impl From<&LibrarySystem> for LibrarySystemSummary {
    fn from(s: &LibrarySystem) -> Self {
        Self {
            id: s.id,
            name: s.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_lookup_works() {
        let mut ids: Vec<_> = SYSTEMS.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SYSTEMS.len());
        assert_eq!(system("kcls").unwrap().name, "King County Library System");
        assert!(system("nope").is_none());
    }

    #[test]
    fn isbns_are_normalized() {
        assert_eq!(normalize_isbn("978-0-593-13520-4"), "9780593135204");
        assert_eq!(normalize_isbn(" 0-8044-2957-x "), "080442957X");
    }

    #[test]
    fn search_url_encodes_title_and_author() {
        let s = system("seattle").unwrap();
        assert_eq!(
            search_url(s, "Project Hail Mary", Some("Andy Weir")),
            "https://seattle.bibliocommons.com/v2/search?query=Project%20Hail%20Mary%20Andy%20Weir&searchType=smart"
        );
        assert_eq!(
            search_url(s, "Dune & More", None),
            "https://seattle.bibliocommons.com/v2/search?query=Dune%20%26%20More&searchType=smart"
        );
    }
}
