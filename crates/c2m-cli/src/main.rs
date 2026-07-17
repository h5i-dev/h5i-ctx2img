//! c2m — turn a repository into a query-conditioned map your AI can read.

mod bench;
mod ops;
mod providers;

use clap::{Parser, Subcommand};
use providers::Provider;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "c2m",
    version,
    about = "context2map: render agent context as images that cost a fraction of the tokens.\n\nMain command — c2m paint <input>: any text-shaped input becomes images that CARRY THE FULL TEXT,\nshaped by its structure (directory → atlas folio of full-source region tiles; markdown → section\nmap; flat text → dense pages), always with a verbatim factsheet.\n\nIndex mode — c2m paint --index -q \"<task>\": index-only atlas (~2k tok) for navigating a repo without reading it.",
    after_help = "repo navigation loop: c2m paint --index -q \"<task>\" → read the atlas → c2m zoom R# [--inscribe] → c2m read F#\nrender/badge are human-facing; build/calibrate/bench are plumbing."
)]
struct Cli {
    /// Repository root (default: current directory).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// THE FRONT DOOR: render any text-shaped input into images carrying the
    /// full text — a directory becomes an atlas folio (overview + full-source
    /// region tiles), markdown becomes a section map, flat text becomes dense
    /// pages. Always with a verbatim factsheet.
    Paint {
        /// Input file or directory (omit to read stdin).
        input: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "claude")]
        provider: Provider,
        /// Mono size in px (smaller = denser; 8px is the validated default
        /// for frontier readers).
        #[arg(long, default_value_t = 8.0)]
        font_px: f32,
        /// Keep one source line per row instead of ↵-reflow packing (also
        /// disables markdown section-map detection).
        #[arg(long)]
        no_reflow: bool,
        /// Total image-token budget (directory: default 12000; document map:
        /// 3600). Flat pages ignore it.
        #[arg(long)]
        budget: Option<u32>,
        /// Optional task/query: conditions region and section relevance.
        #[arg(long, short = 'q', default_value = "")]
        query: String,
        /// Index-only mode (formerly `c2m map`): render just the overview
        /// atlas + legend + handles (~2k tok) — navigate without reading.
        /// Input defaults to the current directory.
        #[arg(long)]
        index: bool,
        /// Machine palette: `vlm` (stark, calibrated default) or `warm`
        /// (parchment-flavored candidate — same grammar, softer colors).
        #[arg(long, default_value = "vlm")]
        theme: String,
        /// Territory layout for text-bearing maps: `boxes` (rectangular,
        /// pxpipe-density, default) or `organic` (Voronoi geography).
        #[arg(long, default_value = "boxes")]
        layout: String,
        /// Human-facing social card instead (parchment SVG, 1280x640) —
        /// the README hero image. Input defaults to the current directory.
        #[arg(long)]
        badge: bool,
        /// Output directory for page PNGs (default: current directory).
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Paint even when text tokens would be cheaper.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Zoom into a region (R#: image tile + roster) or file (F#: symbol detail).
    Zoom {
        handle: String,
        /// Image token budget (default 1200; 3600 in --inscribe mode).
        #[arg(long)]
        budget: Option<u32>,
        #[arg(long, value_enum, default_value = "claude")]
        provider: Provider,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Override the stored query for relevance bands.
        #[arg(long)]
        query: Option<String>,
        /// Roster only, no image tile.
        #[arg(long)]
        text: bool,
        #[arg(long)]
        json: bool,
        /// Inscribe mode (v0.2): typeset each file's actual source inside its
        /// territory — the tile carries the text itself.
        #[arg(long)]
        inscribe: bool,
        /// Mono text size in px for --inscribe.
        #[arg(long, default_value_t = 10.0)]
        text_px: f32,
        /// Machine palette: `vlm` (stark default) or `warm`.
        #[arg(long, default_value = "vlm")]
        theme: String,
        /// Territory layout: `boxes` (default) or `organic`.
        #[arg(long, default_value = "boxes")]
        layout: String,
    },
    /// Print exact source for a handle (F#/S#) or path. Layer 3: always text.
    Read {
        target: String,
        /// Line range a:b (1-based, inclusive).
        #[arg(long)]
        lines: Option<String>,
    },
    /// Find handles by path/symbol substring.
    Locate { pattern: String },
    /// Human-facing map (parchment theme by default).
    Render {
        /// Optional query to condition elevation (default: importance).
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long, default_value = "parchment")]
        theme: String,
        #[arg(long, default_value = "svg")]
        format: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 1400)]
        width: u32,
        #[arg(long, default_value_t = 1000)]
        height: u32,
        #[arg(long)]
        title: Option<String>,
    },
    /// (Re)index the repository into .c2m/ (map does this implicitly).
    Build,
    /// Legibility probes on a synthetic repo (offline bundle or --live).
    Calibrate {
        /// Where to generate the synthetic repo (default: temp dir).
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        live: bool,
        #[arg(long, default_value = c2m_eval::live::DEFAULT_MODEL)]
        model: String,
        /// Machine palette to probe: `vlm` or `warm` — A/B them here before
        /// changing any default.
        #[arg(long, default_value = "vlm")]
        theme: String,
    },
    /// Localization benchmark from a tasks JSON file.
    Bench {
        /// JSON: [{"query": "...", "expected": ["path/fragment"]}]
        tasks: PathBuf,
        #[arg(long)]
        live: bool,
        #[arg(long, default_value = c2m_eval::live::DEFAULT_MODEL)]
        model: String,
        #[arg(long, value_enum, default_value = "claude")]
        provider: Provider,
        #[arg(long, default_value_t = 2000)]
        budget: u32,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let repo = cli.repo.as_deref();
    match cli.cmd {
        Cmd::Build => ops::build(repo),
        Cmd::Zoom {
            handle,
            budget,
            provider,
            out,
            query,
            text,
            json,
            inscribe,
            text_px,
            theme,
            layout,
        } => ops::zoom(
            repo,
            &handle,
            budget,
            provider,
            out.as_deref(),
            query.as_deref(),
            text,
            json,
            inscribe,
            text_px,
            &theme,
            &layout,
        ),
        Cmd::Read { target, lines } => ops::read(repo, &target, lines.as_deref()),
        Cmd::Locate { pattern } => ops::locate(repo, &pattern),
        Cmd::Paint {
            input,
            provider: _,
            font_px: _,
            no_reflow: _,
            budget: _,
            query: _,
            index: _,
            out_dir,
            force: _,
            json: _,
            theme: _,
            layout: _,
            badge,
        } if badge => ops::render(
            input.as_deref().or(repo),
            "",
            "parchment",
            "svg",
            Some(
                out_dir
                    .map(|d| d.join("repo-map.svg"))
                    .unwrap_or_else(|| std::path::PathBuf::from("repo-map.svg"))
                    .as_path(),
            ),
            1280,
            640,
            None,
        ),
        Cmd::Paint {
            input,
            provider,
            font_px: _,
            no_reflow: _,
            budget,
            query,
            index,
            out_dir,
            force: _,
            json,
            theme,
            layout: _,
            badge: _,
        } if index => ops::index_atlas(
            input.as_deref().or(repo),
            &query,
            provider,
            budget.unwrap_or(2000),
            out_dir.map(|d| d.join("atlas.png")).as_deref(),
            json,
            ops::Representation::Auto,
            false,
            &theme,
        ),
        Cmd::Paint {
            input,
            provider,
            font_px,
            no_reflow,
            budget,
            query,
            index: _,
            out_dir,
            force,
            json,
            theme,
            layout,
            badge: _,
        } => ops::paint(
            input.as_deref(),
            provider,
            font_px,
            no_reflow,
            out_dir.as_deref(),
            budget,
            &query,
            force,
            json,
            &theme,
            &layout,
        ),
        Cmd::Render {
            query,
            theme,
            format,
            out,
            width,
            height,
            title,
        } => ops::render(
            repo,
            &query,
            &theme,
            &format,
            out.as_deref(),
            width,
            height,
            title.as_deref(),
        ),
        Cmd::Calibrate {
            dir,
            live,
            model,
            theme,
        } => ops::calibrate(dir.as_deref(), live, &model, &theme),
        Cmd::Bench {
            tasks,
            live,
            model,
            provider,
            budget,
        } => bench::bench(repo, &tasks, live, &model, provider, budget),
    }
}
