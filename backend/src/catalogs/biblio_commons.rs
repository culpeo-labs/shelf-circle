//! BiblioCommons catalogs (Seattle, King County, and hundreds more): search the
//! public gateway and link to the matching record.
//!
//! Strategy: search by title + author and accept records that are the same
//! *work* (see `matching`), preferring one that carries one of our ISBNs (the
//! exact edition), then a plain book over ebook/large print/etc. Only if that
//! finds nothing do we try our ISBNs on their own, which catches translated or
//! retitled editions. ISBN-first was tried and missed ~17 of 18 popular
//! titles: we store one representative edition and libraries hold others.

use std::collections::HashMap;

use serde::Deserialize;

use super::matching;
use super::{normalize_isbn, BookQuery, CatalogError, CatalogMatch};

/// Records per search; a title search returns every format of every edition.
const TITLE_SEARCH_LIMIT: &str = "25";
/// ISBNs tried on their own when the title search finds nothing.
const MAX_ISBN_FALLBACKS: usize = 2;

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    entities: Entities,
}

#[derive(Debug, Default, Deserialize)]
struct Entities {
    #[serde(default)]
    bibs: HashMap<String, Bib>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bib {
    id: String,
    brief_info: BriefInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BriefInfo {
    #[serde(default)]
    title: String,
    /// BiblioCommons format code: `BK` (book), `EBOOK`, `LPRINT`, …
    format: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    isbns: Vec<String>,
    primary_language: Option<String>,
}

impl Bib {
    fn has_isbn(&self, isbn: &str) -> bool {
        self.brief_info
            .isbns
            .iter()
            .any(|i| normalize_isbn(i) == isbn)
    }

    /// Lower is better: plain book, large print, ebook, then everything else.
    fn format_rank(&self) -> u8 {
        match self.brief_info.format.as_deref() {
            Some("BK") => 0,
            Some("LPRINT") => 1,
            Some("EBOOK") => 2,
            _ => 3,
        }
    }
}

pub async fn find(
    http: &reqwest::Client,
    gateway: &str,
    slug: &str,
    host: &str,
    book: &BookQuery<'_>,
) -> Result<Option<CatalogMatch>, CatalogError> {
    let text = match book.author {
        Some(a) if !a.trim().is_empty() => format!("{} {a}", matching::main_title(book.title)),
        _ => matching::main_title(book.title).to_string(),
    };
    let bibs = search(http, gateway, slug, &text, TITLE_SEARCH_LIMIT).await?;
    if let Some(bib) = best_work_match(bibs, book) {
        return Ok(Some(to_match(host, bib)));
    }

    for isbn in book.isbns.iter().take(MAX_ISBN_FALLBACKS) {
        let bibs = search(http, gateway, slug, isbn, "10").await?;
        if let Some(bib) = bibs
            .into_iter()
            .filter(|b| b.has_isbn(isbn))
            .min_by_key(sort_key)
        {
            return Ok(Some(to_match(host, bib)));
        }
    }
    Ok(None)
}

fn to_match(host: &str, bib: Bib) -> CatalogMatch {
    CatalogMatch {
        url: format!("https://{host}/v2/record/{}", bib.id),
        title: bib.brief_info.title,
    }
}

async fn search(
    http: &reqwest::Client,
    gateway: &str,
    slug: &str,
    query: &str,
    limit: &str,
) -> Result<Vec<Bib>, CatalogError> {
    let response = http
        .get(format!("{gateway}/v2/libraries/{slug}/bibs/search"))
        .query(&[("query", query), ("searchType", "smart"), ("limit", limit)])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(CatalogError::Status(response.status()));
    }
    let body: SearchResponse = response.json().await?;
    Ok(body.entities.bibs.into_values().collect())
}

fn sort_key(bib: &Bib) -> (u8, String) {
    (bib.format_rank(), bib.id.clone())
}

/// Records that are the same work as `book` (title, author when we have one,
/// and language). Among those, the exact edition (one of our ISBNs) wins, then
/// format, then id so the answer is stable.
fn best_work_match(bibs: Vec<Bib>, book: &BookQuery<'_>) -> Option<Bib> {
    bibs.into_iter()
        .filter(|b| matching::titles_match(book.title, &b.brief_info.title))
        .filter(|b| match book.author.filter(|a| !a.trim().is_empty()) {
            Some(a) => matching::author_matches(a, &b.brief_info.authors),
            None => true,
        })
        .filter(|b| {
            matching::languages_compatible(book.language, b.brief_info.primary_language.as_deref())
        })
        .min_by_key(|b| {
            let exact_edition = book.isbns.iter().any(|i| b.has_isbn(i));
            (!exact_edition, sort_key(b))
        })
}

pub fn search_url(host: &str, query: &str) -> String {
    let mut url = reqwest::Url::parse(&format!("https://{host}/v2/search"))
        .expect("catalog host is a valid host");
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("searchType", "smart");
    // `query_pairs_mut` form-encodes spaces as '+'; the catalog accepts either,
    // but %20 is what its own links use.
    url.as_str().replace('+', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bib(
        id: &str,
        format: &str,
        title: &str,
        authors: &[&str],
        isbns: &[&str],
        lang: &str,
    ) -> Bib {
        Bib {
            id: id.into(),
            brief_info: BriefInfo {
                title: title.into(),
                format: Some(format.into()),
                authors: authors.iter().map(|s| s.to_string()).collect(),
                isbns: isbns.iter().map(|s| s.to_string()).collect(),
                primary_language: Some(lang.into()),
            },
        }
    }

    fn query<'a>(isbns: &'a [String]) -> BookQuery<'a> {
        BookQuery {
            title: "Project Hail Mary",
            author: Some("Andy Weir"),
            language: Some("en"),
            isbns,
        }
    }

    #[test]
    fn matches_the_work_even_when_the_edition_differs() {
        // Our stored ISBN is a different printing than any the library holds.
        let ours = ["9781529000000".to_string()];
        let picked = best_work_match(
            vec![
                bib("S1", "DVD", "PROJECT HAIL MARY (DVD)", &[], &[], "eng"),
                bib(
                    "S2",
                    "EBOOK",
                    "Project Hail Mary",
                    &["Weir, Andy"],
                    &["9780593135211"],
                    "eng",
                ),
                bib(
                    "S3",
                    "BK",
                    "Project Hail Mary",
                    &["Weir, Andy"],
                    &["9780593135204"],
                    "eng",
                ),
                bib("S4", "BK", "Wan jiu ji hua", &["Weir, Andy"], &[], "chi"),
                bib(
                    "S5",
                    "BK",
                    "Project Hail Mary",
                    &["Someone, Else"],
                    &[],
                    "eng",
                ),
            ],
            &query(&ours),
        )
        .unwrap();
        assert_eq!(
            picked.id, "S3",
            "plain book by the right author, right language"
        );
    }

    #[test]
    fn prefers_the_exact_edition_over_a_better_format() {
        let ours = ["9780593135211".to_string()];
        let picked = best_work_match(
            vec![
                bib(
                    "S2",
                    "EBOOK",
                    "Project Hail Mary",
                    &["Weir, Andy"],
                    &["9780593135211"],
                    "eng",
                ),
                bib(
                    "S3",
                    "BK",
                    "Project Hail Mary",
                    &["Weir, Andy"],
                    &["9780593135204"],
                    "eng",
                ),
            ],
            &query(&ours),
        )
        .unwrap();
        assert_eq!(picked.id, "S2", "we own that edition's ISBN");
    }

    #[test]
    fn rejects_other_works_by_the_same_author() {
        let none: [String; 0] = [];
        assert!(best_work_match(
            vec![bib("S1", "BK", "Artemis", &["Weir, Andy"], &[], "eng")],
            &query(&none),
        )
        .is_none());
    }

    #[test]
    fn without_an_author_only_the_title_must_match() {
        let none: [String; 0] = [];
        let q = BookQuery {
            author: None,
            ..query(&none)
        };
        assert!(best_work_match(
            vec![bib(
                "S1",
                "BK",
                "Project Hail Mary",
                &["Weir, Andy"],
                &[],
                "eng"
            )],
            &q,
        )
        .is_some());
    }
}
