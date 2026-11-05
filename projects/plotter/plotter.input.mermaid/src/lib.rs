#![warn(missing_docs)]
#![doc = include_str!("readme.md")]

use std::collections::{BTreeMap, BTreeSet};

use oak_core::{Builder, SourceText, errors::OakError, parser::session::ParseSession};
use oak_mermaid::{
    MermaidBuilder, MermaidLanguage,
    ast::{
        ClassDiagram, EdgeStyle as MermaidEdgeStyle, ErCardinality, ErDiagram, ErEntity, ErKey, ErRelation, FlowElement,
        Flowchart, MermaidDiagram, MermaidRoot, MessageArrow, NodeShape as MermaidNodeShape, ParticipantKind, SequenceDiagram,
        StateDiagram, StateElement, StateEndpoint, StateKind,
    },
};
use oak_uml::model::{Class, RelationKind};
use plotter_scene::SceneResolver;
use plotter_types::{
    ByteRange, Diagnostic, DiagnosticLabel, DiagnosticLocation, DiagnosticSeverity, LabelRole, Message, NAMESPACE, SourceRef,
    codes, diagnostic,
};

use plotter_diagram::{
    LoweredDiagram,
    compile::{LayoutOptions, compile},
    error::DiagramError,
    render::render,
    spec::{
        Cluster, DiagramEdge, DiagramNode, DiagramSpec, Direction, EdgeStyle, Endpoint, LayoutModel, NodeShape, SequenceLayout,
    },
};

/// Flowchart statements that `oak-mermaid` parses and then discards.
///
/// Styling, interaction, accessibility metadata and inner-direction overrides
/// never reach the spec, so lowering them would drop authored meaning. The
/// guard refuses the statement instead and keeps the source visible.
const DROPPED_FLOWCHART_STATEMENTS: &[&str] =
    &["style", "classDef", "class", "linkStyle", "click", "direction", "accTitle", "accDescr"];

/// Class-diagram statements that `oak-mermaid` parses and then discards.
///
/// `class ... { ... }` declarations are lowered, so `class` is absent here. The
/// guard ignores lines inside a class body: a member may legitimately share a
/// statement keyword. The list mirrors the frontend's own skipped keywords, so
/// a frontend change is visible here as one list to review.
const DROPPED_CLASS_STATEMENTS: &[&str] = &[
    "direction",
    "note",
    "Note",
    "style",
    "cssClass",
    "linkStyle",
    "namespace",
    "click",
    "acctitle",
    "accTitle",
    "accDescr",
    "callback",
    "link",
];

/// Sequence statements that `oak-mermaid` parses and then discards.
///
/// The guard names them before the arrow test: a skipped statement may carry
/// `->` inside its payload (a note that mentions an arrow, a title that spells
/// one out), and the arrow test would otherwise accept the line while the AST
/// dropped the whole statement. `activate` and `deactivate` are listed here as
/// well, the activation shorthand on a message is caught separately.
const DROPPED_SEQUENCE_STATEMENTS: &[&str] = &[
    "title",
    "acctitle",
    "accTitle",
    "accDescr",
    "note",
    "Note",
    "loop",
    "alt",
    "else",
    "opt",
    "par",
    "critical",
    "break",
    "rect",
    "end",
    "activate",
    "deactivate",
    "autonumber",
    "box",
    "create",
    "destroy",
    "link",
    "linkStyle",
];

/// State-diagram statements that `oak-mermaid` parses and then discards.
///
/// Styling, notes, and accessibility metadata never reach the AST, so lowering
/// the rest of the diagram would drop authored meaning. The guard refuses the
/// statement instead and keeps the source visible.
const DROPPED_STATE_STATEMENTS: &[&str] = &[
    "direction",
    "note",
    "Note",
    "style",
    "classDef",
    "class",
    "click",
    "linkStyle",
    "title",
    "acctitle",
    "accTitle",
    "accDescr",
];

/// Entity-relationship statements that `oak-mermaid` parses and then discards.
///
/// The ER parser treats every non-relationship line as an entity, so a styling
/// or accessibility statement would become a phantom entity. The guard names
/// them before that happens.
const DROPPED_ER_STATEMENTS: &[&str] =
    &["title", "acctitle", "accTitle", "accDescr", "direction", "style", "classDef", "click", "linkStyle", "note", "Note"];

/// Diagram families `oak-mermaid` reports no AST for.
///
/// Flowchart, sequence, class, state, and entity-relationship diagrams are
/// built into `MermaidRoot::diagrams`. The other headers parse without an error
/// but contribute nothing, so an empty tree would surface as a whole-body
/// `ast-unavailable`. Naming the authored family header keeps the refusal
/// precise and locatable.
const SUPPORTED_FAMILIES: &[&str] =
    &["flowchart", "graph", "classDiagram", "sequenceDiagram", "stateDiagram-v2", "stateDiagram", "erDiagram"];

struct NodeSeed {
    id: String,
    label: Option<String>,
    shape: Option<NodeShape>,
    cluster: Option<String>,
}

/// Accumulates nodes, edges, and clusters from nested flowchart elements.
#[derive(Default)]
struct Collector {
    nodes: Vec<NodeSeed>,
    indices: BTreeMap<String, usize>,
    edges: Vec<DiagramEdge>,
    clusters: Vec<Cluster>,
}

impl Collector {
    /// Walk one nesting level. `ancestors` holds the enclosing cluster ids,
    /// outermost first.
    fn collect(&mut self, elements: &[FlowElement], ancestors: &[String]) -> Result<(), DiagramError> {
        for element in elements {
            match element {
                FlowElement::Node { id, text, shape } => {
                    let id = require_id(id)?;
                    let label = text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
                    self.upsert_node(&id, label, Some(convert_shape(*shape)), ancestors);
                }
                FlowElement::Edge { from, to, text, style } => {
                    let from = require_id(from)?;
                    let to = require_id(to)?;
                    self.upsert_node(&from, None, None, ancestors);
                    self.upsert_node(&to, None, None, ancestors);
                    let label = text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
                    self.edges.push(DiagramEdge {
                        from: Endpoint { node: from, port: None },
                        to: Endpoint { node: to, port: None },
                        label,
                        style: convert_edge_style(*style),
                    });
                }
                FlowElement::Subgraph(subgraph) => {
                    let id = self.register_cluster(subgraph.id.as_deref(), &subgraph.title);
                    let mut nested = ancestors.to_vec();
                    nested.push(id);
                    self.collect(&subgraph.elements, &nested)?;
                }
            }
        }
        Ok(())
    }

    /// Walk one state-diagram scope. `ancestors` holds the enclosing region
    /// frames and `scope` names the region that owns its `[*]` pseudo states,
    /// so each concurrent region keeps its own initial and final node.
    fn collect_states(&mut self, elements: &[StateElement], ancestors: &[String], scope: &str) -> Result<(), DiagramError> {
        for element in elements {
            match element {
                StateElement::State(state) => {
                    let id = require_id(&state.id)?;
                    let label = state.text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
                    self.upsert_node(&id, label.clone(), Some(state_shape(state.kind)), ancestors);
                    let frame_label = label.clone().unwrap_or_else(|| id.clone());
                    for (index, region) in state.regions.iter().enumerate() {
                        let frame = self.register_cluster(Some(&format!("{id}:region{index}")), &frame_label);
                        let mut nested = ancestors.to_vec();
                        nested.push(frame.clone());
                        self.collect_states(&region.elements, &nested, &frame)?;
                    }
                }
                StateElement::Transition(transition) => {
                    let from = self.state_endpoint(&transition.from, ancestors, scope)?;
                    let to = self.state_endpoint(&transition.to, ancestors, scope)?;
                    self.edges.push(DiagramEdge {
                        from: Endpoint { node: from, port: None },
                        to: Endpoint { node: to, port: None },
                        label: transition.label.as_deref().map(str::trim).filter(|label| !label.is_empty()).map(str::to_string),
                        style: EdgeStyle::Arrow,
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve one transition endpoint. A state that only appears in a
    /// transition becomes an implicit node, matching Mermaid.
    fn state_endpoint(&mut self, endpoint: &StateEndpoint, ancestors: &[String], scope: &str) -> Result<String, DiagramError> {
        match endpoint {
            StateEndpoint::State(id) => {
                let id = require_id(id)?;
                self.upsert_node(&id, None, None, ancestors);
                Ok(id)
            }
            StateEndpoint::Start | StateEndpoint::End => {
                let role = if matches!(endpoint, StateEndpoint::Start) { "start" } else { "end" };
                let id = if scope.is_empty() { format!("[*]:{role}") } else { format!("[*]:{scope}:{role}") };
                self.upsert_node(&id, Some("[*]".to_string()), Some(NodeShape::Circle), ancestors);
                Ok(id)
            }
        }
    }

    fn upsert_node(&mut self, id: &str, label: Option<String>, shape: Option<NodeShape>, ancestors: &[String]) {
        match self.indices.get(id) {
            Some(&index) => {
                let node = &mut self.nodes[index];
                if node.label.is_none() {
                    node.label = label;
                }
                if node.shape.is_none() {
                    node.shape = shape;
                }
            }
            None => {
                self.indices.insert(id.to_string(), self.nodes.len());
                self.nodes.push(NodeSeed { id: id.to_string(), label, shape, cluster: ancestors.last().cloned() });
            }
        }

        for ancestor in ancestors {
            let Some(cluster) = self.clusters.iter_mut().find(|cluster| &cluster.id == ancestor)
            else {
                continue;
            };
            if !cluster.members.iter().any(|member| member == id) {
                cluster.members.push(id.to_string());
            }
        }
    }

    fn register_cluster(&mut self, explicit: Option<&str>, title: &str) -> String {
        let base = explicit
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("cluster-{}", self.clusters.len() + 1));
        let mut id = base.clone();
        let mut suffix = 1;
        while self.clusters.iter().any(|cluster| cluster.id == id) {
            id = format!("{base}-{suffix}");
            suffix += 1;
        }
        let label = title.trim();
        self.clusters.push(Cluster {
            id: id.clone(),
            label: (!label.is_empty()).then(|| label.to_string()),
            members: Vec::new(),
        });
        id
    }

    fn into_parts(self) -> (Vec<DiagramNode>, Vec<DiagramEdge>, Vec<Cluster>) {
        let nodes = self
            .nodes
            .into_iter()
            .map(|node| DiagramNode {
                id: node.id,
                label: node.label,
                shape: node.shape.unwrap_or(NodeShape::Box),
                ports: Vec::new(),
                cluster: node.cluster,
            })
            .collect();
        (nodes, self.edges, self.clusters)
    }
}

/// Parse Mermaid source through the `oak-mermaid` frontend.
///
/// The frontend recognises `graph`/`flowchart`, `sequenceDiagram`,
/// `classDiagram`, `stateDiagram`, and `erDiagram` sections. A known family
/// header that the frontend parses without an AST is refused as
/// `davinci.diagram.unsupported` with the header span, so every public entry
/// that reaches this parser reports the same code for the same source. Source
/// with no diagram at all is refused as `davinci.diagram.ast-unavailable`.
/// Family-specific lowering rules live in the functions below.
pub fn parse_mermaid(source: &str) -> Result<MermaidRoot, DiagramError> {
    let text = SourceText::new(source);
    let language = MermaidLanguage::default();
    let builder = MermaidBuilder::new(&language);
    let mut session = ParseSession::<MermaidLanguage>::default();
    let output = builder.build(&text, &[], &mut session);
    let oak_diagnostics = output.diagnostics.len();

    match output.result {
        Ok(root) if root.diagrams.is_empty() => {
            if let Some((range, header)) = locate_unsupported_family(source) {
                return Err(DiagramError::unsupported(format!(
                    "Mermaid diagram family `{header}` is not lowered, the diagram falls back to its source"
                ))
                .with_span(range));
            }
            Err(DiagramError::ast_unavailable(format!(
                "`oak-mermaid` accepted {} bytes with {oak_diagnostics} Oak parser diagnostics but produced no diagrams",
                source.len()
            )))
        }
        Ok(root) => Ok(root),
        Err(error) => Err(spanned_parse_error(source, &error)),
    }
}

/// Turn an `oak-mermaid` parser error into a refusal that keeps its span.
///
/// A syntax error records the offset of the offending token, so the refused
/// statement stays locatable instead of collapsing into an unpositioned
/// "parse failed" message.
fn spanned_parse_error(source: &str, error: &OakError) -> DiagramError {
    if let oak_core::errors::OakErrorKind::SyntaxError { message, offset, .. } = error.kind() {
        let offset = (*offset).min(source.len());
        let end = source[offset..].find('\n').map(|delta| offset + delta).unwrap_or(source.len());
        return DiagramError::unsupported(message.clone()).with_span(offset..end);
    }
    DiagramError::parse_failed(format!("`oak-mermaid` failed to build an AST: {error}"))
}

/// Lower Mermaid source into a source-independent diagram spec.
///
/// Only a single diagram is accepted. Node ids keep the first seen label, an
/// edge endpoint that has no node definition becomes an implicit `Box` node,
/// and each `subgraph` block becomes a flat [`Cluster`]. Nested subgraphs are
/// flattened: a node joins every ancestor cluster of the block that declares
/// it, so an outer frame still contains nested members.
///
/// Flowcharts, class, state, and entity-relationship diagrams lower to a
/// layered graph spec. Sequence diagrams lower to a
/// [`LayoutModel::TimeOrdered`] spec whose participant axis, message order, and
/// activation spans are declared explicitly, so every frontend family rebuilds
/// its scene with the ordinary `compile` pass.
pub fn mermaid_to_spec(source: &str) -> Result<DiagramSpec, DiagramError> {
    let root = parse_mermaid(source)?;
    match single_diagram(&root, source)? {
        MermaidDiagram::Flowchart(flowchart) => flowchart_spec(flowchart, source),
        MermaidDiagram::Class(class) => class_spec(class, source),
        MermaidDiagram::Sequence(sequence) => sequence_spec(sequence),
        MermaidDiagram::State(state) => state_spec(state, source),
        MermaidDiagram::EntityRelationship(er) => er_spec(er, source),
    }
}

/// Lower Mermaid source to a layout plan and resolved display scene.
///
/// The pipeline is `spec -> compile -> render -> resolve` for every family.
/// [`DiagramSpec::layout`] selects layered or time-ordered compilation.
/// Diagnostics come from the layout plan; rendering and resolution have no loss
/// surface because every plan construct has a deterministic scene mapping.
/// Unsupported sequence statements fail before compilation so the host keeps
/// the authored source visible instead of dropping the diagram silently.
///
/// A known family header without an AST is already refused by [`parse_mermaid`],
/// so this entry reports the same code as [`mermaid_to_spec`] for that source.
pub fn mermaid_to_scene(source: &str) -> Result<LoweredDiagram, DiagramError> {
    let root = parse_mermaid(source)?;
    match single_diagram(&root, source)? {
        MermaidDiagram::Flowchart(flowchart) => lower_graph(flowchart_spec(flowchart, source)?),
        MermaidDiagram::Sequence(sequence) => {
            validate_sequence_source(source)?;
            lower_graph(sequence_spec(sequence)?)
        }
        MermaidDiagram::Class(class) => lower_graph(class_spec(class, source)?),
        MermaidDiagram::State(state) => {
            let mut lowered = lower_graph(state_spec(state, source)?)?;
            let mut diagnostics = composite_state_diagnostics(state, source);
            diagnostics.append(&mut lowered.diagnostics);
            lowered.diagnostics = diagnostics;
            Ok(lowered)
        }
        MermaidDiagram::EntityRelationship(er) => lower_graph(er_spec(er, source)?),
    }
}

fn lower_graph(spec: DiagramSpec) -> Result<LoweredDiagram, DiagramError> {
    let plan = compile(&spec, &LayoutOptions::default());
    let semantic = render(&plan);
    let scene = semantic.resolve(&SceneResolver::default());
    let diagnostics = plan.diagnostics.clone();
    Ok(LoweredDiagram { plan, scene, diagnostics })
}

fn flowchart_spec(flowchart: &Flowchart, source: &str) -> Result<DiagramSpec, DiagramError> {
    if let Some((range, line)) = locate_unsupported_orientation(source) {
        return Err(DiagramError::unsupported(format!("unsupported flowchart orientation `{line}`")).with_span(range));
    }
    if let Some((range, keyword)) = locate_dropped_statement(source, DROPPED_FLOWCHART_STATEMENTS, false) {
        return Err(DiagramError::unsupported(format!(
            "flowchart statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }
    if let Some((range, line)) = locate_flowchart_conjunction(source) {
        return Err(DiagramError::unsupported(format!(
            "flowchart edge conjunction `&` is not lowered, the diagram falls back to its source: `{line}`"
        ))
        .with_span(range));
    }
    if let Some((range, shape)) = locate_unsupported_shape(source) {
        return Err(DiagramError::unsupported(format!(
            "flowchart node shape `{shape}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }
    let direction = direction(&flowchart.orientation).ok_or_else(|| {
        DiagramError::unsupported(format!("unsupported flowchart orientation `{}`", flowchart.orientation))
            .with_optional_span(locate_header(source, "flowchart").or_else(|| locate_header(source, "graph")))
    })?;
    let mut collector = Collector::default();
    collector.collect(&flowchart.elements, &[])?;

    if collector.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("flowchart contains no nodes or edges"));
    }

    let mut spec = DiagramSpec::new(direction);
    let (nodes, edges, clusters) = collector.into_parts();
    spec.nodes = nodes;
    spec.edges = edges;
    spec.clusters = clusters;
    Ok(spec)
}

fn single_diagram<'a>(root: &'a MermaidRoot, source: &str) -> Result<&'a MermaidDiagram, DiagramError> {
    if root.diagrams.is_empty() {
        return Err(DiagramError::ast_unavailable("Mermaid source produced no diagrams"));
    }
    if root.diagrams.len() > 1 {
        return Err(DiagramError::unsupported(format!(
            "a Mermaid root with {} diagrams is not lowered yet, exactly one diagram is supported",
            root.diagrams.len()
        ))
        .with_optional_span(locate_second_header(source)));
    }
    Ok(&root.diagrams[0])
}

/// Lower one `sequenceDiagram` AST into the graph spec used by sequence layout.
///
/// Participants become nodes in declaration order and messages become edges in
/// source order. A message endpoint that has no participant declaration becomes
/// an implicit participant, matching Mermaid. `NodeShape::Box` marks a
/// participant and `NodeShape::Round` marks an actor, so the actor distinction
/// survives without a second node model.
fn sequence_spec(sequence: &SequenceDiagram) -> Result<DiagramSpec, DiagramError> {
    let mut spec = DiagramSpec::new(Direction::TopDown);
    let mut known: BTreeSet<String> = BTreeSet::new();

    for participant in &sequence.participants {
        let id = participant.id.trim();
        if id.is_empty() {
            continue;
        }
        let label = participant.display.as_deref().map(str::trim).filter(|display| !display.is_empty()).map(str::to_string);
        spec.nodes.push(DiagramNode {
            id: id.to_string(),
            label,
            shape: match participant.kind {
                ParticipantKind::Participant => NodeShape::Box,
                ParticipantKind::Actor => NodeShape::Round,
            },
            ports: Vec::new(),
            cluster: None,
        });
        known.insert(id.to_string());
    }

    for message in &sequence.messages {
        let from = message.from.trim();
        let to = message.to.trim();
        if from.is_empty() || to.is_empty() {
            return Err(DiagramError::unsupported("sequence message has an empty participant id"));
        }
        for endpoint in [from, to] {
            if known.insert(endpoint.to_string()) {
                spec.nodes.push(DiagramNode {
                    id: endpoint.to_string(),
                    label: None,
                    shape: NodeShape::Box,
                    ports: Vec::new(),
                    cluster: None,
                });
            }
        }
        spec.edges.push(DiagramEdge {
            from: Endpoint { node: from.to_string(), port: None },
            to: Endpoint { node: to.to_string(), port: None },
            label: Some(message.text.trim().to_string()).filter(|text| !text.is_empty()),
            style: message_style(message.arrow),
        });
    }

    if spec.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("sequence diagram contains no participants"));
    }
    spec.layout = LayoutModel::TimeOrdered(SequenceLayout {
        participants: spec.nodes.iter().map(|node| node.id.clone()).collect(),
        messages: (0..spec.edges.len()).collect(),
        activations: Vec::new(),
    });
    Ok(spec)
}

fn message_style(arrow: MessageArrow) -> EdgeStyle {
    match arrow {
        MessageArrow::Solid => EdgeStyle::Line,
        MessageArrow::Dotted => EdgeStyle::DottedLine,
        MessageArrow::SolidArrow => EdgeStyle::Arrow,
        MessageArrow::DottedArrow => EdgeStyle::DottedArrow,
    }
}

/// One authored class relation line with the operand that receives the marker.
struct ClassRelationLine {
    left: String,
    right: String,
    marker_at_left: bool,
}

/// Lower one `classDiagram` AST into the graph spec used by layered layout.
///
/// A class becomes a `Box` node whose label starts with the class name followed
/// by one line per member, so the existing node measurement and multiline label
/// rendering carry members without a new scene primitive. A class that appears
/// only in a relation becomes an implicit node.
///
/// Relation kinds keep their UML end marker: inheritance uses a hollow arrow,
/// composition a filled diamond, aggregation a hollow diamond, association a
/// filled arrow, and dependency a dotted arrow. The Oak class AST keeps only
/// the relation kind and endpoint order, so the authored arrow spelling is
/// matched back to the relation to place the marker on the correct operand.
/// This keeps `A <|-- B` and `A --|> B` pointing at the same superclass.
fn class_spec(diagram: &ClassDiagram, source: &str) -> Result<DiagramSpec, DiagramError> {
    if let Some((range, keyword)) = locate_dropped_statement(source, DROPPED_CLASS_STATEMENTS, true) {
        return Err(DiagramError::unsupported(format!(
            "class statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }
    let mut spec = DiagramSpec::new(Direction::BottomUp);
    let mut known: BTreeSet<String> = BTreeSet::new();

    for class in &diagram.classes {
        let name = class.name.trim();
        if name.is_empty() {
            continue;
        }
        spec.nodes.push(DiagramNode {
            id: name.to_string(),
            label: Some(class_label(class)),
            shape: NodeShape::Box,
            ports: Vec::new(),
            cluster: None,
        });
        known.insert(name.to_string());
    }

    let relation_lines = class_relation_lines(source);
    let mut used_lines = vec![false; relation_lines.len()];
    for relation in &diagram.relations {
        let source_name = relation.source.trim();
        let target_name = relation.target.trim();
        if source_name.is_empty() || target_name.is_empty() {
            return Err(DiagramError::unsupported("class relation has an empty class name"));
        }
        for endpoint in [source_name, target_name] {
            if known.insert(endpoint.to_string()) {
                spec.nodes.push(DiagramNode {
                    id: endpoint.to_string(),
                    label: None,
                    shape: NodeShape::Box,
                    ports: Vec::new(),
                    cluster: None,
                });
            }
        }

        let marker_at_left = match take_relation_line(&relation_lines, &mut used_lines, source_name, target_name) {
            Some(marker_at_left) => marker_at_left,
            None => matches!(&relation.kind, RelationKind::Inheritance | RelationKind::Composition | RelationKind::Aggregation),
        };
        let (from, to) = if marker_at_left { (target_name, source_name) } else { (source_name, target_name) };
        spec.edges.push(DiagramEdge {
            from: Endpoint { node: from.to_string(), port: None },
            to: Endpoint { node: to.to_string(), port: None },
            label: relation.label.as_deref().map(str::trim).filter(|label| !label.is_empty()).map(str::to_string),
            style: class_edge_style(&relation.kind),
        });
    }

    if spec.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("class diagram contains no classes or relations"));
    }
    Ok(spec)
}

fn class_label(class: &Class) -> String {
    let mut label = class.name.trim().to_string();
    for member in &class.members {
        let name = member.name.trim();
        if !name.is_empty() {
            label.push('\n');
            label.push_str(name);
        }
    }
    label
}

fn class_edge_style(kind: &RelationKind) -> EdgeStyle {
    match kind {
        RelationKind::Inheritance => EdgeStyle::HollowArrow,
        RelationKind::Association => EdgeStyle::Arrow,
        RelationKind::Composition => EdgeStyle::FilledDiamond,
        RelationKind::Aggregation => EdgeStyle::HollowDiamond,
        RelationKind::Dependency => EdgeStyle::DottedArrow,
    }
}

/// Lower one `stateDiagram` AST into the graph spec used by layered layout.
///
/// A state becomes a node whose outline follows its stereotype, and a
/// transition becomes a directed edge carrying its event/guard text. `[*]` is
/// drawn as a circle labelled with the authored token, once per scope: a top
/// level diagram and every concurrent region keep their own initial and final
/// pseudo state, because `[*]` is relative to its region.
///
/// A composite state stays a node, each of its regions becomes a cluster
/// frame titled with the state label, and nested states join every enclosing
/// frame, the same flattening rule as nested flowchart subgraphs.
///
/// The flat frame level means the state node and its region frames are siblings
/// instead of a containment, so the lowering records that loss as
/// `davinci.diagram.approximated`; it is never dropped without a diagnostic.
fn state_spec(diagram: &StateDiagram, source: &str) -> Result<DiagramSpec, DiagramError> {
    if let Some((range, keyword)) = locate_dropped_statement(source, DROPPED_STATE_STATEMENTS, false) {
        return Err(DiagramError::unsupported(format!(
            "state statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }

    let mut collector = Collector::default();
    collector.collect_states(&diagram.elements, &[], "")?;
    if collector.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("state diagram contains no states or transitions"));
    }

    let mut spec = DiagramSpec::new(Direction::TopDown);
    let (nodes, edges, clusters) = collector.into_parts();
    spec.nodes = nodes;
    spec.edges = edges;
    spec.clusters = clusters;
    Ok(spec)
}

fn state_shape(kind: StateKind) -> NodeShape {
    match kind {
        StateKind::Normal => NodeShape::Round,
        StateKind::Fork | StateKind::Join => NodeShape::Box,
        StateKind::Choice => NodeShape::Rhombus,
        StateKind::EntryPoint | StateKind::ExitPoint => NodeShape::Circle,
    }
}

/// Approximation records for every composite state in one diagram.
///
/// The state node survives as an edge endpoint and its regions become flat
/// frames, so the containment between the state and its regions is the loss.
/// One record per composite state names the state and the region count.
fn composite_state_diagnostics(diagram: &StateDiagram, source: &str) -> Vec<Diagnostic> {
    let mut records = Vec::new();
    collect_composite_states(&diagram.elements, source, &mut records);
    records
}

fn collect_composite_states(elements: &[StateElement], source: &str, out: &mut Vec<Diagnostic>) {
    for element in elements {
        let StateElement::State(state) = element
        else {
            continue;
        };
        if !state.regions.is_empty() {
            let label = state.text.as_deref().map(str::trim).filter(|text| !text.is_empty()).unwrap_or(&state.id);
            out.push(approximated_at(
                format!(
                    "composite state `{label}` keeps its own node and its {} region(s) become separate flat frames, the containment is not represented",
                    state.regions.len()
                ),
                locate_composite_state(source, &state.id),
            ));
        }
        for region in &state.regions {
            collect_composite_states(&region.elements, source, out);
        }
    }
}

/// The authored `state <id> {` line of one composite state.
fn locate_composite_state(source: &str, id: &str) -> Option<std::ops::Range<usize>> {
    for (range, line) in trimmed_lines(source) {
        let Some(rest) = line.strip_prefix("state")
        else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) || !rest.contains('{') {
            continue;
        }
        let head = rest.split('{').next().unwrap_or_default().trim();
        let name = head.rsplit(" as ").next().unwrap_or(head).trim().trim_matches('"').trim();
        if name == id {
            return Some(range);
        }
    }
    None
}

/// Build an approximation warning that points at the authored line.
fn approximated_at(message: impl Into<String>, span: Option<std::ops::Range<usize>>) -> Diagnostic {
    let message = message.into();
    let record = diagnostic("diagram", codes::APPROXIMATED, DiagnosticSeverity::Warning, message.clone());
    let Some(span) = span
    else {
        return record;
    };
    let Ok(source) = SourceRef::new(NAMESPACE, "mermaid")
    else {
        return record;
    };
    let Ok(range) = ByteRange::new(span.start as u64, span.end as u64)
    else {
        return record;
    };
    record.with_primary(DiagnosticLabel::new(
        DiagnosticLocation::Text { source, range },
        Message::new(codes::APPROXIMATED).with_fallback(message),
        LabelRole::Primary,
    ))
}

/// Lower one `erDiagram` AST into the graph spec used by layered layout.
///
/// An entity becomes a `Box` node whose label starts with the entity name
/// followed by one line per attribute, so attributes and their key markers
/// render without a new scene primitive. A relationship becomes a plain line
/// carrying both crow's foot cardinalities in its label, left end first. An
/// identifying relationship keeps a solid line and a non-identifying one uses
/// the dotted style, which the layout reports as an approximation.
fn er_spec(diagram: &ErDiagram, source: &str) -> Result<DiagramSpec, DiagramError> {
    if let Some((range, keyword)) = locate_dropped_statement(source, DROPPED_ER_STATEMENTS, true) {
        return Err(DiagramError::unsupported(format!(
            "entity relationship statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }

    let mut spec = DiagramSpec::new(Direction::TopDown);
    let mut known: BTreeSet<String> = BTreeSet::new();

    for entity in &diagram.entities {
        let name = require_id(&entity.name)?;
        spec.nodes.push(DiagramNode {
            id: name.clone(),
            label: Some(er_label(entity)),
            shape: NodeShape::Box,
            ports: Vec::new(),
            cluster: None,
        });
        known.insert(name);
    }

    for relation in &diagram.relations {
        let left = require_id(&relation.left)?;
        let right = require_id(&relation.right)?;
        for name in [&left, &right] {
            if known.insert(name.clone()) {
                spec.nodes.push(DiagramNode {
                    id: name.clone(),
                    label: None,
                    shape: NodeShape::Box,
                    ports: Vec::new(),
                    cluster: None,
                });
            }
        }
        spec.edges.push(DiagramEdge {
            from: Endpoint { node: left, port: None },
            to: Endpoint { node: right, port: None },
            label: Some(er_edge_label(relation)),
            style: if relation.identifying { EdgeStyle::Line } else { EdgeStyle::DottedLine },
        });
    }

    if spec.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("entity relationship diagram contains no entities or relations"));
    }
    Ok(spec)
}

fn er_label(entity: &ErEntity) -> String {
    let mut label = entity.name.trim().to_string();
    for attribute in &entity.attributes {
        let mut line = String::new();
        append_word(&mut line, attribute.type_name.trim());
        append_word(&mut line, attribute.name.trim());
        for key in &attribute.keys {
            append_word(
                &mut line,
                match key {
                    ErKey::Primary => "PK",
                    ErKey::Foreign => "FK",
                },
            );
        }
        if let Some(comment) = attribute.comment.as_deref().map(str::trim).filter(|comment| !comment.is_empty()) {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push('"');
            line.push_str(comment);
            line.push('"');
        }
        if !line.is_empty() {
            label.push('\n');
            label.push_str(&line);
        }
    }
    label
}

fn append_word(line: &mut String, word: &str) {
    if word.is_empty() {
        return;
    }
    if !line.is_empty() {
        line.push(' ');
    }
    line.push_str(word);
}

/// A relation label that keeps both authored crow's foot cardinalities.
fn er_edge_label(relation: &ErRelation) -> String {
    let left = cardinality_token(relation.left_cardinality);
    let right = cardinality_token(relation.right_cardinality);
    match relation.label.as_deref().map(str::trim).filter(|label| !label.is_empty()) {
        Some(label) => format!("{left} {label} {right}"),
        None => format!("{left} {right}"),
    }
}

fn cardinality_token(cardinality: ErCardinality) -> &'static str {
    match cardinality {
        ErCardinality::One => "||",
        ErCardinality::ZeroOrOne => "o|",
        ErCardinality::OneOrMore => "|{",
        ErCardinality::ZeroOrMore => "o{",
    }
}

/// Recover relation lines from the authored `classDiagram` section.
///
/// Only lines outside a class member block are considered, because member
/// defaults can contain arrow-like punctuation. The scan normalizes marker
/// placement only, it does not build a second relation model.
fn class_relation_lines(source: &str) -> Vec<ClassRelationLine> {
    let mut lines = Vec::new();
    let mut inside = false;
    let mut depth = 0usize;
    for raw in source.lines() {
        let line = raw.trim();
        if !inside {
            inside = line.starts_with("classDiagram");
            continue;
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if depth == 0 && DIAGRAM_HEADERS.iter().any(|header| starts_with_keyword(line, header)) {
            break;
        }
        let opens = line.matches('{').count();
        let closes = line.matches('}').count();
        if depth == 0 && opens == 0 && closes == 0 {
            if let Some(relation) = class_relation_line(line) {
                lines.push(relation);
            }
        }
        depth = depth.saturating_add(opens).saturating_sub(closes);
    }
    lines
}

fn class_relation_line(line: &str) -> Option<ClassRelationLine> {
    const ARROWS: &[(&str, bool)] = &[
        ("<|--", true),
        ("--|>", false),
        ("..|>", false),
        ("|>..", true),
        ("*--", true),
        ("--*", false),
        ("o--", true),
        ("--o", false),
        ("..>", false),
        ("<..", true),
        ("-->", false),
        ("<--", true),
        ("..", false),
    ];
    let (index, token, marker_at_left) = ARROWS
        .iter()
        .filter_map(|&(token, marker_at_left)| line.find(token).map(|index| (index, token, marker_at_left)))
        .min_by_key(|&(index, token, _)| (index, std::cmp::Reverse(token.len())))?;
    let left = line[..index].trim();
    let right = line[index + token.len()..].split(':').next().unwrap_or_default().trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some(ClassRelationLine { left: left.to_string(), right: right.to_string(), marker_at_left })
}

fn take_relation_line(lines: &[ClassRelationLine], used: &mut [bool], source: &str, target: &str) -> Option<bool> {
    for (index, line) in lines.iter().enumerate() {
        if used[index] {
            continue;
        }
        if line.left == source && line.right == target {
            used[index] = true;
            return Some(line.marker_at_left);
        }
        if line.left == target && line.right == source {
            used[index] = true;
            return Some(!line.marker_at_left);
        }
    }
    None
}

/// Diagram headers that start a new section in one Mermaid source.
const DIAGRAM_HEADERS: &[&str] = &[
    "flowchart",
    "graph",
    "classDiagram",
    "stateDiagram-v2",
    "stateDiagram",
    "erDiagram",
    "gantt",
    "pie",
    "gitGraph",
    "journey",
    "mindmap",
    "requirementDiagram",
    "timeline",
    "quadrantChart",
    "xychart-beta",
    "sankey-beta",
    "block-beta",
    "C4Context",
    "zenuml",
    "sequenceDiagram",
];

/// The header line of the first diagram family that has no Oak AST.
///
/// Only the first header decides the family, a later header belongs to the
/// multi-diagram refusal. The returned header is always the authored spelling,
/// so the message names what the author wrote.
fn locate_unsupported_family(source: &str) -> Option<(std::ops::Range<usize>, &'static str)> {
    for (range, line) in trimmed_lines(source) {
        let found = DIAGRAM_HEADERS.iter().find(|header| starts_with_keyword(line, header));
        let Some(header) = found
        else {
            continue;
        };
        if SUPPORTED_FAMILIES.contains(header) {
            return None;
        }
        return Some((range, *header));
    }
    None
}

/// The authored arrow that carries a `+` or `-` activation shorthand.
///
/// `oak-mermaid` keeps no activation spans, so accepting `A->>+B` would lower
/// the message and silently drop the activation. The shorthand is refused at
/// its authored line instead.
fn activation_suffix(line: &str) -> Option<&'static str> {
    const SHORTHANDS: &[&str] = &["-->>+", "-->>-", "->>+", "->>-", "-->+", "-->-", "->+", "->-"];
    SHORTHANDS.iter().find(|token| line.contains(**token)).copied()
}

/// Reject sequence statements outside the documented lowering subset.
///
/// `oak-mermaid` skips statements such as `Note`, `loop`, `alt`, `activate`,
/// and `autonumber`. Skipping them in the AST would render a silently
/// incomplete sequence, so this guard checks the authored sequence section
/// before lowering: blank lines, comments, participant/actor declarations, and
/// message arrows are accepted, and every other statement produces a structured
/// `davinci.diagram.unsupported` error so the host falls back to the authored
/// source. The guard classifies statement starts only, it does not build a
/// second AST.
fn validate_sequence_source(source: &str) -> Result<(), DiagramError> {
    let mut inside = false;
    for (range, line) in trimmed_lines(source) {
        if !inside {
            inside = line.starts_with("sequenceDiagram");
            continue;
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if DIAGRAM_HEADERS.iter().any(|header| starts_with_keyword(line, header)) {
            break;
        }
        if starts_with_keyword(line, "participant") || starts_with_keyword(line, "actor") {
            continue;
        }
        let keyword = line.split_whitespace().next().unwrap_or(line).trim_end_matches(':');
        if DROPPED_SEQUENCE_STATEMENTS.contains(&keyword) {
            return Err(DiagramError::unsupported(format!(
                "sequence statement `{keyword}` is not lowered, the diagram falls back to its source"
            ))
            .with_span(range));
        }
        if let Some(keyword) = activation_suffix(line) {
            return Err(DiagramError::unsupported(format!(
                "sequence activation shorthand `{keyword}` is not lowered, the diagram falls back to its source"
            ))
            .with_span(range));
        }
        if line.contains("-->>") || line.contains("->>") || line.contains("-->") || line.contains("->") {
            continue;
        }
        return Err(DiagramError::unsupported(format!(
            "sequence statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }
    Ok(())
}

/// Every non-empty line with its byte range inside the authored source.
///
/// Leading and trailing whitespace is excluded so a span covers the statement
/// text itself. Lines are split on `\n` with a trailing `\r` ignored, which
/// keeps offsets identical on CRLF and LF copies of the same document.
fn trimmed_lines(source: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for raw in source.split_inclusive('\n') {
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let start = offset + (content.len() - content.trim_start().len());
        let trimmed = content.trim();
        lines.push((start..start + trimmed.len(), trimmed));
        offset += raw.len();
    }
    lines
}

/// The header line of an authored orientation outside the supported set.
///
/// `oak-mermaid` normalises an unknown orientation to top-down, which would
/// silently redraw the diagram in a direction the author did not write. The
/// authored token is therefore checked here before lowering.
fn locate_unsupported_orientation(source: &str) -> Option<(std::ops::Range<usize>, &str)> {
    const SUPPORTED: &[&str] = &["TD", "TB", "BT", "LR", "RL"];
    for (range, line) in trimmed_lines(source) {
        let rest = match line.strip_prefix("flowchart").or_else(|| line.strip_prefix("graph")) {
            Some(rest) if rest.starts_with(char::is_whitespace) || rest.is_empty() => rest.trim_start(),
            _ => continue,
        };
        let Some(token) = rest.split_whitespace().next()
        else {
            return None;
        };
        if !SUPPORTED.iter().any(|supported| supported.eq_ignore_ascii_case(token)) {
            return Some((range, line));
        }
        return None;
    }
    None
}

/// The first authored node shape that `davinci-diagram` cannot draw.
///
/// `oak-mermaid` accepts parallelogram, trapezoid, hexagon, and double-circle
/// delimiters but either keeps the delimiters in the label or drops the node
/// entirely. [`plotter_diagram::spec::NodeShape`] has no such variant, so refusing at the
/// authored line is the only outcome that neither rewrites labels nor loses
/// nodes. Quoted labels are skipped so a label may still contain these
/// characters as text.
fn locate_unsupported_shape(source: &str) -> Option<(std::ops::Range<usize>, &'static str)> {
    for (range, line) in trimmed_lines(source) {
        let bytes = line.as_bytes();
        let mut quoted = false;
        for index in 0..bytes.len() {
            match bytes[index] {
                b'"' => quoted = !quoted,
                b'[' if !quoted && index + 1 < bytes.len() => match bytes[index + 1] {
                    b'/' | b'\\' => return Some((range.clone(), "parallelogram or trapezoid")),
                    b'(' if index + 2 < bytes.len() && bytes[index + 2] == b'(' => {
                        return Some((range.clone(), "double circle"));
                    }
                    _ => {}
                },
                b'(' if !quoted && index + 2 < bytes.len() && bytes[index + 1] == b'(' && bytes[index + 2] == b'(' => {
                    return Some((range.clone(), "double circle"));
                }
                b'{' if !quoted && index + 1 < bytes.len() && bytes[index + 1] == b'{' => {
                    return Some((range.clone(), "hexagon"));
                }
                _ => {}
            }
        }

        // Asymmetric node `A>label]`. Only a leading node id is considered so
        // arrow ends (`-->`), edge labels, and HTML line breaks in labels are
        // never mistaken for the shape.
        let head = line.split(|character: char| !character.is_alphanumeric() && character != '_').next().unwrap_or_default();
        if !head.is_empty() {
            if let Some(after) = line[head.len()..].strip_prefix('>') {
                if after.contains(']') && !after.starts_with('-') {
                    return Some((range.clone(), "asymmetric"));
                }
            }
        }
    }
    None
}

/// The first authored flowchart line that uses the `&` edge conjunction.
///
/// `oak-mermaid` keeps only the final operand of `A --> B & C` and drops the
/// rest without a diagnostic, so the AST alone cannot represent the authored
/// edges. The guard refuses the line and lets the diagram fall back to its
/// source instead of silently losing nodes and edges.
fn locate_flowchart_conjunction(source: &str) -> Option<(std::ops::Range<usize>, String)> {
    for (range, line) in trimmed_lines(source) {
        if sanitized_flow_line(line).contains('&') {
            return Some((range, line.to_string()));
        }
    }
    None
}

/// Strip bracketed node text, quoted text, and pipe-delimited edge labels from
/// one flowchart line so an `&` inside an author label is not mistaken for the
/// edge conjunction.
fn sanitized_flow_line(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut piped = false;
    for ch in line.chars() {
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        if depth == 0 {
            if ch == '|' {
                piped = !piped;
                continue;
            }
            if piped {
                continue;
            }
        }
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// The first authored statement that the frontend parses and then discards.
///
/// The statement token is the first word of a trimmed line with a trailing
/// `:` removed, so `accTitle: Graph` matches `accTitle`. When `skip_braced` is
/// set, lines inside `{ ... }` blocks are ignored, which keeps class members
/// from being mistaken for statements. Each hit reports the line byte range, so
/// the refusal names authored bytes rather than an AST position the frontend
/// does not keep.
fn locate_dropped_statement(source: &str, keywords: &[&str], skip_braced: bool) -> Option<(std::ops::Range<usize>, String)> {
    let mut depth = 0usize;
    for (range, line) in trimmed_lines(source) {
        if skip_braced && depth > 0 {
            depth = depth.saturating_add(line.matches('{').count()).saturating_sub(line.matches('}').count());
            continue;
        }
        let token = line.split_whitespace().next().unwrap_or_default().trim_end_matches(':');
        if keywords.iter().any(|keyword| *keyword == token) {
            return Some((range, token.to_string()));
        }
        if skip_braced {
            depth = depth.saturating_add(line.matches('{').count()).saturating_sub(line.matches('}').count());
        }
    }
    None
}

/// The byte range of the first line that starts `keyword`.
fn locate_header(source: &str, keyword: &str) -> Option<std::ops::Range<usize>> {
    trimmed_lines(source).into_iter().find(|(_, line)| starts_with_keyword(line, keyword)).map(|(range, _)| range)
}

/// The byte range of the second diagram header, used for multi-diagram refusals.
fn locate_second_header(source: &str) -> Option<std::ops::Range<usize>> {
    let mut seen = 0usize;
    for (range, line) in trimmed_lines(source) {
        if DIAGRAM_HEADERS.iter().any(|header| starts_with_keyword(line, header)) {
            seen += 1;
            if seen == 2 {
                return Some(range);
            }
        }
    }
    None
}

fn starts_with_keyword(line: &str, keyword: &str) -> bool {
    line.strip_prefix(keyword)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with(':'))
}

fn require_id(id: &str) -> Result<String, DiagramError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(DiagramError::unsupported("diagram contains a node or edge endpoint with an empty id"));
    }
    Ok(trimmed.to_string())
}

fn direction(orientation: &str) -> Option<Direction> {
    match orientation.trim().to_ascii_uppercase().as_str() {
        "TD" | "TB" => Some(Direction::TopDown),
        "BT" => Some(Direction::BottomUp),
        "LR" => Some(Direction::LeftToRight),
        "RL" => Some(Direction::RightToLeft),
        _ => None,
    }
}

fn convert_shape(shape: MermaidNodeShape) -> NodeShape {
    match shape {
        MermaidNodeShape::Box => NodeShape::Box,
        MermaidNodeShape::Round => NodeShape::Round,
        MermaidNodeShape::Stadium => NodeShape::Stadium,
        MermaidNodeShape::Subroutine => NodeShape::Subroutine,
        MermaidNodeShape::Cylinder => NodeShape::Cylinder,
        MermaidNodeShape::Circle => NodeShape::Circle,
        MermaidNodeShape::Rhombus => NodeShape::Rhombus,
    }
}

fn convert_edge_style(style: MermaidEdgeStyle) -> EdgeStyle {
    match style {
        MermaidEdgeStyle::Line => EdgeStyle::Line,
        MermaidEdgeStyle::Arrow => EdgeStyle::Arrow,
        MermaidEdgeStyle::DottedArrow => EdgeStyle::DottedArrow,
        MermaidEdgeStyle::ThickArrow => EdgeStyle::ThickArrow,
    }
}
