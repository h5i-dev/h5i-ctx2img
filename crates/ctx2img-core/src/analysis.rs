//! Orchestration: assemble the full analysis model from parts. The pieces
//! are exposed individually so ctx2img-index can interpose its parse cache.

use crate::embed::{self, Embeddings};
use crate::graph::{self, FileGraph};
use crate::history::History;
use crate::ingest::{self, IngestOptions, IngestedFile};
use crate::regions::{self, RegionTree};
use crate::relevance::{self, Relevance};
use crate::types::{FileInfo, ParsedFile};
use anyhow::Result;
use rayon::prelude::*;
use std::path::Path;

pub struct AnalyzeOptions {
    pub query: String,
    pub ingest: IngestOptions,
    /// Epoch seconds "now" for churn decay; pass a fixed value for
    /// reproducible output.
    pub now_epoch: u64,
    pub use_history: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        AnalyzeOptions {
            query: String::new(),
            ingest: IngestOptions::default(),
            now_epoch: 0,
            use_history: true,
        }
    }
}

pub struct Analysis {
    pub files: Vec<FileInfo>,
    pub parsed: Vec<ParsedFile>,
    pub graph: FileGraph,
    /// Global PageRank importance per file.
    pub importance: Vec<f32>,
    pub relevance: Relevance,
    pub tree: RegionTree,
    pub embeddings: Embeddings,
}

/// One-shot convenience: ingest + parse (uncached) + assemble.
pub fn analyze(root: &Path, opts: &AnalyzeOptions) -> Result<Analysis> {
    let ingested = ingest::ingest(root, &opts.ingest)?;
    let parsed: Vec<ParsedFile> = ingested
        .par_iter()
        .map(|f| crate::parse::parse_file(f.info.lang, &f.content))
        .collect();
    let files: Vec<FileInfo> = ingested.into_iter().map(|f| f.info).collect();
    let history = if opts.use_history {
        crate::history::collect(root, opts.now_epoch)
    } else {
        History::default()
    };
    Ok(assemble(files, parsed, history, &opts.query))
}

/// Core assembly given already-parsed files (cache-friendly entry point).
pub fn assemble(
    files: Vec<FileInfo>,
    parsed: Vec<ParsedFile>,
    history: History,
    query: &str,
) -> Analysis {
    let graph = graph::build(&files, &parsed, &history);
    let importance = graph::page_rank(files.len(), &graph.edges, None);
    let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let embeddings = embed::embed_all(&parsed, &paths);
    let relevance = relevance::score(
        query,
        &files,
        &parsed,
        &graph.edges,
        &embeddings,
        &importance,
        &history,
    );
    let tree = regions::build(&files);
    Analysis {
        files,
        parsed,
        graph,
        importance,
        relevance,
        tree,
        embeddings,
    }
}

/// Re-score relevance for a new query without re-analyzing (fast path for
/// consecutive maps over the same working tree).
pub fn rescore(analysis: &mut Analysis, query: &str, history: &History) {
    analysis.relevance = relevance::score(
        query,
        &analysis.files,
        &analysis.parsed,
        &analysis.graph.edges,
        &analysis.embeddings,
        &analysis.importance,
        history,
    );
}

pub use ingest::ingest as ingest_files;
pub type Ingested = IngestedFile;
