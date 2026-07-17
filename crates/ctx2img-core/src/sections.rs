//! Structure extraction for non-code text: markdown heading trees become
//! sections, which `ctx2img paint` renders as territories the way directories
//! become regions for a repo.

#[derive(Debug, Clone)]
pub struct Section {
    pub title: String,
    /// Heading level (1 = `#`); 0 for preamble before the first heading.
    pub level: u8,
    pub text: String,
}

/// Max territories on a painted document map; the tail merges into "…".
const MAX_SECTIONS: usize = 24;

/// Split markdown into sections at the shallowest heading level present.
/// Deeper headings stay inside their parent's text. Returns None when the
/// text has no usable heading structure (callers fall back to flat pages).
pub fn split_markdown(text: &str) -> Option<Vec<Section>> {
    let headings: Vec<(usize, u8, String)> = text
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let hashes = l.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&hashes) && l.as_bytes().get(hashes) == Some(&b' ') {
                Some((i, hashes as u8, l[hashes + 1..].trim().to_string()))
            } else {
                None
            }
        })
        .collect();
    // split at the first (shallowest) level that yields ≥2 sections — a
    // document with a single `#` title splits at its `##` chapters
    let mut levels: Vec<u8> = headings.iter().map(|&(_, lvl, _)| lvl).collect();
    levels.sort_unstable();
    levels.dedup();
    let top = *levels
        .iter()
        .find(|&&lvl| headings.iter().filter(|&&(_, l, _)| l == lvl).count() >= 2)?;
    let splits: Vec<&(usize, u8, String)> =
        headings.iter().filter(|&&(_, lvl, _)| lvl == top).collect();

    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<Section> = Vec::new();
    // preamble before the first top-level heading
    let first = splits[0].0;
    if lines[..first].iter().any(|l| !l.trim().is_empty()) {
        sections.push(Section {
            title: "(intro)".into(),
            level: 0,
            text: lines[..first].join("\n"),
        });
    }
    for (si, &&(start, _, ref title)) in splits.iter().enumerate() {
        let end = splits
            .get(si + 1)
            .map(|&&(e, _, _)| e)
            .unwrap_or(lines.len());
        sections.push(Section {
            title: title.clone(),
            level: top,
            text: lines[start..end].join("\n"),
        });
    }

    if sections.len() > MAX_SECTIONS {
        let tail: Vec<Section> = sections.split_off(MAX_SECTIONS - 1);
        let merged = tail
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let titles = tail.len();
        sections.push(Section {
            title: format!("… ({titles} more sections)"),
            level: top,
            text: merged,
        });
    }
    Some(sections)
}

/// Optional query-conditioned banding for sections (1..=5), mirroring the
/// repo relevance bands: term frequency, length-damped, rank-quantized.
pub fn band_sections(sections: &[Section], query: &str) -> Vec<u8> {
    let terms = crate::tokens::query_terms(query);
    if terms.is_empty() || sections.is_empty() {
        return vec![2; sections.len()];
    }
    let scores: Vec<f32> = sections
        .iter()
        .map(|s| {
            let lower = s.text.to_ascii_lowercase();
            let hits: usize = terms
                .iter()
                .map(|t| lower.matches(t.as_str()).count())
                .sum();
            hits as f32 / (s.text.len() as f32).sqrt().max(1.0)
        })
        .collect();
    let max = scores.iter().copied().fold(0f32, f32::max);
    if max <= 0.0 {
        return vec![2; sections.len()];
    }
    scores
        .iter()
        .map(|&s| {
            let r = s / max;
            if r >= 0.9 {
                5
            } else if r >= 0.5 {
                4
            } else if r >= 0.2 {
                3
            } else if r > 0.0 {
                2
            } else {
                1
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_shallowest_level() {
        let md = "intro line\n\n## Setup\nsteps here\n### deep\nnested\n## Usage\nrun it\n";
        let s = split_markdown(md).unwrap();
        let titles: Vec<&str> = s.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, vec!["(intro)", "Setup", "Usage"]);
        assert!(
            s[1].text.contains("### deep"),
            "deeper headings stay inside"
        );
    }

    #[test]
    fn flat_text_is_none() {
        assert!(split_markdown("no headings\njust prose\n").is_none());
        assert!(split_markdown("# only-one\nbody\n").is_none());
    }

    #[test]
    fn query_bands_rank_sections() {
        let s = split_markdown("# A\nzephyr zephyr zephyr\n# B\nnothing here\n").unwrap();
        let bands = band_sections(&s, "zephyr");
        assert_eq!(bands[0], 5);
        assert!(bands[1] <= 2);
    }
}
