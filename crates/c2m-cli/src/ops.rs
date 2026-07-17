//! Command implementations.

use crate::providers::Provider;
use anyhow::{bail, Context, Result};
use c2m_core::hazard;
use c2m_index::legend::{build_legend, estimate_tokens, human_loc, LegendOptions};
use c2m_index::sidecar;
use c2m_index::workspace::{Built, Workspace};
use c2m_index::{HandleRegistry, Kind};
use c2m_layout::SavedSites;
use c2m_render::{render_png, render_svg, scene, ParchmentTheme, SceneConfig, Theme, VlmTheme};
use std::path::{Path, PathBuf};

pub const FOOTER: &str =
    "next: `c2m zoom R#` region detail · `c2m read F#|S#` exact source · `c2m locate <pat>` search";

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

fn last_query(ws: &Workspace) -> String {
    std::fs::read_to_string(ws.dir.join("last-query.txt")).unwrap_or_default()
}

// ---------------------------------------------------------------- build

pub fn build(repo: Option<&Path>) -> Result<()> {
    let ctx = open(repo)?;
    let built = ctx.ws.build("", ctx.now, true)?;
    print_stats(&built);
    println!(
        "indexed {} files, {} regions, {} edges → {}",
        built.analysis.files.len(),
        built.analysis.tree.regions.len(),
        built.analysis.graph.edges.len(),
        ctx.ws.dir.display()
    );
    Ok(())
}

// ---------------------------------------------------------------- map

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Representation {
    Auto,
    Image,
    Text,
}

#[allow(clippy::too_many_arguments)]
pub fn map(
    repo: Option<&Path>,
    query: &str,
    provider: Provider,
    budget: u32,
    out: Option<&Path>,
    json: bool,
    representation: Representation,
    no_history: bool,
) -> Result<()> {
    let ctx = open(repo)?;
    let built = ctx.ws.build(query, ctx.now, !no_history)?;
    print_stats(&built);
    save_last_query(&ctx.ws, query);

    let legend = build_legend(&built, query, &LegendOptions::default());
    let legend_full = build_legend(
        &built,
        query,
        &LegendOptions {
            top_files: 8,
            schema: true,
        },
    );
    let (w, h) = provider.solve(budget, 1.0);
    let image_tokens = provider.tokens(w, h);
    let text_tokens = estimate_tokens(&legend_full);
    let image_total = image_tokens + estimate_tokens(&legend);

    let use_image = match representation {
        Representation::Image => true,
        Representation::Text => false,
        Representation::Auto => image_total < text_tokens,
    };

    if !use_image {
        eprintln!(
            "· representation: text (full roster ~{text_tokens} tok ≤ atlas ~{image_total} tok)"
        );
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "representation": "text",
                    "legend": legend_full,
                    "legend_tokens": text_tokens,
                })
            );
        } else {
            println!("{legend_full}");
            println!("# {FOOTER}");
        }
        return Ok(());
    }

    let atlas_path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ctx.ws.dir.join("atlas.png"));
    let legend_path = atlas_path.with_extension("legend.txt");
    let sidecar_path = ctx.ws.dir.join("index.json");

    let mut saved = SavedSites::load(&ctx.ws.layout_path());
    let cfg = SceneConfig {
        width: w,
        height: h,
        title: repo_name(&ctx.ws),
        seed: seed_for(&repo_name(&ctx.ws)),
        ..Default::default()
    };
    let s = scene::build_l1(&built, &mut saved, &cfg);
    saved.save(&ctx.ws.layout_path())?;
    let png = render_png(&s, &VlmTheme)?;
    std::fs::write(&atlas_path, &png).with_context(|| format!("write {atlas_path:?}"))?;
    std::fs::write(&legend_path, &legend)?;
    sidecar::write(&sidecar::build_sidecar(&built, query), &sidecar_path)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "representation": "image",
                "atlas_path": atlas_path,
                "legend_path": legend_path,
                "sidecar_path": sidecar_path,
                "legend": legend,
                "provider": provider.name(),
                "width": w,
                "height": h,
                "image_tokens": image_tokens,
                "legend_tokens": estimate_tokens(&legend),
            })
        );
    } else {
        println!("{legend}");
        println!(
            "# atlas: {} ({w}x{h}, ~{image_tokens} image tok on {}) — READ THIS IMAGE for the full geography",
            atlas_path.display(),
            provider.name()
        );
        println!("# {FOOTER}");
    }
    Ok(())
}

fn seed_for(name: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------- zoom

#[allow(clippy::too_many_arguments)]
pub fn zoom(
    repo: Option<&Path>,
    handle: &str,
    budget: Option<u32>,
    provider: Provider,
    out: Option<&Path>,
    query: Option<&str>,
    text_only: bool,
    json: bool,
    inscribe: bool,
    text_px: f32,
) -> Result<()> {
    // inscribe tiles carry actual source text: they earn a larger default canvas
    let budget = budget.unwrap_or(if inscribe { 3600 } else { 1200 });
    let ctx = open(repo)?;
    let query = query
        .map(str::to_string)
        .unwrap_or_else(|| last_query(&ctx.ws));
    let built = ctx.ws.build(&query, ctx.now, false)?;
    print_stats(&built);
    let mut registry = HandleRegistry::load(&ctx.ws.registry_path());

    let entry = registry
        .resolve(handle)
        .with_context(|| format!("unknown handle {handle} (run `c2m map` first)"))?
        .clone();

    match entry.kind {
        Kind::Region => {
            let ri = built
                .analysis
                .tree
                .regions
                .iter()
                .position(|r| r.display_name() == entry.key)
                .with_context(|| format!("region {} no longer exists", entry.key))?;
            let roster = region_roster(&built, ri, &mut registry);

            let mut answer = serde_json::json!({ "roster": roster });
            if !text_only {
                let (w, h) = provider.solve(budget, 1.0);
                let mut saved = SavedSites::load(&ctx.ws.dir.join(format!("layout-{handle}.json")));
                let cfg = SceneConfig {
                    width: w,
                    height: h,
                    title: format!("{} · {}", repo_name(&ctx.ws), entry.key),
                    seed: seed_for(&entry.key),
                    text_px,
                    ..Default::default()
                };
                let root = ctx.ws.root.clone();
                let loader = move |path: &str| std::fs::read_to_string(root.join(path)).ok();
                let s = scene::build_l2(
                    &built,
                    ri,
                    &mut registry,
                    &mut saved,
                    &cfg,
                    if inscribe { Some(&loader) } else { None },
                );
                saved.save(&ctx.ws.dir.join(format!("layout-{handle}.json")))?;
                let png = render_png(&s, &VlmTheme)?;
                let path = out
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| ctx.ws.dir.join(format!("zoom-{handle}.png")));
                std::fs::write(&path, &png)?;
                answer["tile_path"] = serde_json::json!(path);
                answer["image_tokens"] = serde_json::json!(provider.tokens(w, h));
                if !json {
                    println!("{roster}");
                    println!(
                        "# tile: {} ({w}x{h}, ~{} image tok) — READ THIS IMAGE for {}'s interior",
                        path.display(),
                        provider.tokens(w, h),
                        handle
                    );
                    println!("# {FOOTER}");
                }
            } else if !json {
                println!("{roster}");
                println!("# {FOOTER}");
            }
            registry.save(&ctx.ws.registry_path())?;
            if json {
                println!("{answer}");
            }
        }
        Kind::File => {
            let fi = built
                .analysis
                .files
                .iter()
                .position(|f| f.path == entry.key)
                .with_context(|| format!("file {} no longer exists", entry.key))?;
            let detail = file_detail(&built, fi, &mut registry);
            registry.save(&ctx.ws.registry_path())?;
            if json {
                println!("{}", serde_json::json!({ "detail": detail }));
            } else {
                println!("{detail}");
                println!("# {FOOTER}");
            }
        }
        Kind::Symbol | Kind::External => {
            bail!("zoom works on R (region) and F (file) handles; use `c2m read {handle}`")
        }
    }
    Ok(())
}

fn region_roster(built: &Built, ri: usize, registry: &mut HandleRegistry) -> String {
    let a = &built.analysis;
    let r = &a.tree.regions[ri];
    let sums = built.region_summaries();
    let mut out = format!(
        "{} {} ▲{} — {} files, {}\n",
        built.region_handles[ri],
        r.display_name(),
        sums[ri].band,
        r.files.len(),
        human_loc(r.loc)
    );
    for &(fi, _) in &sums[ri].ranked_files {
        let f = &a.files[fi];
        let mut line = format!(
            "  {} {} ▲{} {} {}",
            built.file_handles[fi],
            f.path,
            a.relevance.bands[fi],
            human_loc(f.loc as u64),
            f.lang.tag()
        );
        let tags = hazard::tags(a.parsed[fi].hazards);
        if !tags.is_empty() {
            line.push_str(&format!(" ⚠{}", tags.join(",")));
        }
        // top symbols with S handles
        let syms: Vec<String> = a.parsed[fi]
            .symbols
            .iter()
            .take(4)
            .map(|sym| {
                let h = registry.assign_symbol(
                    &f.path,
                    &sym.name,
                    sym.kind.tag(),
                    (sym.line, sym.line_end),
                );
                format!("{h}:{}", sym.name)
            })
            .collect();
        if !syms.is_empty() {
            line.push_str(&format!(" | {}", syms.join(" ")));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn file_detail(built: &Built, fi: usize, registry: &mut HandleRegistry) -> String {
    let a = &built.analysis;
    let f = &a.files[fi];
    let p = &a.parsed[fi];
    let mut out = format!(
        "{} {} ▲{} — {} {}\n",
        built.file_handles[fi],
        f.path,
        a.relevance.bands[fi],
        human_loc(f.loc as u64),
        f.lang.tag()
    );
    let tags = hazard::tags(p.hazards);
    if !tags.is_empty() {
        out.push_str(&format!("hazards: ⚠{}\n", tags.join(",")));
    }
    if !p.imports.is_empty() {
        out.push_str(&format!("imports: {}\n", p.imports.join(" · ")));
    }
    out.push_str("symbols:\n");
    for sym in &p.symbols {
        let h =
            registry.assign_symbol(&f.path, &sym.name, sym.kind.tag(), (sym.line, sym.line_end));
        out.push_str(&format!(
            "  {h} {} {} (L{}–{})\n",
            sym.kind.tag(),
            sym.name,
            sym.line,
            sym.line_end
        ));
    }
    out.push_str(&format!(
        "exact source: `c2m read {}`\n",
        built.file_handles[fi]
    ));
    out
}

// ---------------------------------------------------------------- read

pub fn read(repo: Option<&Path>, target: &str, lines: Option<&str>) -> Result<()> {
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

// ---------------------------------------------------------------- locate

pub fn locate(repo: Option<&Path>, pattern: &str) -> Result<()> {
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

#[allow(clippy::too_many_arguments)]
pub fn render(
    repo: Option<&Path>,
    query: &str,
    theme_name: &str,
    format: &str,
    out: Option<&Path>,
    width: u32,
    height: u32,
    title: Option<&str>,
) -> Result<()> {
    let ctx = open(repo)?;
    let built = ctx.ws.build(query, ctx.now, true)?;
    print_stats(&built);
    let theme: &dyn Theme = match theme_name {
        "vlm" => &VlmTheme,
        _ => &ParchmentTheme,
    };
    let name = repo_name(&ctx.ws);
    let mut saved = SavedSites::load(&ctx.ws.layout_path());
    let cfg = SceneConfig {
        width,
        height,
        title: title
            .map(str::to_string)
            .unwrap_or_else(|| format!("The Realm of {name}")),
        seed: seed_for(&name),
        ..Default::default()
    };
    let s = scene::build_l1(&built, &mut saved, &cfg);
    saved.save(&ctx.ws.layout_path())?;

    let path = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("repo-map.{format}")));
    match format {
        "png" => std::fs::write(&path, render_png(&s, theme)?)?,
        "svg" => std::fs::write(&path, render_svg(&s, theme))?,
        other => bail!("unknown format {other} (png|svg)"),
    }
    println!("map written to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------- paint

/// Render arbitrary text (file or stdin) into dense image pages + a
/// verbatim factsheet. The generic "any context → image" entry point.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    input: Option<&Path>,
    provider: Provider,
    font_px: f32,
    no_reflow: bool,
    out_dir: Option<&Path>,
    force: bool,
    json: bool,
) -> Result<()> {
    use c2m_render::paint as painter;
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

    let profile = match provider {
        Provider::Openai | Provider::OpenaiMini => painter::PaintProfile::openai(font_px),
        _ => painter::PaintProfile::claude(font_px),
    };
    let content = if no_reflow {
        text.clone()
    } else {
        painter::reflow(&text)
    };
    let pages = painter::paint(&content, &profile, !no_reflow)?;

    // token accounting: honest counterfactual, estimate on both sides
    let text_tokens = estimate_tokens(&text);
    let image_tokens: u32 = pages
        .iter()
        .map(|p| provider.tokens(p.width, p.height))
        .sum();
    // +10% safety margin on the image side, mirroring field practice
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

    let facts = c2m_core::factsheet::extract(&text, 40);
    let sheet = c2m_core::factsheet::render_sheet(&facts);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "pages": paths,
                "image_tokens": image_tokens,
                "text_tokens_estimate": text_tokens,
                "savings_pct": (1.0 - image_tokens as f64 / text_tokens.max(1) as f64) * 100.0,
                "factsheet": sheet,
                "banner": painter::banner(pages.len(), !no_reflow),
            })
        );
    } else {
        println!("{}", painter::banner(pages.len(), !no_reflow));
        for (p, page) in paths.iter().zip(&pages) {
            println!(
                "# page: {} ({}x{}, {} chars, ~{} image tok)",
                p.display(),
                page.width,
                page.height,
                page.chars,
                provider.tokens(page.width, page.height)
            );
        }
        if !sheet.is_empty() {
            println!("{sheet}");
        }
        println!(
            "# ~{image_tokens} image tok vs ~{text_tokens} text tok (~{:.0}% cut) — READ THE PAGES IN ORDER; quote identifiers from the factsheet line, not the image",
            (1.0 - image_tokens as f64 / text_tokens.max(1) as f64) * 100.0
        );
    }
    Ok(())
}

// ---------------------------------------------------------------- calibrate

pub fn calibrate(dir: Option<&Path>, live: bool, model: &str) -> Result<()> {
    let tmp;
    let dir = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            tmp = tempfile::tempdir()?;
            tmp.path().to_path_buf()
        }
    };
    std::fs::create_dir_all(&dir)?;
    let gt = c2m_eval::generate_repo(&dir, 6, 4)?;
    let ws = Workspace::open(&dir)?;
    let built = ws.build(&gt.query, 1_700_000_000, false)?;
    let probes = c2m_eval::probes::build_probes(&built, &gt);

    let legend = build_legend(&built, &gt.query, &LegendOptions::default());
    let mut saved = SavedSites::default();
    let cfg = SceneConfig {
        width: 1092,
        height: 1092,
        title: "calibration".into(),
        ..Default::default()
    };
    let s = scene::build_l1(&built, &mut saved, &cfg);
    let png = render_png(&s, &VlmTheme)?;

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
            let answer = c2m_eval::live::ask_about_image(&png, &legend, &p.question, model)?;
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
