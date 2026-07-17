//! Query-conditioned relevance ("elevation"): BM25 + embedding cosine +
//! personalized-PageRank diffusion + churn, rank-fused and quantized into
//! discrete bands. With no query, elevation falls back to global importance
//! so `ctx2img render` still produces meaningful terrain.

use crate::embed::Embeddings;
use crate::graph::{self, Edge};
use crate::history::History;
use crate::tokens;
use crate::types::{FileId, FileInfo, ParsedFile};
use std::collections::HashMap;

pub const BANDS: u8 = 5;
/// Band quantiles: fractions of files that sit *below* each band boundary.
/// Top band ≈ top 2%, so the map always has a small, obvious summit.
const BAND_QUANTILES: [f32; 4] = [0.55, 0.80, 0.93, 0.98];

const SEED_COUNT: usize = 12;
const W_LEXICAL: f32 = 0.35;
const W_SEMANTIC: f32 = 0.25;
const W_DIFFUSION: f32 = 0.30;
const W_CHURN: f32 = 0.10;
const PATH_HIT_BONUS: f32 = 0.15;

#[derive(Debug, Clone)]
pub struct Relevance {
    /// Fused score in [0, 1+] per file (rank-normalized components).
    pub scores: Vec<f32>,
    /// Elevation band 1..=5 per file.
    pub bands: Vec<u8>,
    /// Seed files used for diffusion (top lexical+semantic hits).
    pub seeds: Vec<FileId>,
    /// The parsed query terms (used for legend echo).
    pub terms: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn score(
    query: &str,
    files: &[FileInfo],
    parsed: &[ParsedFile],
    edges: &[Edge],
    embeddings: &Embeddings,
    importance: &[f32],
    history: &History,
) -> Relevance {
    let n = files.len();
    let terms = tokens::query_terms(query);
    if n == 0 || terms.is_empty() {
        // No query: elevation = global importance.
        let bands = quantize(importance);
        return Relevance {
            scores: importance.to_vec(),
            bands,
            seeds: Vec::new(),
            terms,
        };
    }

    // --- component scores ---
    let lexical = bm25(&terms, parsed);
    let qvec = embeddings.embed_query(&terms);
    let semantic: Vec<f32> = (0..n)
        .map(|i| embeddings.cosine(&qvec, i).max(0.0))
        .collect();
    let path_bonus: Vec<f32> = files
        .iter()
        .map(|f| {
            let p = f.path.to_ascii_lowercase();
            let hits = terms.iter().filter(|t| p.contains(*t)).count() as f32;
            (hits * PATH_HIT_BONUS).min(2.0 * PATH_HIT_BONUS)
        })
        .collect();

    // --- seeds: best lexical+semantic combined ---
    let lex_n = rank_normalize(&lexical);
    let sem_n = rank_normalize(&semantic);
    // Raw-signal seed scores (not rank-normalized): rank normalization would
    // let pure noise qualify as a seed on repos where few files truly match.
    let lex_max = lexical.iter().copied().fold(0f32, f32::max);
    let sem_max = semantic.iter().copied().fold(0f32, f32::max);
    let mut seed_rank: Vec<(usize, f32)> = (0..n)
        .map(|i| {
            let lex = if lex_max > 0.0 {
                lexical[i] / lex_max
            } else {
                0.0
            };
            let sem = if sem_max > 0.0 {
                semantic[i] / sem_max
            } else {
                0.0
            };
            (i, lex + 0.6 * sem + path_bonus[i])
        })
        .collect();
    seed_rank.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let top_seed = seed_rank.first().map(|&(_, s)| s).unwrap_or(0.0);
    let seeds: Vec<(FileId, f32)> = seed_rank
        .iter()
        .take(SEED_COUNT)
        .filter(|&&(_, s)| s > 0.0 && s >= top_seed * 0.30)
        .map(|&(i, s)| (FileId(i as u32), s))
        .collect();

    let diffusion = if seeds.is_empty() {
        vec![0f32; n]
    } else {
        graph::page_rank(n, edges, Some(&seeds))
    };

    let churn: Vec<f32> = files
        .iter()
        .map(|f| history.churn.get(&f.path).copied().unwrap_or(0.0))
        .collect();

    // --- fuse (rank-normalized so no component dominates by scale) ---
    let dif_n = rank_normalize(&diffusion);
    let chn_n = rank_normalize(&churn);
    let scores: Vec<f32> = (0..n)
        .map(|i| {
            W_LEXICAL * lex_n[i]
                + W_SEMANTIC * sem_n[i]
                + W_DIFFUSION * dif_n[i]
                + W_CHURN * chn_n[i]
                + path_bonus[i]
        })
        .collect();

    let bands = quantize(&scores);
    Relevance {
        scores,
        bands,
        seeds: seeds.into_iter().map(|(id, _)| id).collect(),
        terms,
    }
}

fn bm25(terms: &[String], parsed: &[ParsedFile]) -> Vec<f32> {
    let n = parsed.len() as f32;
    let (k1, b) = (1.2f32, 0.75f32);
    let avg_len: f32 =
        (parsed.iter().map(|p| p.ident_count()).sum::<u64>() as f32 / n.max(1.0)).max(1.0);

    let mut df: HashMap<&str, u32> = HashMap::new();
    for p in parsed {
        for (t, _) in &p.idents {
            if terms.iter().any(|q| q == t) {
                *df.entry(t.as_str()).or_insert(0) += 1;
            }
        }
    }
    parsed
        .iter()
        .map(|p| {
            let len = p.ident_count() as f32;
            let mut s = 0f32;
            for term in terms {
                let tf = p
                    .idents
                    .iter()
                    .find(|(t, _)| t == term)
                    .map(|(_, c)| *c as f32)
                    .unwrap_or(0.0);
                if tf == 0.0 {
                    continue;
                }
                let d = *df.get(term.as_str()).unwrap_or(&1) as f32;
                let idf = (1.0 + (n - d + 0.5) / (d + 0.5)).ln();
                s += idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * len / avg_len));
            }
            s
        })
        .collect()
}

/// Map scores to [0,1] by rank percentile; zeros stay zero.
fn rank_normalize(scores: &[f32]) -> Vec<f32> {
    let n = scores.len();
    if n == 0 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(a.cmp(&b)));
    let mut out = vec![0f32; n];
    let denom = (n.max(2) - 1) as f32;
    for (rank, &i) in idx.iter().enumerate() {
        out[i] = if scores[i] <= 0.0 {
            0.0
        } else {
            rank as f32 / denom
        };
    }
    out
}

/// Quantile-based banding for contrast — but only *within the files that
/// carry real signal*. Files at ≤2% of the max score stay at band 1, so a
/// concentrated query on a huge repo yields one summit, not a uniformly
/// "hot" map (quantiles over everything would force half the repo high).
fn quantize(scores: &[f32]) -> Vec<u8> {
    let n = scores.len();
    if n == 0 {
        return Vec::new();
    }
    let max = scores.iter().copied().fold(0f32, f32::max);
    if max <= 0.0 {
        return vec![1; n];
    }
    let floor = max * 0.02;
    let mut positive: Vec<f32> = scores.iter().copied().filter(|&s| s > floor).collect();
    positive.sort_by(f32::total_cmp);
    let m = positive.len().max(1);
    let cut = |q: f32| -> f32 { positive[((m - 1) as f32 * q) as usize] };
    let cuts = [
        cut(BAND_QUANTILES[0]),
        cut(BAND_QUANTILES[1]),
        cut(BAND_QUANTILES[2]),
        cut(BAND_QUANTILES[3]),
    ];
    scores
        .iter()
        .map(|&s| {
            if s <= floor {
                return 1u8;
            }
            let mut band = 2u8;
            for (i, &c) in cuts.iter().enumerate().skip(1) {
                if s > c {
                    band = i as u8 + 2;
                }
            }
            band
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_file;
    use crate::types::Lang;

    #[test]
    fn query_elevates_matching_and_connected_files() {
        let srcs = [
            (
                "auth/session.rs",
                "pub struct Session; pub fn session_expiry(s: &Session) -> u64 { s.expires }",
            ),
            (
                "auth/middleware.rs",
                "use crate::auth::session::Session; fn guard(s: Session) { session_expiry(&s); }",
            ),
            (
                "ui/button.rs",
                "fn draw_button(w: Widget) { w.paint_rect(); }",
            ),
            (
                "ui/panel.rs",
                "fn draw_panel(w: Widget) { w.paint_grid(); }",
            ),
        ];
        let files: Vec<crate::types::FileInfo> = srcs
            .iter()
            .map(|(p, s)| crate::types::FileInfo {
                path: p.to_string(),
                lang: Lang::Rust,
                size: s.len() as u64,
                loc: 3,
                hash: String::new(),
            })
            .collect();
        let parsed: Vec<_> = srcs
            .iter()
            .map(|(_, s)| parse_file(Lang::Rust, s))
            .collect();
        let hist = History::default();
        let g = graph::build(&files, &parsed, &hist);
        let emb = crate::embed::embed_all(
            &parsed,
            &files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
        );
        let imp = graph::page_rank(files.len(), &g.edges, None);
        let rel = score(
            "fix session expiry bug",
            &files,
            &parsed,
            &g.edges,
            &emb,
            &imp,
            &hist,
        );
        assert!(
            rel.bands[0] > rel.bands[2],
            "session.rs should outrank button.rs"
        );
        assert!(
            rel.scores[1] > rel.scores[3],
            "middleware (connected) should outrank panel"
        );
    }

    #[test]
    fn empty_query_uses_importance() {
        let imp = vec![0.1, 0.5, 0.4];
        let bands = quantize(&imp);
        assert_eq!(bands.len(), 3);
        assert!(bands[1] >= bands[0]);
    }
}
