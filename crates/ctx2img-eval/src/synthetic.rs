//! Synthetic repository generator with planted ground truth.

use anyhow::Result;
use std::path::Path;

pub struct GroundTruth {
    /// Region directory that should reach elevation band 5 for [`QUERY`].
    pub hot_region: String,
    /// Region directory containing the hazard-flagged file.
    pub hazard_region: String,
    /// The query that makes `hot_region` hot.
    pub query: String,
    /// (importer_region, imported_region) planted dependency.
    pub dependency: (String, String),
}

pub const QUERY: &str = "zephyr quantum flux calibration";

/// Deterministically generate a synthetic repo under `dir`.
pub fn generate_repo(dir: &Path, n_regions: usize, files_per_region: usize) -> Result<GroundTruth> {
    let n_regions = n_regions.max(4);
    let names = region_names(n_regions);
    for (ri, name) in names.iter().enumerate() {
        let region_dir = dir.join(name);
        std::fs::create_dir_all(&region_dir)?;
        for fi in 0..files_per_region {
            let mut body = String::new();
            // cross-region import chain: region i imports from region i+1
            if fi == 0 && ri + 1 < names.len() {
                body.push_str(&format!(
                    "use crate::{}::worker_{};\n",
                    names[ri + 1],
                    ri + 1
                ));
            }
            body.push_str(&format!(
                "pub struct Widget{ri}_{fi};\npub fn worker_{ri}() -> u32 {{ {fi} }}\n"
            ));
            for k in 0..6 {
                body.push_str(&format!(
                    "pub fn helper_{ri}_{fi}_{k}(input: u32) -> u32 {{ input + {k} }}\n"
                ));
            }
            // plant the hot topic in region 1
            if ri == 1 {
                body.push_str(
                    "pub fn zephyr_quantum_flux() -> f64 { 42.0 }\npub fn flux_calibration(zephyr: f64) -> f64 { zephyr * 2.0 }\n",
                );
            }
            // plant a hazard in region 2's first file
            if ri == 2 && fi == 0 {
                body.push_str(
                    "pub fn fetch_remote() { let key = std::env::var(\"API_KEY\"); let _ = reqwest::get(key); }\n",
                );
            }
            std::fs::write(region_dir.join(format!("mod_{fi}.rs")), body)?;
        }
    }
    Ok(GroundTruth {
        hot_region: names[1].clone(),
        hazard_region: names[2].clone(),
        query: QUERY.to_string(),
        dependency: (names[0].clone(), names[1].clone()),
    })
}

fn region_names(n: usize) -> Vec<String> {
    const BASE: [&str; 12] = [
        "engine",
        "parser",
        "storage",
        "network",
        "auth",
        "metrics",
        "scheduler",
        "codec",
        "cache",
        "router",
        "plugin",
        "telemetry",
    ];
    (0..n)
        .map(|i| format!("src/{}", BASE[i % BASE.len()]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_repo() {
        let dir = tempfile::tempdir().unwrap();
        let gt = generate_repo(dir.path(), 5, 3).unwrap();
        assert!(dir.path().join(&gt.hot_region).join("mod_0.rs").exists());
        let hot =
            std::fs::read_to_string(dir.path().join(&gt.hot_region).join("mod_0.rs")).unwrap();
        assert!(hot.contains("zephyr_quantum_flux"));
    }
}
