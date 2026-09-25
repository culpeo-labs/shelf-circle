//! Cleaning up provider-supplied blurbs into plain text we can show as-is.
//!
//! Open Library descriptions are user-edited markdown-ish text (`\r\n` line
//! breaks, `*emphasis*`, `[label](url)` links, sometimes a `----------` rule
//! followed by source footnotes); Google Books ones can contain HTML.

/// Longest description we keep (characters); longer ones are cut with "…".
const MAX_CHARS: usize = 4000;

/// Plain text or `None` if nothing readable is left.
pub fn clean_description(raw: &str) -> Option<String> {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");

    // Everything after a horizontal rule is source/footnote boilerplate.
    let mut kept: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.len() >= 3 && t.chars().all(|c| c == '-') {
            break;
        }
        kept.push(line.trim_end());
    }
    let text = strip_markdown(&kept.join("\n"));

    // Collapse runs of blank lines to a single paragraph break.
    let mut out = String::new();
    let mut blank_run = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank_run > 0 { "\n\n" } else { "\n" });
        }
        blank_run = 0;
        out.push_str(line.trim());
    }

    if out.is_empty() {
        return None;
    }
    if out.chars().count() > MAX_CHARS {
        let cut: String = out.chars().take(MAX_CHARS).collect();
        // Prefer ending on a word boundary.
        let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
        out = format!("{}…", cut.trim_end());
    }
    Some(out)
}

/// `[label](url)` → `label`, `[label][1]` → `label`, and drop `*`/`_`-style
/// emphasis markers around words.
fn strip_markdown(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']').map(|p| i + p) {
                let label: String = chars[i + 1..close].iter().collect();
                let after = chars.get(close + 1).copied();
                if matches!(after, Some('(') | Some('[')) {
                    let end_ch = if after == Some('(') { ')' } else { ']' };
                    if let Some(end) = chars[close + 2..]
                        .iter()
                        .position(|&c| c == end_ch)
                        .map(|p| close + 2 + p)
                    {
                        out.push_str(&label);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        match chars[i] {
            '*' => {}
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Tags → text (block tags become line breaks) plus the few entities that show
/// up in blurbs.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in s.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if matches!(
                    name.as_str(),
                    "br" | "p" | "div" | "li" | "h1" | "h2" | "h3"
                ) {
                    out.push('\n');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_newlines_and_collapses_blank_runs() {
        assert_eq!(
            clean_description("Line one.\r\nLine two.\r\n\r\n\r\n\r\nNew paragraph.  ").unwrap(),
            "Line one.\nLine two.\n\nNew paragraph."
        );
    }

    #[test]
    fn drops_source_footers_after_a_rule() {
        assert_eq!(
            clean_description(
                "A great book.\r\n\r\n----------\r\nAlso available as [1]\r\n[1]: http://x"
            )
            .unwrap(),
            "A great book."
        );
    }

    #[test]
    fn strips_markdown_emphasis_and_links() {
        assert_eq!(
            clean_description(
                "*Cien años* is **great**. See [the wiki](http://w.org) and [Source][1]."
            )
            .unwrap(),
            "Cien años is great. See the wiki and Source."
        );
    }

    #[test]
    fn empty_or_rule_only_is_none() {
        assert_eq!(clean_description("  \r\n "), None);
        assert_eq!(clean_description("----------\r\nfooter"), None);
    }

    #[test]
    fn long_text_is_cut_on_a_word_boundary() {
        let long = "word ".repeat(2000);
        let out = clean_description(&long).unwrap();
        assert!(out.ends_with("word…"), "{}", &out[out.len() - 12..]);
        assert!(out.chars().count() <= MAX_CHARS + 1);
    }

    #[test]
    fn html_becomes_text() {
        assert_eq!(
            clean_description(&strip_html(
                "<p>It&#39;s <b>bold</b> &amp; brisk.</p><p>Second<br>line</p>"
            ))
            .unwrap(),
            "It's bold & brisk.\n\nSecond\nline"
        );
    }
}
