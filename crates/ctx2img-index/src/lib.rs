//! ctx2img-index — persistence around the pure analysis core: stable handle
//! registry, content-hash parse cache, legend/roster emission, and the
//! sidecar index that resolves handles back to exact source.

pub mod cache;
pub mod handles;
pub mod legend;
pub mod sidecar;
pub mod workspace;

pub use handles::{HandleRegistry, Kind};
pub use workspace::{BuildStats, Built, Workspace};
