//! Probe questions with objective answers, generated against a built
//! workspace + ground truth. A model reading the atlas correctly must
//! answer these; failures localize what the rendering got wrong.

use crate::synthetic::GroundTruth;
use ctx2img_index::workspace::Built;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Probe {
    pub id: String,
    pub question: String,
    /// Any of these substrings in the answer counts as correct.
    pub accept: Vec<String>,
    /// What this probe measures (elevation/hazard/handles/edges).
    pub dimension: String,
}

pub fn build_probes(built: &Built, gt: &GroundTruth) -> Vec<Probe> {
    let a = &built.analysis;
    let region_handle = |path: &str| -> Option<String> {
        a.tree
            .regions
            .iter()
            .position(|r| r.path == path)
            .map(|ri| built.region_handles[ri].clone())
    };
    let mut probes = Vec::new();

    if let Some(h) = region_handle(&gt.hot_region) {
        probes.push(Probe {
            id: "elevation-summit".into(),
            question: "Which region handle has the highest elevation (▲5) on this map? Answer with the handle only.".into(),
            accept: vec![h, gt.hot_region.clone()],
            dimension: "elevation".into(),
        });
    }
    if let Some(h) = region_handle(&gt.hazard_region) {
        probes.push(Probe {
            id: "hazard-region".into(),
            question: "Which region is marked with the trust-hazard overlay (red hatch / ⚠)? Answer with the handle only.".into(),
            accept: vec![h, gt.hazard_region.clone()],
            dimension: "hazard".into(),
        });
    }
    if let (Some(from), Some(to)) = (
        region_handle(&gt.dependency.0),
        region_handle(&gt.dependency.1),
    ) {
        probes.push(Probe {
            id: "dependency-edge".into(),
            question: format!(
                "Region {from} depends on exactly one other region via an import road. Which one? Answer with the handle only."
            ),
            accept: vec![to, gt.dependency.1.clone()],
            dimension: "edges".into(),
        });
    }
    // handle-reading probe: name a file handle from the hot region roster
    if let Some(ri) = a.tree.regions.iter().position(|r| r.path == gt.hot_region) {
        let sums = built.region_summaries();
        if let Some(&(fi, _)) = sums[ri].ranked_files.first() {
            probes.push(Probe {
                id: "handle-read".into(),
                question: format!(
                    "What is the file handle of the top-ranked file inside region {}? Answer with the handle only.",
                    built.region_handles[ri]
                ),
                accept: vec![built.file_handles[fi].clone()],
                dimension: "handles".into(),
            });
        }
    }
    probes
}

#[derive(Debug, Serialize)]
pub struct ProbeScore {
    pub id: String,
    pub dimension: String,
    pub pass: bool,
}

/// Grade free-text answers (one per probe, in order).
pub fn score_answers(probes: &[Probe], answers: &[String]) -> Vec<ProbeScore> {
    probes
        .iter()
        .zip(answers)
        .map(|(p, ans)| ProbeScore {
            id: p.id.clone(),
            dimension: p.dimension.clone(),
            pass: p.accept.iter().any(|a| ans.contains(a.as_str())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx2img_index::Workspace;

    #[test]
    fn probes_reference_real_handles() {
        let dir = tempfile::tempdir().unwrap();
        let gt = crate::synthetic::generate_repo(dir.path(), 5, 3).unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let built = ws.build(&gt.query, 1_700_000_000, false).unwrap();
        let probes = build_probes(&built, &gt);
        assert!(probes.len() >= 3, "got {} probes", probes.len());

        // the planted hot region must actually reach the top band
        let sums = built.region_summaries();
        let ri = built
            .analysis
            .tree
            .regions
            .iter()
            .position(|r| r.path == gt.hot_region)
            .unwrap();
        assert_eq!(sums[ri].band, 5, "planted region should be the summit");

        // scoring: correct answer passes, wrong fails
        let answers: Vec<String> = probes.iter().map(|p| p.accept[0].clone()).collect();
        assert!(score_answers(&probes, &answers).iter().all(|s| s.pass));
        let wrong: Vec<String> = probes.iter().map(|_| "R999".to_string()).collect();
        assert!(!score_answers(&probes, &wrong).iter().all(|s| s.pass));
    }
}
