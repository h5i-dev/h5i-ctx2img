//! Stable handles: `F103` (file), `R3` (region), `S1042` (symbol), `X7`
//! (external dep). Handles appear in conversation transcripts and cached
//! prompts, so they are **never reused**: deletions tombstone, renames are
//! followed by content hash.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    File,
    Region,
    Symbol,
    External,
}

impl Kind {
    fn prefix(self) -> char {
        match self {
            Kind::File => 'F',
            Kind::Region => 'R',
            Kind::Symbol => 'S',
            Kind::External => 'X',
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub handle: String,
    /// File: repo-relative path. Region: directory path. Symbol:
    /// `path#name#kind`. External: package name.
    pub key: String,
    /// Content hash for files (rename following); empty otherwise.
    #[serde(default)]
    pub hash: String,
    pub kind: Kind,
    /// Tombstoned: key no longer exists, handle stays reserved.
    #[serde(default)]
    pub dead: bool,
    /// Symbol line range, for `read S…` (1-based, inclusive).
    #[serde(default)]
    pub lines: Option<(u32, u32)>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HandleRegistry {
    next: BTreeMap<char, u32>,
    /// key -> entry (live entries).
    entries: BTreeMap<String, Entry>,
    /// handle -> key, includes tombstones.
    by_handle: BTreeMap<String, String>,
}

impl HandleRegistry {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }

    fn mint(&mut self, kind: Kind) -> String {
        let counter = self.next.entry(kind.prefix()).or_insert(1);
        let handle = format!("{}{}", kind.prefix(), *counter);
        *counter += 1;
        handle
    }

    fn key_for(kind: Kind, key: &str) -> String {
        format!("{}:{}", kind.prefix(), key)
    }

    /// Get-or-assign the handle for `key`. For files, pass the content hash:
    /// a missing path with a matching hash on a dead entry is a rename and
    /// keeps its old handle.
    pub fn assign(&mut self, kind: Kind, key: &str, hash: &str) -> String {
        let full = Self::key_for(kind, key);
        if let Some(e) = self.entries.get_mut(&full) {
            e.dead = false;
            if !hash.is_empty() {
                e.hash = hash.to_string();
            }
            return e.handle.clone();
        }
        // rename following: same content hash on a dead file entry
        if kind == Kind::File && !hash.is_empty() {
            let found = self
                .entries
                .iter()
                .find(|(_, e)| e.kind == Kind::File && e.dead && e.hash == hash)
                .map(|(k, e)| (k.clone(), e.handle.clone()));
            if let Some((old_key, handle)) = found {
                let mut e = self.entries.remove(&old_key).unwrap();
                e.key = key.to_string();
                e.dead = false;
                self.by_handle.insert(handle.clone(), full.clone());
                self.entries.insert(full, e);
                return handle;
            }
        }
        let handle = self.mint(kind);
        self.by_handle.insert(handle.clone(), full.clone());
        self.entries.insert(
            full,
            Entry {
                handle: handle.clone(),
                key: key.to_string(),
                hash: hash.to_string(),
                kind,
                dead: false,
                lines: None,
            },
        );
        handle
    }

    pub fn assign_symbol(
        &mut self,
        path: &str,
        name: &str,
        kind_tag: &str,
        lines: (u32, u32),
    ) -> String {
        let key = format!("{path}#{name}#{kind_tag}");
        let h = self.assign(Kind::Symbol, &key, "");
        if let Some(e) = self.entries.get_mut(&Self::key_for(Kind::Symbol, &key)) {
            e.lines = Some(lines);
        }
        h
    }

    /// Mark all file entries not in `live_paths` as dead (handles reserved).
    pub fn sweep_files(&mut self, live_paths: &std::collections::BTreeSet<String>) {
        for e in self.entries.values_mut() {
            if e.kind == Kind::File && !live_paths.contains(&e.key) {
                e.dead = true;
            }
        }
    }

    /// Resolve a handle like "F103" to its entry.
    pub fn resolve(&self, handle: &str) -> Option<&Entry> {
        let key = self.by_handle.get(handle)?;
        self.entries.get(key).filter(|e| !e.dead)
    }

    pub fn handle_of(&self, kind: Kind, key: &str) -> Option<&str> {
        self.entries
            .get(&Self::key_for(kind, key))
            .filter(|e| !e.dead)
            .map(|e| e.handle.as_str())
    }

    pub fn live_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().filter(|e| !e.dead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn handles_stable_and_never_reused() {
        let mut r = HandleRegistry::default();
        let a = r.assign(Kind::File, "src/a.rs", "hash_a");
        let b = r.assign(Kind::File, "src/b.rs", "hash_b");
        assert_eq!(a, "F1");
        assert_eq!(b, "F2");
        assert_eq!(r.assign(Kind::File, "src/a.rs", "hash_a"), "F1");

        // delete a, add c: c must NOT get F1
        let live: BTreeSet<String> = ["src/b.rs".to_string()].into_iter().collect();
        r.sweep_files(&live);
        assert!(r.resolve("F1").is_none());
        let c = r.assign(Kind::File, "src/c.rs", "hash_c");
        assert_eq!(c, "F3");
    }

    #[test]
    fn rename_followed_by_hash() {
        let mut r = HandleRegistry::default();
        let a = r.assign(Kind::File, "src/old.rs", "same_hash");
        let live: BTreeSet<String> = BTreeSet::new();
        r.sweep_files(&live);
        let b = r.assign(Kind::File, "src/new.rs", "same_hash");
        assert_eq!(a, b, "rename should keep its handle");
        assert_eq!(r.resolve(&a).unwrap().key, "src/new.rs");
    }

    #[test]
    fn roundtrips_via_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("handles.json");
        let mut r = HandleRegistry::default();
        r.assign(Kind::Region, "src/auth", "");
        r.save(&p).unwrap();
        let r2 = HandleRegistry::load(&p);
        assert_eq!(r2.handle_of(Kind::Region, "src/auth"), Some("R1"));
    }
}
