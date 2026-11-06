//! Semantic extraction over fetch artifacts.

#![deny(missing_docs)]

use crawler_types::{
    ExtractBudget, ExtractContext, ExtractInput, ExtractResult, ExtractStatus, FetchArtifact, LinkCandidate, LinkRelation,
    ProbeResult, ProbeStatus,
};
use document_formats::import::html::{HtmlImport, HtmlLink, HtmlLinkRole, import_html_bytes_with_links};

mod encoding;
mod html;
mod inspect;

pub use html::HtmlExtractor;
pub use inspect::inspect_artifact;

/// Semantic extractor over fetch artifacts.
pub trait Extractor {
    /// Budgeted media-type probe.
    fn probe(&self, artifact: &FetchArtifact, budget: ExtractBudget) -> ProbeResult;
    /// Construct a document graph and discovered links.
    fn extract(&self, input: ExtractInput<'_>, ctx: &ExtractContext) -> ExtractResult;
}

/// Pick the first extractor that supports the artifact.
pub fn select_extractor<'a>(
    extractors: &'a [&'a dyn Extractor],
    artifact: &FetchArtifact,
    budget: ExtractBudget,
) -> Option<(&'a dyn Extractor, ProbeResult)> {
    for extractor in extractors {
        let probe = (*extractor).probe(artifact, budget);
        if probe.status == ProbeStatus::Supported {
            return Some((*extractor, probe));
        }
    }
    None
}

/// Run offline extraction against a fetch artifact.
pub fn run_extract(
    artifact: &FetchArtifact,
    extractors: &[&dyn Extractor],
    budget: ExtractBudget,
    ctx: &ExtractContext,
) -> ExtractResult {
    let input = ExtractInput { artifact, budget };
    if let Some((extractor, _)) = select_extractor(extractors, artifact, budget) {
        return extractor.extract(input, ctx);
    }
    ExtractResult {
        status: ExtractStatus::Unsupported,
        source_url: artifact.final_url.clone(),
        document: None,
        discovered_links: Vec::new(),
        losses: Vec::new(),
        extractor: "none".into(),
    }
}

/// Import a decoded HTML page through `notedown-formats`.
///
/// `HtmlExtractor` sniffs the character encoding first and always passes UTF-8
/// bytes here, so a source-declared legacy charset does not become a parse
/// failure.
pub(crate) fn import_html_page(page_url: &url::Url, bytes: &[u8]) -> Result<HtmlImport, document_formats::FormatError> {
    import_html_bytes_with_links(&page_url.to_string(), bytes)
}

/// Build link and asset candidates from a Notedown HTML import.
///
/// Format semantics stay in Notedown. This only applies crawl policy: base URI
/// resolution, non-crawlable target rejection, and relation classification.
pub(crate) fn link_candidates_from_import(
    page_url: &url::Url,
    base_href: Option<&str>,
    links: &[HtmlLink],
) -> Vec<LinkCandidate> {
    let base = resolve_base_url(page_url, base_href);
    links
        .iter()
        .filter(|link| is_crawl_candidate(&link.target))
        .map(|link| {
            let hint = link_role_hint(link.role);
            classify_candidate(&base, &link.target, hint)
        })
        .collect()
}

fn link_role_hint(role: HtmlLinkRole) -> &'static str {
    match role {
        HtmlLinkRole::Href => "href",
        HtmlLinkRole::FormAction => "form-action",
        HtmlLinkRole::Src => "src",
        HtmlLinkRole::SrcSet => "srcset",
    }
}

/// Resolve a document base URI against the fetched page URL.
pub(crate) fn resolve_base_url(page_url: &url::Url, base_href: Option<&str>) -> url::Url {
    let Some(href) = base_href
    else {
        return page_url.clone();
    };
    if let Ok(absolute) = url::Url::parse(href) {
        return absolute;
    }
    page_url.join(href).unwrap_or_else(|_| page_url.clone())
}

fn is_crawl_candidate(target: &str) -> bool {
    let trimmed = target.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('#')
        && !trimmed.starts_with("javascript:")
        && !trimmed.starts_with("mailto:")
        && !trimmed.starts_with("data:")
}

/// Classify a discovered href relative to a page URL.
pub fn classify_link(page_url: &url::Url, target: &str) -> (LinkRelation, String) {
    let resolved = page_url.join(target).map(|url| url.to_string());
    match resolved {
        Ok(absolute) => {
            let relation = match url::Url::parse(&absolute) {
                Ok(parsed) if page_url.host_str() == parsed.host_str() => LinkRelation::Navigation,
                _ => LinkRelation::External,
            };
            (relation, absolute)
        }
        Err(_) => (LinkRelation::External, target.to_string()),
    }
}

/// Build link candidates from raw href strings.
pub fn link_candidates_from_hrefs(page_url: &url::Url, hrefs: &[String]) -> Vec<LinkCandidate> {
    hrefs.iter().map(|href| classify_candidate(page_url, href, "href")).collect()
}

fn classify_candidate(page_url: &url::Url, target: &str, source_hint: &'static str) -> LinkCandidate {
    let (relation, target) = classify_link_with_hint(page_url, target, source_hint);
    LinkCandidate { target, relation, source_hint: Some(source_hint.into()) }
}

fn classify_link_with_hint(page_url: &url::Url, target: &str, source_hint: &str) -> (LinkRelation, String) {
    let (relation, resolved) = classify_link(page_url, target);
    if matches!(source_hint, "src" | "srcset") {
        return (LinkRelation::Asset, resolved);
    }
    (relation, resolved)
}
