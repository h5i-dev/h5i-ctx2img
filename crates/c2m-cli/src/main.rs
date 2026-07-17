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
    about = "context2map: render agent context as images that cost a fraction of the tokens.\n\nMain command — c2m paint <input>: any text-shaped input becomes images that CARRY THE FULL TEXT,\nshaped by its structure (directory → atlas folio of full-source region tiles; markdown → section\nmap; flat text → dense pages), always with a verbatim factsheet.\n\nSpecialist — c2m map \"<task>\": index-only atlas (~2k tok) for navigating a repo without reading it.",
    after_help = "repo navigation loop: c2m map \"<task>\" → read the atlas → c2m zoom R# [--inscribe] → c2m read F#\nrender/badge are human-facing; build/calibrate/bench are plumbing."
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
        #[arg(long, default_value = "")]
        query: String,
        /// Output directory for page PNGs (default: current directory).
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Paint even when text tokens would be cheaper.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Render the query-conditioned atlas: image + legend + handles.
    Map {
        /// The task/query that conditions elevation (what you're working on).
        query: String,
        #[arg(long, value_enum, default_value = "claude")]
        provider: Provider,
        /// Image token budget for the atlas.
        #[arg(long, default_value_t = 2000)]
        budget: u32,
        /// Atlas output path (default: .c2m/atlas.png).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Machine-readable output {atlas_path, legend, ...}.
        #[arg(long)]
        json: bool,
        /// auto picks the cheaper of image vs text for this repo size.
        #[arg(long, value_enum, default_value = "auto")]
        representation: ops::Representation,
        /// Skip git history signals (churn, co-change).
        #[arg(long)]
        no_history: bool,
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
    /// README hero image: parchment SVG at social-card size.
    Badge {
        #[arg(long)]
        out: Option<PathBuf>,
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
        Cmd::Map {
            query,
            provider,
            budget,
            out,
            json,
            representation,
            no_history,
        } => ops::map(
            repo,
            &query,
            provider,
            budget,
            out.as_deref(),
            json,
            representation,
            no_history,
        ),
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
        ),
        Cmd::Read { target, lines } => ops::read(repo, &target, lines.as_deref()),
        Cmd::Locate { pattern } => ops::locate(repo, &pattern),
        Cmd::Paint {
            input,
            provider,
            font_px,
            no_reflow,
            budget,
            query,
            out_dir,
            force,
            json,
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
        Cmd::Badge { out } => ops::render(
            repo,
            "",
            "parchment",
            "svg",
            out.as_deref()
                .or(Some(std::path::Path::new("repo-map.svg"))),
            1280,
            640,
            None,
        ),
        Cmd::Calibrate { dir, live, model } => ops::calibrate(dir.as_deref(), live, &model),
        Cmd::Bench {
            tasks,
            live,
            model,
            provider,
            budget,
        } => bench::bench(repo, &tasks, live, &model, provider, budget),
    }
}
