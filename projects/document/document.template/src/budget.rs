use crate::{RenderLimits, TemplateError, codes};

/// Per-render accounting shared by every engine.
///
/// Engines count each evaluated node with [`RenderBudget::step`], append output
/// through [`RenderBudget::write`], and check one template source through
/// [`RenderBudget::source`]. A limit is a hard ceiling: the render fails with a
/// stable code instead of truncating output.
#[derive(Debug)]
pub struct RenderBudget {
    limits: RenderLimits,
    steps: u32,
    output_bytes: usize,
}

impl RenderBudget {
    /// Create a budget from render limits.
    pub fn new(limits: RenderLimits) -> Self {
        Self { limits, steps: 0, output_bytes: 0 }
    }

    /// Limits this budget enforces.
    #[must_use]
    pub fn limits(&self) -> &RenderLimits {
        &self.limits
    }

    /// Bytes already written to the output.
    #[must_use]
    pub fn output_bytes(&self) -> usize {
        self.output_bytes
    }

    /// Count one evaluated node.
    pub fn step(&mut self) -> Result<(), TemplateError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.limits.max_steps {
            return Err(TemplateError::with_args(
                codes::STEP_BUDGET,
                format!("render exceeded {} node evaluations", self.limits.max_steps),
                vec![("limit", self.limits.max_steps.to_string())],
            ));
        }
        Ok(())
    }

    /// Reject one template source above the per-template byte ceiling.
    pub fn source(&self, source: &str) -> Result<(), TemplateError> {
        if source.len() > self.limits.max_source_bytes {
            return Err(TemplateError::with_args(
                codes::SOURCE_BUDGET,
                format!("template source is {} bytes, above the {} byte limit", source.len(), self.limits.max_source_bytes),
                vec![("actual", source.len().to_string()), ("limit", self.limits.max_source_bytes.to_string())],
            ));
        }
        Ok(())
    }

    /// Append text while enforcing the output ceiling.
    pub fn write(&mut self, output: &mut String, text: &str) -> Result<(), TemplateError> {
        let next = self.output_bytes.saturating_add(text.len());
        if next > self.limits.max_output_bytes {
            return Err(TemplateError::with_args(
                codes::OUTPUT_BUDGET,
                format!("rendered output would exceed {} bytes", self.limits.max_output_bytes),
                vec![("limit", self.limits.max_output_bytes.to_string())],
            ));
        }
        self.output_bytes = next;
        output.push_str(text);
        Ok(())
    }
}
