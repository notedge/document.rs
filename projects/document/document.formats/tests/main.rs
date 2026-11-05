//! 冒烟入口：能力表与 feature 开关的一致性断言，以及 Markdown 损失 span 与数学 IR。

mod capability;
mod markdown_loss_span;
mod markdown_math_ir;

/// Cross-reference and verbatim constructs that used to fail or drift.
#[cfg(all(feature = "html", feature = "tex"))]
mod tex_export_smoke {
    use document_formats::{export::html::export_html, import::tex::import_tex_bytes};

    #[test]
    fn section_label_ref_resolves_to_the_block_anchor() {
        let graph = import_tex_bytes("label-ref", "\\section{A}\\label{a}\nsee \\ref{a}\n").expect("tex import");
        let html = export_html(&graph).expect("html export");
        assert!(html.contains(r#"id="node-1""#), "{html}");
        assert!(html.contains(r##"href="#node-1""##), "{html}");
    }

    #[test]
    fn verbatim_stays_inline_code() {
        let graph = import_tex_bytes("verb", "\\verb|a|\n").expect("tex import");
        let html = export_html(&graph).expect("html export");
        assert!(html.contains("<code>a</code>"), "{html}");
        assert!(!html.contains("<pre>"), "{html}");
    }

    #[test]
    fn sizing_options_are_a_loss_not_a_refusal() {
        let graph = import_tex_bytes("sizing", "\\includegraphics[width=2cm]{a.png}\n").expect("tex import");
        let marker =
            graph.coverage.loss.iter().find(|marker| marker.code == "import.tex.image_dimensions").expect("sizing loss marker");
        assert_eq!(marker.status, document_ir::SemanticStatus::Partial, "{:?}", graph.coverage.loss);
        let html = export_html(&graph).expect("html export");
        assert!(html.contains(r#"src="a.png""#), "{html}");
    }
}

/// Constructs the HTML projection degrades or defers instead of failing.
#[cfg(all(feature = "html", feature = "tex"))]
mod tex_local_degradation_smoke {
    use document_formats::{export::html::export_html, import::tex::import_tex_bytes};
    use document_ir::DocumentGraph;

    fn has_loss(graph: &DocumentGraph, code: &str) -> bool {
        graph.coverage.loss.iter().any(|marker| marker.code == code)
    }

    #[test]
    fn citation_degrades_to_its_key() {
        let graph = import_tex_bytes("cite", "\\cite{a}\n").expect("tex import");
        let html = export_html(&graph).expect("html export");
        assert!(html.contains(r#"data-citation-key="a""#), "{html}");
        assert!(html.contains("[a]"), "{html}");
    }

    #[test]
    fn macro_definition_stays_visible_and_recorded() {
        let graph = import_tex_bytes("newcommand", "\\newcommand{\\x}{y}\nHello\n").expect("tex import");
        assert!(has_loss(&graph, "import.tex.macro_definition"), "{:?}", graph.coverage.loss);
        let html = export_html(&graph).expect("html export");
        assert!(html.contains(r#"data-opaque-kind="tex.newcommand""#), "{html}");
        assert!(html.contains("Hello"), "{html}");
    }
}

/// Typst dynamic constructs are skipped locally, so the rest of the page still
/// exports and the deferral stays in the loss markers.
#[cfg(all(feature = "html", feature = "typst"))]
mod typst_local_degradation_smoke {
    use document_formats::{export::html::export_html, import::typst::import_typst_bytes};

    #[test]
    fn dynamic_expression_does_not_fail_the_document() {
        let graph = import_typst_bytes("dynamic", "#let x = 1\nHello\n").expect("typst import");
        assert!(graph.coverage.loss.iter().any(|marker| marker.code == "import.typst.dynamic_content"));
        let html = export_html(&graph).expect("html export");
        assert!(html.contains("Hello"), "{html}");
        assert!(!html.contains("#let"), "{html}");
    }
}

/// HTML structures the importer used to lose or merge.
#[cfg(feature = "html")]
mod html_structure_smoke {
    use document_formats::{export::html::export_html, import::html::import_html_text};

    fn html(source: &str) -> String {
        let graph = import_html_text("html-structure", source).expect("html import");
        export_html(&graph).expect("html export")
    }

    fn loss_codes(source: &str) -> Vec<String> {
        let graph = import_html_text("html-structure", source).expect("html import");
        graph.coverage.loss.iter().map(|marker| marker.code.clone()).collect()
    }

    #[test]
    fn unquoted_attribute_value_stays_in_the_tag() {
        let output = html("<a href=/x>x</a>");
        assert!(output.contains(r#"href="/x""#), "{output}");
    }

    #[test]
    fn boolean_attribute_keeps_its_spelling() {
        let output = html(r#"<a hidden href="/x">x</a>"#);
        assert!(output.contains("hidden"), "{output}");
    }

    #[test]
    fn table_sections_keep_their_cells() {
        let output = html(
            "<table><colgroup><col span=2></colgroup><thead><tr><th>h</th></tr></thead><tbody><tr><td>b</td></tr></tbody></table>",
        );
        assert!(output.contains("<thead><tr><th>h</th></tr></thead>"), "{output}");
        assert!(output.contains("<tbody><tr><td>b</td></tr></tbody>"), "{output}");
        assert!(!loss_codes("<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>b</td></tr></tbody></table>")
            .contains(&"import.html.table_sections".to_string()));
    }

    #[test]
    fn header_rows_after_body_rows_are_a_recorded_loss() {
        let output = html("<table><tbody><tr><td>b</td></tr></tbody><thead><tr><th>h</th></tr></thead></table>");
        assert!(output.contains("<th>h</th>"), "{output}");
        assert!(loss_codes("<table><tbody><tr><td>b</td></tr></tbody><thead><tr><th>h</th></tr></thead></table>")
            .contains(&"import.html.table_sections".to_string()));
    }

    #[test]
    fn foster_parented_text_precedes_the_table() {
        let output = html("<table>stray<tr><td>x</td></tr></table>");
        assert!(output.contains("<p>stray</p>"), "{output}");
    }

    #[test]
    fn implied_paragraph_close_splits_paragraphs() {
        let output = html("<p>a<p>b");
        assert_eq!(output.matches("<p>").count(), 2, "{output}");
    }

    #[test]
    fn implied_list_item_close_splits_items() {
        let output = html("<ul><li>a<li>b</ul>");
        assert!(output.contains("<li>a</li>"), "{output}");
        assert!(output.contains("<li>b</li>"), "{output}");
    }

    #[test]
    fn template_content_stays_inert() {
        let output = html("<template><p>x</p></template>");
        assert!(output.contains("<template></template>"), "{output}");
        assert!(!output.contains("<p>x</p>"), "{output}");
    }

    #[test]
    fn trailing_text_after_self_closed_non_void_element_survives() {
        let output = html("<div></div>after");
        assert!(output.contains("after"), "{output}");
    }
}

/// A link whose text is an image keeps both the outer link and the image.
#[cfg(all(feature = "html", feature = "markdown"))]
mod markdown_export_smoke {
    use document_formats::{export::html::export_html, import::markdown::import_markdown_bytes};

    #[test]
    fn link_text_image_is_not_truncated() {
        let graph = import_markdown_bytes("nested-image", "[![a](i.png)](/u)\n").expect("markdown import");
        let html = export_html(&graph).expect("html export");
        assert!(html.contains(r#"href="/u""#), "{html}");
        assert!(html.contains(r#"src="i.png""#), "{html}");
        assert!(html.contains(r#"alt="a""#), "{html}");
    }
}

/// Loss status classification: kept content is a loss, dropped content is a
/// whole-construct refusal.
#[cfg(all(feature = "html", feature = "tex", feature = "typst"))]
mod loss_status_smoke {
    use document_formats::{
        export::html::export_html,
        import::{html::import_html_bytes, tex::import_tex_bytes, typst::import_typst_bytes},
    };
    use document_ir::{Block, DocumentGraph, SemanticStatus};

    fn status(graph: &DocumentGraph, code: &str) -> SemanticStatus {
        graph.coverage.loss.iter().find(|loss| loss.code == code).unwrap_or_else(|| panic!("missing loss `{code}`")).status
    }

    /// The opaque block a marker describes carries the same status as the marker,
    /// so the node never claims a mapping the projection skips.
    fn opaque_status(graph: &DocumentGraph, prefix: &str) -> SemanticStatus {
        graph
            .blocks
            .iter()
            .find_map(|node| match &node.block {
                Block::Opaque { kind, status, .. } if kind.starts_with(prefix) => Some(*status),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing `{prefix}` opaque block"))
    }

    #[test]
    fn kept_tex_content_is_a_loss_not_a_refusal() {
        let pair = import_tex_bytes("left-pair", "\\left[ a\n").expect("tex import");
        assert_eq!(status(&pair, "import.tex.unclosed_delimiters"), SemanticStatus::Partial);
        assert!(export_html(&pair).expect("html export").contains("tex-delimiter-pair"));

        let math = import_tex_bytes("unclosed-math", "$a\n").expect("tex import");
        assert_eq!(status(&math, "import.tex.unclosed_math"), SemanticStatus::Partial);
        assert!(export_html(&math).expect("html export").contains(r#"class="math""#));

        let macro_graph = import_tex_bytes("macro", "\\newcommand{\\foo}{bar}\ntext\n").expect("tex import");
        assert_eq!(status(&macro_graph, "import.tex.macro_definition"), SemanticStatus::Partial);
        assert_eq!(opaque_status(&macro_graph, "tex."), SemanticStatus::Partial);
        assert!(export_html(&macro_graph).expect("html export").contains(r#"data-opaque-kind="tex.newcommand""#));
    }

    #[test]
    fn dropped_content_stays_a_refusal() {
        let typst = import_typst_bytes("let", "#let x = 1\nhello\n").expect("typst import");
        assert_eq!(status(&typst, "import.typst.dynamic_content"), SemanticStatus::Unsupported);
        assert_eq!(opaque_status(&typst, "typst."), SemanticStatus::Unsupported);
        let typst_html = export_html(&typst).expect("html export");
        assert!(!typst_html.contains("#let"), "{typst_html}");

        let template = import_html_bytes("template", b"<template><p>x</p></template>").expect("html import");
        assert_eq!(status(&template, "import.html.template_contents"), SemanticStatus::Unsupported);
        let template_html = export_html(&template).expect("html export");
        assert!(template_html.contains("<template></template>"), "{template_html}");
        assert!(!template_html.contains("<p>x</p>"), "{template_html}");
    }
}
