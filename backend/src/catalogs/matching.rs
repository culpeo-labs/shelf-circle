//! Catalog-agnostic "is this catalog record the same book?" rules, shared by
//! every catalog kind. Matching is by *work* (title + author), not by edition:
//! we store one representative edition per book, and libraries usually hold a
//! different printing, so an exact-ISBN match alone misses most books.

/// Lower-cased alphanumerics and single spaces; `&` → `and`.
fn squash(s: &str) -> String {
    let s = s.to_lowercase().replace('&', " and ");
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn drop_leading_article(s: &str) -> &str {
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = s.strip_prefix(article) {
            return rest;
        }
    }
    s
}

/// Title without any subtitle ("Sapiens: A Brief History" → "Sapiens").
pub fn main_title(title: &str) -> &str {
    title.split(':').next().unwrap_or(title).trim()
}

fn key(title: &str) -> String {
    let squashed = squash(title);
    drop_leading_article(&squashed).to_string()
}

/// Same work by title. Equal after normalizing (case, punctuation, a leading
/// "The"), or one side's *main* title (before a colon) equals the other's full
/// title — a record listing just "Sapiens" matches "Sapiens: A Brief History…".
/// Two main titles are deliberately not compared with each other, so "Star
/// Wars: Thrawn" doesn't match "Star Wars: Thrawn Ascendancy".
pub fn titles_match(a: &str, b: &str) -> bool {
    let (full_a, full_b) = (key(a), key(b));
    !full_a.is_empty()
        && (full_a == full_b || key(main_title(a)) == full_b || full_a == key(main_title(b)))
}

const NAME_SUFFIXES: [&str; 6] = ["jr", "sr", "ii", "iii", "iv", "phd"];

/// Family name from "Andy Weir", "Weir, Andy" or "Martin Luther King Jr.".
fn surname(author: &str) -> Option<String> {
    let name = match author.split_once(',') {
        Some((last, _)) => last.to_string(),
        None => {
            let words: Vec<String> = squash(author).split(' ').map(String::from).collect();
            words
                .into_iter()
                .rev()
                .find(|w| !NAME_SUFFIXES.contains(&w.as_str()))?
        }
    };
    let name = squash(&name);
    name.split(' ')
        .next_back()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// The book's author appears among the catalog record's authors (compared by
/// surname, since catalogs write "Weir, Andy" and we store "Andy Weir").
/// Records with no authors (DVDs, some serials) never match.
pub fn author_matches(book_author: &str, catalog_authors: &[String]) -> bool {
    let Some(surname) = surname(book_author) else {
        return false;
    };
    catalog_authors
        .iter()
        .any(|a| squash(a).split(' ').any(|w| w == surname))
}

/// Every distinct title (by normalized form) to search under / accept: the
/// canonical one first, then alternates, capped so a book with many editions
/// doesn't fan out into many searches.
pub fn title_variants<'a>(title: &'a str, alts: &'a [String], max: usize) -> Vec<&'a str> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for t in std::iter::once(title).chain(alts.iter().map(String::as_str)) {
        let k = key(main_title(t));
        if !k.is_empty() && !seen.contains(&k) {
            seen.push(k);
            out.push(t);
        }
        if out.len() == max {
            break;
        }
    }
    out
}

/// True when the record is in the book's language. Languages are compared by
/// their first two letters ("eng"/"en"/"en-US" agree); unknown on either side
/// counts as a match (nothing to disagree about).
pub fn same_language(book: Option<&str>, catalog: Option<&str>) -> bool {
    let two = |s: &str| s.trim().to_lowercase().chars().take(2).collect::<String>();
    match (book, catalog) {
        (Some(b), Some(c)) if !b.trim().is_empty() && !c.trim().is_empty() => two(b) == two(c),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_match_ignores_case_punctuation_articles_and_subtitles() {
        assert!(titles_match("Project Hail Mary", "PROJECT HAIL MARY"));
        assert!(titles_match("The Midnight Library", "Midnight Library"));
        assert!(titles_match(
            "Tomorrow, and Tomorrow, and Tomorrow",
            "Tomorrow and tomorrow and tomorrow"
        ));
        assert!(titles_match(
            "Sapiens: A Brief History of Humankind",
            "Sapiens"
        ));
        assert!(titles_match("Sapiens", "Sapiens: A brief history"));
        assert!(titles_match("Salt & Fat", "Salt and Fat"));
    }

    #[test]
    fn titles_match_rejects_different_works() {
        assert!(!titles_match("Dune", "Dune Messiah"));
        assert!(!titles_match(
            "Star Wars: Thrawn",
            "Star Wars: Thrawn Ascendancy"
        ));
        assert!(!titles_match("Circe", "Circe's Island"));
        assert!(!titles_match("", ""), "empty never matches");
    }

    #[test]
    fn authors_match_by_surname_in_either_name_order() {
        let cat = |a: &str| vec![a.to_string()];
        assert!(author_matches("Andy Weir", &cat("Weir, Andy")));
        assert!(author_matches("Weir, Andy", &cat("Weir, Andy")));
        assert!(author_matches(
            "J. R. R. Tolkien",
            &cat("Tolkien, J. R. R. (John Ronald Reuel)")
        ));
        assert!(author_matches(
            "Martin Luther King Jr.",
            &cat("King, Martin Luther, Jr.")
        ));
        assert!(!author_matches("Andy Weir", &cat("Sewitch, Donna")));
        assert!(!author_matches("Andy Weir", &[]), "no catalog authors");
    }

    #[test]
    fn language_compat() {
        assert!(same_language(Some("en"), Some("eng")));
        assert!(same_language(Some("en-US"), Some("eng")));
        assert!(!same_language(Some("en"), Some("spa")));
        assert!(same_language(None, Some("spa")));
        assert!(same_language(Some("en"), None));
    }

    #[test]
    fn title_variants_dedupe_and_cap() {
        let alts = vec![
            "One Hundred Years of Solitude".to_string(),
            "the cien años de soledad".to_string(), // same as canonical once normalized
            "Cent ans de solitude".to_string(),
            "Hundert Jahre Einsamkeit".to_string(),
        ];
        assert_eq!(
            title_variants("Cien años de soledad", &alts, 3),
            [
                "Cien años de soledad",
                "One Hundred Years of Solitude",
                "Cent ans de solitude"
            ]
        );
        assert_eq!(title_variants("Dune", &[], 3), ["Dune"]);
    }
}
