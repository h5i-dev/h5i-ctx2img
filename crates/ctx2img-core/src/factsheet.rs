//! Verbatim factsheet: precision-critical tokens extracted as *text* to ride
//! alongside rendered images. VLM reading fails by silent confabulation on
//! high-entropy strings (hex, IDs, paths) — so those never rely on pixels;
//! the model quotes them from this sheet instead. (Approach field-validated
//! by pxpipe: ~5% of source chars covers most exact-recall misses.)
//!
//! Deterministic: fixed category priority, then length-desc/lexical — same
//! text always yields the same sheet (cache-safe).

/// Extraction categories, highest priority first: short opaque identifiers
/// carry zero redundancy (a misread is unrecoverable), URLs are mostly
/// reconstructable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    HexId,     // ≥7 hex chars (SHAs, UUIDs fragments)
    Version,   // 1.2.3-style
    BigNumber, // ≥5 digits
    Path,      // segments with '/' and an extension or dotfile
    ConstId,   // UPPER_SNAKE
    CamelCase, // mixedCase identifiers
    Url,
}

pub fn extract(text: &str, max_items: usize) -> Vec<String> {
    let mut found: Vec<(Class, String)> = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || "()[]{}<>\"'`,;".contains(c)) {
        let tok = raw.trim_matches(|c: char| ".:!?".contains(c));
        if tok.len() < 4 || tok.len() > 120 {
            continue;
        }
        if let Some(class) = classify(tok) {
            found.push((class, tok.to_string()));
        }
    }
    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.len().cmp(&a.1.len()))
            .then(a.1.cmp(&b.1))
    });
    found.dedup_by(|a, b| a.1 == b.1);
    // re-dedup across classes (same string classified twice can't happen,
    // but substrings of kept paths add nothing)
    let mut out: Vec<String> = Vec::new();
    let mut urls = 0usize;
    for (class, tok) in found {
        if out.len() >= max_items {
            break;
        }
        if class == Class::Url {
            urls += 1;
            if urls > 8 {
                continue;
            }
        }
        if !out.contains(&tok) {
            out.push(tok);
        }
    }
    out
}

/// Render the sheet as the text block that accompanies image pages.
pub fn render_sheet(items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    format!(
        "[Exact identifiers from the rendered pages — quote these verbatim instead of transcribing them from the image: {}]",
        items.join(" · ")
    )
}

fn classify(tok: &str) -> Option<Class> {
    let bytes = tok.as_bytes();
    let n_digit = bytes.iter().filter(|b| b.is_ascii_digit()).count();
    let n_alpha = bytes.iter().filter(|b| b.is_ascii_alphabetic()).count();

    if tok.starts_with("http://") || tok.starts_with("https://") {
        return Some(Class::Url);
    }
    // hex id: ≥7 chars, all hex, at least one digit AND one letter (avoids
    // plain words like "decade" and plain numbers, which BigNumber covers)
    if tok.len() >= 7 && bytes.iter().all(|b| b.is_ascii_hexdigit()) && n_digit > 0 && n_alpha > 0 {
        return Some(Class::HexId);
    }
    // version: digits and dots, ≥2 dots or vX.Y
    let dots = bytes.iter().filter(|&&b| b == b'.').count();
    if n_digit >= 2
        && dots >= 1
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b'.' || *b == b'v' || *b == b'-')
        && tok
            .chars()
            .next()
            .is_some_and(|c| c == 'v' || c.is_ascii_digit())
    {
        return Some(Class::Version);
    }
    if n_digit >= 5
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b'_' || *b == b',')
    {
        return Some(Class::BigNumber);
    }
    if tok.contains('/')
        && !tok.contains("//")
        && tok
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains('.') || last.starts_with('.'))
    {
        return Some(Class::Path);
    }
    if tok.len() >= 5
        && tok.contains('_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Some(Class::ConstId);
    }
    // camelCase: lower start, interior uppercase, alnum only
    if tok.len() >= 6
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().skip(1).any(|b| b.is_ascii_uppercase())
        && bytes.iter().all(|b| b.is_ascii_alphanumeric())
    {
        return Some(Class::CamelCase);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_precision_tokens_by_priority() {
        let text = "Commit 5a7373d4187f fixed src/auth/session.rs — see \
                    https://example.com/x and MAX_RETRY_COUNT plus getUserName \
                    at version 1.2.3 with 30000 items and the word decade.";
        let items = extract(text, 20);
        assert!(items.contains(&"5a7373d4187f".to_string()), "{items:?}");
        assert!(items.contains(&"src/auth/session.rs".to_string()));
        assert!(items.contains(&"MAX_RETRY_COUNT".to_string()));
        assert!(items.contains(&"getUserName".to_string()));
        assert!(items.contains(&"1.2.3".to_string()));
        assert!(items.contains(&"30000".to_string()));
        assert!(
            !items.contains(&"decade".to_string()),
            "plain words excluded"
        );
        // hex id must outrank the URL
        let hex = items.iter().position(|t| t == "5a7373d4187f").unwrap();
        let url = items.iter().position(|t| t.starts_with("https")).unwrap();
        assert!(hex < url);
    }

    #[test]
    fn deterministic() {
        let text = "abc1234 def5678 abc1234 src/a.rs src/b.rs";
        assert_eq!(extract(text, 10), extract(text, 10));
    }
}
