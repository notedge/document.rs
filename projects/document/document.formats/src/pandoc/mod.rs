//! Pandoc JSON exchange format.
//!
//! The crate owns the typed Pandoc AST, the strict wire codec, and the IR
//! adapters. It does not own Oak or Acorn semantics: JSON syntax is handled by
//! `serde_json`, while Pandoc constructors live only in this module.

pub mod codec;
mod export;
mod import;
pub mod model;

pub use codec::{FORMAT_ID, PandocBudget};
pub use export::{PandocExport, export_pandoc_json};
pub use import::{
    PandocImportOptions, PandocLossPolicy, import_pandoc_json, import_pandoc_json_bytes, import_pandoc_json_bytes_with,
    import_pandoc_json_text,
};
