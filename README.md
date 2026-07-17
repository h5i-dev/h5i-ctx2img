# context2map (`c2m`)

**Render agent context as images. Same text, ~60–75% fewer tokens.**

An image is billed by its pixels, not by how much text it holds. `c2m paint`
typesets any text an agent must ingest — a repo, a markdown doc, a prompt,
tool output — into dense, structured images a vision LLM reads directly,
plus a small text *factsheet* so exact identifiers are never trusted to
pixels.

![This repo's 33k-char design doc as one image](assets/design-map.png)

*`c2m paint docs/DESIGN.md` — this repo's entire design document (~9,800
text tokens) as **one 2,550-token image**: 74% cheaper, every section a
labeled box, thicker borders = more relevant to your query.*

## Usage

```bash
cargo install --path crates/c2m-cli

c2m paint <file|dir|->            # THE command: any text → dense image(s)
```

What you get depends on the input shape:

| Input | Output | Measured effect |
|---|---|---|
| markdown/doc | one section-map image + `.legend.txt` | 33k-char doc: **9.8k → 2.5k tokens (−74%)** |
| directory | atlas folio: overview + full-source tiles per module | 14-file crate in one 2.6k-token tile (~2–3× vs text) |
| flat text / stdin | reflowed pages (1568×728, provider-safe) | dense code/JSON ≈ 3× fewer tokens |
| directory + `--budget 2000` | navigation only: atlas + legend, no source | whole repo ≈ **2k tokens**; 5k files mapped in **0.4s** |

Drill down when needed — pixels are for reading, text is for exactness:

```bash
c2m paint src/auth -q "<task>" # focus: one module's full source as tiles
c2m read F103 --lines 40:120   # guaranteed-exact text for quoting/editing
c2m read --find "session"      # search paths + symbols, answers in handles
```

Every render prints its counterfactual (`~2550 image tok vs ~9772 text tok`)
and **refuses to paint when text would be cheaper**. Handles (`R3`, `F103`,
`§4`) are stable; the factsheet carries paths/SHAs/IDs as text because VLMs
misread high-entropy strings silently.

## Coding agents

```bash
cp -r skills/c2m ~/.claude/skills/   # Claude Code; any VLM agent can use the CLI
```

## More

- `--provider claude|openai|gemini|qwen` — budgets solve against each
  provider's real image-token formula; canvases shrink to fit the content.
- `--theme warm|dark`, `--layout organic`, `c2m render` (the pretty
  parchment map) — cosmetics, gated by `c2m calibrate`.
- Design rationale, evidence, benchmark harness: [docs/DESIGN.md](docs/DESIGN.md).

Apache-2.0 · embedded DejaVu fonts under their own license (`assets/fonts/`).
