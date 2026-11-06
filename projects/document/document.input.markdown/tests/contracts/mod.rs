//! The `supported` ledger of `input-contracts.json` is executable: every
//! claimed construct is imported and rendered from its stated sample, so the
//! contract file cannot promise more than the profile delivers.

use document_compose::Severity;
use document_input_markdown::{import_source, render_svg};
use document_layout::Diagrams;

const CONTRACTS: &str = include_str!("../../../input-contracts.json");

fn supported_samples() -> Vec<(String, String)> {
    let contracts: serde_json::Value = serde_json::from_str(CONTRACTS).expect("the contract file must parse");
    let family = contracts["families"]
        .as_array()
        .expect("families must be an array")
        .iter()
        .find(|family| family["language"] == "markdown")
        .expect("the markdown family must exist");
    family["supported"]
        .as_array()
        .expect("supported must be an array")
        .iter()
        .map(|entry| {
            let construct = entry["construct"].as_str().expect("construct must be a string").to_string();
            let sample = entry["sample"].as_str().unwrap_or_else(|| panic!("supported entry without a sample: {construct}"));
            (construct, sample.to_string())
        })
        .collect()
}

#[test]
fn every_supported_construct_is_importable_and_renderable() {
    let samples = supported_samples();
    assert!(!samples.is_empty(), "the ledger must claim at least one construct");
    for (construct, sample) in samples {
        let label = format!("virtual://contract-{}.md", construct.replace(' ', "-"));
        let imported = import_source(&label, &sample).unwrap_or_else(|error| panic!("{construct}: {error}"));
        assert!(
            imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
            "{construct} is claimed supported but reports an error: {:#?}",
            imported.diagnostics
        );
        render_svg(&label, &sample, &Diagrams::new()).unwrap_or_else(|error| panic!("{construct}: {error}"));
    }
}
