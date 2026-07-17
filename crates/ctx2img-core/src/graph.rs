//! File dependency graph: import resolution, reference edges (aider-style
//! def→ref matching), co-change coupling, plus PageRank / personalized
//! PageRank over the combined weighted graph.

use crate::history::History;
use crate::tokens;
use crate::types::{FileId, FileInfo, ParsedFile};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Resolved static import (drawn as dashed road).
    Import,
    /// Identifier reference to a definition (drawn as solid road).
    Reference,
    /// Files that change together in git history.
    CoChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: FileId,
    pub to: FileId,
    pub kind: EdgeKind,
    pub weight: f32,
}

#[derive(Debug, Default)]
pub struct FileGraph {
    pub edges: Vec<Edge>,
    /// Unresolved import targets that look external (crate/package names),
    /// with usage counts — these become the offshore islands.
    pub external: Vec<(String, u32)>,
}

const REF_MIN_NAME_LEN: usize = 4;
const REF_COMMON_DEF_LIMIT: usize = 5;
const REF_TOP_PER_FILE: usize = 30;

pub fn build(files: &[FileInfo], parsed: &[ParsedFile], history: &History) -> FileGraph {
    let mut g = FileGraph::default();
    let resolver = ImportResolver::new(files);
    let path_to_id: HashMap<&str, FileId> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.as_str(), FileId(i as u32)))
        .collect();

    // --- import edges + external deps ---
    let mut external: HashMap<String, u32> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let from = FileId(i as u32);
        let mut seen: Vec<FileId> = Vec::new();
        for imp in &pf.imports {
            match resolver.resolve(&files[i].path, imp) {
                Resolution::Internal(target) if target != from && !seen.contains(&target) => {
                    seen.push(target);
                    g.edges.push(Edge {
                        from,
                        to: target,
                        kind: EdgeKind::Import,
                        weight: 1.0,
                    });
                }
                Resolution::External(name) => *external.entry(name).or_insert(0) += 1,
                _ => {}
            }
        }
    }

    // --- reference edges: defined name -> referencing files ---
    let mut defs_by_name: HashMap<String, Vec<FileId>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        for sym in &pf.symbols {
            if sym.name.len() < REF_MIN_NAME_LEN {
                continue;
            }
            let key = sym.name.to_ascii_lowercase().replace(['_', '-'], "");
            let v = defs_by_name.entry(key).or_default();
            if !v.contains(&FileId(i as u32)) {
                v.push(FileId(i as u32));
            }
        }
    }
    for (i, pf) in parsed.iter().enumerate() {
        let from = FileId(i as u32);
        let mut cands: Vec<(FileId, f32)> = Vec::new();
        for (tok, count) in &pf.idents {
            if tok.len() < REF_MIN_NAME_LEN {
                continue;
            }
            if let Some(defs) = defs_by_name.get(tok) {
                let mult = if defs.len() > REF_COMMON_DEF_LIMIT {
                    0.1
                } else {
                    1.0
                };
                let w = (*count as f32).sqrt() * mult / defs.len() as f32;
                for &d in defs {
                    if d != from {
                        cands.push((d, w));
                    }
                }
            }
        }
        // merge duplicate targets, keep the strongest few
        cands.sort_by_key(|c| c.0);
        let mut merged: Vec<(FileId, f32)> = Vec::new();
        for (id, w) in cands {
            match merged.last_mut() {
                Some((last, lw)) if *last == id => *lw += w,
                _ => merged.push((id, w)),
            }
        }
        merged.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (to, weight) in merged.into_iter().take(REF_TOP_PER_FILE) {
            g.edges.push(Edge {
                from,
                to,
                kind: EdgeKind::Reference,
                weight,
            });
        }
    }

    // --- co-change edges (undirected: emit both directions) ---
    let mut cc: Vec<(&(String, String), &u32)> = history.co_change.iter().collect();
    cc.sort();
    for ((a, b), &n) in cc {
        if n < 2 {
            continue; // one shared commit is noise
        }
        if let (Some(&ia), Some(&ib)) = (path_to_id.get(a.as_str()), path_to_id.get(b.as_str())) {
            let w = (n as f32).sqrt() * 0.5;
            g.edges.push(Edge {
                from: ia,
                to: ib,
                kind: EdgeKind::CoChange,
                weight: w,
            });
            g.edges.push(Edge {
                from: ib,
                to: ia,
                kind: EdgeKind::CoChange,
                weight: w,
            });
        }
    }

    let mut ext: Vec<(String, u32)> = external.into_iter().collect();
    ext.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ext.truncate(24);
    g.external = ext;
    g
}

/// Kind weights when collapsing the multigraph for PageRank walks.
fn kind_weight(kind: EdgeKind) -> f32 {
    match kind {
        EdgeKind::Import => 1.0,
        EdgeKind::Reference => 0.6,
        EdgeKind::CoChange => 0.4,
    }
}

/// Weighted PageRank. `restart`: None = uniform (global importance);
/// Some(seeds) = personalized (relevance diffusion). Deterministic.
pub fn page_rank(n: usize, edges: &[Edge], restart: Option<&[(FileId, f32)]>) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    let damping = 0.85f32;
    let iterations = 40;

    // CSR-ish adjacency with normalized out-weights
    let mut out_weight = vec![0f32; n];
    for e in edges {
        out_weight[e.from.idx()] += e.weight * kind_weight(e.kind);
    }
    let mut adj: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
    for e in edges {
        let ow = out_weight[e.from.idx()];
        if ow > 0.0 {
            adj[e.from.idx()].push((e.to.0, e.weight * kind_weight(e.kind) / ow));
        }
    }

    let base = {
        let mut v = vec![0f32; n];
        match restart {
            None => v.iter_mut().for_each(|x| *x = 1.0 / n as f32),
            Some(seeds) => {
                let total: f32 = seeds.iter().map(|(_, w)| w).sum();
                if total <= 0.0 {
                    v.iter_mut().for_each(|x| *x = 1.0 / n as f32);
                } else {
                    for (id, w) in seeds {
                        v[id.idx()] += w / total;
                    }
                }
            }
        }
        v
    };

    let mut rank = base.clone();
    let mut next = vec![0f32; n];
    for _ in 0..iterations {
        next.iter_mut().for_each(|x| *x = 0.0);
        let mut dangling = 0f32;
        for i in 0..n {
            if adj[i].is_empty() {
                dangling += rank[i];
                continue;
            }
            for &(j, w) in &adj[i] {
                next[j as usize] += rank[i] * w;
            }
        }
        for i in 0..n {
            next[i] = (1.0 - damping) * base[i] + damping * (next[i] + dangling * base[i]);
        }
        std::mem::swap(&mut rank, &mut next);
    }
    rank
}

/// Aggregate file edges onto display regions (`assign[file] = region index`,
/// usize::MAX = unassigned). Returns (a, b, kind, weight) with a != b.
pub fn aggregate_edges(
    edges: &[Edge],
    assign: &[usize],
    n_regions: usize,
) -> Vec<(usize, usize, EdgeKind, f32)> {
    let mut acc: HashMap<(usize, usize, u8), f32> = HashMap::new();
    for e in edges {
        let (a, b) = (assign[e.from.idx()], assign[e.to.idx()]);
        if a == b || a == usize::MAX || b == usize::MAX || a >= n_regions || b >= n_regions {
            continue;
        }
        let k = match e.kind {
            EdgeKind::Import => 0u8,
            EdgeKind::Reference => 1,
            EdgeKind::CoChange => 2,
        };
        *acc.entry((a, b, k)).or_insert(0.0) += e.weight * kind_weight(e.kind);
    }
    let mut out: Vec<(usize, usize, EdgeKind, f32)> = acc
        .into_iter()
        .map(|((a, b, k), w)| {
            let kind = match k {
                0 => EdgeKind::Import,
                1 => EdgeKind::Reference,
                _ => EdgeKind::CoChange,
            };
            (a, b, kind, w)
        })
        .collect();
    out.sort_by(|x, y| y.3.total_cmp(&x.3).then((x.0, x.1).cmp(&(y.0, y.1))));
    out
}

enum Resolution {
    Internal(FileId),
    External(String),
    Unknown,
}

/// Heuristic, language-agnostic import resolution: match path-ish segments
/// of the import string against file stems/paths, preferring the candidate
/// sharing the longest trailing segment run and the nearest directory.
struct ImportResolver {
    stem_index: HashMap<String, Vec<(FileId, Vec<String>)>>,
    exact_paths: HashMap<String, FileId>,
}

impl ImportResolver {
    fn new(files: &[FileInfo]) -> Self {
        let mut stem_index: HashMap<String, Vec<(FileId, Vec<String>)>> = HashMap::new();
        let mut exact_paths = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            let id = FileId(i as u32);
            exact_paths.insert(f.path.clone(), id);
            let noext = f.path.rsplit_once('.').map(|(a, _)| a).unwrap_or(&f.path);
            // normalize - vs _ so `use ctx2img_layout::…` matches dir `ctx2img-layout`
            let segs: Vec<String> = noext
                .split('/')
                .map(|s| s.to_ascii_lowercase().replace('-', "_"))
                .collect();
            if let Some(stem) = segs.last() {
                // `mod.rs`, `__init__.py`, `index.ts` resolve to their directory name
                let key = if stem == "mod" || stem == "__init__" || stem == "index" {
                    segs.iter()
                        .rev()
                        .nth(1)
                        .cloned()
                        .unwrap_or_else(|| stem.clone())
                } else {
                    stem.clone()
                };
                stem_index.entry(key).or_default().push((id, segs));
            }
        }
        ImportResolver {
            stem_index,
            exact_paths,
        }
    }

    fn resolve(&self, from_path: &str, import: &str) -> Resolution {
        // 1) relative path imports (JS/TS style): resolve against from's dir
        if let Some(rel) = relative_target(import) {
            let dir = from_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if let Some(id) = self.resolve_relative(dir, &rel) {
                return Resolution::Internal(id);
            }
        }

        // 2) segment matching: try each path-ish token as a stem, best match wins
        let segs: Vec<String> = import_segments(import);
        if segs.is_empty() {
            return Resolution::Unknown;
        }
        let mut best: Option<(FileId, usize)> = None;
        for (pos, seg) in segs.iter().enumerate().rev() {
            if let Some(cands) = self.stem_index.get(seg) {
                for (id, cand_segs) in cands {
                    // score: how many of the import segments before `seg`
                    // appear, in order, at the tail of the candidate path
                    let mut score = 1usize;
                    let mut ci = cand_segs.len().saturating_sub(1);
                    for q in segs[..pos].iter().rev() {
                        if ci == 0 {
                            break;
                        }
                        ci -= 1;
                        if &cand_segs[ci] == q {
                            score += 1;
                        }
                    }
                    if best.map(|(_, s)| score > s).unwrap_or(true) {
                        best = Some((*id, score));
                    }
                }
                if best.is_some() {
                    break; // rightmost resolvable segment wins
                }
            }
        }
        if let Some((id, _)) = best {
            return Resolution::Internal(id);
        }

        // 3) unresolved: first segment is likely an external package name —
        // unless it's a type name (capitalized in source) or a language builtin
        let first = segs[0].clone();
        let capitalized = import.split([':', '.', ' ', '{', ',']).any(|t| {
            t.trim().to_ascii_lowercase() == first && t.trim().starts_with(char::is_uppercase)
        });
        if first.len() >= 2 && !capitalized && !BUILTIN_MODULES.contains(&first.as_str()) {
            Resolution::External(first)
        } else {
            Resolution::Unknown
        }
    }

    fn resolve_relative(&self, from_dir: &str, rel: &str) -> Option<FileId> {
        let mut parts: Vec<&str> = if from_dir.is_empty() {
            Vec::new()
        } else {
            from_dir.split('/').collect()
        };
        for seg in rel.split('/') {
            match seg {
                "." | "" => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let base = parts.join("/");
        const EXTS: &[&str] = &[
            "",
            ".ts",
            ".tsx",
            ".js",
            ".jsx",
            ".mjs",
            ".py",
            ".rs",
            "/index.ts",
            "/index.tsx",
            "/index.js",
            "/mod.rs",
            "/__init__.py",
        ];
        for ext in EXTS {
            if let Some(&id) = self.exact_paths.get(&format!("{base}{ext}")) {
                return Some(id);
            }
        }
        None
    }
}

/// Standard-library roots across supported languages: real imports, but
/// noise as "external dependency" islands.
const BUILTIN_MODULES: &[&str] = &[
    // rust
    "std",
    "core",
    "alloc",
    // python
    "os",
    "sys",
    "typing",
    "collections",
    "json",
    "re",
    "math",
    "time",
    "pathlib",
    "dataclasses",
    "functools",
    "itertools",
    "abc",
    "enum",
    "logging",
    "unittest",
    "asyncio",
    // go
    "fmt",
    "errors",
    "strings",
    "strconv",
    "context",
    "bytes",
    "bufio",
    "sort",
    // node
    "fs",
    "path",
    "util",
    "events",
    "url",
    // java
    "java",
    "javax",
];

/// Extract a `./x/y`-style relative specifier from an import line, if any.
fn relative_target(import: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let mut it = import.split(quote);
        it.next();
        if let Some(spec) = it.next() {
            if spec.starts_with("./") || spec.starts_with("../") {
                return Some(spec.to_string());
            }
        }
    }
    None
}

/// Path-ish lowercase segments from an import statement, stopwords removed
/// (`use`, `import`, `from`, `as`, …) but order preserved.
fn import_segments(import: &str) -> Vec<String> {
    const IMPORT_NOISE: &[&str] = &[
        "use", "import", "from", "as", "pub", "static", "type", "crate", "self", "super",
    ];
    let mut out = Vec::new();
    for raw in tokens::raw_idents(import) {
        let t = raw.to_ascii_lowercase();
        if IMPORT_NOISE.contains(&t.as_str()) || t.len() < 2 {
            continue;
        }
        out.push(t);
        if out.len() >= 8 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Lang;

    fn fi(path: &str) -> FileInfo {
        FileInfo {
            path: path.to_string(),
            lang: Lang::from_path(path),
            size: 0,
            loc: 10,
            hash: String::new(),
        }
    }

    #[test]
    fn resolves_rust_use_and_python_from() {
        let files = vec![
            fi("src/auth/session.rs"),
            fi("src/main.rs"),
            fi("lib/db.py"),
            fi("app.py"),
        ];
        let r = ImportResolver::new(&files);
        match r.resolve("src/main.rs", "use crate::auth::session::Session") {
            Resolution::Internal(id) => assert_eq!(id, FileId(0)),
            _ => panic!("expected internal"),
        }
        match r.resolve("app.py", "from lib.db import connect") {
            Resolution::Internal(id) => assert_eq!(id, FileId(2)),
            _ => panic!("expected internal"),
        }
        match r.resolve("app.py", "import numpy as np") {
            Resolution::External(name) => assert_eq!(name, "numpy"),
            _ => panic!("expected external"),
        }
    }

    #[test]
    fn resolves_js_relative() {
        let files = vec![fi("web/src/util/http.ts"), fi("web/src/app.ts")];
        let r = ImportResolver::new(&files);
        match r.resolve("web/src/app.ts", "import { get } from './util/http'") {
            Resolution::Internal(id) => assert_eq!(id, FileId(0)),
            _ => panic!("expected internal"),
        }
    }

    #[test]
    fn pagerank_prefers_referenced() {
        let edges = vec![
            Edge {
                from: FileId(1),
                to: FileId(0),
                kind: EdgeKind::Import,
                weight: 1.0,
            },
            Edge {
                from: FileId(2),
                to: FileId(0),
                kind: EdgeKind::Import,
                weight: 1.0,
            },
        ];
        let pr = page_rank(3, &edges, None);
        assert!(pr[0] > pr[1]);
        assert!((pr.iter().sum::<f32>() - 1.0).abs() < 1e-3);
    }
}
