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


