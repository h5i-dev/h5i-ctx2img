//! Language-agnostic identifier tokenization: the shared vocabulary for
//! the reference graph, TF-IDF embeddings, and BM25 relevance.

/// Extract identifier-shaped tokens from source text, split into sub-tokens
/// (camelCase / snake_case / kebab-case), lowercased, with counts.
/// Deterministic order: first-seen.
pub fn ident_bag(src: &str) -> Vec<(String, u32)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for raw in raw_idents(src) {
        for sub in split_subtokens(raw) {
            if sub.len() < 3 || STOPWORDS.contains(&sub.as_str()) {
                continue;
            }
            match counts.entry(sub) {
                std::collections::hash_map::Entry::Occupied(mut e) => *e.get_mut() += 1,
                std::collections::hash_map::Entry::Vacant(e) => {
                    order.push(e.key().clone());
                    e.insert(1);
                }
            }
        }
    }
    order
        .into_iter()
        .map(|t| {
            let c = counts[&t];
            (t, c)
        })
        .collect()
}

/// Raw `[A-Za-z_][A-Za-z0-9_]*` tokens, no splitting.
pub fn raw_idents(src: &str) -> impl Iterator<Item = &str> {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'_' || b.is_ascii_alphabetic() {
                let start = i;
                while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                return Some(&src[start..i]);
            }
            i += 1;
        }
        None
    })
}

/// Split an identifier into lowercase sub-tokens: `getUserName` -> [get, user, name],
/// `HTTP_CLIENT` -> [http, client]. Also yields the whole identifier lowercased when
/// it differs from its single sub-token (so exact-name matches still score).
pub fn split_subtokens(ident: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = ident.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // boundary: lower->Upper, or Upper followed by Upper+lower (HTTPServer -> HTTP, Server)
        if c.is_ascii_uppercase() && !cur.is_empty() {
            let prev_lower = chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit();
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let prev_upper = chars[i - 1].is_ascii_uppercase();
            if prev_lower || (prev_upper && next_lower) {
                parts.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    if parts.len() > 1 {
        let whole = ident.to_ascii_lowercase().replace(['_', '-'], "");
        if whole.len() >= 3 {
            parts.push(whole);
        }
    }
    parts
}

/// Tokenize a free-text query with the same rules as source code.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in raw_idents(query) {
        for sub in split_subtokens(raw) {
            if sub.len() >= 3 && !STOPWORDS.contains(&sub.as_str()) && !out.contains(&sub) {
                out.push(sub);
            }
        }
    }
    out
}

/// Common language keywords and glue words — noise for both retrieval and graphs.
pub const STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "not",
    "with",
    "this",
    "that",
    "from",
    "have",
    "will",
    "your",
    "are",
    "was",
    "were",
    "been",
    "than",
    "then",
    "them",
    "they",
    "there",
    "which",
    "would",
    "could",
    "should",
    "about",
    "into",
    "over",
    "some",
    "when",
    "where",
    "what",
    "while",
    "each",
    // language keywords (union across supported langs)
    "let",
    "mut",
    "impl",
    "use",
    "mod",
    "crate",
    "self",
    "super",
    "dyn",
    "ref",
    "match",
    "loop",
    "async",
    "await",
    "move",
    "type",
    "where",
    "unsafe",
    "extern",
    "static",
    "const",
    "enum",
    "struct",
    "trait",
    "true",
    "false",
    "none",
    "some",
    "def",
    "class",
    "import",
    "return",
    "pass",
    "elif",
    "else",
    "lambda",
    "yield",
    "global",
    "nonlocal",
    "assert",
    "raise",
    "except",
    "try",
    "finally",
    "print",
    "function",
    "var",
    "new",
    "delete",
    "typeof",
    "instanceof",
    "void",
    "null",
    "undefined",
    "export",
    "default",
    "extends",
    "implements",
    "interface",
    "public",
    "private",
    "protected",
    "package",
    "throws",
    "throw",
    "catch",
    "final",
    "abstract",
    "func",
    "chan",
    "defer",
    "fallthrough",
    "range",
    "select",
    "goto",
    "map",
    "string",
    "int",
    "bool",
    "byte",
    "float",
    "double",
    "long",
    "short",
    "char",
    "usize",
    "isize",
    "vec",
    "str",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_camel_and_snake() {
        assert_eq!(
            split_subtokens("getUserName"),
            vec!["get", "user", "name", "getusername"]
        );
        assert_eq!(
            split_subtokens("HTTP_CLIENT"),
            vec!["http", "client", "httpclient"]
        );
        assert_eq!(
            split_subtokens("HTTPServer"),
            vec!["http", "server", "httpserver"]
        );
        assert_eq!(split_subtokens("session"), vec!["session"]);
    }

    #[test]
    fn bag_counts_and_filters() {
        let bag = ident_bag("fn check_session(session: Session) { session.expire() }");
        let get = |t: &str| bag.iter().find(|(x, _)| x == t).map(|(_, c)| *c);
        assert_eq!(get("session"), Some(4));
        assert_eq!(get("expire"), Some(1));
        assert_eq!(get("fn"), None); // too short
    }

    #[test]
    fn query_terms_dedup() {
        assert_eq!(
            query_terms("fix the session expiry, session bug"),
            vec!["fix", "session", "expiry", "bug"]
        );
    }
}
