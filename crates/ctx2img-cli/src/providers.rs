//! Provider profiles: image-token accounting + the budget solver that picks
//! raster dimensions snapped to each provider's patch grid, so no budget is
//! wasted on padding. Formulas verified against provider docs (2026-07).

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Provider {
    /// Anthropic Claude: 28-px patches, high-res tier ≤2576px / ≤4784 tok.
    Claude,
    /// OpenAI tile-based (gpt-4o/4.1/5): 70 base + 140 per 512-px tile.
    Openai,
    /// OpenAI patch-based minis (gpt-4.1-mini/gpt-5-mini): 32-px patches ×1.62.
    OpenaiMini,
    /// Gemini 3: fixed budgets via media_resolution (280/560/1120/2240).
    Gemini,
    /// Qwen3-VL: 32-px blocks (+2 wrapper tokens).
    Qwen,
}

impl Provider {
    #[allow(dead_code)] // used by future provider-labeled reports
    pub fn name(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Openai => "openai",
            Provider::OpenaiMini => "openai-mini",
            Provider::Gemini => "gemini",
            Provider::Qwen => "qwen",
        }
    }

    /// Image tokens the provider will charge for a w×h render.
    pub fn tokens(&self, w: u32, h: u32) -> u32 {
        match self {
            Provider::Claude => w.div_ceil(28) * h.div_ceil(28),
            Provider::Openai => {
                // detail:high — shortest side scaled to 768, count 512 tiles
                let scale = 768.0 / w.min(h) as f64;
                let (sw, sh) = (
                    (w as f64 * scale).min(2048.0),
                    (h as f64 * scale).min(2048.0),
                );
                let tiles = (sw / 512.0).ceil() as u32 * (sh / 512.0).ceil() as u32;
                70 + 140 * tiles
            }
            Provider::OpenaiMini => {
                let patches = (w.div_ceil(32) * h.div_ceil(32)).min(1536);
                (patches as f64 * 1.62).round() as u32
            }
            Provider::Gemini => {
                // fixed per media_resolution step; assume the step chosen by solve()
                let side = w.max(h);
                if side <= 768 {
                    280
                } else if side <= 1024 {
                    560
                } else if side <= 1536 {
                    1120
                } else {
                    2240
                }
            }
            Provider::Qwen => w.div_ceil(32) * h.div_ceil(32) + 2,
        }
    }

    /// Largest square-ish canvas whose token cost fits `budget`.
    pub fn solve(&self, budget: u32, aspect: f32) -> (u32, u32) {
        match self {
            Provider::Claude => solve_patches(budget, aspect, 28, 4784, 2576),
            Provider::Qwen => {
                let (w, h) = solve_patches(budget.saturating_sub(2), aspect, 32, 16384, 3584);
                (w, h)
            }
            Provider::OpenaiMini => {
                let patch_budget = ((budget as f64 / 1.62) as u32).min(1536);
                solve_patches(patch_budget, aspect, 32, 1536, 2048)
            }
            Provider::Openai => {
                // tiles at 512px after shortest-side-768 normalization; render
                // directly at the normalized size so nothing is resampled
                let max_tiles = (budget.saturating_sub(70) / 140).max(1);
                let (mut tx, mut ty) = (1u32, 1u32);
                loop {
                    let (nx, ny) = if (tx as f32 / ty as f32) < aspect {
                        (tx + 1, ty)
                    } else {
                        (tx, ty + 1)
                    };
                    if nx * ny > max_tiles {
                        break;
                    }
                    tx = nx;
                    ty = ny;
                }
                // shortest side exactly 768
                let (w, h) = (tx * 512, ty * 512);
                let scale = 768.0 / w.min(h) as f64;
                (
                    ((w as f64 * scale) as u32).min(2048),
                    ((h as f64 * scale) as u32).min(2048),
                )
            }
            Provider::Gemini => {
                // pick the largest media_resolution step ≤ budget
                let side: u32 = if budget >= 2240 {
                    1568
                } else if budget >= 1120 {
                    1344
                } else if budget >= 560 {
                    1008
                } else {
                    768
                };
                if aspect >= 1.0 {
                    (side, (side as f32 / aspect) as u32)
                } else {
                    ((side as f32 * aspect) as u32, side)
                }
            }
        }
    }
}

/// Max (w, h) with w/h ≈ aspect such that patch count ≤ budget, both sides
/// multiples of `patch`, sides ≤ max_side, patches ≤ max_patches.
fn solve_patches(
    budget: u32,
    aspect: f32,
    patch: u32,
    max_patches: u32,
    max_side: u32,
) -> (u32, u32) {
    let budget = budget.min(max_patches).max(64);
    let pw_f = (budget as f32 * aspect).sqrt();
    let mut pw = pw_f.floor().max(4.0) as u32;
    let mut ph = (budget / pw.max(1)).max(4);
    let max_p = max_side / patch;
    pw = pw.min(max_p);
    ph = ph.min(max_p);
    // greedy: grow the smaller side while budget allows
    while (pw + 1) * ph <= budget && (pw + 1) <= max_p && (pw as f32 / ph as f32) < aspect {
        pw += 1;
    }
    while pw * (ph + 1) <= budget && (ph + 1) <= max_p {
        ph += 1;
    }
    (pw * patch, ph * patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_matches_documented_examples() {
        assert_eq!(Provider::Claude.tokens(1092, 1092), 39 * 39); // 1521, docs table
        assert_eq!(Provider::Claude.tokens(1024, 1024), 37 * 37); // 1369
    }

    #[test]
    fn openai_tile_example() {
        assert_eq!(Provider::Openai.tokens(1024, 1024), 70 + 140 * 4); // 630
    }

    #[test]
    fn qwen_block_formula() {
        assert_eq!(Provider::Qwen.tokens(1024, 1024), 32 * 32 + 2);
    }

    #[test]
    fn solver_respects_budget_and_grid() {
        for provider in [Provider::Claude, Provider::Qwen, Provider::OpenaiMini] {
            for budget in [800u32, 1500, 2000, 4000] {
                let (w, h) = provider.solve(budget, 1.0);
                assert!(
                    provider.tokens(w, h) <= budget,
                    "{provider:?} {budget}: {w}x{h}"
                );
                assert!(w >= 400, "degenerate width {w}");
            }
        }
        let (w, h) = Provider::Claude.solve(2000, 1.0);
        assert_eq!(w % 28, 0);
        assert_eq!(h % 28, 0);
    }
}
