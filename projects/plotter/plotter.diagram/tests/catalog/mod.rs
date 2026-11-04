//! 冒烟：图表示例的拒绝码目录与实现保持集合一致。

use std::collections::BTreeSet;

use plotter_types::{code_docs, codes};

/// Every diagram code this crate can emit.
const SHIPPED: [&str; 5] =
    [codes::PARSE_FAILED, codes::AST_UNAVAILABLE, codes::UNSUPPORTED, codes::APPROXIMATED, codes::EMPTY_DIAGRAM];

#[test]
fn the_catalog_documents_every_diagram_code_and_invents_none() {
    let docs = code_docs();
    let documented: BTreeSet<&str> =
        docs.iter().map(|doc| doc.code.as_str()).filter(|code| code.starts_with("davinci.diagram.")).collect();

    for code in SHIPPED {
        assert!(documented.contains(code), "`{code}` is emitted but missing from the catalog");
    }
    for code in &documented {
        assert!(SHIPPED.contains(code), "the catalog documents `{code}`, which this crate cannot emit");
    }
}
