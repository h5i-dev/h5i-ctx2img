//! ctx2img-eval — legibility calibration and retrieval benchmarks.
//!
//! Philosophy: legibility is a *tested* property. `calibrate` builds a
//! synthetic repo with planted ground truth, renders the atlas, and emits
//! objective probe questions; `score` grades a model's answers. Probes can
//! run offline (emit a bundle for any harness) or live against the
//! Anthropic API via `curl` when ANTHROPIC_API_KEY is set.

pub mod live;
pub mod probes;
pub mod synthetic;

pub use probes::{score_answers, Probe};
pub use synthetic::generate_repo;
