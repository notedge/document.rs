//! Page geometry and body metrics declared by a source document.
//!
//! A page setup is presentation, not document semantics: it records what the
//! source format declared (page box, margins, column count, body size, line
//! height) so an exporter can repeat the declaration instead of falling back
//! to its own defaults. Every field is optional because most formats declare
//! only part of it.

use serde::{Deserialize, Serialize};

/// Page geometry and body metrics declared by the source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageSetup {
    /// Page width in points.
    pub page_width: Option<f64>,
    /// Page height in points.
    pub page_height: Option<f64>,
    /// Top margin in points, measured from the page edge.
    pub margin_top: Option<f64>,
    /// Right margin in points, measured from the page edge.
    pub margin_right: Option<f64>,
    /// Bottom margin in points, measured from the page edge.
    pub margin_bottom: Option<f64>,
    /// Left margin in points, measured from the page edge.
    pub margin_left: Option<f64>,
    /// Column count when the source declared one.
    pub columns: Option<u16>,
    /// Body text size in points.
    pub base_font_size: Option<f64>,
    /// Line box height as a multiple of the body size.
    pub line_height: Option<f64>,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            page_width: None,
            page_height: None,
            margin_top: None,
            margin_right: None,
            margin_bottom: None,
            margin_left: None,
            columns: None,
            base_font_size: None,
            line_height: None,
        }
    }
}

impl PageSetup {
    /// Whether the source declared any page or body fact.
    ///
    /// Callers keep `DocumentGraph::page_setup` as `None` when this is false so
    /// an unknown page setup stays distinguishable from a declared one.
    pub fn has_values(&self) -> bool {
        self.page_width.is_some()
            || self.page_height.is_some()
            || self.margin_top.is_some()
            || self.margin_right.is_some()
            || self.margin_bottom.is_some()
            || self.margin_left.is_some()
            || self.columns.is_some()
            || self.base_font_size.is_some()
            || self.line_height.is_some()
    }
}
