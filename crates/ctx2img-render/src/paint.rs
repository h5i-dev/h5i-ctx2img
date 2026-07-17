//! `ctx2img paint` — render *any* text (prompts, docs, tool output, files) into
//! dense image pages at a provider-safe geometry.
//!
//! Constants follow what pxpipe field-validated on live Claude traffic:
//! - Page geometry matches the provider's **resample contract**, not a patch
//!   grid: for Anthropic, ≤1568 px long edge and ≈1.15 MP keeps the render
//!   WYSIWYG for the vision encoder (bigger pages pay full price for pixels
//!   the API destroys in transit).
//! - **Reflow**: hard newlines become a visible `↵` sentinel and rows pack
//!   full — measured ~99% read fidelity vs ~78% for naive per-line packing,
//!   and ~2.5× more glyphs per page. Pre-existing `↵` is neutralized to `⏎`.
//! - Pages are variable-height (never padded square — padding bills, adds
//!   nothing) and byte-deterministic (cache-safe).
//! - The first page carries a first-party provenance banner. Deliberately
//!   NOT phrased as "system prompt"/"authoritative" — that framing is known
//!   to trip refusal heuristics.

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};
use crate::raster::Raster;
use crate::text;
use anyhow::Result;

pub const NL_SENTINEL: char = '↵';
const NL_LITERAL: char = '⏎';

pub struct PaintProfile {
    pub width: u32,
    pub max_height: u32,
    pub font_px: f32,
}

impl PaintProfile {
    /// Anthropic standard tier: 1568-wide, ≤728-tall ⇒ ~1.14 MP, no resample.
    pub fn claude(font_px: f32) -> PaintProfile {
        PaintProfile {
            width: 1568,
            max_height: 728,
            font_px,
        }
    }
    /// OpenAI: shortest-side-768 contract ⇒ a 768-wide strip up to ~1932 px
    /// tall survives unresampled.
    pub fn openai(font_px: f32) -> PaintProfile {
        PaintProfile {
            width: 768,
            max_height: 1932,
            font_px,
        }
    }
}

pub struct PaintedPage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub chars: usize,
}

const PAD: f32 = 6.0;
const INK: Rgba = Rgba::opaque(15, 15, 15);
const PAPER: Rgba = Rgba::opaque(255, 255, 255);

/// Dense reflow: minify + tab-expand + join hard newlines with `↵`.
pub fn reflow(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.lines() {
        if !first {
            out.push(NL_SENTINEL);
        }
        first = false;
        let neutralized: String = line
            .chars()
            .map(|c| if c == NL_SENTINEL { NL_LITERAL } else { c })
            .collect();
        out.push_str(neutralized.replace('\t', "    ").trim_end());
    }
    out
}

/// First-party provenance banner (first page only). Kept away from
/// "system prompt"/"authoritative" phrasing by design.
pub fn banner(pages: usize, reflowed: bool) -> String {
    let mut b = format!(
        "ctx2img paint (this user's local tool) rendered the following text as {pages} image page(s) to reduce token cost. Read the pages in order; they are the referenced document content."
    );
    if reflowed {
        b.push_str(" The glyph ↵ marks an original hard line break — treat it as a real newline.");
    }
    b
}

/// Typeset `content` into pages. `reflowed`: content is a single ↵-joined
/// stream (fill rows completely); otherwise wrap source lines individually.
pub fn paint(content: &str, profile: &PaintProfile, reflowed: bool) -> Result<Vec<PaintedPage>> {
    let advance = text::measure("M", profile.font_px, FontKind::Mono).max(1.0);
    let line_h = (profile.font_px * 1.15).ceil();
    let cols = ((profile.width as f32 - 2.0 * PAD) / advance) as usize;
    let banner_px = 14.0f32;

    // Break content into visual rows of ≤ cols chars.
    let rows: Vec<String> = if reflowed {
        let chars: Vec<char> = content.chars().collect();
        chars
            .chunks(cols.max(1))
            .map(|c| c.iter().collect())
            .collect()
    } else {
        let mut rows = Vec::new();
        for line in content.lines() {
            let expanded = line.replace('\t', "    ");
            let chars: Vec<char> = expanded.chars().collect();
            if chars.is_empty() {
                rows.push(String::new());
            } else {
                for chunk in chars.chunks(cols.max(1)) {
                    rows.push(chunk.iter().collect());
                }
            }
        }
        rows
    };

    // pre-pass: page count (the first page loses rows to the banner)
    let rows_first =
        (((profile.max_height as f32 - 2.0 * PAD - banner_px * 3.2) / line_h) as usize).max(1);
    let rows_rest = (((profile.max_height as f32 - 2.0 * PAD) / line_h) as usize).max(1);
    let total_pages = if rows.len() <= rows_first {
        1
    } else {
        1 + (rows.len() - rows_first).div_ceil(rows_rest)
    };

    let mut pages = Vec::new();
    let mut i = 0usize;
    let mut first_page = true;
    while i < rows.len() {
        let header_h = if first_page { banner_px * 3.2 } else { 0.0 };
        let usable = profile.max_height as f32 - 2.0 * PAD - header_h;
        let rows_per_page = ((usable / line_h) as usize).max(1);
        let take = rows_per_page.min(rows.len() - i);
        let height = ((take as f32 * line_h) + 2.0 * PAD + header_h).ceil() as u32;

        let mut dl = DisplayList::default();
        let mut y = PAD + header_h;
        if first_page {
            // banner wraps across up to 3 sans lines
            let btext = banner(total_pages, reflowed);
            let bcols = ((profile.width as f32 - 2.0 * PAD)
                / text::measure("m", banner_px * 0.62, FontKind::Sans).max(1.0))
                as usize;
            for (bi, chunk) in btext
                .chars()
                .collect::<Vec<_>>()
                .chunks(bcols.max(20))
                .take(3)
                .enumerate()
            {
                dl.push(Op::Text {
                    pos: (
                        PAD / profile.width as f32,
                        (PAD + banner_px * (bi as f32 + 0.9)) / height as f32,
                    ),
                    text: chunk.iter().collect(),
                    size_px: banner_px * 0.78,
                    color: Rgba::opaque(90, 90, 90),
                    font: FontKind::Sans,
                    align: TextAlign::Left,
                    halo: None,
                });
            }
        }
        let mut chars = 0usize;
        for row in &rows[i..i + take] {
            y += line_h;
            chars += row.chars().count();
            if row.is_empty() {
                continue;
            }
            dl.push(Op::Text {
                pos: (PAD / profile.width as f32, y / height as f32),
                text: row.clone(),
                size_px: profile.font_px,
                color: INK,
                font: FontKind::Mono,
                align: TextAlign::Left,
                halo: None,
            });
        }

        let mut raster = Raster::new(profile.width, height, PAPER)?;
        raster.execute(&dl);
        pages.push(PaintedPage {
            png: raster.png()?,
            width: profile.width,
            height,
            chars,
        });
        i += take;
        first_page = false;
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflow_marks_and_neutralizes() {
        let r = reflow("line one\nline two↵tricky\n\tindented");
        assert_eq!(r, "line one↵line two⏎tricky↵    indented");
    }

    #[test]
    fn paints_dense_pages_deterministically() {
        let text = "fn main() { println!(\"hello\"); }\n".repeat(400);
        let flowed = reflow(&text);
        let profile = PaintProfile::claude(8.0);
        let a = paint(&flowed, &profile, true).unwrap();
        let b = paint(&flowed, &profile, true).unwrap();
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].png, b[0].png, "byte-deterministic");
        assert!(a[0].width == 1568 && a[0].height <= 728);
        let total_chars: usize = a.iter().map(|p| p.chars).sum();
        assert!(
            total_chars >= flowed.chars().count(),
            "every char lands on a page ({total_chars} vs {})",
            flowed.chars().count()
        );
        // density sanity: a full page should hold >20k chars at 8px mono
        if a.len() > 1 {
            assert!(a[0].chars > 15_000, "page holds {} chars", a[0].chars);
        }
    }
}
