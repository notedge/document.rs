//! OPC and PresentationML reading.
//!
//! The reader walks the presentation part, its masters, layouts and slides,
//! resolves placeholder inheritance from the layout and master chain and keeps
//! every refused construct as a structured issue. Charts, SmartArt, OLE
//! objects, media and group shapes never disappear silently.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use acorn_core::{Budget, BudgetTracker};
use acorn_docx::{OpcPackage, OpcRelationship};
use oak_xml::ast::XmlElement;

use super::model::{
    BodyProperties, ChartRef, ChartSeries, Fill, IssueStatus, Line, Paragraph, PlaceholderKind, PlaceholderRef, PptxIssue,
    PptxPresentation, PptxStats, PptxTable, PptxTableCell, PptxTableRow, Run, Shape, ShapeKind, Slide, SlideImage, SlideSize,
    TextFrame, TextStyle, Transform, codes,
};
use crate::{
    FormatError,
    import::office::{
        attribute_value, child_named, children_named, descendants_named, document_root, element_is, element_text, local_name,
        parse_xml, u32_attribute,
    },
};

const PRESENTATION_PART: &str = "ppt/presentation.xml";
const DEFAULT_CX: i64 = 12192000;
const DEFAULT_CY: i64 = 6858000;

/// Imports a PPTX presentation from disk.
pub fn import_pptx(path: impl AsRef<Path>) -> Result<PptxPresentation, FormatError> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
    import_pptx_bytes(&path.display().to_string(), &bytes)
}

/// Imports PPTX bytes into the format-native presentation model.
pub fn import_pptx_bytes(label: &str, bytes: &[u8]) -> Result<PptxPresentation, FormatError> {
    if !looks_like_zip(bytes) {
        return Err(FormatError::invalid_input("input is not a ZIP-based PPTX package"));
    }
    let package = OpcPackage::open(label.to_string(), bytes.to_vec()).map_err(map_opc_error)?;
    let mut ledger = BudgetTracker::new(Budget::default());
    let mut issues = Vec::new();

    let presentation_part =
        package.main_document_part_path(&mut ledger).map_err(map_opc_error)?.unwrap_or_else(|| PRESENTATION_PART.to_owned());
    let presentation_xml = package.read_part(&presentation_part, &mut ledger).map_err(map_opc_error)?;
    let presentation_value = parse_xml(&presentation_xml, "pptx")?;
    let presentation_root = document_root(&presentation_value, "pptx")?;
    let presentation_rels = read_rels(&package, &mut ledger, &presentation_part);

    let size = parse_slide_size(presentation_root);
    let title = read_core_title(&package, &mut ledger);

    if package.part_paths().iter().any(|path| local_name(path).eq_ignore_ascii_case("vbaProject.bin")) {
        issues.push(issue(
            codes::MACRO,
            "VBA project detected and refused by the PPTX reader",
            IssueStatus::Unsupported,
            None,
            None,
        ));
    }

    let masters = parse_masters(&package, &mut ledger, presentation_root, &presentation_rels, &mut issues);
    let mut theme_colors = BTreeMap::new();
    let mut clr_map = HashMap::new();
    if let Some(master) = masters.first() {
        theme_colors = master.theme.clone();
        clr_map = master.clr_map.clone();
    }
    let presentation_levels = parse_levels_from(child_named(presentation_root, "defaultTextStyle"));

    let mut slides = Vec::new();
    let slide_entries: Vec<&XmlElement> =
        child_named(presentation_root, "sldIdLst").map(|list| children_named(list, "sldId")).unwrap_or_default();
    for (position, entry) in slide_entries.iter().enumerate() {
        let index = position as u32 + 1;
        let Some(target) = relationship_attribute(entry, "id")
            .as_deref()
            .and_then(|id| presentation_rels.get(id))
            .map(|relationship| resolve_target(&presentation_part, &relationship.target))
        else {
            issues.push(issue(
                codes::GROUP,
                format!("slide {index} has no resolvable relationship target"),
                IssueStatus::Unsupported,
                Some(index),
                None,
            ));
            continue;
        };
        let Ok(slide_xml) = package.read_part(&target, &mut ledger)
        else {
            issues.push(issue(
                codes::GROUP,
                format!("slide {index} part `{target}` is missing"),
                IssueStatus::Unsupported,
                Some(index),
                None,
            ));
            continue;
        };
        let slide_value = parse_xml(&slide_xml, "pptx")?;
        let slide_root = document_root(&slide_value, "pptx")?;
        let slide_rels = read_rels(&package, &mut ledger, &target);
        let layout_part = find_part(&slide_rels, "/slideLayout", &target);
        let layout = layout_part
            .as_deref()
            .and_then(|part| masters.iter().flat_map(|master| master.layouts.iter()).find(|layout| layout.part == part));
        let master = masters
            .iter()
            .find(|master| master.layouts.iter().any(|candidate| Some(&candidate.part) == layout_part.as_ref()))
            .or_else(|| masters.first());

        let hidden = attribute_value(slide_root, "show").map(|value| matches!(value.as_str(), "0" | "false")).unwrap_or(false);
        if hidden {
            issues.push(issue(
                codes::HIDDEN_SLIDE,
                format!("slide {index} is hidden and is excluded from the IR projection"),
                IssueStatus::Partial,
                Some(index),
                None,
            ));
        }
        if child_named(slide_root, "timing").is_some() {
            issues.push(issue(
                codes::ANIMATION,
                format!("slide {index} declares transitions or animations that are not modeled"),
                IssueStatus::Partial,
                Some(index),
                None,
            ));
        }

        let background = parse_background(slide_root, &theme_colors, &clr_map);
        let mut shapes = Vec::new();
        if let Some(tree) = child_named(child_named(slide_root, "cSld").unwrap_or(slide_root), "spTree") {
            for child in children_named(tree, "sp") {
                shapes.extend(parse_shape(
                    &package,
                    &mut ledger,
                    child,
                    &slide_rels,
                    &target,
                    index,
                    layout,
                    master,
                    &presentation_levels,
                    &theme_colors,
                    &clr_map,
                    &mut issues,
                ));
            }
            for child in children_named(tree, "pic") {
                shapes.extend(parse_picture(
                    &package,
                    &mut ledger,
                    child,
                    &slide_rels,
                    &target,
                    index,
                    &theme_colors,
                    &clr_map,
                    &mut issues,
                ));
            }
            for child in children_named(tree, "graphicFrame") {
                shapes.extend(parse_graphic_frame(
                    &package,
                    &mut ledger,
                    child,
                    &slide_rels,
                    &target,
                    index,
                    &theme_colors,
                    &clr_map,
                    &mut issues,
                ));
            }
            for child in children_named(tree, "grpSp") {
                let id = shape_id(child, 0);
                issues.push(issue(
                    codes::GROUP,
                    format!("slide {index} group shape `{id}` is refused instead of being flattened"),
                    IssueStatus::Unsupported,
                    Some(index),
                    Some(id),
                ));
                shapes.push(Shape {
                    id,
                    name: shape_name(child, "group"),
                    kind: ShapeKind::Group,
                    placeholder: None,
                    transform: parse_transform(child_named(child_named(child, "grpSpPr").unwrap_or(child), "xfrm")),
                    inherited_transform: false,
                    fill: None,
                    line: None,
                    geometry: None,
                    text: None,
                    image: None,
                    table: None,
                    chart: None,
                });
            }
        }

        let notes = read_notes(&package, &mut ledger, &slide_rels, &target);
        if notes.is_some() {
            issues.push(issue(
                codes::NOTES,
                format!("slide {index} notes are carried beside the slide body"),
                IssueStatus::Partial,
                Some(index),
                None,
            ));
        }

        slides.push(Slide {
            index,
            part: target,
            hidden,
            layout_part,
            layout_name: layout.map(|layout| layout.name.clone()),
            master_part: master.map(|master| master.part.clone()),
            shapes,
            notes,
            background,
        });
    }

    let stats = compute_stats(&slides, &masters);
    issues.push(issue(
        "import.pptx.partial-coverage",
        "PPTX import maps slides, layouts, masters, placeholder inheritance, text, shapes, fills, lines, pictures, tables and notes. Charts, SmartArt, OLE objects, media and group shapes are refused with structured codes",
        IssueStatus::Partial,
        None,
        None,
    ));
    Ok(PptxPresentation { label: label.to_owned(), size, title, theme_colors, slides, stats, issues })
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") || bytes.starts_with(b"PK\x07\x08")
}

fn map_opc_error(error: acorn_docx::OpcError) -> FormatError {
    FormatError::parse("pptx", error.to_string())
}

fn read_rels(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> HashMap<String, OpcRelationship> {
    let rels = rels_part_path(part);
    package
        .read_relationships(&rels, ledger)
        .unwrap_or_default()
        .into_iter()
        .map(|relationship| (relationship.id.clone(), relationship))
        .collect()
}

fn rels_part_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolves a relationship target against the owning part directory.
///
/// The shared Acorn helper expects a caller-supplied base, so this reader
/// normalises the `..` segments itself to keep layout, theme, chart, notes and
/// media targets aligned with the package part names.
fn resolve_target(owner: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let base = owner.rsplit_once('/').map(|(directory, _)| directory).unwrap_or("");
    let mut segments: Vec<&str> = base.split('/').filter(|segment| !segment.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

fn find_part(relationships: &HashMap<String, OpcRelationship>, kind_suffix: &str, owner: &str) -> Option<String> {
    relationships
        .values()
        .find(|relationship| relationship.relationship_type.ends_with(kind_suffix))
        .map(|relationship| resolve_target(owner, &relationship.target))
}

fn read_part_optional(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> Option<Vec<u8>> {
    package.read_part(part, ledger).ok()
}

/// Reads a namespaced relationship attribute such as `r:id` or `r:embed`.
///
/// `p:sldId` carries both a plain `id` and an `r:id`, so matching the local
/// name alone would pick the slide number instead of the relationship.
fn relationship_attribute(element: &XmlElement, local: &str) -> Option<String> {
    element
        .attributes
        .iter()
        .find(|attribute| local_name(&attribute.name) == local && attribute.name.contains(':'))
        .map(|attribute| attribute.value.clone())
}

fn parse_part(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> Option<oak_xml::ast::XmlValue> {
    let xml = read_part_optional(package, ledger, part)?;
    parse_xml(&xml, "pptx").ok()
}

fn parse_slide_size(root: &XmlElement) -> SlideSize {
    match child_named(root, "sldSz") {
        Some(element) => SlideSize {
            cx: attribute_value(element, "cx").and_then(|value| value.parse().ok()).unwrap_or(DEFAULT_CX),
            cy: attribute_value(element, "cy").and_then(|value| value.parse().ok()).unwrap_or(DEFAULT_CY),
        },
        None => SlideSize { cx: DEFAULT_CX, cy: DEFAULT_CY },
    }
}

fn read_core_title(package: &OpcPackage, ledger: &mut BudgetTracker) -> Option<String> {
    let part = "docProps/core.xml";
    let xml = read_part_optional(package, ledger, part)?;
    let value = parse_xml(&xml, "pptx").ok()?;
    let root = document_root(&value, "pptx").ok()?;
    let title = element_text(child_named(root, "title")?);
    (!title.trim().is_empty()).then_some(title)
}

/// A placeholder definition read from a layout or master.
#[derive(Debug, Clone, Default)]
struct PlaceholderStyle {
    reference: Option<PlaceholderRef>,
    transform: Option<Transform>,
    fill: Option<Fill>,
    line: Option<Line>,
    body: BodyProperties,
    levels: Vec<LevelStyle>,
}

#[derive(Debug, Clone, Default)]
struct LevelStyle {
    size_pt: Option<f64>,
    bold: bool,
    italic: bool,
    color: Option<String>,
    font: Option<String>,
}

struct LayoutRecord {
    part: String,
    name: String,
    placeholders: Vec<PlaceholderStyle>,
}

struct MasterRecord {
    part: String,
    theme: BTreeMap<String, String>,
    clr_map: HashMap<String, String>,
    layouts: Vec<LayoutRecord>,
    placeholders: Vec<PlaceholderStyle>,
    title_style: Vec<LevelStyle>,
    body_style: Vec<LevelStyle>,
    other_style: Vec<LevelStyle>,
}

fn parse_masters(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    presentation_root: &XmlElement,
    presentation_rels: &HashMap<String, OpcRelationship>,
    issues: &mut Vec<PptxIssue>,
) -> Vec<MasterRecord> {
    let mut masters = Vec::new();
    let entries: Vec<&XmlElement> =
        child_named(presentation_root, "sldMasterIdLst").map(|list| children_named(list, "sldMasterId")).unwrap_or_default();
    for entry in entries {
        let Some(target) = relationship_attribute(entry, "id")
            .as_deref()
            .and_then(|id| presentation_rels.get(id))
            .map(|relationship| resolve_target(PRESENTATION_PART, &relationship.target))
        else {
            continue;
        };
        let Some(master_value) = parse_part(package, ledger, &target)
        else {
            issues.push(issue(
                codes::GROUP,
                format!("slide master part `{target}` is missing"),
                IssueStatus::Unsupported,
                None,
                None,
            ));
            continue;
        };
        let Ok(root) = document_root(&master_value, "pptx")
        else {
            continue;
        };
        let rels = read_rels(package, ledger, &target);
        let theme = find_part(&rels, "/theme", &target)
            .and_then(|part| parse_part(package, ledger, &part))
            .and_then(|value| document_root(&value, "pptx").ok().map(parse_theme))
            .unwrap_or_default();
        let clr_map = parse_clr_map(root);
        let placeholders = parse_placeholders(root, &theme, &clr_map);
        let title_style = parse_tx_style(child_named(root, "txStyles").and_then(|styles| child_named(styles, "titleStyle")));
        let body_style = parse_tx_style(child_named(root, "txStyles").and_then(|styles| child_named(styles, "bodyStyle")));
        let other_style = parse_tx_style(child_named(root, "txStyles").and_then(|styles| child_named(styles, "otherStyle")));

        let mut layouts = Vec::new();
        for relationship in rels.values() {
            if !relationship.relationship_type.ends_with("/slideLayout") {
                continue;
            }
            let part = resolve_target(&target, &relationship.target);
            let Some(layout_value) = parse_part(package, ledger, &part)
            else {
                issues.push(issue(
                    codes::GROUP,
                    format!("slide layout part `{part}` is missing"),
                    IssueStatus::Unsupported,
                    None,
                    None,
                ));
                continue;
            };
            let Ok(layout_root) = document_root(&layout_value, "pptx")
            else {
                continue;
            };
            let layout_theme = theme.clone();
            let layout_clr = clr_map.clone();
            layouts.push(LayoutRecord {
                part,
                name: child_named(layout_root, "cSld").and_then(|common| attribute_value(common, "name")).unwrap_or_default(),
                placeholders: parse_placeholders(layout_root, &layout_theme, &layout_clr),
            });
        }
        if child_named(root, "cSld").and_then(|cld| child_named(cld, "spTree")).is_some() {
            issues.push(issue(
                codes::MASTER_SHAPE,
                format!("slide master `{target}` shapes are not projected onto slides"),
                IssueStatus::Partial,
                None,
                None,
            ));
        }
        masters.push(MasterRecord {
            part: target,
            theme,
            clr_map,
            layouts,
            placeholders,
            title_style,
            body_style,
            other_style,
        });
    }
    masters
}

fn parse_theme(root: &XmlElement) -> BTreeMap<String, String> {
    let mut colors = BTreeMap::new();
    let Some(scheme) = descendants_named(root, "clrScheme").first().copied()
    else {
        return colors;
    };
    for slot in &scheme.children {
        let Some(element) = slot.as_element()
        else {
            continue;
        };
        let name = local_name(&element.name).to_owned();
        let value = children_named(element, "srgbClr")
            .first()
            .and_then(|color| attribute_value(color, "val"))
            .or_else(|| children_named(element, "sysClr").first().and_then(|color| attribute_value(color, "lastClr")));
        if let Some(value) = value {
            colors.insert(name, normalize_color(&value));
        }
    }
    colors
}

fn parse_clr_map(root: &XmlElement) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(element) = child_named(root, "clrMap")
    else {
        return map;
    };
    for attribute in &element.attributes {
        map.insert(local_name(&attribute.name).to_owned(), attribute.value.clone());
    }
    map
}

fn normalize_color(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('#') { trimmed.to_ascii_uppercase() } else { format!("#{}", trimmed.to_ascii_uppercase()) }
}

fn resolve_scheme(name: &str, theme: &BTreeMap<String, String>, clr_map: &HashMap<String, String>) -> Option<String> {
    let mapped = clr_map.get(name).map(String::as_str).unwrap_or(name);
    theme.get(mapped).cloned().or_else(|| theme.get(name).cloned())
}

fn parse_placeholders(
    root: &XmlElement,
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
) -> Vec<PlaceholderStyle> {
    let Some(tree) = child_named(root, "cSld").and_then(|cld| child_named(cld, "spTree"))
    else {
        return Vec::new();
    };
    let mut placeholders = Vec::new();
    for shape in children_named(tree, "sp") {
        let Some(reference) = parse_placeholder(
            child_named(shape, "nvSpPr").and_then(|nv| child_named(nv, "nvPr")).and_then(|nv| child_named(nv, "ph")),
        )
        else {
            continue;
        };
        let properties = child_named(shape, "spPr");
        let body = child_named(shape, "txBody");
        placeholders.push(PlaceholderStyle {
            reference: Some(reference),
            transform: properties.and_then(|properties| parse_transform(child_named(properties, "xfrm"))),
            fill: properties.and_then(|properties| parse_fill(properties, theme, clr_map)),
            line: properties.and_then(|properties| parse_line(properties, theme, clr_map)),
            body: body.and_then(|body| child_named(body, "bodyPr")).map(parse_body).unwrap_or_default(),
            levels: body
                .and_then(|body| child_named(body, "lstStyle"))
                .map(|style| parse_levels_from(Some(style)))
                .unwrap_or_default(),
        });
    }
    placeholders
}

fn parse_placeholder(ph: Option<&XmlElement>) -> Option<PlaceholderRef> {
    let ph = ph?;
    Some(PlaceholderRef {
        kind: PlaceholderKind::parse(&attribute_value(ph, "type").unwrap_or_else(|| "body".to_owned())),
        index: u32_attribute(ph, "idx"),
        size_hint: attribute_value(ph, "sz"),
    })
}

fn parse_body(element: &XmlElement) -> BodyProperties {
    BodyProperties {
        wrap: !matches!(attribute_value(element, "wrap").as_deref(), Some("none")),
        anchor: attribute_value(element, "anchor"),
        autofit: ["normAutofit", "spAutoFit", "noAutofit"]
            .iter()
            .find(|name| child_named(element, *name).is_some())
            .map(|name| (*name).to_owned()),
    }
}

fn parse_levels_from(style: Option<&XmlElement>) -> Vec<LevelStyle> {
    let Some(style) = style
    else {
        return Vec::new();
    };
    let mut levels = Vec::new();
    for level in 1..=9 {
        let name = format!("lvl{level}pPr");
        let Some(element) = child_named(style, &name)
        else {
            levels.push(LevelStyle::default());
            continue;
        };
        let defaults = child_named(element, "defRPr");
        levels.push(LevelStyle {
            size_pt: defaults
                .and_then(|defaults| attribute_value(defaults, "sz"))
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value / 100.0),
            bold: defaults.and_then(|defaults| attribute_value(defaults, "b")).map(|value| value == "1").unwrap_or(false),
            italic: defaults.and_then(|defaults| attribute_value(defaults, "i")).map(|value| value == "1").unwrap_or(false),
            color: None,
            font: defaults
                .and_then(|defaults| child_named(defaults, "latin"))
                .and_then(|latin| attribute_value(latin, "typeface")),
        });
    }
    levels
}

fn parse_tx_style(style: Option<&XmlElement>) -> Vec<LevelStyle> {
    parse_levels_from(style)
}

fn parse_transform(xfrm: Option<&XmlElement>) -> Option<Transform> {
    let xfrm = xfrm?;
    let offset = child_named(xfrm, "off")?;
    let extent = child_named(xfrm, "ext")?;
    Some(Transform {
        x: attribute_value(offset, "x").and_then(|value| value.parse().ok())?,
        y: attribute_value(offset, "y").and_then(|value| value.parse().ok())?,
        cx: attribute_value(extent, "cx").and_then(|value| value.parse().ok())?,
        cy: attribute_value(extent, "cy").and_then(|value| value.parse().ok())?,
        rotation_deg: attribute_value(xfrm, "rot")
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| value / 60000.0)
            .unwrap_or(0.0),
    })
}

fn parse_fill(element: &XmlElement, theme: &BTreeMap<String, String>, clr_map: &HashMap<String, String>) -> Option<Fill> {
    if child_named(element, "noFill").is_some() {
        return Some(Fill::None);
    }
    if let Some(solid) = child_named(element, "solidFill") {
        return Some(color_from(solid, theme, clr_map).map(Fill::Solid).unwrap_or(Fill::None));
    }
    if ["gradFill", "pattFill", "blipFill"].iter().any(|name| child_named(element, *name).is_some()) {
        return Some(Fill::None);
    }
    None
}

fn color_from(fill: &XmlElement, theme: &BTreeMap<String, String>, clr_map: &HashMap<String, String>) -> Option<String> {
    if let Some(srgb) = child_named(fill, "srgbClr") {
        return attribute_value(srgb, "val").map(|value| normalize_color(&value));
    }
    if let Some(scheme) = child_named(fill, "schemeClr") {
        let name = attribute_value(scheme, "val")?;
        return resolve_scheme(&name, theme, clr_map);
    }
    if let Some(system) = child_named(fill, "sysClr") {
        return attribute_value(system, "lastClr").map(|value| normalize_color(&value));
    }
    None
}

fn parse_line(element: &XmlElement, theme: &BTreeMap<String, String>, clr_map: &HashMap<String, String>) -> Option<Line> {
    let line = child_named(element, "ln")?;
    let width_pt =
        attribute_value(line, "w").and_then(|value| value.parse::<f64>().ok()).map(|value| value / 12700.0).unwrap_or(1.0);
    let color = child_named(line, "solidFill").and_then(|fill| color_from(fill, theme, clr_map));
    if color.is_none() && child_named(line, "noFill").is_some() {
        return None;
    }
    Some(Line { color, width_pt })
}

fn parse_background(root: &XmlElement, theme: &BTreeMap<String, String>, clr_map: &HashMap<String, String>) -> Option<Fill> {
    let cld = child_named(root, "cSld")?;
    let background = child_named(cld, "bg")?;
    if let Some(reference) = child_named(background, "bgRef") {
        let scheme = child_named(reference, "schemeClr").and_then(|color| attribute_value(color, "val"))?;
        return resolve_scheme(&scheme, theme, clr_map).map(Fill::Solid);
    }
    let properties = child_named(background, "bgPr")?;
    parse_fill(properties, theme, clr_map)
}

#[allow(clippy::too_many_arguments)]
fn parse_shape(
    _package: &OpcPackage,
    _ledger: &mut BudgetTracker,
    element: &XmlElement,
    _rels: &HashMap<String, OpcRelationship>,
    _owner: &str,
    slide: u32,
    layout: Option<&LayoutRecord>,
    master: Option<&MasterRecord>,
    presentation_levels: &[LevelStyle],
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
    issues: &mut Vec<PptxIssue>,
) -> Vec<Shape> {
    let id = shape_id(element, 0);
    let name = shape_name(element, "shape");
    let properties = child_named(element, "spPr");
    let placeholder = parse_placeholder(
        child_named(element, "nvSpPr").and_then(|nv| child_named(nv, "nvPr")).and_then(|nv| child_named(nv, "ph")),
    );
    let mut transform = properties.and_then(|properties| parse_transform(child_named(properties, "xfrm")));
    let mut inherited = false;
    let mut fill = properties.and_then(|properties| parse_fill(properties, theme, clr_map));
    let mut line = properties.and_then(|properties| parse_line(properties, theme, clr_map));
    let geometry = properties
        .and_then(|properties| child_named(properties, "prstGeom"))
        .and_then(|geometry| attribute_value(geometry, "prst"));

    let layout_placeholder = placeholder
        .as_ref()
        .and_then(|reference| find_placeholder(layout.map(|layout| layout.placeholders.as_slice()).unwrap_or(&[]), reference));
    let master_placeholder = placeholder
        .as_ref()
        .and_then(|reference| find_placeholder(master.map(|master| master.placeholders.as_slice()).unwrap_or(&[]), reference));

    if transform.is_none() {
        transform = layout_placeholder
            .and_then(|style| style.transform)
            .or_else(|| master_placeholder.and_then(|style| style.transform));
        inherited = transform.is_some();
        if inherited {
            issues.push(issue(
                codes::PLACEHOLDER_INHERITANCE,
                format!("slide {slide} shape `{id}` inherits its placement from the layout or master placeholder"),
                IssueStatus::Partial,
                Some(slide),
                Some(id),
            ));
        }
    }
    if fill.is_none() {
        fill = layout_placeholder
            .and_then(|style| style.fill.clone())
            .or_else(|| master_placeholder.and_then(|style| style.fill.clone()));
    }
    if line.is_none() {
        line = layout_placeholder
            .and_then(|style| style.line.clone())
            .or_else(|| master_placeholder.and_then(|style| style.line.clone()));
    }
    if properties.and_then(|properties| child_named(properties, "gradFill")).is_some() {
        issues.push(issue(
            codes::FILL_APPROXIMATED,
            format!("slide {slide} shape `{id}` gradient fill is flattened to no fill"),
            IssueStatus::Partial,
            Some(slide),
            Some(id),
        ));
    }
    if let Some(transform) = transform {
        if transform.rotation_deg.abs() > f64::EPSILON {
            issues.push(issue(
                codes::ROTATION,
                format!("slide {slide} shape `{id}` rotation of {:.1} degrees has no IR carrier", transform.rotation_deg),
                IssueStatus::Partial,
                Some(slide),
                Some(id),
            ));
        }
    }

    let levels = inherited_levels(layout_placeholder, master_placeholder, master, placeholder.as_ref(), presentation_levels);
    let text = child_named(element, "txBody").map(|body| parse_text_frame(body, &levels, theme, clr_map));
    let body_properties = child_named(element, "txBody")
        .and_then(|body| child_named(body, "bodyPr"))
        .map(parse_body)
        .or_else(|| layout_placeholder.map(|style| style.body.clone()))
        .or_else(|| master_placeholder.map(|style| style.body.clone()))
        .unwrap_or_default();
    let text = text.map(|mut frame| {
        frame.body = body_properties;
        frame
    });

    vec![Shape {
        id,
        name,
        kind: ShapeKind::Shape,
        placeholder,
        transform,
        inherited_transform: inherited,
        fill,
        line,
        geometry,
        text,
        image: None,
        table: None,
        chart: None,
    }]
}

fn inherited_levels<'a>(
    layout: Option<&'a PlaceholderStyle>,
    master: Option<&'a PlaceholderStyle>,
    master_record: Option<&'a MasterRecord>,
    placeholder: Option<&PlaceholderRef>,
    presentation: &'a [LevelStyle],
) -> Vec<LevelStyle> {
    if let Some(layout) = layout {
        if !layout.levels.is_empty() {
            return layout.levels.clone();
        }
    }
    if let Some(master) = master {
        if !master.levels.is_empty() {
            return master.levels.clone();
        }
    }
    if let Some(master) = master_record {
        let kind = placeholder.map(|placeholder| &placeholder.kind);
        let candidate = match kind {
            Some(PlaceholderKind::Title | PlaceholderKind::CenterTitle) => &master.title_style,
            Some(PlaceholderKind::Body | PlaceholderKind::Subtitle | PlaceholderKind::Object) => &master.body_style,
            _ => &master.other_style,
        };
        if !candidate.is_empty() {
            return candidate.clone();
        }
    }
    presentation.to_vec()
}

fn find_placeholder<'a>(placeholders: &'a [PlaceholderStyle], reference: &PlaceholderRef) -> Option<&'a PlaceholderStyle> {
    if let Some(index) = reference.index {
        if let Some(matched) = placeholders
            .iter()
            .find(|placeholder| placeholder.reference.as_ref().and_then(|reference| reference.index) == Some(index))
        {
            return Some(matched);
        }
    }
    placeholders.iter().find(|placeholder| {
        placeholder
            .reference
            .as_ref()
            .map(|candidate| candidate.kind == reference.kind || (candidate.kind.is_title() && reference.kind.is_title()))
            .unwrap_or(false)
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_picture(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    element: &XmlElement,
    rels: &HashMap<String, OpcRelationship>,
    owner: &str,
    slide: u32,
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
    issues: &mut Vec<PptxIssue>,
) -> Vec<Shape> {
    let id = shape_id(element, 0);
    let name = shape_name(element, "picture");
    let properties = child_named(element, "spPr");
    let transform = properties.and_then(|properties| parse_transform(child_named(properties, "xfrm")));
    let embed = child_named(element, "blipFill")
        .and_then(|fill| child_named(fill, "blip"))
        .and_then(|blip| relationship_attribute(blip, "embed"));
    let image = embed.and_then(|id| rels.get(&id)).map(|relationship| {
        let part = resolve_target(owner, &relationship.target);
        let bytes = read_part_optional(package, ledger, &part);
        SlideImage { media_type: media_type_for(&part), part, bytes }
    });
    if image.as_ref().and_then(|image| image.bytes.as_ref()).is_none() {
        issues.push(issue(
            codes::VIEW_IMAGE_MISSING,
            format!("slide {slide} picture `{id}` has no readable media bytes"),
            IssueStatus::Partial,
            Some(slide),
            Some(id),
        ));
    }
    let _ = (theme, clr_map);
    vec![Shape {
        id,
        name,
        kind: ShapeKind::Picture,
        placeholder: None,
        transform,
        inherited_transform: false,
        fill: None,
        line: None,
        geometry: Some("rect".to_owned()),
        text: None,
        image,
        table: None,
        chart: None,
    }]
}

#[allow(clippy::too_many_arguments)]
fn parse_graphic_frame(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    element: &XmlElement,
    rels: &HashMap<String, OpcRelationship>,
    owner: &str,
    slide: u32,
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
    issues: &mut Vec<PptxIssue>,
) -> Vec<Shape> {
    let id = shape_id(element, 0);
    let name = shape_name(element, "frame");
    let transform = parse_transform(child_named(element, "xfrm"));
    let Some(data) = child_named(element, "graphic").and_then(|graphic| child_named(graphic, "graphicData"))
    else {
        issues.push(issue(
            codes::GROUP,
            format!("slide {slide} frame `{id}` has no graphic data"),
            IssueStatus::Unsupported,
            Some(slide),
            Some(id),
        ));
        return Vec::new();
    };
    let uri = attribute_value(data, "uri").unwrap_or_default();
    if let Some(table) = child_named(data, "tbl") {
        return vec![Shape {
            id,
            name,
            kind: ShapeKind::Table,
            placeholder: None,
            transform,
            inherited_transform: false,
            fill: None,
            line: None,
            geometry: None,
            text: None,
            image: None,
            table: Some(parse_table(table, theme, clr_map, slide, id, issues)),
            chart: None,
        }];
    }
    if let Some(chart) = child_named(data, "chart") {
        let part = relationship_attribute(chart, "id")
            .and_then(|relationship| rels.get(&relationship))
            .map(|relationship| resolve_target(owner, &relationship.target));
        if let Some(part) = part.as_deref() {
            let chart = read_chart_ref(package, ledger, part);
            issues.push(issue(
                codes::CHART,
                format!(
                    "slide {slide} chart `{id}` of type `{}` keeps cached categories and values only, the canvas refuses its visual style",
                    chart.kind.as_deref().unwrap_or("unknown")
                ),
                IssueStatus::Partial,
                Some(slide),
                Some(id),
            ));
            return vec![Shape {
                id,
                name,
                kind: ShapeKind::Chart,
                placeholder: None,
                transform,
                inherited_transform: false,
                fill: None,
                line: None,
                geometry: None,
                text: None,
                image: None,
                table: None,
                chart: Some(chart),
            }];
        }
    }
    let code = if uri.ends_with("/diagram") {
        codes::SMART_ART
    }
    else if uri.ends_with("/ole") {
        codes::OLE_OBJECT
    }
    else {
        codes::GROUP
    };
    issues.push(issue(
        code,
        format!("slide {slide} frame `{id}` carries `{uri}`, which this reader refuses"),
        IssueStatus::Unsupported,
        Some(slide),
        Some(id),
    ));
    vec![Shape {
        id,
        name,
        kind: ShapeKind::Other,
        placeholder: None,
        transform,
        inherited_transform: false,
        fill: None,
        line: None,
        geometry: None,
        text: None,
        image: None,
        table: None,
        chart: None,
    }]
}

fn parse_table(
    table: &XmlElement,
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
    slide: u32,
    shape: u32,
    issues: &mut Vec<PptxIssue>,
) -> PptxTable {
    let columns: Vec<i64> = child_named(table, "tblGrid")
        .map(|grid| {
            children_named(grid, "gridCol")
                .iter()
                .filter_map(|column| attribute_value(column, "w"))
                .filter_map(|value| value.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    for row in children_named(table, "tr") {
        let height = attribute_value(row, "h").and_then(|value| value.parse().ok()).unwrap_or(0);
        let mut cells = Vec::new();
        for cell in children_named(row, "tc") {
            let colspan = u32_attribute(cell, "gridSpan").unwrap_or(1);
            let rowspan = u32_attribute(cell, "rowSpan").unwrap_or(1);
            let properties = child_named(cell, "tcPr");
            let fill = properties.and_then(|properties| parse_fill(properties, theme, clr_map));
            let text = child_named(cell, "txBody").map(|body| parse_text_frame(body, &[], theme, clr_map)).unwrap_or_default();
            cells.push(PptxTableCell { text, fill, colspan, rowspan });
        }
        rows.push(PptxTableRow { height, cells });
    }
    if child_named(child_named(table, "tblPr").unwrap_or(table), "tableStyleId").is_some() {
        issues.push(issue(
            codes::TABLE_STYLE,
            format!("slide {slide} table `{shape}` declares a table style id that is detected but not applied"),
            IssueStatus::Partial,
            Some(slide),
            Some(shape),
        ));
    }
    PptxTable { column_widths: columns, rows }
}

fn parse_text_frame(
    body: &XmlElement,
    levels: &[LevelStyle],
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
) -> TextFrame {
    let mut paragraphs = Vec::new();
    for paragraph in children_named(body, "p") {
        let properties = child_named(paragraph, "pPr");
        let level = properties
            .and_then(|properties| attribute_value(properties, "lvl"))
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(0)
            .min(8);
        let align = properties.and_then(|properties| attribute_value(properties, "algn"));
        let mut runs = Vec::new();
        for child in &paragraph.children {
            let Some(element) = child.as_element()
            else {
                continue;
            };
            match local_name(&element.name) {
                "r" => runs.push(Run {
                    text: element_text(element),
                    style: run_style(element, level, levels, theme, clr_map),
                    hyperlink: child_named(element, "rPr")
                        .and_then(|properties| child_named(properties, "hlinkClick"))
                        .and_then(|link| relationship_attribute(link, "id")),
                }),
                "br" => runs.push(Run { text: "\n".to_owned(), style: TextStyle::default(), hyperlink: None }),
                "fld" => runs.push(Run {
                    text: element_text(element),
                    style: run_style(element, level, levels, theme, clr_map),
                    hyperlink: None,
                }),
                _ => {}
            }
        }
        if runs.is_empty() {
            continue;
        }
        paragraphs.push(Paragraph { level, align, runs });
    }
    TextFrame { body: BodyProperties::default(), paragraphs }
}

fn run_style(
    run: &XmlElement,
    level: u8,
    levels: &[LevelStyle],
    theme: &BTreeMap<String, String>,
    clr_map: &HashMap<String, String>,
) -> TextStyle {
    let properties = child_named(run, "rPr");
    let inherited = levels.get(level as usize).cloned().unwrap_or_default();
    let declared_size = properties
        .and_then(|properties| attribute_value(properties, "sz"))
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value / 100.0);
    TextStyle {
        bold: properties
            .and_then(|properties| attribute_value(properties, "b"))
            .map(|value| value == "1")
            .unwrap_or(inherited.bold),
        italic: properties
            .and_then(|properties| attribute_value(properties, "i"))
            .map(|value| value == "1")
            .unwrap_or(inherited.italic),
        underline: properties
            .and_then(|properties| attribute_value(properties, "u"))
            .map(|value| value != "none")
            .unwrap_or(false),
        strike: properties
            .and_then(|properties| attribute_value(properties, "strike"))
            .map(|value| value != "noStrike")
            .unwrap_or(false),
        size_pt: declared_size.or(inherited.size_pt).or(Some(18.0)),
        color: properties
            .and_then(|properties| child_named(properties, "solidFill"))
            .and_then(|fill| color_from(fill, theme, clr_map))
            .or(inherited.color),
        font: properties
            .and_then(|properties| child_named(properties, "latin"))
            .and_then(|latin| attribute_value(latin, "typeface"))
            .or(inherited.font),
        language: properties.and_then(|properties| attribute_value(properties, "lang")),
    }
}

fn read_chart_ref(package: &OpcPackage, ledger: &mut BudgetTracker, part: &str) -> ChartRef {
    let mut reference =
        ChartRef { part: part.to_owned(), kind: None, series_count: 0, categories: Vec::new(), series: Vec::new() };
    let Some(value) = parse_part(package, ledger, part)
    else {
        return reference;
    };
    let Ok(root) = document_root(&value, "pptx")
    else {
        return reference;
    };
    reference.kind = descendants_named(root, "plotArea").first().copied().and_then(|plot| {
        plot.children
            .iter()
            .filter_map(|child| child.as_element())
            .map(|element| local_name(&element.name))
            .find(|name| name.ends_with("Chart"))
            .map(|name| name.to_owned())
    });
    let series = descendants_named(root, "ser");
    reference.series_count = series.len() as u32;
    for entry in &series {
        let name = child_named(entry, "tx").and_then(|tx| descendants_named(tx, "v").first().copied()).map(element_text);
        let values = child_named(entry, "val")
            .map(|points| {
                descendants_named(points, "pt")
                    .iter()
                    .filter_map(|point| {
                        child_named(point, "v").map(element_text).and_then(|text| text.trim().parse::<f64>().ok())
                    })
                    .collect::<Vec<f64>>()
            })
            .unwrap_or_default();
        if reference.categories.is_empty() {
            if let Some(categories) = child_named(entry, "cat") {
                reference.categories = descendants_named(categories, "pt")
                    .iter()
                    .filter_map(|point| child_named(point, "v").map(element_text))
                    .collect();
            }
        }
        reference.series.push(ChartSeries { name, values });
    }
    reference
}

fn read_notes(
    package: &OpcPackage,
    ledger: &mut BudgetTracker,
    rels: &HashMap<String, OpcRelationship>,
    owner: &str,
) -> Option<String> {
    let part = find_part(rels, "/notesSlide", owner)?;
    let value = parse_part(package, ledger, &part)?;
    let root = document_root(&value, "pptx").ok()?;
    let tree = child_named(root, "cSld").and_then(|cld| child_named(cld, "spTree"))?;
    let mut lines = Vec::new();
    for shape in children_named(tree, "sp") {
        let kind = child_named(shape, "nvSpPr")
            .and_then(|nv| child_named(nv, "nvPr"))
            .and_then(|nv| child_named(nv, "ph"))
            .and_then(|ph| attribute_value(ph, "type"));
        if kind.as_deref() != Some("body") {
            continue;
        }
        if let Some(body) = child_named(shape, "txBody") {
            let frame = parse_text_frame(body, &[], &BTreeMap::new(), &HashMap::new());
            for paragraph in &frame.paragraphs {
                let text = paragraph.text();
                if !text.trim().is_empty() {
                    lines.push(text);
                }
            }
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn media_type_for(part: &str) -> String {
    match part.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("emf") => "image/emf",
        Some("wmf") => "image/wmf",
        Some("mp4") => "video/mp4",
        Some("m4a") => "audio/mp4",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn shape_id(element: &XmlElement, fallback: u32) -> u32 {
    let holder = if element_is(element, "sp") {
        child_named(element, "nvSpPr")
    }
    else if element_is(element, "pic") {
        child_named(element, "nvPicPr")
    }
    else if element_is(element, "graphicFrame") {
        child_named(element, "nvGraphicFramePr")
    }
    else if element_is(element, "grpSp") {
        child_named(element, "nvGrpSpPr")
    }
    else {
        None
    };
    holder
        .and_then(|holder| child_named(holder, "cNvPr"))
        .and_then(|non_visual| u32_attribute(non_visual, "id"))
        .unwrap_or(fallback)
}

fn shape_name(element: &XmlElement, fallback: &str) -> String {
    let holder = if element_is(element, "sp") {
        child_named(element, "nvSpPr")
    }
    else if element_is(element, "pic") {
        child_named(element, "nvPicPr")
    }
    else if element_is(element, "graphicFrame") {
        child_named(element, "nvGraphicFramePr")
    }
    else if element_is(element, "grpSp") {
        child_named(element, "nvGrpSpPr")
    }
    else {
        None
    };
    holder
        .and_then(|holder| child_named(holder, "cNvPr"))
        .and_then(|non_visual| attribute_value(non_visual, "name"))
        .unwrap_or_else(|| fallback.to_owned())
}

fn compute_stats(slides: &[Slide], masters: &[MasterRecord]) -> PptxStats {
    let mut stats = PptxStats {
        slide_count: slides.len() as u32,
        hidden_slide_count: slides.iter().filter(|slide| slide.hidden).count() as u32,
        master_count: masters.len() as u32,
        layout_count: masters.iter().map(|master| master.layouts.len()).sum::<usize>() as u32,
        ..PptxStats::default()
    };
    for slide in slides {
        for shape in &slide.shapes {
            stats.shape_count += 1;
            if shape.image.is_some() {
                stats.image_count += 1;
            }
            if shape.table.is_some() {
                stats.table_count += 1;
            }
            if shape.chart.is_some() {
                stats.chart_count += 1;
            }
            if let Some(text) = &shape.text {
                count_text(text, &mut stats);
            }
            if let Some(table) = &shape.table {
                for row in &table.rows {
                    for cell in &row.cells {
                        count_text(&cell.text, &mut stats);
                    }
                }
            }
        }
        if slide.notes.is_some() {
            stats.notes_count += 1;
        }
    }
    stats
}

fn count_text(frame: &TextFrame, stats: &mut PptxStats) {
    for paragraph in &frame.paragraphs {
        for run in &paragraph.runs {
            stats.char_count += run.text.chars().count() as u32;
            stats.non_whitespace_chars += run.text.chars().filter(|character| !character.is_whitespace()).count() as u32;
        }
    }
}

fn issue(code: &str, message: impl Into<String>, status: IssueStatus, slide: Option<u32>, shape: Option<u32>) -> PptxIssue {
    PptxIssue { code: code.to_owned(), message: message.into(), status, slide, shape }
}
