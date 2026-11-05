//! 冒烟：诊断码只允许 `davinci.*` 前缀，防止命名空间被静默换名。

use plotter_types::{NAMESPACE, cases, code_docs, codes, matrix};

/// The only accepted prefix for a shipped diagnostic code.
const PREFIX: &str = "davinci.";

#[test]
fn every_diagnostic_code_keeps_the_davinci_namespace() {
    assert_eq!(NAMESPACE, "davinci", "the wire contract pins the product namespace");

    let mut offenders: Vec<String> = Vec::new();

    for doc in code_docs() {
        if !doc.code.starts_with(PREFIX) {
            offenders.push(format!("catalog code `{}`", doc.code));
        }
    }
    for case in cases() {
        if !case.code.starts_with(PREFIX) {
            offenders.push(format!("catalog case `{}` code `{}`", case.id, case.code));
        }
    }
    for row in matrix() {
        if let Some(code) = row.code.as_deref() {
            if !code.starts_with(PREFIX) {
                offenders.push(format!("matrix row `{}` code `{code}`", row.id));
            }
        }
    }
    for code in [
        codes::INVALID_GEOMETRY,
        codes::INVALID_STYLE,
        codes::INVALID_TRANSFORM,
        codes::SERIALIZATION_FAILED,
        codes::PARSE_FAILED,
        codes::AST_UNAVAILABLE,
        codes::UNSUPPORTED,
        codes::APPROXIMATED,
        codes::EMPTY_DIAGRAM,
    ] {
        if !code.starts_with(PREFIX) {
            offenders.push(format!("`codes` constant `{code}`"));
        }
    }

    let contracts: serde_json::Value =
        serde_json::from_str(include_str!("../../../input-contracts.json")).expect("input contracts are valid JSON");
    let mut contract_codes = Vec::new();
    collect_codes(&contracts, &mut contract_codes);
    for code in contract_codes {
        if !code.starts_with(PREFIX) {
            offenders.push(format!("`input-contracts.json` code `{code}`"));
        }
    }

    assert!(offenders.is_empty(), "diagnostic codes outside the canonical namespace:\n{}", offenders.join("\n"));
}

/// Collect every string held under a `code` key, at any depth.
fn collect_codes(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "code" {
                    if let Some(code) = child.as_str() {
                        out.push(code.to_string());
                    }
                }
                collect_codes(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_codes(item, out);
            }
        }
        _ => {}
    }
}
