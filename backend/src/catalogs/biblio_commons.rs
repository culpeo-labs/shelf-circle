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

/// Title variants searched per lookup (canonical + translations/retitles).
const MAX_TITLE_VARIANTS: usize = 3;

pub async fn find(
    http: &reqwest::Client,
    gateway: &str,
    slug: &str,
    host: &str,
    book: &BookQuery<'_>,
) -> Result<Option<CatalogMatch>, CatalogError> {
    // One search per distinct title we know the work by; pool the records.
    // Usually that's one search — several only for translated works.
    let variants = matching::title_variants(book.title, book.alt_titles, MAX_TITLE_VARIANTS);
    let mut pooled: HashMap<String, Bib> = HashMap::new();
    for (i, variant) in variants.iter().enumerate() {
        let text = match book.author {
            Some(a) if !a.trim().is_empty() => format!("{} {a}", matching::main_title(variant)),
            _ => matching::main_title(variant).to_string(),
        };
        match search(http, gateway, slug, &text, TITLE_SEARCH_LIMIT).await {
            Ok(bibs) => {
                for bib in bibs {
                    pooled.entry(bib.id.clone()).or_insert(bib);
                }
            }
            // The work's own title failing means we can't say anything; an
            // alternate title failing just means fewer candidates.
            Err(e) if i == 0 => return Err(e),
            Err(e) => tracing::warn!("catalog search for alternate title {variant:?} failed: {e}"),
        }
    }
    if let Some(bib) = best_work_match(pooled.into_values().collect(), book, &variants) {
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

/// Records that are the same work as `book` (any known title, and the author
/// when we have one). Among those, ranked:
/// 1. the record titled like the book *as the app shows it* (`book.title`) —
///    for a translated work that's the original-language title, and it's what
///    the user saw and picked; the saved edition's language is arbitrary (we
///    just take Open Library's first English one), so it must not outrank this;
/// 2. the exact edition (one of our ISBNs);
/// 3. the book's own language;
/// 4. format, then id (stable).
fn best_work_match(bibs: Vec<Bib>, book: &BookQuery<'_>, titles: &[&str]) -> Option<Bib> {
    bibs.into_iter()
        .filter(|b| {
            titles
                .iter()
                .any(|t| matching::titles_match(t, &b.brief_info.title))
        })
        .filter(|b| match book.author.filter(|a| !a.trim().is_empty()) {
            Some(a) => matching::author_matches(a, &b.brief_info.authors),
            None => true,
        })
        .min_by_key(|b| {
            let shown_title = matching::titles_match(book.title, &b.brief_info.title);
            let exact_edition = book.isbns.iter().any(|i| b.has_isbn(i));
            let language =
                matching::same_language(book.language, b.brief_info.primary_language.as_deref());
            (!shown_title, !exact_edition, !language, sort_key(b))
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
            alt_titles: &[],
            author: Some("Andy Weir"),
            language: Some("en"),
            isbns,
        }
    }

    fn pick(bibs: Vec<Bib>, q: &BookQuery<'_>) -> Option<String> {
        let titles = matching::title_variants(q.title, q.alt_titles, MAX_TITLE_VARIANTS);
        best_work_match(bibs, q, &titles).map(|b| b.id)
    }

    #[test]
    fn matches_the_work_even_when_the_edition_differs() {
        // Our stored ISBN is a different printing than any the library holds.
        let ours = ["9781529000000".to_string()];
        let picked = pick(
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
        );
        assert_eq!(picked.as_deref(), Some("S3"), "plain book, right author");
    }

    #[test]
    fn prefers_the_exact_edition_over_a_better_format() {
        let ours = ["9780593135211".to_string()];
        let picked = pick(
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
        );
        assert_eq!(picked.as_deref(), Some("S2"), "we own that edition's ISBN");
    }

    #[test]
    fn rejects_other_works_by_the_same_author() {
        let none: [String; 0] = [];
        assert!(pick(
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
        assert!(pick(
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

    #[test]
    fn translated_works_prefer_the_title_the_app_shows() {
        // The work is filed (and displayed) under its Spanish title, but the
        // edition we happened to save is the English translation.
        let none: [String; 0] = [];
        let alts = ["One Hundred Years of Solitude".to_string()];
        let q = BookQuery {
            title: "Cien años de soledad",
            alt_titles: &alts,
            author: Some("Gabriel García Márquez"),
            language: Some("en"),
            isbns: &none,
        };
        let bibs = || {
            vec![
                bib(
                    "SP",
                    "BK",
                    "Cien años de soledad",
                    &["García Márquez, Gabriel"],
                    &[],
                    "spa",
                ),
                bib(
                    "EN",
                    "EBOOK",
                    "One Hundred Years of Solitude",
                    &["García Márquez, Gabriel"],
                    &[],
                    "eng",
                ),
            ]
        };
        assert_eq!(
            pick(bibs(), &q).as_deref(),
            Some("SP"),
            "the title shown to the user wins"
        );
        // A library with only the English translation still gets a link.
        assert_eq!(
            pick(bibs().into_iter().skip(1).collect(), &q).as_deref(),
            Some("EN")
        );
    }
}
