//! Git history signals via a `git` subprocess: recency-weighted churn and
//! co-change coupling. Subprocess over a git library is deliberate for v1:
//! zero build-time cost, fast (`git log` streams), and trivially robust —
//! any failure degrades to "no history signal".

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default)]
pub struct History {
    /// path -> recency-weighted commit count (half-life ~90 days).
    pub churn: HashMap<String, f32>,
    /// (path_a, path_b) with a < b -> co-commit count.
    pub co_change: HashMap<(String, String), u32>,
}

const MAX_COMMITS: &str = "1000";
/// Commits touching more than this many files are treated as mechanical
/// (formatting, vendoring) and skipped for co-change purposes.
const MAX_COCHANGE_FILES: usize = 30;
const HALF_LIFE_SECS: f64 = 90.0 * 24.0 * 3600.0;

/// `now_epoch` is passed in (not read from the clock) so results are
/// reproducible in tests and cacheable.
pub fn collect(root: &Path, now_epoch: u64) -> History {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--no-renames",
            "--name-only",
            "-n",
            MAX_COMMITS,
            "--pretty=format:@%ct",
        ])
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return History::default(),
    };
    let text = String::from_utf8_lossy(&out.stdout);

    let mut hist = History::default();
    let mut commit_files: Vec<String> = Vec::new();
    let mut commit_ts: u64 = 0;
    let flush = |files: &mut Vec<String>, ts: u64, hist: &mut History| {
        if files.is_empty() {
            return;
        }
        let age = (now_epoch.saturating_sub(ts)) as f64;
        let weight = (0.5f64).powf(age / HALF_LIFE_SECS) as f32;
        for f in files.iter() {
            *hist.churn.entry(f.clone()).or_insert(0.0) += weight.max(0.01);
        }
        if files.len() <= MAX_COCHANGE_FILES {
            let mut sorted = files.clone();
            sorted.sort();
            sorted.dedup();
            for i in 0..sorted.len() {
                for j in (i + 1)..sorted.len() {
                    *hist
                        .co_change
                        .entry((sorted[i].clone(), sorted[j].clone()))
                        .or_insert(0) += 1;
                }
            }
        }
        files.clear();
    };

    for line in text.lines() {
        if let Some(ts) = line.strip_prefix('@') {
            flush(&mut commit_files, commit_ts, &mut hist);
            commit_ts = ts.trim().parse().unwrap_or(now_epoch);
        } else if !line.trim().is_empty() {
            commit_files.push(line.trim().to_string());
        }
    }
    flush(&mut commit_files, commit_ts, &mut hist);
    hist
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_git_dir_degrades_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let h = collect(dir.path(), 1_700_000_000);
        assert!(h.churn.is_empty());
    }
}
