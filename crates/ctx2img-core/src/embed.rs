//! Hashed TF-IDF embeddings: zero model downloads, CPU-only, deterministic.
//! 256-dim feature-hashed vectors over the shared identifier vocabulary.
//! Good enough for "which files are about session expiry"; a learned model
//! can slot in behind the same two functions later.

use crate::types::ParsedFile;
use std::collections::HashMap;

pub const DIM: usize = 256;

pub struct Embeddings {
    pub vectors: Vec<[f32; DIM]>,
    df: HashMap<String, u32>,
    n_docs: u32,
}

pub fn embed_all(parsed: &[ParsedFile], paths: &[String]) -> Embeddings {
    let mut df: HashMap<String, u32> = HashMap::new();
    for pf in parsed {
        for (tok, _) in &pf.idents {
            *df.entry(tok.clone()).or_insert(0) += 1;
        }
    }
    let n_docs = parsed.len() as u32;
    let vectors = parsed
        .iter()
        .zip(paths)
        .map(|(pf, path)| {
            // path tokens count double: file location is a strong topic signal
            let path_toks: Vec<(String, u32)> =
                crate::tokens::ident_bag(&path.replace(['/', '.'], " "))
                    .into_iter()
                    .map(|(t, c)| (t, c * 2))
                    .collect();
            vectorize(pf.idents.iter().chain(path_toks.iter()), &df, n_docs)
        })
        .collect();
    Embeddings {
        vectors,
        df,
        n_docs,
    }
}

impl Embeddings {
    pub fn embed_query(&self, terms: &[String]) -> [f32; DIM] {
        let bag: Vec<(String, u32)> = terms.iter().map(|t| (t.clone(), 1)).collect();
        vectorize(bag.iter(), &self.df, self.n_docs)
    }

    pub fn cosine(&self, query: &[f32; DIM], file: usize) -> f32 {
        dot(query, &self.vectors[file])
    }
}

fn vectorize<'a>(
    toks: impl Iterator<Item = &'a (String, u32)>,
    df: &HashMap<String, u32>,
    n_docs: u32,
) -> [f32; DIM] {
    let mut v = [0f32; DIM];
    for (tok, count) in toks {
        let d = *df.get(tok).unwrap_or(&1) as f32;
        let idf = (1.0 + n_docs as f32 / d).ln();
        let w = (1.0 + (*count as f32).ln()) * idf;
        let h = fnv1a(tok.as_bytes());
        let slot = (h % DIM as u64) as usize;
        let sign = if h & (1 << 32) == 0 { 1.0 } else { -1.0 };
        v[slot] += sign * w;
    }
    let norm = dot(&v, &v).sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

fn dot(a: &[f32; DIM], b: &[f32; DIM]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;
    use crate::types::Lang;

    #[test]
    fn related_files_are_closer() {
        let a = parse_file(
            Lang::Rust,
            "fn session_expiry(session: Session) -> Expiry { session.expire_at() }",
        );
        let b = parse_file(
            Lang::Rust,
            "fn session_check(session: &Session) { validate_session_expiry(session) }",
        );
        let c = parse_file(
            Lang::Rust,
            "fn render_button(color: Color) -> Widget { draw_rect(color) }",
        );
        let e = embed_all(
            &[a, b, c],
            &[
                "auth/expiry.rs".into(),
                "auth/check.rs".into(),
                "ui/button.rs".into(),
            ],
        );
        let q = e.embed_query(&["session".into(), "expiry".into()]);
        assert!(e.cosine(&q, 0) > e.cosine(&q, 2));
        assert!(e.cosine(&q, 1) > e.cosine(&q, 2));
    }
}
