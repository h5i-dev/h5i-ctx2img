//! `ctx2img bench` — localization benchmark: atlas+legend vs text-only legend
//! at matched budgets. Offline it emits reproducible bundles; with
//! ANTHROPIC_API_KEY and --live it scores a real model.

use crate::ops;
use crate::providers::Provider;
use anyhow::{Context, Result};
use ctx2img_index::legend::{build_legend, LegendOptions};
use ctx2img_layout::SavedSites;
use ctx2img_render::{render_png, scene, SceneConfig, VlmTheme};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Task {
    pub query: String,
    /// Path fragments; an answer containing any of them scores a hit.
    pub expected: Vec<String>,
}

pub fn bench(
    repo: Option<&Path>,
    tasks_path: &Path,
    live: bool,
    model: &str,
    provider: Provider,
    budget: u32,
) -> Result<()> {
    let tasks: Vec<Task> = serde_json::from_str(
        &std::fs::read_to_string(tasks_path)
            .with_context(|| format!("read {}", tasks_path.display()))?,
    )
    .context("tasks file: JSON array of {query, expected:[...]}")?;
    let ctx = ops::open(repo)?;

    let mut hits_atlas = 0usize;
    let mut hits_text = 0usize;
    for (i, task) in tasks.iter().enumerate() {
        let built = ctx.ws.build(&task.query, ctx.now, false)?;
        let legend = build_legend(&built, &task.query, &LegendOptions::default());
        let legend_full = build_legend(
            &built,
            &task.query,
            &LegendOptions {
                top_files: 8,
                schema: true,
            },
        );
        let (w, h) = provider.solve(budget, 1.0);
        let mut saved = SavedSites::load(&ctx.ws.layout_path());
        let cfg = SceneConfig {
            width: w,
            height: h,
            title: String::new(),
            ..Default::default()
        };
        let s = scene::build_l1(&built, &mut saved, &cfg);
        let png = render_png(&s, &VlmTheme)?;

        let bundle = ctx.ws.dir.join("bench").join(format!("task-{i}"));
        std::fs::create_dir_all(&bundle)?;
        std::fs::write(bundle.join("atlas.png"), &png)?;
        std::fs::write(bundle.join("legend.txt"), &legend)?;
        std::fs::write(bundle.join("legend-full.txt"), &legend_full)?;

        if live {
            let q = format!(
                "Task: {}\nWhich file most likely needs to be edited? Answer with one repository-relative file path only.",
                task.query
            );
            let a_atlas = ctx2img_eval::live::ask_about_image(&png, &legend, &q, model)?;
            let a_text = ctx2img_eval::live::ask_text(&format!("{legend_full}\n\n{q}"), model)?;
            let hit = |ans: &str| task.expected.iter().any(|e| ans.contains(e.as_str()));
            let (ha, ht) = (hit(&a_atlas), hit(&a_text));
            hits_atlas += ha as usize;
            hits_text += ht as usize;
            println!(
                "task {i}: atlas {} · text {} — \"{}\"",
                if ha { "HIT " } else { "miss" },
                if ht { "HIT " } else { "miss" },
                task.query
            );
        }
    }
    if live {
        println!(
            "\nlocalization hit@1 — atlas+legend: {hits_atlas}/{} · text-only: {hits_text}/{} ({model})",
            tasks.len(),
            tasks.len()
        );
    } else {
        println!(
            "offline mode: wrote {} bundles under {} — set ANTHROPIC_API_KEY and pass --live to score",
            tasks.len(),
            ctx.ws.dir.join("bench").display()
        );
    }
    Ok(())
}
