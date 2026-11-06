use crawler_types::{
    ExtractBudget, ExtractContext, ExtractInput, ExtractResult, ExtractStatus, FetchArtifact, ProbeResult, ProbeStatus,
};
use document_formats::from_format_error;
use document_ir::{Block, DocumentGraph, DocumentId, Inline, LossMarker, SemanticStatus, SourceKind, SourceSpan};

use super::{Extractor, encoding::decode_html, import_html_page, link_candidates_from_import, resolve_base_url};

/// HTML extractor backed by the `notedown-formats` HTML import.
///
/// The format layer owns document lowering, provenance, and loss. This
/// extractor only applies crawl policy: base URI resolution for assets and
/// navigation candidate classification.
#[derive(Debug, Clone, Copy, Default)]
pub struct HtmlExtractor;

impl Extractor for HtmlExtractor {
    fn probe(&self, artifact: &FetchArtifact, budget: ExtractBudget) -> ProbeResult {
        let supported = artifact.content_type.as_deref().map(|value| value.contains("html")).unwrap_or(false);
        if !supported {
            return ProbeResult { status: ProbeStatus::Unsupported, extractor: None };
        }
        let bytes = artifact.body_bytes().unwrap_or(&[]);
        if bytes.len() as u64 > budget.max_probe_bytes {
            return ProbeResult { status: ProbeStatus::Failed, extractor: Some("html".into()) };
        }
        ProbeResult { status: ProbeStatus::Supported, extractor: Some("html".into()) }
    }

    fn extract(&self, input: ExtractInput<'_>, ctx: &ExtractContext) -> ExtractResult {
        let artifact = input.artifact;
        let label = artifact.final_url.to_string();
        let decoded = match decode_html(artifact) {
            Ok(text) => text,
            Err(error) => return decode_failure(artifact, &error),
        };
        let import = match import_html_page(&artifact.final_url, decoded.as_bytes()) {
            Ok(import) => import,
            Err(format_error) => {
                // Preserve the `notedown.format.*` code and typed args so
                // downstream consumers don't see a free-form Crawler-specific
                // string that would make localization and retry logic hard.
                let diagnostic = from_format_error(&format_error);
                let fallback = diagnostic
                    .message()
                    .fallback()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("`notedown-formats` could not import HTML from {label}"));
                return ExtractResult {
                    status: ExtractStatus::Failed,
                    source_url: artifact.final_url.clone(),
                    document: None,
                    discovered_links: Vec::new(),
                    losses: vec![
                        // Primary: the authoritative Notedown code preserved
                        // from the formats-layer diagnostic.
                        LossMarker {
                            code: diagnostic.code().as_str().to_string(),
                            message: fallback,
                            status: SemanticStatus::Partial,
                        },
                        // Secondary: Crawler-side context that the failure
                        // originated at HTML extraction time.
                        LossMarker {
                            code: "crawler.extract.html_import_failed".into(),
                            message: format!("HTML extraction stage: {}", diagnostic.code().as_str()),
                            status: SemanticStatus::Partial,
                        },
                    ],
                    extractor: "html".into(),
                };
            }
        };

        let discovered_links = link_candidates_from_import(&artifact.final_url, import.base_href.as_deref(), &import.links);
        let base_url = resolve_base_url(&artifact.final_url, import.base_href.as_deref());
        let mut document = import.document;
        document.id = page_document_id(ctx.page_identity.as_deref().unwrap_or(&label));
        resolve_asset_targets(&mut document, &base_url);
        classify_diagram_resources(&mut document);

        let status = if document.validate().is_valid() {
            if document.coverage.complete { ExtractStatus::Complete } else { ExtractStatus::Partial }
        }
        else {
            ExtractStatus::Failed
        };
        let losses = document.coverage.loss.clone();

        ExtractResult {
            status,
            source_url: artifact.final_url.clone(),
            document: if status == ExtractStatus::Failed { None } else { Some(document) },
            discovered_links,
            losses,
            extractor: "html".into(),
        }
    }
}

/// Report an undecodable HTML body as a failed extraction.
///
/// Encoding is an extractor capability, so the crawl treats an undecodable
/// body as a permanent page failure instead of retrying it through the
/// frontier.
fn decode_failure(artifact: &FetchArtifact, error: &super::encoding::DecodeError) -> ExtractResult {
    let message = error.message();
    ExtractResult {
        status: ExtractStatus::Failed,
        source_url: artifact.final_url.clone(),
        document: None,
        discovered_links: Vec::new(),
        losses: vec![LossMarker {
            code: "crawler.extract.encoding_failed".into(),
            message: message.clone(),
            status: SemanticStatus::Partial,
        }],
        extractor: "html".into(),
    }
}

/// Resolve asset targets against the document base URI so downloads and
/// reports see absolute locations. The format layer keeps authored targets.
fn resolve_asset_targets(document: &mut DocumentGraph, base_url: &url::Url) {
    for asset in &mut document.assets {
        let Some(source) = asset.source.clone()
        else {
            continue;
        };
        if let Some(resolved) = resolve_target(base_url, &source) {
            asset.source = Some(resolved);
        }
    }
    for node in &mut document.blocks {
        resolve_block_targets(&mut node.block, base_url);
    }
}

fn resolve_block_targets(block: &mut Block, base_url: &url::Url) {
    match block {
        Block::Section { title, .. } => resolve_inline_targets(title, base_url),
        Block::Paragraph { content } | Block::Quote { content, .. } => resolve_inline_targets(content, base_url),
        Block::List { items, .. } => {
            for item in items {
                resolve_inline_targets(&mut item.content, base_url);
            }
        }
        Block::Table { rows, caption: _ } => {
            for row in rows {
                for cell in &mut row.cells {
                    resolve_inline_targets(&mut cell.content, base_url);
                }
            }
        }
        _ => {}
    }
}

fn resolve_inline_targets(inlines: &mut [Inline], base_url: &url::Url) {
    for inline in inlines {
        match inline {
            Inline::Image { source, .. } if !source.is_empty() => {
                if let Some(resolved) = resolve_target(base_url, source) {
                    *source = resolved;
                }
            }
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => {
                resolve_inline_targets(children, base_url);
            }
            _ => {}
        }
    }
}

fn resolve_target(base_url: &url::Url, target: &str) -> Option<String> {
    url::Url::parse(target).or_else(|_| base_url.join(target)).ok().map(|url| url.to_string())
}

/// Classify diagram resources by available information, in three tiers:
///
/// 1. `Block::Diagram` — the authored source (e.g. Mermaid) is already kept
///    verbatim in the IR with byte-span provenance attached by the importer.
///    Nothing to do here; the tier exists so downstream keeps it source-first.
/// 2. `Block::RawHtml` carrying `<svg>` markup — the page shipped a rendered
///    vector graphic instead of source. Register it as an image asset with the
///    exact markup as payload so consumers treat it as a graphic resource
///    reference rather than prose.
/// 3. Bitmap-only diagrams (`<img class="mermaid">` etc.) — keep the image
///    asset (URL + alt) and push an explicit loss marker: 位图，无结构. Crawler
///    never OCR-reconstructs diagram structure from raster bytes and never
///    silently flattens the image into plain text.
fn classify_diagram_resources(document: &mut DocumentGraph) {
    let mut svg_payloads: Vec<(document_ir::NodeId, String)> = Vec::new();
    let mut pending_losses: Vec<(usize, LossMarker, Option<SourceSpan>)> = Vec::new();
    for (index, node) in document.blocks.iter().enumerate() {
        match &node.block {
            Block::RawHtml { html } if html.contains("<svg") => {
                svg_payloads.push((node.id, html.clone()));
                if looks_like_rendered_diagram(html) {
                    pending_losses.push((
                        index,
                        LossMarker {
                            code: "crawler.extract.diagram_source_missing".into(),
                            message: "无法获取源码: the page shipped a rendered diagram as `<svg>` markup without authored diagram source; kept as an image asset".into(),
                            status: SemanticStatus::Partial,
                        },
                        authored_span(document, node.id),
                    ));
                }
            }
            block => {
                let mut bitmaps = Vec::new();
                collect_bitmap_diagrams(block, &mut bitmaps);
                if !bitmaps.is_empty() {
                    let span = authored_span(document, node.id);
                    for (source, alt) in bitmaps {
                        pending_losses.push((
                            index,
                            LossMarker {
                                code: "crawler.extract.diagram_bitmap".into(),
                                message: format!("位图，无结构: diagram available only as raster image `{source}` (alt `{alt}`); kept as image asset without structural recovery"),
                                status: SemanticStatus::Partial,
                            },
                            span,
                        ));
                    }
                }
            }
        }
    }

    let mut next_asset = document.assets.iter().map(|asset| asset.id.0).max().unwrap_or(0);
    let mut next_link = document.relations.iter().map(|relation| relation.id.0).max().unwrap_or(0);
    for (node, markup) in svg_payloads {
        next_asset += 1;
        next_link += 1;
        let asset_id = document_ir::AssetId(next_asset);
        document.push_asset(document_ir::Asset {
            id: asset_id,
            kind: document_ir::AssetKind::Image,
            content_identity: None,
            source: None,
            media_type: Some("image/svg+xml".into()),
            status: SemanticStatus::Resolved,
            bytes: Some(markup.into_bytes()),
        });
        document.push_relation(document_ir::Relation {
            id: document_ir::LinkId(next_link),
            kind: document_ir::RelationKind::Embeds,
            source: document_ir::RelationEndpoint::Node(node),
            target: document_ir::RelationEndpoint::Asset(asset_id),
        });
    }

    // Losses keep document order so repeated extractions of one page produce
    // the same loss sequence and every marker points at its authored block.
    pending_losses.sort_by_key(|(index, _, _)| *index);
    for (_, marker, span) in pending_losses {
        document.push_loss_at(marker, span);
    }
}

/// The authored byte span of one block.
///
/// Notedown's HTML importer records provenance through `SourceRef` entries
/// rather than block-slot spans, so both locations are consulted before giving
/// up.
fn authored_span(document: &DocumentGraph, id: document_ir::NodeId) -> Option<SourceSpan> {
    if let Some(span) = document.block_span(id) {
        return Some(span);
    }
    document.sources.iter().find_map(|source| {
        if source.node != id {
            return None;
        }
        match source.kind {
            SourceKind::TextSpan { start, end } => SourceSpan::new(start as usize, end as usize),
            _ => None,
        }
    })
}

/// Detects markup that a diagram runtime rendered in place of authored source.
///
/// Generic inline SVG stays an ordinary graphic resource; only Mermaid's
/// rendered signatures trigger the missing-source loss.
fn looks_like_rendered_diagram(markup: &str) -> bool {
    ["aria-roledescription=", "class=\"mermaid", "id=\"mermaid", "data-processed=\"true\""]
        .iter()
        .any(|signal| markup.contains(signal))
}

/// Collect bitmap-only diagrams: authored `<img>` occurrences marked with the
/// `mermaid` (or `diagram`) class, which the importer keeps as an attributed
/// inline image.
fn collect_bitmap_diagrams(block: &Block, out: &mut Vec<(String, String)>) {
    match block {
        Block::Section { title, .. } => collect_bitmap_inlines(title, out),
        Block::Paragraph { content } | Block::Quote { content, .. } => collect_bitmap_inlines(content, out),
        Block::List { items, .. } => {
            for item in items {
                collect_bitmap_inlines(&item.content, out);
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &row.cells {
                    collect_bitmap_inlines(&cell.content, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_bitmap_inlines(inlines: &[Inline], out: &mut Vec<(String, String)>) {
    for inline in inlines {
        match inline {
            Inline::Attributed { attributes, children } => {
                let marked = attributes.classes.iter().any(|class| class == "mermaid" || class == "diagram");
                if marked {
                    for (source, alt) in children.iter().filter_map(image_parts) {
                        out.push((source, alt));
                    }
                }
                collect_bitmap_inlines(children, out);
            }
            Inline::Styled { children, .. } => collect_bitmap_inlines(children, out),
            _ => {}
        }
    }
}

fn image_parts(inline: &Inline) -> Option<(String, String)> {
    match inline {
        Inline::Image { source, alt, .. } if !source.is_empty() => {
            let alt = alt.iter().map(Inline::plain_text).collect::<String>();
            Some((source.clone(), alt))
        }
        _ => None,
    }
}

fn page_document_id(page_identity: &str) -> DocumentId {
    let mut hash = 0u64;
    for byte in page_identity.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    DocumentId(hash)
}
