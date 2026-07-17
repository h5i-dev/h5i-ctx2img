---
name: c2m
description: Cut context tokens by rendering it as images. Survey a whole repository in ~2k tokens as a semantic map image (Repository Atlas), zoom into inscribe tiles that carry the actual source, and resolve exact text via stable handles; `c2m paint` compresses ANY bulky text (docs, tool output, specs) into dense image pages. Use at the START of any task in an unfamiliar or large repo, or before ingesting any large text.
---

# c2m — Repository Atlas

`c2m` compiles the repository into a **query-conditioned map**: an image where
position = module topology, cell area = code size, **elevation (▲1–▲5) =
relevance to your current task**, hatched red = trust hazards, arrows =
dependencies. Every region/file/symbol carries a stable handle (`R3`, `F103`,
`S12`) that resolves back to exact source. The image is an *index*, never the
source of truth — exact code always comes from `c2m read` as text.

## Workflow

1. **Map the repo against your task** (auto-builds the index on first run):

   ```bash
   c2m map "<one line describing your task>" --provider claude --budget 2000
   ```

   stdout is the legend (region roster with handles + elevation). If it prints
   an `# atlas: <path>` line, **Read that PNG file now** — the image carries
   the geography the legend can't. On small repos it may print a text-only
   roster instead (`representation: text`) — that alone is a complete map; no
   image to read.

2. **Pick the summit.** Start from the highest-elevation regions (▲5/▲4) and
   their top files. `⚠net/exec/secrets/eval` tags mark files that touch the
   outside world — relevant for anything security-adjacent.

3. **Zoom one level** when a region looks right:

   ```bash
   c2m zoom R3            # writes a region tile image + prints a file/symbol roster
   c2m zoom R3 --inscribe    # tile with each file's ACTUAL SOURCE typeset in its cell
   c2m zoom R3 --text     # roster only, no image
   c2m zoom F103          # file detail: symbols with S-handles, imports, hazards
   ```

   Read the tile image the same way if one is written. Prefer `--inscribe` when
   you want to *read the region's code* (several files in one image, ~2–4×
   cheaper than the same text); prefer the plain tile when you only need the
   structure.

4. **Get exact source as text** (never trust pixels for code):

   ```bash
   c2m read F103                  # whole file, numbered
   c2m read S12                   # just that symbol's line range
   c2m read F103 --lines 40:120
   c2m locate "session|expiry"    # find handles by substring
   ```

5. **Compress any other bulky text** (a long doc, tool output, a spec) before
   ingesting it:

   ```bash
   c2m paint big-context.md --out-dir /tmp/pages   # or: cat text | c2m paint
   ```

   Then Read the page PNGs in order. Keep the printed **factsheet line** —
   it carries the exact identifiers (paths, SHAs, IDs) as text; quote those
   from the factsheet, never by transcribing them from the image. If paint
   says text is cheaper, just read the text.

## Rules

- The atlas is for *navigation*; inscribe tiles and paint pages are for
  *reading*. Quote, edit, hash-compare, and reason over `c2m read` output or
  the factsheet (text), never over strings you transcribed from an image —
  image misreads are silent and look plausible.
- Handles are stable across runs and queries — safe to mention in commits,
  notes, and follow-up commands.
- Re-run `c2m map "<new task>"` whenever your task changes; it re-elevates the
  same geography in well under a second (warm cache), so map early and often.
- `--json` on `map`/`zoom` gives `{atlas_path, legend, ...}` when you need to
  script it.
- For a human-facing map (README, PR description):
  `c2m render --out map.svg` (parchment theme) or `c2m badge`.
