use crawler_types::{ExtractBudget, ExtractContext, ExtractStatus, FetchArtifact, InspectReport, InspectStage, ProbeStatus};

use super::{Extractor, import_html_page, link_candidates_from_import, run_extract, select_extractor};

/// Inspect a fetch artifact without committing IR.
pub fn inspect_artifact(
    artifact: &FetchArtifact,
    extractors: &[&dyn Extractor],
    stage: InspectStage,
    budget: ExtractBudget,
) -> InspectReport {
    let mut report = InspectReport::empty(artifact.final_url.clone(), stage);
    report.content_type = artifact.content_type.clone();
    report.body_bytes = artifact.body_bytes().map(|body| body.len() as u64).unwrap_or(0);

    if matches!(stage, InspectStage::Probe | InspectStage::ExtractPlan) {
        if let Some((_, probe)) = select_extractor(extractors, artifact, budget) {
            report.probe = probe;
        }
    }

    if matches!(stage, InspectStage::Links | InspectStage::ExtractPlan) {
        if let Some(bytes) = artifact.body_bytes() {
            if let Ok(import) = import_html_page(&artifact.final_url, bytes) {
                report.discovered_links =
                    link_candidates_from_import(&artifact.final_url, import.base_href.as_deref(), &import.links);
            }
        }
    }

    if stage == InspectStage::ExtractPlan {
        let result = run_extract(artifact, extractors, budget, &ExtractContext::default());
        if result.status == ExtractStatus::Complete || result.status == ExtractStatus::Partial {
            report.probe.status = ProbeStatus::Supported;
            report.probe.extractor = Some(result.extractor);
            if report.discovered_links.is_empty() {
                report.discovered_links = result.discovered_links;
            }
        }
    }

    report
}
