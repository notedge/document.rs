use document_compose::Diagnostic;
use document_presentation::display::DisplayPresentation;

/// Output backend contract. Implementations must not read source languages.
pub trait RenderBackend {
    /// Render a display presentation into the target format bytes.
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic>;
}
