---
name: ctx2img
description: Cut context tokens by rendering it as images. `ctx2img paint <path|->` turns any text-shaped input (a repo, a directory, a file, markdown, stdin) into dense images that carry the full text at 60-75% fewer tokens, with stable handles and a verbatim factsheet; `ctx2img read` recovers guaranteed-exact text. Use before ingesting any large text or at the START of any task in an unfamiliar or large repo.
---

# ctx2img — Repository Atlas

`ctx2img` compiles the repository into a **query-conditioned map**: an image where
position = module topology, cell area = code size, **elevation (▲1–▲5) =
relevance to your current task**, hatched red = trust hazards, arrows =
dependencies. Every region/file/symbol carries a stable handle (`R3`, `F103`,
`S12`) that resolves back to exact source. The image is an *index*, never the
source of truth — exact code always comes from `ctx2img read` as text.

## Workflow

1. **Index the repo against your task** (auto-builds on first run):

   ```bash
   ctx2img paint . --budget 2000 -q "<one line describing your task>"
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
   ctx2img paint src/auth -q "<task>"   # one module's FULL SOURCE as image tiles
   ctx2img paint src/auth/session.rs    # a single file as dense pages
   ```

   Subdirectory paints share the whole repo's handles and relevance, so the
   tiles line up with the atlas you already saw.

4. **Get exact source as text** (never trust pixels for code):

   ```bash
   ctx2img read F103                  # whole file, numbered
   ctx2img read S12                   # just that symbol's line range
   ctx2img read F103 --lines 40:120
   ctx2img read --find "session"      # search paths+symbols, answers in handles
   ```

5. **Compress any bulky text** (a long doc, tool output, a spec — or a whole
   directory) before ingesting it:

   ```bash
   ctx2img paint doc.md --out-dir /tmp/pages     # markdown → section map (headings = territories)
   ctx2img paint some/dir --query "<task>"       # dir → atlas folio: overview + full-source tiles
   cat text | ctx2img paint --out-dir /tmp/pages # flat text → dense pages
   ```

   Then Read the page PNGs in order. Keep the printed **factsheet line** —
   it carries the exact identifiers (paths, SHAs, IDs) as text; quote those
   from the factsheet, never by transcribing them from the image. If paint
   says text is cheaper, just read the text.

## Rules

- The atlas is for *navigation*; inscribe tiles and paint pages are for
  *reading*. Quote, edit, hash-compare, and reason over `ctx2img read` output or
  the factsheet (text), never over strings you transcribed from an image —
  image misreads are silent and look plausible.
- Handles are stable across runs and queries — safe to mention in commits,
  notes, and follow-up commands.
- Re-run `ctx2img paint . --budget 2000 -q "<new task>"` whenever your task changes; it re-elevates the
  same geography in well under a second (warm cache), so map early and often.
- `--json` on `paint` gives `{pages, legend_path, ...}` when you need to
  script it.
- For a human-facing map (README, PR description):
  `ctx2img paint . --theme parchment`.
