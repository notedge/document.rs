/// Hard ceilings applied to one render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderLimits {
    /// Largest accepted source, in bytes, per template.
    pub max_source_bytes: usize,
    /// Largest produced output, in bytes.
    pub max_output_bytes: usize,
    /// Deepest include recursion.
    pub max_include_depth: u32,
    /// Node evaluations allowed for one render.
    pub max_steps: u32,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 8 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_include_depth: 32,
            max_steps: 1_000_000,
        }
    }
}

/// One render's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderOptions {
    /// Reject missing names and members instead of rendering them empty.
    pub strict_undefined: bool,
    /// Budget ceilings.
    pub limits: RenderLimits,
}
