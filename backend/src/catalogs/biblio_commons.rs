//! BiblioCommons catalogs (Seattle, King County, and hundreds more): search the
//! public gateway by ISBN and link to the matching record.

use std::collections::HashMap;

use serde::Deserialize;

use super::{normalize_isbn, CatalogError, CatalogMatch};

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
struct BriefInfo {
    /// BiblioCommons format code: `BK` (book), `EBOOK`, `LPRINT`, …
    format: Option<String>,
    #[serde(default)]
    isbns: Vec<String>,
}

pub async fn find_by_isbn(
    http: &reqwest::Client,
    gateway: &str,
    slug: &str,
    host: &str,
    isbn: &str,
) -> Result<Option<CatalogMatch>, CatalogError> {
    let url = format!("{gateway}/v2/libraries/{slug}/bibs/search");
    let response = http
        .get(&url)
        .query(&[("query", isbn), ("searchType", "smart"), ("limit", "10")])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(CatalogError::Status(response.status()));
    }
    let body: SearchResponse = response.json().await?;
    Ok(
        best_match(body.entities.bibs.into_values().collect(), isbn).map(|bib| CatalogMatch {
            url: format!("https://{host}/v2/record/{}", bib.id),
        }),
    )
}

/// Among records that really carry this ISBN (the search is "smart", so don't
/// trust it blindly), prefer a plain book over ebook/large print/etc., then
/// break ties by id so the answer is stable.
fn best_match(mut bibs: Vec<Bib>, isbn: &str) -> Option<Bib> {
    bibs.retain(|b| b.brief_info.isbns.iter().any(|i| normalize_isbn(i) == isbn));
    bibs.sort_by(|a, b| {
        let plain = |x: &Bib| x.brief_info.format.as_deref() != Some("BK");
        plain(a).cmp(&plain(b)).then_with(|| a.id.cmp(&b.id))
    });
    bibs.into_iter().next()
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

    fn bib(id: &str, format: &str, isbns: &[&str]) -> Bib {
        Bib {
            id: id.into(),
            brief_info: BriefInfo {
                format: Some(format.into()),
                isbns: isbns.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    #[test]
    fn prefers_plain_books_and_requires_the_isbn() {
        let picked = best_match(
            vec![
                bib("S1", "EBOOK", &["9780593135204"]),
                bib("S3", "BK", &["9780593135204"]),
                bib("S2", "BK", &["9999999999999"]),
            ],
            "9780593135204",
        )
        .unwrap();
        assert_eq!(picked.id, "S3", "the BK that actually has the ISBN");

        assert!(best_match(vec![bib("S2", "BK", &["1111111111111"])], "9780593135204").is_none());
    }
}
