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

1. **Index the repo against your task** (auto-builds on first run):

   ```bash
   c2m paint . --budget 2000 -q "<one line describing your task>"
   ```

   stdout is the legend (region roster with handles + elevation). If it prints
   an `# atlas: <path>` line, **Read that PNG file now** — the image carries
   the geography the legend can't. On small repos it may print a text-only
   roster instead (`representation: text`) — that alone is a complete map; no
   image to read.

2. **Pick the summit.** Start from the highest-elevation regions (▲5/▲4) and
   their top files. `⚠net/exec/secrets/eval` tags mark files that touch the
   outside world — relevant for anything security-adjacent.

3. **Focus where the task points** — paint just that module or file:

   ```bash
   c2m paint src/auth -q "<task>"   # one module's FULL SOURCE as image tiles
   c2m paint src/auth/session.rs    # a single file as dense pages
   ```

   Subdirectory paints share the whole repo's handles and relevance, so the
   tiles line up with the atlas you already saw.

4. **Get exact source as text** (never trust pixels for code):

   ```bash
   c2m read F103                  # whole file, numbered
   c2m read S12                   # just that symbol's line range
   c2m read F103 --lines 40:120
   c2m read --find "session"      # search paths+symbols, answers in handles
   ```

5. **Compress any bulky text** (a long doc, tool output, a spec — or a whole
   directory) before ingesting it:

   ```bash
   c2m paint doc.md --out-dir /tmp/pages     # markdown → section map (headings = territories)
   c2m paint some/dir --query "<task>"       # dir → atlas folio: overview + full-source tiles
   cat text | c2m paint --out-dir /tmp/pages # flat text → dense pages
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
- Re-run `c2m paint . --budget 2000 -q "<new task>"` whenever your task changes; it re-elevates the
  same geography in well under a second (warm cache), so map early and often.
- `--json` on `map`/`zoom` gives `{atlas_path, legend, ...}` when you need to
  script it.
- For a human-facing map (README, PR description):
  `c2m render --out map.svg` (parchment theme).
