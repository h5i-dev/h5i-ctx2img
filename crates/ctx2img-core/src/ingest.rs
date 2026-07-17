//! Repository walk: gitignore-aware, size-capped, binary-filtered.

use crate::types::{FileInfo, Lang};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::path::Path;

/// A file with its content in memory, ready for parsing.
pub struct IngestedFile {
    pub info: FileInfo,
    pub content: String,
}

pub struct IngestOptions {
    /// Files larger than this are indexed by name only (content skipped).
    pub max_file_bytes: u64,
    /// Hard cap on file count as a runaway guard; the walk stops beyond it.
    pub max_files: usize,
}

impl Default for IngestOptions {
    fn default() -> Self {
        IngestOptions {
            max_file_bytes: 1_000_000,
            max_files: 100_000,
        }
    }
}

/// Walk `root`, honoring `.gitignore` / `.ignore` / `.ctx2imgignore`, and read
/// text contents. Deterministic: results sorted by path.
pub fn ingest(root: &Path, opts: &IngestOptions) -> Result<Vec<IngestedFile>> {
    let mut walker = ignore::WalkBuilder::new(root);
    walker
        .hidden(true)
        .require_git(false)
        .add_custom_ignore_filename(".ctx2imgignore")
        .follow_links(false);
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in walker.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        // .git is covered by hidden(); also skip our own cache dir.
        if entry
            .path()
            .components()
            .any(|c| c.as_os_str() == ".ctx2img" || c.as_os_str() == ".h5i-ctx")
        {
            continue;
        }
        paths.push(entry.into_path());
        if paths.len() >= opts.max_files {
            break;
        }
    }
    paths.sort();

    let mut files: Vec<IngestedFile> = paths
        .par_iter()
        .filter_map(|p| read_one(root, p, opts).ok().flatten())
        .collect();
    files.sort_by(|a, b| a.info.path.cmp(&b.info.path));
    Ok(files)
}

fn read_one(root: &Path, path: &Path, opts: &IngestOptions) -> Result<Option<IngestedFile>> {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let meta = std::fs::metadata(path).with_context(|| format!("stat {rel}"))?;
    let size = meta.len();
    let oversized = size > opts.max_file_bytes;
    let bytes = if oversized {
        Vec::new()
    } else {
        std::fs::read(path).with_context(|| format!("read {rel}"))?
    };
    if looks_binary(&bytes) {
        return Ok(None);
    }
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let loc = content.lines().filter(|l| !l.trim().is_empty()).count() as u32;
    let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    Ok(Some(IngestedFile {
        info: FileInfo {
            lang: Lang::from_path(&rel),
            path: rel,
            size,
            loc,
            hash,
        },
        content,
    }))
}

fn looks_binary(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(4096)];
    probe.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_and_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/junk.rs"), "fn x() {}\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        std::fs::write(dir.path().join("bin.dat"), [0u8, 1, 2]).unwrap();

        let files = ingest(dir.path(), &IngestOptions::default()).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.info.path.as_str()).collect();
        assert!(paths.contains(&"a.rs"));
        assert!(!paths.iter().any(|p| p.starts_with("target/")));
        assert!(!paths.contains(&"bin.dat"));
    }
}
