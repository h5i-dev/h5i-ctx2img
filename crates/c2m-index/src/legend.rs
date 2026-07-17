//! Legend emission: the text half of every atlas. Token-frugal, but a
//! complete degraded fallback — legend-only ≈ an aider-style repo map, so
//! a model that can't read the image still gets a useful index.

use crate::workspace::{Built, RegionSummary};
use c2m_core::hazard;

pub struct LegendOptions {
    /// Max files listed per region roster line.
    pub top_files: usize,
    /// Include the fixed schema header (skip when the host already sent it).
    pub schema: bool,
}

impl Default for LegendOptions {
    fn default() -> Self {
        LegendOptions {
            top_files: 3,
            schema: true,
        }
    }
}

pub fn build_legend(built: &Built, query: &str, opts: &LegendOptions) -> String {
    let a = &built.analysis;
    let sums = built.region_summaries();
    let mut out = String::with_capacity(4096);

    if opts.schema {
        out.push_str("# ATLAS v1 · map legend\n");
        out.push_str("# elevation ▲1..▲5 = relevance (▲5 = summit, look here first) · cell area = code size\n");
        out.push_str("# ⚠tags = trust hazards (net/exec/secrets/eval) · →R… = depends on region\n");
        out.push_str(
            "# handles: R=region F=file X=external-dep · never guess content — zoom in:\n",
        );
        out.push_str("#   `c2m paint <dir>` module source as images · `c2m read F103` exact text · `c2m read --find <pat>` search\n");
    }
    if !query.is_empty() {
        out.push_str(&format!("# query: {query}\n"));
    }
    out.push('\n');

    // Regions, highest band first (summit-first reading order), then by size.
    let mut order: Vec<usize> = (0..a.tree.regions.len()).collect();
    order.sort_by(|&x, &y| {
        sums[y]
            .band
            .cmp(&sums[x].band)
            .then(a.tree.regions[y].loc.cmp(&a.tree.regions[x].loc))
            .then(x.cmp(&y))
    });

    for ri in order {
        let r = &a.tree.regions[ri];
        let s = &sums[ri];
        out.push_str(&region_line(built, ri, r, s, opts.top_files));
        out.push('\n');
    }

    if !a.graph.external.is_empty() {
        let ext: Vec<String> = a
            .graph
            .external
            .iter()
            .zip(&built.ext_handles)
            .take(12)
            .map(|((name, n), h)| format!("{h} {name}({n})"))
            .collect();
        out.push_str(&format!("\nX external: {}\n", ext.join(" ")));
    }
    out
}

fn region_line(
    built: &Built,
    ri: usize,
    r: &c2m_core::Region,
    s: &RegionSummary,
    top_files: usize,
) -> String {
    let a = &built.analysis;
    let mut line = format!(
        "{} {} ▲{}",
        built.region_handles[ri],
        r.display_name(),
        s.band
    );
    let tags = hazard::tags(s.hazards);
    if !tags.is_empty() {
        line.push_str(&format!(" ⚠{}", tags.join(",")));
    }
    line.push_str(&format!(
        " {}f {} {}",
        r.files.len(),
        human_loc(r.loc),
        r.dominant_lang.tag()
    ));

    let tops: Vec<String> = s
        .ranked_files
        .iter()
        .take(top_files)
        .map(|&(i, _)| {
            let name = a.files[i]
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&a.files[i].path);
            format!(
                "{} {}▲{}",
                built.file_handles[i], name, a.relevance.bands[i]
            )
        })
        .collect();
    if !tops.is_empty() {
        line.push_str(&format!(" | {}", tops.join(" ")));
    }

    let deps: Vec<&str> = s
        .out_edges
        .iter()
        .take(3)
        .map(|&(to, _, _)| built.region_handles[to].as_str())
        .collect();
    if !deps.is_empty() {
        line.push_str(&format!(" | →{}", deps.join(" ")));
    }
    line
}

pub fn human_loc(loc: u64) -> String {
    if loc >= 10_000 {
        format!("{}kloc", loc / 1000)
    } else if loc >= 1_000 {
        format!("{:.1}kloc", loc as f64 / 1000.0)
    } else {
        format!("{loc}loc")
    }
}

/// Rough token estimate for budgeting text vs image (chars/3.6, code-ish).
pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as f64 / 3.6).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn legend_contains_handles_and_bands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/auth")).unwrap();
        std::fs::write(
            dir.path().join("src/auth/session.rs"),
            "pub fn session_expiry() -> u64 { 0 }\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let b = ws.build("session expiry", 1_700_000_000, false).unwrap();
        let legend = build_legend(&b, "session expiry", &LegendOptions::default());
        assert!(legend.contains("▲"), "bands present");
        assert!(legend.contains("session.rs"), "top files listed");
        assert!(legend.contains("c2m read"), "affordances stated");
        assert!(
            legend.lines().any(|l| l.starts_with('R')),
            "region handles present"
        );
    }
}
