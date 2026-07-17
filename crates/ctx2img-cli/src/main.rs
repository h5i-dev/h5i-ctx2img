//! ctx2img — turn a repository into a query-conditioned map your AI can read.

mod bench;
mod ops;
mod providers;

use clap::{Parser, Subcommand};
use providers::Provider;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ctx2img",
    version,
    about = "ctx2img: render agent context as images that cost a fraction of the tokens.\n\nMain command — ctx2img paint <input>: any text-shaped input becomes images that CARRY THE FULL TEXT,\nshaped by its structure (directory → atlas folio of full-source region tiles; markdown → section\nmap; flat text → dense pages), always with a verbatim factsheet.\n\nNavigation mode — ctx2img paint <dir> --budget 2000 -q \"<task>\": at a small budget the folio degrades to the index atlas (or a text roster if that's cheaper).",
    after_help = "repo navigation loop: ctx2img paint . --budget 2000 -q \"<task>\" → read the atlas → ctx2img paint <dir> → ctx2img read F#\n--theme parchment is the human map; calibrate/bench are plumbing."
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
        /// `vlm` (stark black-on-white default), `warm`, `dark` — machine
        /// palettes sharing one grammar, A/B'd by `ctx2img calibrate` — or
        /// `parchment`: the decorative human map of a directory (SVG).
        #[arg(long, default_value = "vlm")]
        theme: String,
        /// Territory layout for text-bearing maps: `boxes` (rectangular,
        /// pxpipe-density, default) or `organic` (Voronoi geography).
        #[arg(long, default_value = "boxes")]
        layout: String,
        /// Output directory for page PNGs (default: current directory).
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Paint even when text tokens would be cheaper.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Exact source text for a handle (F#/S#) or path — the escape hatch
    /// pixels can't provide (quoting, editing, hash-comparing).
    Read {
        /// F#/S# handle or repo-relative path (omit when using --find).
        target: Option<String>,
        /// Line range a:b (1-based, inclusive).
        #[arg(long)]
        lines: Option<String>,
        /// Search instead: substring over paths and symbol names, answers
        /// in handles ready for `read`.
        #[arg(long)]
        find: Option<String>,
    },
    /// Legibility probes on a synthetic repo (offline bundle or --live).
    Calibrate {
        /// Where to generate the synthetic repo (default: temp dir).
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        live: bool,
        #[arg(long, default_value = ctx2img_eval::live::DEFAULT_MODEL)]
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
        #[arg(long, default_value = ctx2img_eval::live::DEFAULT_MODEL)]
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
        Cmd::Read {
            target,
            lines,
            find,
        } => ops::read(repo, target.as_deref(), lines.as_deref(), find.as_deref()),
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
            theme,
            layout,
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
