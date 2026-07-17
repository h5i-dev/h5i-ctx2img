//! The `.ctx2img/` workspace: cache-aware build orchestration tying the pure
//! core to persistent handles.

use crate::cache::ParseCache;
use crate::handles::{HandleRegistry, Kind};
use anyhow::{Context, Result};
use ctx2img_core::analysis::{assemble, Analysis};
use ctx2img_core::graph::{aggregate_edges, EdgeKind};
use ctx2img_core::ingest::{ingest, IngestOptions};
use ctx2img_core::types::{FileId, FileInfo, ParsedFile};
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Workspace {
    pub root: PathBuf,
    pub dir: PathBuf,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct BuildStats {
    pub files: usize,
    pub cache_hits: usize,
    pub ingest_ms: u128,
    pub parse_ms: u128,
    pub assemble_ms: u128,
}

pub struct Built {
    pub analysis: Analysis,
    pub registry: HandleRegistry,
    /// Parallel to `analysis.files`.
    pub file_handles: Vec<String>,
    /// Parallel to `analysis.tree.regions`.
    pub region_handles: Vec<String>,
    /// Parallel to `analysis.graph.external`.
    pub ext_handles: Vec<String>,
    pub stats: BuildStats,
}

impl Workspace {
    pub fn open(root: &Path) -> Result<Workspace> {
        let root = root
            .canonicalize()
            .with_context(|| format!("no such dir: {root:?}"))?;
        let dir = root.join(".ctx2img");
        std::fs::create_dir_all(&dir)?;
        // Self-ignoring cache dir: git never sees it unless the user opts in.
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n")?;
        }
        Ok(Workspace { root, dir })
    }

    pub fn registry_path(&self) -> PathBuf {
        self.dir.join("handles.json")
    }

    fn cache_path(&self) -> PathBuf {
        self.dir.join("parse-cache.json")
    }

    pub fn layout_path(&self) -> PathBuf {
        self.dir.join("layout.json")
    }

    /// Full build: ingest → cached parse → history → assemble → handles.
    pub fn build(&self, query: &str, now_epoch: u64, use_history: bool) -> Result<Built> {
        let t0 = Instant::now();
        let ingested = ingest(&self.root, &IngestOptions::default())?;
        let ingest_ms = t0.elapsed().as_millis();

        let t1 = Instant::now();
        let mut cache = ParseCache::load(&self.cache_path());
        let hits_before: Vec<Option<ParsedFile>> = ingested
            .iter()
            .map(|f| cache.get(&f.info.hash).cloned())
            .collect();
        let cache_hits = hits_before.iter().filter(|h| h.is_some()).count();
        let parsed: Vec<ParsedFile> = ingested
            .par_iter()
            .zip(hits_before)
            .map(|(f, hit)| {
                hit.unwrap_or_else(|| ctx2img_core::parse::parse_file(f.info.lang, &f.content))
            })
            .collect();
        for (f, p) in ingested.iter().zip(parsed.iter()) {
            if cache.get(&f.info.hash).is_none() {
                cache.insert(f.info.hash.clone(), p.clone());
            }
        }
        let live_hashes: Vec<String> = ingested.iter().map(|f| f.info.hash.clone()).collect();
        cache.save_pruned(&self.cache_path(), &live_hashes)?;
        let parse_ms = t1.elapsed().as_millis();

        let t2 = Instant::now();
        let history = if use_history {
            ctx2img_core::history::collect(&self.root, now_epoch)
        } else {
            ctx2img_core::history::History::default()
        };
        let files: Vec<FileInfo> = ingested.into_iter().map(|f| f.info).collect();
        let analysis = assemble(files, parsed, history, query);
        let assemble_ms = t2.elapsed().as_millis();

        // --- handles ---
        let mut registry = HandleRegistry::load(&self.registry_path());
        let live: BTreeSet<String> = analysis.files.iter().map(|f| f.path.clone()).collect();
        registry.sweep_files(&live);
        let file_handles: Vec<String> = analysis
            .files
            .iter()
            .map(|f| registry.assign(Kind::File, &f.path, &f.hash))
            .collect();
        let region_handles: Vec<String> = analysis
            .tree
            .regions
            .iter()
            .map(|r| registry.assign(Kind::Region, r.display_name(), ""))
            .collect();
        let ext_handles: Vec<String> = analysis
            .graph
            .external
            .iter()
            .map(|(name, _)| registry.assign(Kind::External, name, ""))
            .collect();
        registry.save(&self.registry_path())?;

        let stats = BuildStats {
            files: analysis.files.len(),
            cache_hits,
            ingest_ms,
            parse_ms,
            assemble_ms,
        };
        Ok(Built {
            analysis,
            registry,
            file_handles,
            region_handles,
            ext_handles,
            stats,
        })
    }
}

/// Per-region aggregates shared by the legend and the map scene.
#[derive(Debug, Clone)]
pub struct RegionSummary {
    pub band: u8,
    pub hazards: u8,
    /// (file idx, relevance score) — best first, all members.
    pub ranked_files: Vec<(usize, f32)>,
    /// (target region idx, kind, weight) — strongest first, deduped by target.
    pub out_edges: Vec<(usize, EdgeKind, f32)>,
}

impl Built {
    pub fn region_summaries(&self) -> Vec<RegionSummary> {
        let a = &self.analysis;
        let n_regions = a.tree.regions.len();
        let agg = aggregate_edges(&a.graph.edges, &a.tree.assignment, n_regions);
        let mut out: Vec<RegionSummary> = a
            .tree
            .regions
            .iter()
            .map(|r| {
                let mut band = 1u8;
                let mut hazards = 0u8;
                let mut ranked: Vec<(usize, f32)> = r
                    .files
                    .iter()
                    .map(|&FileId(i)| {
                        let i = i as usize;
                        band = band.max(a.relevance.bands[i]);
                        hazards |= a.parsed[i].hazards;
                        // blend relevance with importance so empty-query maps
                        // still rank meaningfully
                        (i, a.relevance.scores[i] + a.importance[i] * 2.0)
                    })
                    .collect();
                ranked.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
                RegionSummary {
                    band,
                    hazards,
                    ranked_files: ranked,
                    out_edges: Vec::new(),
                }
            })
            .collect();
        for (from, to, kind, w) in agg {
            let edges = &mut out[from].out_edges;
            if edges.iter().any(|(t, _, _)| *t == to) {
                continue; // keep only the strongest kind per target
            }
            edges.push((to, kind, w));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("src/auth")).unwrap();
        std::fs::create_dir_all(p.join("src/db")).unwrap();
        std::fs::write(
            p.join("src/auth/session.rs"),
            "use crate::db::pool;\npub struct Session { pub expires: u64 }\npub fn session_expiry(s: &Session) -> u64 { s.expires }\n",
        )
        .unwrap();
        std::fs::write(
            p.join("src/db/pool.rs"),
            "pub fn connect() -> Pool { Pool::new() }\npub struct Pool;\n",
        )
        .unwrap();
        std::fs::write(
            p.join("src/main.rs"),
            "use crate::auth::session::Session;\nfn main() {}\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn build_twice_hits_cache_and_keeps_handles() {
        let dir = demo_repo();
        let ws = Workspace::open(dir.path()).unwrap();
        let b1 = ws.build("session expiry", 1_700_000_000, false).unwrap();
        assert_eq!(b1.stats.cache_hits, 0);
        let session_handle = {
            let i = b1
                .analysis
                .files
                .iter()
                .position(|f| f.path.ends_with("session.rs"))
                .unwrap();
            b1.file_handles[i].clone()
        };

        let b2 = ws
            .build("different query entirely", 1_700_000_000, false)
            .unwrap();
        assert_eq!(
            b2.stats.cache_hits, b2.stats.files,
            "second build should be fully cached"
        );
        let i = b2
            .analysis
            .files
            .iter()
            .position(|f| f.path.ends_with("session.rs"))
            .unwrap();
        assert_eq!(
            b2.file_handles[i], session_handle,
            "handles stable across queries"
        );
    }

    #[test]
    fn summaries_have_edges_and_bands() {
        let dir = demo_repo();
        let ws = Workspace::open(dir.path()).unwrap();
        let b = ws.build("session expiry", 1_700_000_000, false).unwrap();
        let sums = b.region_summaries();
        assert_eq!(sums.len(), b.analysis.tree.regions.len());
        let auth_idx = b
            .analysis
            .tree
            .regions
            .iter()
            .position(|r| r.path.contains("auth"))
            .unwrap();
        assert!(
            sums[auth_idx].band >= 4,
            "queried region should be high elevation"
        );
        assert!(
            sums.iter().any(|s| !s.out_edges.is_empty()),
            "imports should aggregate"
        );
    }
}
