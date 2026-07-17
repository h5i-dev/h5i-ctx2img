//! Command implementations.

use crate::providers::Provider;
use anyhow::{bail, Context, Result};
use ctx2img_index::legend::{build_legend, estimate_tokens, human_loc, LegendOptions};
use ctx2img_index::sidecar;
use ctx2img_index::workspace::{Built, Workspace};
use ctx2img_index::{HandleRegistry, Kind};
use ctx2img_layout::SavedSites;
use ctx2img_render::{
    render_png, render_svg, scene, DarkTheme, ParchmentTheme, SceneConfig, Theme, VlmTheme,
    WarmTheme,
};
use std::path::{Path, PathBuf};

pub const FOOTER: &str = "next: `ctx2img paint <dir>` module source as images · `ctx2img read F#|S#|path` exact text · `ctx2img read --find <pat>` search";

/// Resolve a machine-theme name; stark is the calibrated default.
pub fn machine_theme(name: &str) -> Result<&'static dyn Theme> {
    Ok(match name {
        "vlm" | "stark" => &VlmTheme,
        "warm" => &WarmTheme,
        "dark" => &DarkTheme,
        other => bail!("unknown machine theme `{other}` (vlm|warm|dark)"),
    })
}

pub struct Ctx {
    pub ws: Workspace,
    pub now: u64,
}

pub fn open(repo: Option<&Path>) -> Result<Ctx> {
    let root = repo
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir()?);
    let ws = Workspace::open(&root)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(Ctx { ws, now })
}

fn repo_name(ws: &Workspace) -> String {
    ws.root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into())
}

fn print_stats(built: &Built) {
    let s = &built.stats;
    let pct = (s.cache_hits * 100).checked_div(s.files).unwrap_or(100);
    eprintln!(
        "· {} files ({pct}% cached) · ingest {}ms · parse {}ms · analyze {}ms",
        s.files, s.ingest_ms, s.parse_ms, s.assemble_ms
    );
}

fn save_last_query(ws: &Workspace, query: &str) {
    let _ = std::fs::write(ws.dir.join("last-query.txt"), query);
}

// ---------------------------------------------------------------- map

fn seed_for(name: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------- read

pub fn read(
    repo: Option<&Path>,
    target: Option<&str>,
    lines: Option<&str>,
    find: Option<&str>,
) -> Result<()> {
    if let Some(pattern) = find {
        return find_handles(repo, pattern);
    }
    let Some(target) = target else {
        bail!("pass a handle/path to read, or --find <pattern> to search");
    };
    let ctx = open(repo)?;
    let registry = HandleRegistry::load(&ctx.ws.registry_path());
    let (path, mut range): (String, Option<(u32, u32)>) =
        if let Some(entry) = registry.resolve(target) {
            match entry.kind {
                Kind::File => (entry.key.clone(), None),
                Kind::Symbol => {
                    let path = entry.key.split('#').next().unwrap_or("").to_string();
                    (path, entry.lines)
                }
                _ => bail!("`read` works on F/S handles or paths, got {target}"),
            }
        } else {
            (target.to_string(), None) // treat as path
        };
    if let Some(spec) = lines {
        let (a, b) = spec
            .split_once(':')
            .with_context(|| format!("--lines wants a:b, got {spec}"))?;
        range = Some((a.parse()?, b.parse()?));
    }

    let full = ctx.ws.root.join(&path);
    let content =
        std::fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))?;
    let total = content.lines().count() as u32;
    let (start, end) = range.unwrap_or((1, total));
    println!("── {path} L{start}–{} of {total} ──", end.min(total));
    for (i, line) in content.lines().enumerate() {
        let n = i as u32 + 1;
        if n >= start && n <= end {
            println!("{n:>5}│ {line}");
        }
    }
    Ok(())
}

// ------------------------------------------------------- read --find

/// `read --find`: substring search over paths and symbol names, answering
/// in handles ready for `read`.
fn find_handles(repo: Option<&Path>, pattern: &str) -> Result<()> {
    let ctx = open(repo)?;
    let built = ctx.ws.build("", ctx.now, false)?;
    print_stats(&built);
    let mut registry = HandleRegistry::load(&ctx.ws.registry_path());
    let pat = pattern.to_ascii_lowercase();
    let mut shown = 0;
    for (fi, f) in built.analysis.files.iter().enumerate() {
        if shown >= 40 {
            println!("… (more matches truncated — narrow the pattern)");
            break;
        }
        let path_hit = f.path.to_ascii_lowercase().contains(&pat);
        let mut sym_hits: Vec<String> = Vec::new();
        for sym in &built.analysis.parsed[fi].symbols {
            if sym.name.to_ascii_lowercase().contains(&pat) {
                let h = registry.assign_symbol(
                    &f.path,
                    &sym.name,
                    sym.kind.tag(),
                    (sym.line, sym.line_end),
                );
                sym_hits.push(format!("{h}:{}@L{}", sym.name, sym.line));
            }
        }
        if path_hit || !sym_hits.is_empty() {
            let mut line = format!(
                "{} {} ▲{}",
                built.file_handles[fi], f.path, built.analysis.relevance.bands[fi]
            );
            if !sym_hits.is_empty() {
                line.push_str(&format!(" | {}", sym_hits.join(" ")));
            }
            println!("{line}");
            shown += 1;
        }
    }
    registry.save(&ctx.ws.registry_path())?;
    if shown == 0 {
        println!("no matches for `{pattern}`");
    } else {
        println!("# {FOOTER}");
    }
    Ok(())
}

// ---------------------------------------------------------------- render / badge

/// Token budget that a box layout actually NEEDS for `chars` of content at
/// `font_px` — canvases are sized to content, not to the allowance, so
/// boxes carry no built-in slack (and unused budget becomes savings).
fn fit_budget(chars: usize, n_boxes: usize, font_px: f32, budget: u32) -> u32 {
    content_tokens(chars, n_boxes, font_px).clamp(500.min(budget), budget)
}

/// Unclamped token estimate for `chars` of box-laid content at `font_px`.
/// Used for pagination arithmetic, where a floor would inflate small
/// sections and split pages prematurely.
fn content_tokens(chars: usize, n_boxes: usize, font_px: f32) -> u32 {
    let advance =
        ctx2img_render::text::measure("M", font_px, ctx2img_render::display::FontKind::Mono);
    let line_h = font_px * 1.22;
    let header_px2 = (font_px * 1.2 * 2.4) * 420.0; // header strip per box
    let px2 = chars as f32 * advance * line_h * 1.08 + n_boxes as f32 * header_px2;
    (px2 / 750.0) as u32
}

// ---------------------------------------------------------------- paint

/// The universal front door: render ANY text-shaped input into images that
/// carry the full text, exploiting whatever structure the input has.
///
/// Input-shape dispatch:
/// - **directory** → atlas folio: L1 overview + inscribe tiles per region
///   (full source in-territory), budget-governed, coverage reported;
/// - **markdown** (headings) → document map: sections as territories;
/// - **flat text** → dense reflowed pages.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    input: Option<&Path>,
    provider: Provider,
    font_px: f32,
    no_reflow: bool,
    out_dir: Option<&Path>,
    budget: Option<u32>,
    query: &str,
    force: bool,
    json: bool,
    theme: &str,
    layout: &str,
) -> Result<()> {
    // the decorative human map is a theme, not a command
    if theme == "parchment" {
        let root = input
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?);
        if !root.is_dir() {
            bail!("--theme parchment paints the human map of a DIRECTORY (got a file/stdin)");
        }
        return paint_parchment(&root, query, out_dir);
    }
    if let Some(p) = input {
        if p.is_dir() {
            return paint_repo(
                p, provider, font_px, out_dir, budget, query, json, theme, layout,
            );
        }
    }
    let (text, source_name) = match input {
        Some(p) => (
            std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?,
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or("text".into()),
        ),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            (buf, "stdin".to_string())
        }
    };
    if text.trim().is_empty() {
        bail!("nothing to paint (empty input)");
    }

    // structured text → document map — but ONLY for actual prose formats.
    // Code files often contain `# `-lines inside string literals; treating
    // them as headings would route source through the section map and cost
    // coverage. Code takes the flat path, which paginates losslessly.
    let prose = match input {
        Some(p) => matches!(
            p.extension().and_then(|e| e.to_str()).unwrap_or(""),
            "md" | "markdown" | "rst" | "txt"
        ),
        // stdin: require unambiguous markdown shape
        None => {
            text.lines().take(3).any(|l| l.starts_with("# "))
                && text.lines().filter(|l| l.starts_with('#')).count() >= 3
        }
    };
    if !no_reflow && prose {
        if let Some(sections) = ctx2img_core::sections::split_markdown(&text) {
            return paint_doc(
                &text,
                &sections,
                &source_name,
                provider,
                font_px,
                out_dir,
                budget,
                query,
                force,
                json,
                theme,
                layout,
            );
        }
    }
    paint_flat(
        &text,
        &source_name,
        provider,
        font_px,
        no_reflow,
        out_dir,
        force,
        json,
    )
}

/// Flat text → dense reflowed pages (the original paint path).
#[allow(clippy::too_many_arguments)]
fn paint_flat(
    text: &str,
    source_name: &str,
    provider: Provider,
    font_px: f32,
    no_reflow: bool,
    out_dir: Option<&Path>,
    force: bool,
    json: bool,
) -> Result<()> {
    use ctx2img_render::paint as painter;
    let profile = match provider {
        Provider::Openai | Provider::OpenaiMini => painter::PaintProfile::openai(font_px),
        _ => painter::PaintProfile::claude(font_px),
    };
    let content = if no_reflow {
        text.to_string()
    } else {
        painter::reflow(text)
    };
    let pages = painter::paint(&content, &profile, !no_reflow)?;

    let text_tokens = estimate_tokens(text);
    let image_tokens: u32 = pages
        .iter()
        .map(|p| provider.tokens(p.width, p.height))
        .sum();
    let image_side = (image_tokens as f32 * 1.10) as u32;
    if image_side >= text_tokens && !force {
        eprintln!(
            "· not painted: text is cheaper for this input (~{text_tokens} text tok ≤ ~{image_side} image tok incl. margin). Pass --force to paint anyway."
        );
        return Ok(());
    }

    let dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;
    let mut paths = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let path = dir.join(format!("{source_name}-page{}.png", i + 1));
        std::fs::write(&path, &page.png)?;
        paths.push(path);
    }
    let sheet = ctx2img_core::factsheet::render_sheet(&ctx2img_core::factsheet::extract(text, 40));
    report_paint(
        json,
        &painter::banner(pages.len(), !no_reflow),
        &paths
            .iter()
            .zip(&pages)
            .map(|(p, page)| (p.clone(), page.width, page.height))
            .collect::<Vec<_>>(),
        provider,
        image_tokens,
        text_tokens,
        &sheet,
        None,
    );
    Ok(())
}

/// Markdown → one document map: sections as territories carrying full text.
#[allow(clippy::too_many_arguments)]
fn paint_doc(
    full_text: &str,
    sections: &[ctx2img_core::sections::Section],
    source_name: &str,
    provider: Provider,
    font_px: f32,
    out_dir: Option<&Path>,
    budget: Option<u32>,
    query: &str,
    force: bool,
    json: bool,
    theme: &str,
    layout: &str,
) -> Result<()> {
    let theme = machine_theme(theme)?;
    // profitability: the map must cost less than the text it carries; size
    // it down to ~70% of the text cost, and refuse below a useful floor
    let text_tokens = estimate_tokens(full_text);
    let affordable = (text_tokens as f32 * 0.70) as u32;
    let requested = budget.unwrap_or(3600);
    let effective = requested.min(affordable);
    if effective < 500 && !force {
        eprintln!(
            "· not painted: text is cheaper for this document (~{text_tokens} text tok; a useful section map needs ≥500 image tok). Pass --force to paint anyway."
        );
        return Ok(());
    }
    let page_cap = if force { requested } else { effective.max(500) };
    let font = font_px.max(8.0);
    let bands = ctx2img_core::sections::band_sections(sections, query);

    // Pagination: a document larger than one page's budget gets MORE PAGES,
    // never silent truncation — coverage is always 100% at section level.
    // Greedy grouping in document order; an oversized section gets its own
    // page (spilling internally, with an explicit marker).
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_tok = 0u32;
    for (i, s) in sections.iter().enumerate() {
        let t = content_tokens(s.text.len(), 1, font);
        // 8% tolerance: better one slightly-fuller page than a near-empty tail
        if !cur.is_empty() && cur_tok + t > page_cap + page_cap / 12 {
            groups.push(std::mem::take(&mut cur));
            cur_tok = 0;
        }
        cur.push(i);
        cur_tok += t;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }

    let dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;

    let mut pages: Vec<(PathBuf, u32, u32)> = Vec::new();
    let mut image_tokens = 0u32;
    let mut page_of = vec![0usize; sections.len()];
    for (pi, group) in groups.iter().enumerate() {
        let doc_sections: Vec<scene::DocSection> = group
            .iter()
            .map(|&i| scene::DocSection {
                title: sections[i].title.clone(),
                text: sections[i].text.clone(),
                band: bands[i],
            })
            .collect();
        for &i in group {
            page_of[i] = pi + 1;
        }
        let group_chars: usize = group.iter().map(|&i| sections[i].text.len()).sum();
        // canvas fits the group's real content — a small tail page becomes a
        // small (cheap, dense) image, not a big white one
        let page_budget =
            content_tokens(group_chars, group.len(), font).clamp(300.min(page_cap), page_cap);
        let (w, h) = provider.solve(page_budget, 1.0);
        let cfg = SceneConfig {
            width: w,
            height: h,
            title: source_name.to_string(),
            seed: seed_for(source_name) ^ pi as u64,
            text_px: font,
            boxes: layout != "organic",
            ..Default::default()
        };
        let s = scene::build_doc(&doc_sections, &cfg);
        let png = render_png(&s, theme)?;
        let path = if groups.len() == 1 {
            dir.join(format!("{source_name}-map.png"))
        } else {
            dir.join(format!("{source_name}-map{}.png", pi + 1))
        };
        std::fs::write(&path, &png)?;
        image_tokens += provider.tokens(w, h);
        pages.push((path, w, h));
    }

    let sheet =
        ctx2img_core::factsheet::render_sheet(&ctx2img_core::factsheet::extract(full_text, 40));
    let toc: Vec<String> = sections
        .iter()
        .enumerate()
        .map(|(i, sec)| {
            if groups.len() == 1 {
                format!("§{} {} ▲{}", i + 1, sec.title, bands[i])
            } else {
                format!("§{} {} ▲{} (p{})", i + 1, sec.title, bands[i], page_of[i])
            }
        })
        .collect();
    report_paint(
        json,
        &format!(
            "ctx2img paint (this user's local tool) rendered this document as {} section-map page(s) — {} territories, each carrying its full text. Read every territory, pages in order.",
            groups.len(),
            sections.len()
        ),
        &pages,
        provider,
        image_tokens,
        text_tokens,
        &sheet,
        Some(&toc.join(" · ")),
    );
    Ok(())
}

/// Directory → atlas folio: an L1 overview page plus inscribe tiles that
/// carry the FULL SOURCE of each region, highest-relevance regions first,
/// until the token budget is spent. Coverage is reported, never implied.
#[allow(clippy::too_many_arguments)]
/// The human-facing map: organic Voronoi geography, parchment styling,
/// infinite-zoom SVG. Cosmetic output; carries no source text.
fn paint_parchment(root: &Path, query: &str, out_dir: Option<&Path>) -> Result<()> {
    let input = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let repo_root = discover_root(&input);
    let ws = Workspace::open(&repo_root)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let built = ws.build(query, now, true)?;
    print_stats(&built);
    let name = repo_name(&ws);
    let mut saved = SavedSites::load(&ws.layout_path());
    let cfg = SceneConfig {
        width: 1400,
        height: 1000,
        title: format!("The Realm of {name}"),
        seed: seed_for(&name),
        ..Default::default()
    };
    let s = scene::build_l1(&built, &mut saved, &cfg);
    saved.save(&ws.layout_path())?;
    let dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}-parchment.svg"));
    std::fs::write(&path, render_svg(&s, &ParchmentTheme))?;
    println!("map written to {}", path.display());
    Ok(())
}

/// Walk up from `start` to the enclosing repo root (.git or .ctx2img marker).
/// LOC in scope: whole repo, or just the focused subtree.
fn order2_total(built: &Built, subtree: &Option<String>) -> u64 {
    match subtree {
        None => built.analysis.files.iter().map(|f| f.loc as u64).sum(),
        Some(prefix) => built
            .analysis
            .files
            .iter()
            .filter(|f| f.path.starts_with(prefix.as_str()))
            .map(|f| f.loc as u64)
            .sum(),
    }
}

fn discover_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    let mut cur = start.clone();
    loop {
        if cur.join(".git").exists() || cur.join(".ctx2img").exists() {
            return cur;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return start,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_repo(
    root: &Path,
    provider: Provider,
    font_px: f32,
    out_dir: Option<&Path>,
    budget: Option<u32>,
    query: &str,
    json: bool,
    theme: &str,
    layout: &str,
) -> Result<()> {
    let theme = machine_theme(theme)?;
    let boxes = layout != "organic";
    let budget = budget.unwrap_or(12_000);
    // painting a subdirectory analyzes the WHOLE repo (consistent handles,
    // relevance, caches) and filters the folio to that subtree
    let input = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let repo_root = discover_root(&input);
    let subtree: Option<String> = input
        .strip_prefix(&repo_root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let ws = Workspace::open(&repo_root)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let built = ws.build(query, now, true)?;
    print_stats(&built);
    save_last_query(&ws, query);
    let name = repo_name(&ws);
    let dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ws.dir.clone());
    std::fs::create_dir_all(&dir)?;

    // handles must be resolvable afterwards (zoom/read): sidecar always
    sidecar::write(
        &sidecar::build_sidecar(&built, query),
        &ws.dir.join("index.json"),
    )?;

    // small budget ⇒ this IS navigation mode; if the full text roster is
    // cheaper than any useful overview image, emit text and stop
    let legend_full = build_legend(
        &built,
        query,
        &LegendOptions {
            top_files: 8,
            schema: true,
        },
    );
    let roster_tokens = estimate_tokens(&legend_full);
    if subtree.is_none() && roster_tokens <= 900.min(budget) {
        eprintln!("· representation: text (full roster ~{roster_tokens} tok fits the budget best)");
        println!("{legend_full}");
        println!("# {FOOTER}");
        return Ok(());
    }

    // page 1: the L1 overview (index) — cheap situational awareness;
    // skipped when focusing a subtree (the caller knows where they are)
    let mut pages: Vec<(PathBuf, u32, u32)> = Vec::new();
    let mut spent: u32 = 0;
    if subtree.is_none() {
        let (ow, oh) = provider.solve(1800.min(budget).max(900), 1.0);
        let mut saved = SavedSites::load(&ws.layout_path());
        let cfg = SceneConfig {
            width: ow,
            height: oh,
            title: name.clone(),
            seed: seed_for(&name),
            ..Default::default()
        };
        let overview = scene::build_l1(&built, &mut saved, &cfg);
        saved.save(&ws.layout_path())?;
        let overview_path = dir.join(format!("{name}-atlas.png"));
        std::fs::write(&overview_path, render_png(&overview, theme)?)?;
        std::fs::write(
            overview_path.with_extension("legend.txt"),
            build_legend(&built, query, &LegendOptions::default()),
        )?;
        spent += provider.tokens(ow, oh);
        pages.push((overview_path, ow, oh));
    }

    // inscribe tiles, summit-first, until the budget runs out; a subtree
    // focus keeps only regions inside (or containing) the requested path
    let sums = built.region_summaries();
    let mut order: Vec<usize> = (0..built.analysis.tree.regions.len())
        .filter(|&ri| {
            let Some(prefix) = &subtree else { return true };
            let rp = built.analysis.tree.regions[ri].path.as_str();
            rp == prefix
                || rp.starts_with(&format!("{prefix}/"))
                || prefix.starts_with(&format!("{rp}/"))
        })
        .collect();
    order.sort_by(|&x, &y| {
        sums[y]
            .band
            .cmp(&sums[x].band)
            .then(
                built.analysis.tree.regions[y]
                    .loc
                    .cmp(&built.analysis.tree.regions[x].loc),
            )
            .then(x.cmp(&y))
    });
    let mut registry = HandleRegistry::load(&ws.registry_path());
    let per_tile = 2600u32;
    let mut painted_loc: u64 = 0;
    let mut painted_text = String::new();
    let mut skipped: Vec<String> = Vec::new();
    for ri in order {
        let region = &built.analysis.tree.regions[ri];
        let handle = built.region_handles[ri].clone();
        if spent + per_tile > budget {
            skipped.push(format!("{handle} {}", region.display_name()));
            continue;
        }
        let tile_budget = if boxes {
            let bytes: usize = region
                .files
                .iter()
                .filter_map(|f| {
                    std::fs::metadata(ws.root.join(&built.analysis.files[f.idx()].path))
                        .ok()
                        .map(|m| m.len() as usize)
                })
                .sum();
            per_tile.min(fit_budget(
                bytes,
                region.files.len().min(48),
                font_px.max(8.0),
                per_tile,
            ))
        } else {
            per_tile
        };
        let (tw, th) = provider.solve(tile_budget, 1.0);
        let mut tile_sites = SavedSites::load(&ws.dir.join(format!("layout-{handle}.json")));
        let tcfg = SceneConfig {
            width: tw,
            height: th,
            title: format!("{name} · {}", region.display_name()),
            seed: seed_for(region.display_name()),
            text_px: font_px.max(8.0),
            boxes,
            ..Default::default()
        };
        let root_buf = ws.root.clone();
        let loader = move |p: &str| std::fs::read_to_string(root_buf.join(p)).ok();
        let tile = scene::build_l2(
            &built,
            ri,
            &mut registry,
            &mut tile_sites,
            &tcfg,
            Some(&loader),
        );
        tile_sites.save(&ws.dir.join(format!("layout-{handle}.json")))?;
        let path = dir.join(format!("{name}-{handle}.png"));
        std::fs::write(&path, render_png(&tile, theme)?)?;
        spent += provider.tokens(tw, th);
        pages.push((path, tw, th));
        painted_loc += region.loc;
        for &fid in &region.files {
            if let Ok(src) =
                std::fs::read_to_string(ws.root.join(&built.analysis.files[fid.idx()].path))
            {
                painted_text.push_str(&src);
                painted_text.push('\n');
            }
        }
    }
    registry.save(&ws.registry_path())?;

    let total_loc: u64 = order2_total(&built, &subtree);
    let coverage = (painted_loc as f64 / total_loc.max(1) as f64) * 100.0;
    let text_tokens = estimate_tokens(&painted_text);
    let sheet = ctx2img_core::factsheet::render_sheet(&ctx2img_core::factsheet::extract(
        &painted_text[..painted_text.len().min(512 * 1024)],
        40,
    ));
    let legend = build_legend(&built, query, &LegendOptions::default());
    let mut note = if subtree.is_some() {
        format!(
            "subtree folio: each tile carries the FULL SOURCE of one region under {}, most relevant first. Coverage: {coverage:.0}% of {}. ",
            subtree.as_deref().unwrap_or("."),
            human_loc(total_loc)
        )
    } else {
        format!(
            "atlas folio: page 1 is the overview map; the following tiles carry the FULL SOURCE of each region, most relevant first. Coverage: {coverage:.0}% of {}. ",
            human_loc(total_loc)
        )
    };
    if !skipped.is_empty() {
        note.push_str(&format!(
            "Not painted (budget): {} — paint or read them on demand.",
            skipped.join(", ")
        ));
    }
    if !json {
        println!("{legend}");
    }
    report_paint(
        json,
        &note,
        &pages,
        provider,
        spent,
        text_tokens,
        &sheet,
        None,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn report_paint(
    json: bool,
    banner: &str,
    pages: &[(PathBuf, u32, u32)],
    provider: Provider,
    image_tokens: u32,
    text_tokens: u32,
    sheet: &str,
    toc: Option<&str>,
) {
    // the accompanying text (banner + toc + factsheet) — printed AND written
    // beside the first page, since images deliberately carry no chrome
    let mut companion = String::new();
    companion.push_str(banner);
    companion.push('\n');
    if let Some(toc) = toc {
        companion.push_str(&format!("# sections: {toc}\n"));
    }
    if !sheet.is_empty() {
        companion.push_str(sheet);
        companion.push('\n');
    }
    companion.push_str(&format!(
        "# ~{image_tokens} image tok vs ~{text_tokens} text tok (~{:.0}% cut) — READ THE PAGES IN ORDER; quote identifiers from the factsheet, not the image\n",
        (1.0 - image_tokens as f64 / text_tokens.max(1) as f64) * 100.0
    ));
    let legend_path = pages
        .first()
        .map(|(p, _, _)| p.with_extension("legend.txt"));
    if let Some(lp) = &legend_path {
        let _ = std::fs::write(lp, &companion);
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "banner": banner,
                "pages": pages.iter().map(|(p, _, _)| p).collect::<Vec<_>>(),
                "legend_path": legend_path,
                "image_tokens": image_tokens,
                "text_tokens_estimate": text_tokens,
                "savings_pct": (1.0 - image_tokens as f64 / text_tokens.max(1) as f64) * 100.0,
                "factsheet": sheet,
                "toc": toc,
            })
        );
        return;
    }
    for (p, w, h) in pages {
        println!(
            "# page: {} ({w}x{h}, ~{} image tok)",
            p.display(),
            provider.tokens(*w, *h)
        );
    }
    print!("{companion}");
}

// ---------------------------------------------------------------- calibrate

pub fn calibrate(dir: Option<&Path>, live: bool, model: &str, theme: &str) -> Result<()> {
    let theme = machine_theme(theme)?;
    let tmp;
    let dir = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            tmp = tempfile::tempdir()?;
            tmp.path().to_path_buf()
        }
    };
    std::fs::create_dir_all(&dir)?;
    let gt = ctx2img_eval::generate_repo(&dir, 6, 4)?;
    let ws = Workspace::open(&dir)?;
    let built = ws.build(&gt.query, 1_700_000_000, false)?;
    let probes = ctx2img_eval::probes::build_probes(&built, &gt);

    let legend = build_legend(&built, &gt.query, &LegendOptions::default());
    let mut saved = SavedSites::default();
    let cfg = SceneConfig {
        width: 1092,
        height: 1092,
        title: "calibration".into(),
        ..Default::default()
    };
    let s = scene::build_l1(&built, &mut saved, &cfg);
    let png = render_png(&s, theme)?;

    let bundle = ws.dir.join("calibration");
    std::fs::create_dir_all(&bundle)?;
    std::fs::write(bundle.join("atlas.png"), &png)?;
    std::fs::write(bundle.join("legend.txt"), &legend)?;
    std::fs::write(
        bundle.join("probes.json"),
        serde_json::to_string_pretty(&probes)?,
    )?;
    println!("calibration bundle: {}", bundle.display());

    if live {
        let mut passes = 0;
        for p in &probes {
            let answer = ctx2img_eval::live::ask_about_image(&png, &legend, &p.question, model)?;
            let ok = p.accept.iter().any(|a| answer.contains(a.as_str()));
            println!(
                "{} [{}] {} — got: {}",
                if ok { "PASS" } else { "FAIL" },
                p.dimension,
                p.id,
                answer.trim()
            );
            passes += ok as usize;
        }
        println!("legibility: {passes}/{} on {model}", probes.len());
    } else {
        println!("offline mode — set ANTHROPIC_API_KEY and pass --live to probe a model");
    }
    Ok(())
}
