//! Smoke: the HTML snapshot embeds a lowered diagram scene as inline SVG.

use document_ir::{Block, DocumentGraph, DocumentId, Inline};
use document_layout::{Diagrams, LayoutHint, layout_with_diagrams};
use document_presentation::{build_display_presentation, display::DisplayPresentation};
use document_renderer_html::render_html_snapshot;
use plotter_input_mermaid::mermaid_to_scene;

const MERMAID: &str = "flowchart LR\n    A[TeX source] --> B[SVG output]\n";

fn diagram_presentation() -> DisplayPresentation {
    let mut graph = DocumentGraph::new(DocumentId(7));
    let node = graph.push_block(Block::Diagram { language: "mermaid".into(), source: MERMAID.into() });
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "after diagram".into() }] });

    let lowered = mermaid_to_scene(MERMAID).expect("the mermaid source must lower");
    let mut diagrams = Diagrams::new();
    diagrams.insert(node, lowered.scene, "Mermaid diagram");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    build_display_presentation(&plan)
}

fn graphic_box(html: &str) -> &str {
    let after = html.split("data-vutex-item=\"graphic\"").nth(1).expect("snapshot must carry a graphic item");
    let end = after.find("</div>").expect("the graphic box must close");
    &after[..end]
}

#[test]
fn snapshot_html_embeds_the_diagram_scene_as_svg() {
    let html = render_html_snapshot(&diagram_presentation()).expect("snapshot html");

    assert!(html.contains("data-diagram=\"Mermaid diagram\""), "the graphic box must carry the diagram label: {html}");
    let box_markup = graphic_box(&html);
    assert!(box_markup.contains("<svg"), "the graphic box must embed the scene svg, got: {box_markup}");
}
