#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

pub mod codes;

mod budget;
mod capability;
mod catalog;
mod error;
mod limits;
mod render;

pub use budget::RenderBudget;
pub use capability::{TemplateCapability, TemplateContentKind, TemplateStatus};
pub use catalog::{MapTemplateLoader, TemplateId, TemplateLoader, TemplateRoot, normalize_path, split_scheme};
pub use error::{COMPONENT, NAMESPACE, TemplateError, origin};
pub use limits::{RenderLimits, RenderOptions};
pub use render::{RenderOutput, RenderRequest, TemplateEngine};

/// Stable contract identifier for the template namespace (`v0`).
pub const TEMPLATE_CONTRACT_V0: &str = "template.contract/v0";
