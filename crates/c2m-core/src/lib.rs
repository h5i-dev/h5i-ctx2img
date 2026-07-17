//! c2m-core — repository analysis: ingest, parse, graph, relevance.
//!
//! Pure analysis: given a repository root, produce an [`Analysis`] model
//! (files, symbols, dependency graph, importance, query relevance).
//! No rendering, no persistence — those live in sibling crates.

pub mod analysis;
pub mod embed;
pub mod factsheet;
pub mod graph;
pub mod hazard;
pub mod history;
pub mod ingest;
pub mod parse;
pub mod regions;
pub mod relevance;
pub mod sections;
pub mod tokens;
pub mod types;

pub use analysis::{analyze, Analysis, AnalyzeOptions};
pub use regions::{Region, RegionTree};
pub use relevance::{Relevance, BANDS};
pub use types::{FileId, FileInfo, Lang, ParsedFile, Symbol, SymbolKind};
