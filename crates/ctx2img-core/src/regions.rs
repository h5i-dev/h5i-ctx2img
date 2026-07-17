//! Region tree: the map's territorial hierarchy, derived from directories
//! with adaptive splitting (a repo where everything lives under `src/`
//! descends into it) and a cap so the L1 map stays readable.

use crate::types::{FileId, FileInfo, Lang};
use std::collections::BTreeMap;

/// A display region: one country on the map.
#[derive(Debug, Clone)]
pub struct Region {
    /// Directory path ("" = repo root pseudo-region, shown as "/").
    pub path: String,
    /// Member files (direct + descendants not claimed by a sibling region).
    pub files: Vec<FileId>,
    pub loc: u64,
    pub dominant_lang: Lang,
    /// Directory depth (for border weight styling).
    pub depth: u8,
}

impl Region {
    pub fn display_name(&self) -> &str {
        if self.path.is_empty() {
            "/"
        } else {
            &self.path
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegionTree {
    pub regions: Vec<Region>,
    /// file index -> region index (usize::MAX if the repo is empty).
    pub assignment: Vec<usize>,
}

/// Max countries on the L1 map. Beyond this, smallest regions merge into "…".
const MAX_REGIONS: usize = 36;
/// Stop splitting once the map is this busy (dominant or not).
const SPLIT_CEILING: usize = 24;
/// A region is split into subdirectories while it dominates the repo —
/// however many siblings it has (`crates/` holding 70% of a workspace must
/// split even when five top-level dirs exist).
const DOMINANCE: f32 = 0.5;

pub fn build(files: &[FileInfo]) -> RegionTree {
    // Start with top-level dirs (depth 1) + root files.
    let mut regions: Vec<(String, Vec<FileId>)> = group_by_prefix(files, "");

    // Adaptive descent: while one region dominates, split it.
    loop {
        if regions.len() >= SPLIT_CEILING {
            break;
        }
        let total: usize = regions.iter().map(|(_, f)| f.len()).sum();
        let Some((i, _)) = regions
            .iter()
            .enumerate()
            .filter(|(_, (p, f))| !p.is_empty() && f.len() as f32 > total as f32 * DOMINANCE)
            .max_by_key(|(_, (_, f))| f.len())
        else {
            break;
        };
        let (path, members) = regions.remove(i);
        let mut subs = group_by_prefix_members(files, &path, &members);
        // Chain-descend through single-child directories (src -> src/auth),
        // but stop if the only "child" is the directory's own direct files.
        if subs.len() == 1 && subs[0].0 == path {
            regions.push((path, members));
            break;
        }
        regions.append(&mut subs);
    }

    // Cap: merge the smallest tail into a single "…" region.
    regions.sort_by_key(|(p, f)| (std::cmp::Reverse(f.len()), p.clone()));
    if regions.len() > MAX_REGIONS {
        let tail: Vec<(String, Vec<FileId>)> = regions.split_off(MAX_REGIONS - 1);
        let mut misc: Vec<FileId> = tail.into_iter().flat_map(|(_, f)| f).collect();
        misc.sort();
        regions.push(("…".to_string(), misc));
    }
    regions.sort_by(|a, b| a.0.cmp(&b.0));

    let mut assignment = vec![usize::MAX; files.len()];
    let built: Vec<Region> = regions
        .into_iter()
        .enumerate()
        .map(|(ri, (path, members))| {
            let mut loc = 0u64;
            let mut lang_loc: BTreeMap<Lang, u64> = BTreeMap::new();
            for &id in &members {
                assignment[id.idx()] = ri;
                let f = &files[id.idx()];
                loc += f.loc as u64;
                *lang_loc.entry(f.lang).or_insert(0) += f.loc as u64;
            }
            let dominant_lang = lang_loc
                .iter()
                .filter(|(l, _)| !matches!(l, Lang::Markdown | Lang::Config | Lang::Other))
                .max_by_key(|(_, &v)| v)
                .or_else(|| lang_loc.iter().max_by_key(|(_, &v)| v))
                .map(|(&l, _)| l)
                .unwrap_or(Lang::Other);
            let depth = if path.is_empty() {
                0
            } else {
                path.matches('/').count() as u8 + 1
            };
            Region {
                path,
                files: members,
                loc,
                dominant_lang,
                depth,
            }
        })
        .collect();

    RegionTree {
        regions: built,
        assignment,
    }
}

/// Children of `prefix`: one region per immediate subdirectory, plus a
/// pseudo-region for files directly inside `prefix`.
fn group_by_prefix(files: &[FileInfo], prefix: &str) -> Vec<(String, Vec<FileId>)> {
    let all: Vec<FileId> = (0..files.len() as u32).map(FileId).collect();
    group_by_prefix_members(files, prefix, &all)
}

fn group_by_prefix_members(
    files: &[FileInfo],
    prefix: &str,
    members: &[FileId],
) -> Vec<(String, Vec<FileId>)> {
    let mut groups: BTreeMap<String, Vec<FileId>> = BTreeMap::new();
    for &id in members {
        let path = &files[id.idx()].path;
        let rest = if prefix.is_empty() {
            path.as_str()
        } else {
            match path.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) {
                Some(r) => r,
                None => continue,
            }
        };
        let key = match rest.split_once('/') {
            Some((dir, _)) => {
                if prefix.is_empty() {
                    dir.to_string()
                } else {
                    format!("{prefix}/{dir}")
                }
            }
            None => prefix.to_string(), // file directly in prefix
        };
        groups.entry(key).or_default().push(id);
    }
    groups.into_iter().collect()
}

/// Zoom support: sub-regions of one region (its immediate subdirectories +
/// direct files), for rendering an L2 tile with the same machinery.
pub fn subdivide(files: &[FileInfo], region: &Region) -> RegionTree {
    let groups = group_by_prefix_members(files, &region.path, &region.files);
    let mut assignment = vec![usize::MAX; files.len()];
    let regions: Vec<Region> = groups
        .into_iter()
        .enumerate()
        .map(|(ri, (path, members))| {
            let mut loc = 0u64;
            let mut lang_loc: BTreeMap<Lang, u64> = BTreeMap::new();
            for &id in &members {
                assignment[id.idx()] = ri;
                loc += files[id.idx()].loc as u64;
                *lang_loc.entry(files[id.idx()].lang).or_insert(0) += files[id.idx()].loc as u64;
            }
            let dominant_lang = lang_loc
                .iter()
                .max_by_key(|(_, &v)| v)
                .map(|(&l, _)| l)
                .unwrap_or(Lang::Other);
            let depth = if path.is_empty() {
                0
            } else {
                path.matches('/').count() as u8 + 1
            };
            Region {
                path,
                files: members,
                loc,
                dominant_lang,
                depth,
            }
        })
        .collect();
    RegionTree {
        regions,
        assignment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(path: &str, loc: u32) -> FileInfo {
        FileInfo {
            path: path.into(),
            lang: Lang::from_path(path),
            size: 0,
            loc,
            hash: String::new(),
        }
    }

    #[test]
    fn descends_into_dominant_src() {
        let files = vec![
            fi("src/auth/session.rs", 100),
            fi("src/auth/jwt.rs", 80),
            fi("src/db/pool.rs", 90),
            fi("src/main.rs", 20),
            fi("README.md", 10),
        ];
        let tree = build(&files);
        let names: Vec<&str> = tree.regions.iter().map(|r| r.path.as_str()).collect();
        assert!(
            names.contains(&"src/auth"),
            "should split dominant src/: {names:?}"
        );
        assert!(names.contains(&"src/db"));
        // every file assigned
        assert!(tree.assignment.iter().all(|&a| a != usize::MAX));
    }

    #[test]
    fn subdivide_gives_children() {
        let files = vec![
            fi("src/auth/session.rs", 10),
            fi("src/auth/oauth/google.rs", 5),
        ];
        let tree = build(&files);
        let auth = tree
            .regions
            .iter()
            .find(|r| r.path.contains("auth"))
            .unwrap();
        let sub = subdivide(&files, auth);
        assert!(!sub.regions.is_empty());
    }
}
