//! Parse cache keyed by content hash: unchanged files never re-parse.
//! This is what makes warm rebuilds fast — tree-sitter parsing dominates
//! cold build time.

use ctx2img_core::types::ParsedFile;
use std::collections::HashMap;
use std::path::Path;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct ParseCache {
    /// content blake3 hex -> parse output
    entries: HashMap<String, ParsedFile>,
}

impl ParseCache {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, hash: &str) -> Option<&ParsedFile> {
        self.entries.get(hash)
    }

    pub fn insert(&mut self, hash: String, parsed: ParsedFile) {
        self.entries.insert(hash, parsed);
    }

    /// Persist only entries referenced by the current file set, so the cache
    /// never grows past one generation of stale entries.
    pub fn save_pruned(&self, path: &Path, live_hashes: &[String]) -> anyhow::Result<()> {
        let live: std::collections::HashSet<&str> =
            live_hashes.iter().map(|s| s.as_str()).collect();
        let pruned = ParseCache {
            entries: self
                .entries
                .iter()
                .filter(|(h, _)| live.contains(h.as_str()))
                .map(|(h, p)| (h.clone(), p.clone()))
                .collect(),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec(&pruned)?)?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cache.json");
        let mut c = ParseCache::default();
        c.insert("aaa".into(), ParsedFile::default());
        c.insert("bbb".into(), ParsedFile::default());
        c.save_pruned(&p, &["aaa".to_string()]).unwrap();
        let c2 = ParseCache::load(&p);
        assert!(c2.get("aaa").is_some());
        assert!(c2.get("bbb").is_none());
    }
}
