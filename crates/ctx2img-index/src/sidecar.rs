//! The sidecar index: handle → exact source resolution. Written next to the
//! atlas, consumed by `ctx2img read`/`ctx2img zoom` — never placed in model context.

use crate::workspace::Built;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Sidecar {
    pub version: u32,
    pub query: String,
    /// F handle -> file record
    pub files: BTreeMap<String, FileRecord>,
    /// R handle -> region record
    pub regions: BTreeMap<String, RegionRecord>,
    /// X handle -> external package name
    pub externals: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub loc: u32,
    pub band: u8,
    pub hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegionRecord {
    pub path: String,
    pub band: u8,
    pub files: Vec<String>,
}

pub fn build_sidecar(built: &Built, query: &str) -> Sidecar {
    let a = &built.analysis;
    let sums = built.region_summaries();
    let files: BTreeMap<String, FileRecord> = a
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            (
                built.file_handles[i].clone(),
                FileRecord {
                    path: f.path.clone(),
                    loc: f.loc,
                    band: a.relevance.bands[i],
                    hash: f.hash.clone(),
                },
            )
        })
        .collect();
    let regions: BTreeMap<String, RegionRecord> = a
        .tree
        .regions
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            (
                built.region_handles[ri].clone(),
                RegionRecord {
                    path: r.display_name().to_string(),
                    band: sums[ri].band,
                    files: r
                        .files
                        .iter()
                        .map(|f| built.file_handles[f.idx()].clone())
                        .collect(),
                },
            )
        })
        .collect();
    let externals: BTreeMap<String, String> = a
        .graph
        .external
        .iter()
        .zip(&built.ext_handles)
        .map(|((name, _), h)| (h.clone(), name.clone()))
        .collect();
    Sidecar {
        version: 1,
        query: query.to_string(),
        files,
        regions,
        externals,
    }
}

pub fn write(sidecar: &Sidecar, path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(sidecar)?)?;
    Ok(())
}
