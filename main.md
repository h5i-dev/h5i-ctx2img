# Project Roadmap

## Goal
Design a Rust tool (context2map) that converts repo/context into query-conditioned semantic map images + zoomable handles for VLM token reduction

## Milestones
- [x] Initial setup
- [x] wrote design-notes.md
- [x] Literature digest complete: optical/visual context compression papers
- [x] Rust ecosystem sanity check complete
- [x] Researched image token accounting from current Anthropic + OpenAI docs
- [x] wrote design-notes.md
- [x] Collected image-token accounting facts for Gemini 2.5/3 and Qwen2.5-VL/Qwen3-VL
- [x] Prior-art research complete: aider repo-map mechanism, code-city/map viz landscape, optical context compression (DeepSeek-OCR, Glyph, PIXEL)
- [x] Research digest complete: VLM image-token formulas (Anthropic 28px-patch, OpenAI tile+patch, Gemini media_resolution, Qwen 28/32px) + repo-map/code-cartography prior art + Rust crate audit
- [x] System design for context2map complete
- [x] wrote docs/DESIGN.md
- [x] wrote docs/DESIGN.md
- [x] edited docs/DESIGN.md; edited docs/DESIGN.md; edited docs/DESIGN.md
- [x] edited docs/DESIGN.md; edited docs/DESIGN.md; edited docs/DESIGN.md
- [x] Full context2map implementation shipped
- [x] edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs
- [x] edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-render/src/scene.rs
- [x] edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-render/src/scene.rs
- [x] edited crates/c2m-render/src/scene.rs; wrote h5i-commit-quoting.md; wrote MEMORY.md
- [x] edited crates/c2m-render/src/scene.rs; wrote h5i-commit-quoting.md; wrote MEMORY.md
- [x] edited crates/c2m-cli/src/main.rs; edited crates/c2m-render/src/paint.rs; edited crates/c2m-render/src/paint.rs
- [x] edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md
- [x] edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [x] edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [x] edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [x] edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [x] edited crates/c2m-render/src/theme_vlm.rs; wrote paint_block.rs; edited crates/c2m-core/src/sections.rs
- [x] edited crates/c2m-render/src/theme_vlm.rs; wrote paint_block.rs; edited crates/c2m-core/src/sections.rs

## Active Branches
- main (primary)

## Notes
- [2026-07-17 02:16 UTC] `wip`: edited crates/c2m-render/src/theme_vlm.rs; wrote paint_block.rs; edited crates/c2m-core/src/sections.rs
- [2026-07-17 02:14 UTC] `wip`: edited crates/c2m-render/src/theme_vlm.rs; wrote paint_block.rs; edited crates/c2m-core/src/sections.rs
- [2026-07-17 02:02 UTC] `wip`: edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [2026-07-17 01:59 UTC] `wip`: edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [2026-07-17 01:57 UTC] `wip`: edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [2026-07-17 01:56 UTC] `wip`: edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited README.md
- [2026-07-17 01:54 UTC] `wip`: edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md; edited skills/c2m/SKILL.md
- [2026-07-17 01:49 UTC] `wip`: edited crates/c2m-cli/src/main.rs; edited crates/c2m-render/src/paint.rs; edited crates/c2m-render/src/paint.rs
- [2026-07-17 01:39 UTC] `wip`: edited crates/c2m-render/src/scene.rs; wrote h5i-commit-quoting.md; wrote MEMORY.md
- [2026-07-17 01:38 UTC] `wip`: edited crates/c2m-render/src/scene.rs; wrote h5i-commit-quoting.md; wrote MEMORY.md
- [2026-07-17 01:21 UTC] `wip`: edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-render/src/scene.rs
- [2026-07-17 01:19 UTC] `wip`: edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-render/src/scene.rs
- [2026-07-17 01:15 UTC] `wip`: edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs; edited crates/c2m-core/src/regions.rs
- [2026-07-17 01:15 UTC] `wip`: Full context2map implementation shipped
- [2026-07-17 00:20 UTC] `wip`: edited docs/DESIGN.md; edited docs/DESIGN.md; edited docs/DESIGN.md
- [2026-07-17 00:19 UTC] `wip`: edited docs/DESIGN.md; edited docs/DESIGN.md; edited docs/DESIGN.md
- [2026-07-17 00:15 UTC] `wip`: wrote docs/DESIGN.md
- [2026-07-17 00:13 UTC] `wip`: wrote docs/DESIGN.md
- [2026-07-17 00:12 UTC] `wip`: System design for context2map complete
- [2026-07-17 00:08 UTC] `wip`: Research digest complete: VLM image-token formulas (Anthropic 28px-patch, OpenAI tile+patch, Gemini media_resolution, Qwen 28/32px) + repo-map/code-cartography prior art + Rust crate audit
- [2026-07-17 00:06 UTC] `wip`: Prior-art research complete: aider repo-map mechanism, code-city/map viz landscape, optical context compression (DeepSeek-OCR, Glyph, PIXEL)
- [2026-07-17 00:06 UTC] `wip`: Collected image-token accounting facts for Gemini 2.5/3 and Qwen2.5-VL/Qwen3-VL
- [2026-07-17 00:06 UTC] `wip`: wrote design-notes.md
- [2026-07-17 00:05 UTC] `wip`: Researched image token accounting from current Anthropic + OpenAI docs
- [2026-07-17 00:05 UTC] `wip`: Rust ecosystem sanity check complete
- [2026-07-17 00:04 UTC] `wip`: Literature digest complete: optical/visual context compression papers
- [2026-07-17 00:03 UTC] `wip`: wrote design-notes.md
_Add project-wide notes here._
