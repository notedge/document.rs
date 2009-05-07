use document_compose::{Diagnostic, Severity};
use document_presentation::display::DisplayPresentation;
use document_renderer::RenderBackend;

/// Raster renderer stub.
pub fn render_raster(presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
    let backend = RasterBackend;
    backend.render_display(presentation)
}

struct RasterBackend;

impl RenderBackend for RasterBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        let _ = presentation;
        Err(Diagnostic {
            code: "vutex.render.raster.unimplemented".into(),
            severity: Severity::Error,
            message: "Raster backend is not implemented yet".into(),
            span: None,
        })
    }
}
