#![warn(missing_docs)]
#![doc = include_str!("readme.md")]

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use oak_core::{Builder, SourceText, errors::OakError, parser::session::ParseSession};
use oak_plantuml::{
    PlantUmlBuilder, PlantUmlLanguage,
    ast::{
        ActivityDecision, ActivityDiagram, ActivityEndpoint, ActivityFork, ActivityNode, ActivitySwimlane, ActivityTransition,
        ClassDeclaration, ClassDiagram, ClassKeyword, ClassMember, ClassRelation, ClassRelationKind, ComponentDiagram,
        ComponentElement, ComponentNode, ComponentPackage, PortDirection, MemberVisibility, MessageArrow, ParticipantKind,
        PlantUmlDiagram, PlantUmlRoot, SequenceDiagram, StateDiagram, StateElement, StateEndpoint, StateKind,
    },
};
use plotter_scene::SceneResolver;
use plotter_types::{
    ByteRange, Diagnostic, DiagnosticLabel, DiagnosticLocation, DiagnosticSeverity, LabelRole, Message, NAMESPACE, SourceRef,
    codes, diagnostic,
};

use plotter_diagram::{
    LoweredDiagram,
    compile::{LayoutOptions, compile_with_source},
    error::DiagramError,
    render::render,
    source::SourceMap,
    spec::{
        Activation, Cluster, DiagramEdge, DiagramNode, DiagramSpec, Direction, EdgeStyle, Endpoint, LayoutModel, NodeShape,
        SequenceLayout,
    },
};

/// Source identity used by refusal and approximation labels.
const SOURCE: &str = "plantuml";

/// Diagram types PlantUML also understands but this frontend does not lower.
///
/// Activity, state, and component diagrams are absent because they are lowered
/// from their own `oak-plantuml` AST. Deployment, use case, object, timing and
/// the mind-map families still reach this frontend as flat text without an AST,
/// so their family keyword is refused at its authored line.
const OTHER_DIAGRAM_KEYWORDS: &[&str] = &[
    "deployment",
    "usecase",
    "object",
    "timing",
    "gantt",
    "mindmap",
    "wbs",
    "salt",
    "ditaa",
    "nwdiag",
    "rackdiag",
    "packetdiag",
    "archimate",
    "gitgraph",
    "chart",
];

/// Statements refused inside a class or sequence source.
///
/// The check is scoped to the family the AST reports, so `else`, `end`, and
/// `fork` stay valid activity statements while remaining refusals here.
const REJECTED_STATEMENTS: &[&str] = &[
    "skinparam",
    "note",
    "alt",
    "else",
    "loop",
    "group",
    "par",
    "opt",
    "break",
    "critical",
    "ref",
    "box",
    "title",
    "caption",
    "header",
    "footer",
    "legend",
    "scale",
    "autonumber",
    "newpage",
    "hide",
    "show",
    "left",
    "right",
    "top",
    "bottom",
    "allow_mixing",
];

/// Activity statements that `oak-plantuml` parses and then discards.
///
/// The builder keeps only actions, decisions, forks, swimlanes, and the legacy
/// arrow transitions. Loop constructs, notes, styling, and page metadata would
/// otherwise disappear while the rest of the diagram renders, so each one is
/// refused at its authored line.
const DROPPED_ACTIVITY_STATEMENTS: &[&str] = &[
    "note",
    "skinparam",
    "title",
    "caption",
    "header",
    "footer",
    "legend",
    "scale",
    "autonumber",
    "newpage",
    "hide",
    "show",
    "allow_mixing",
    "repeat",
    "while",
    "endwhile",
    "detach",
    "kill",
    "partition",
    "alt",
    "else",
    "loop",
    "group",
    "par",
    "opt",
    "break",
    "critical",
    "ref",
    "box",
    "activate",
    "deactivate",
    "destroy",
    "participant",
    "actor",
    "database",
    "boundary",
    "control",
    "collections",
    "queue",
    "left",
    "right",
    "top",
    "bottom",
];

/// State statements that `oak-plantuml` parses and then discards.
const DROPPED_STATE_STATEMENTS: &[&str] = &[
    "note",
    "skinparam",
    "title",
    "caption",
    "header",
    "footer",
    "legend",
    "scale",
    "autonumber",
    "newpage",
    "hide",
    "show",
    "allow_mixing",
    "direction",
    "style",
    "left",
    "right",
    "top",
    "bottom",
];

/// Component statements that `oak-plantuml` parses and then discards.
///
/// Only `component`, `interface`, `port`, `package`, `[Name]`, and relations
/// reach the AST. The other UML node kinds name a declaration the AST has no
/// field for, so they are refused instead of vanishing from the spec.
const DROPPED_COMPONENT_STATEMENTS: &[&str] = &[
    "note",
    "skinparam",
    "title",
    "caption",
    "header",
    "footer",
    "legend",
    "scale",
    "autonumber",
    "newpage",
    "hide",
    "show",
    "allow_mixing",
    "database",
    "node",
    "cloud",
    "artifact",
    "folder",
    "frame",
    "rectangle",
    "queue",
    "storage",
    "left",
    "right",
    "top",
    "bottom",
];

/// Parse PlantUML source into the typed `oak-plantuml` AST.
///
/// The refusal scan runs before the parser so rejected constructs keep the
/// byte range of the whole line instead of whatever token the generic lexer
/// happened to stop on.
pub fn parse_plantuml(source: &str) -> Result<PlantUmlRoot, DiagramError> {
    reject_unsupported(source)?;
    let text = SourceText::new(source);
    let language = PlantUmlLanguage::default();
    let builder = PlantUmlBuilder::new(&language);
    let mut session = ParseSession::<PlantUmlLanguage>::default();
    let output = builder.build(&text, &[], &mut session);
    let oak_diagnostics = output.diagnostics.len();

    match output.result {
        Ok(root) if root.diagrams.is_empty() => Err(DiagramError::ast_unavailable(format!(
            "`oak-plantuml` accepted {} bytes with {oak_diagnostics} Oak parser diagnostics but produced no diagram",
            source.len()
        ))),
        Ok(root) => Ok(root),
        Err(error) => Err(spanned_parse_error(source, &error)),
    }
}

/// Turn an `oak-plantuml` parser error into a refusal that keeps its span.
///
/// The parser records a syntax error at the token that contradicted the
/// expected grammar, which can be the second word of a discarded statement.
/// The span is widened back to the trimmed authored line so a refusal names
/// the statement rather than the token the lexer stopped on.
fn spanned_parse_error(source: &str, error: &OakError) -> DiagramError {
    if let oak_core::errors::OakErrorKind::SyntaxError { message, offset, .. } = error.kind() {
        let offset = (*offset).min(source.len());
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        let start = line_start + (source[line_start..offset].len() - source[line_start..offset].trim_start().len());
        let line_end = source[offset..].find('\n').map_or(source.len(), |delta| offset + delta);
        let end = start + source[start..line_end].trim_end().len();
        return DiagramError::unsupported(message.clone()).with_span(start..end);
    }
    DiagramError::parse_failed(format!("`oak-plantuml` failed to build an AST: {error}"))
}

/// Lower PlantUML source into a source-independent diagram spec.
pub fn plantuml_to_spec(source: &str) -> Result<DiagramSpec, DiagramError> {
    let root = parse_plantuml(source)?;
    plantuml_root_to_spec(&root, source).map(|(spec, _, _)| spec)
}

/// Lower PlantUML source to a layout plan and resolved display scene.
///
/// Class diagrams use the same layered pass as Mermaid class diagrams and DOT
/// graphs. Sequence diagrams declare their time-ordered layout, so activation
/// spans reach the shared sequence compiler instead of being dropped.
pub fn plantuml_to_scene(source: &str) -> Result<LoweredDiagram, DiagramError> {
    let root = parse_plantuml(source)?;
    let (spec, mut diagnostics, sources) = plantuml_root_to_spec(&root, source)?;
    let plan = compile_with_source(&spec, &LayoutOptions::default(), &sources);
    diagnostics.extend(plan.diagnostics.iter().cloned());
    let semantic = render(&plan);
    let scene = semantic.resolve(&SceneResolver::default());
    Ok(LoweredDiagram { plan, scene, diagnostics })
}

fn single_diagram(root: &PlantUmlRoot) -> Result<&PlantUmlDiagram, DiagramError> {
    if root.diagrams.len() > 1 {
        return Err(DiagramError::unsupported(format!(
            "a PlantUML root with {} diagrams is not lowered, exactly one diagram is supported",
            root.diagrams.len()
        )));
    }
    root.diagrams.first().ok_or_else(|| DiagramError::ast_unavailable("PlantUML source produced no diagrams"))
}

fn plantuml_root_to_spec(root: &PlantUmlRoot, source: &str) -> Result<(DiagramSpec, Vec<Diagnostic>, SourceMap), DiagramError> {
    match single_diagram(root)? {
        PlantUmlDiagram::Class(class) => class_spec(class, source),
        PlantUmlDiagram::Sequence(sequence) => sequence_spec(sequence, source).map(|(spec, sources)| (spec, Vec::new(), sources)),
        PlantUmlDiagram::Activity(activity) => activity_spec(activity, source),
        PlantUmlDiagram::State(state) => state_spec(state, source),
        PlantUmlDiagram::Component(component) => component_spec(component, source),
    }
}

/// Lower one PlantUML class diagram into the graph spec used by layered layout.
///
/// A class becomes a `Box` node whose label starts with the authored class
/// name followed by one line per member, so members travel through the existing
/// multiline label measurement. A class that appears only in a relation becomes
/// an implicit node.
///
/// The AST already reports which operand carries the UML end marker, so
/// `A <|-- B` and `A --|> B` point at the same superclass without a second
/// scan of the source text.
fn class_spec(diagram: &ClassDiagram, source: &str) -> Result<(DiagramSpec, Vec<Diagnostic>, SourceMap), DiagramError> {
    refuse_dropped(source, REJECTED_STATEMENTS, true, "class")?;
    let mut spec = DiagramSpec::new(Direction::BottomUp);
    let mut diagnostics = Vec::new();
    let mut sources = SourceMap::new(SOURCE);
    let mut known: BTreeSet<String> = BTreeSet::new();

    for class in &diagram.classes {
        let name = class.name.trim();
        if name.is_empty() {
            continue;
        }
        spec.nodes.push(DiagramNode {
            id: name.to_string(),
            label: Some(class_label(class, source)),
            shape: NodeShape::Box,
            ports: Vec::new(),
            cluster: None,
        });
        sources.push_node(Some(class.span.clone().into()));
        known.insert(name.to_string());
    }

    for relation in &diagram.relations {
        let left = relation.left.trim();
        let right = relation.right.trim();
        if left.is_empty() || right.is_empty() {
            return Err(
                DiagramError::unsupported("class relation has an empty class name").with_span(relation.span.clone().into())
            );
        }
        for endpoint in [left, right] {
            if known.insert(endpoint.to_string()) {
                spec.nodes.push(DiagramNode {
                    id: endpoint.to_string(),
                    label: None,
                    shape: NodeShape::Box,
                    ports: Vec::new(),
                    cluster: None,
                });
                sources.push_node(Some(relation.span.clone().into()));
            }
        }

        let (from, to) = if relation.marker_at_left { (right, left) } else { (left, right) };
        if relation.left_multiplicity.is_some() || relation.right_multiplicity.is_some() {
            diagnostics.push(approximated_at(
                "class relation multiplicities share the single edge label because the spec has one label per edge",
                relation.span.clone().into(),
            ));
        }
        spec.edges.push(DiagramEdge {
            from: Endpoint { node: from.to_string(), port: None },
            to: Endpoint { node: to.to_string(), port: None },
            label: relation_label(relation),
            style: class_edge_style(&relation.kind),
        });
        sources.push_edge(Some(relation.span.clone().into()));
    }

    if spec.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("class diagram contains no classes or relations"));
    }
    Ok((spec, diagnostics, sources))
}

/// Lower one PlantUML sequence diagram into the time-ordered sequence spec.
///
/// Participants keep their declaration order, messages their source order, and
/// activation spans the indices the AST recorded while scanning. A message
/// endpoint with no declaration becomes an implicit participant, matching the
/// Mermaid sequence lowering.
fn sequence_spec(diagram: &SequenceDiagram, source: &str) -> Result<(DiagramSpec, SourceMap), DiagramError> {
    refuse_dropped(source, REJECTED_STATEMENTS, false, "sequence")?;
    let mut spec = DiagramSpec::new(Direction::TopDown);
    let mut sources = SourceMap::new(SOURCE);
    let mut known: BTreeSet<String> = BTreeSet::new();

    for participant in &diagram.participants {
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
        sources.push_node(Some(participant.span.clone().into()));
        known.insert(id.to_string());
    }

    for message in &diagram.messages {
        let from = message.from.trim();
        let to = message.to.trim();
        if from.is_empty() || to.is_empty() {
            return Err(DiagramError::unsupported("sequence message has an empty participant id")
                .with_span(message.span.clone().into()));
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
                sources.push_node(Some(message.span.clone().into()));
            }
        }
        spec.edges.push(DiagramEdge {
            from: Endpoint { node: from.to_string(), port: None },
            to: Endpoint { node: to.to_string(), port: None },
            label: message.text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string),
            style: message_style(&message.arrow),
        });
        sources.push_edge(Some(message.span.clone().into()));
    }

    let message_count = spec.edges.len();
    let mut activations = Vec::with_capacity(diagram.activations.len());
    for span in &diagram.activations {
        if span.from_message > span.to_message || span.to_message >= message_count {
            return Err(DiagramError::unsupported(format!(
                "sequence activation span {}..={} is outside the {} authored messages",
                span.from_message, span.to_message, message_count
            )));
        }
        activations.push(Activation {
            participant: span.participant.clone(),
            from_message: span.from_message,
            to_message: span.to_message,
        });
    }

    if spec.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("sequence diagram contains no participants"));
    }
    spec.layout = LayoutModel::TimeOrdered(SequenceLayout {
        participants: spec.nodes.iter().map(|node| node.id.clone()).collect(),
        messages: (0..message_count).collect(),
        activations,
    });
    Ok((spec, sources))
}

/// One spec node before its outline is settled.
struct GraphNode {
    id: String,
    label: Option<String>,
    shape: Option<NodeShape>,
    cluster: Option<String>,
    span: Option<Range<usize>>,
}

/// Accumulates nodes, edges, and clusters with a source map kept in lockstep.
///
/// The activity and state families share this accumulator because both lower to
/// a layered graph. Nodes are inserted once and later declarations only fill
/// the missing label or outline, which is what a transition endpoint referring
/// to a state declared further down needs.
#[derive(Default)]
struct Graph {
    nodes: Vec<GraphNode>,
    indices: BTreeMap<String, usize>,
    edges: Vec<DiagramEdge>,
    edge_spans: Vec<Option<Range<usize>>>,
    clusters: Vec<Cluster>,
    diagnostics: Vec<Diagnostic>,
}

impl Graph {
    fn upsert(
        &mut self,
        id: &str,
        label: Option<String>,
        shape: Option<NodeShape>,
        ancestors: &[String],
        span: Option<Range<usize>>,
    ) {
        match self.indices.get(id) {
            Some(&index) => {
                let node = &mut self.nodes[index];
                if node.label.is_none() {
                    node.label = label;
                }
                if node.shape.is_none() {
                    node.shape = shape;
                }
                if node.span.is_none() {
                    node.span = span;
                }
            }
            None => {
                let index = self.nodes.len();
                self.indices.insert(id.to_string(), index);
                self.nodes.push(GraphNode {
                    id: id.to_string(),
                    label,
                    shape,
                    cluster: ancestors.last().cloned(),
                    span,
                });
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
        let title = title.trim();
        self.clusters.push(Cluster {
            id: id.clone(),
            label: (!title.is_empty()).then(|| title.to_string()),
            members: Vec::new(),
        });
        id
    }

    fn push_edge(&mut self, from: String, to: String, label: Option<String>, style: EdgeStyle, span: Option<Range<usize>>) {
        self.edges.push(DiagramEdge {
            from: Endpoint { node: from, port: None },
            to: Endpoint { node: to, port: None },
            label,
            style,
        });
        self.edge_spans.push(span);
    }

    fn into_parts(self) -> (Vec<DiagramNode>, Vec<DiagramEdge>, Vec<Cluster>, SourceMap, Vec<Diagnostic>) {
        let mut sources = SourceMap::new(SOURCE);
        for node in &self.nodes {
            sources.push_node(node.span.clone());
        }
        for span in &self.edge_spans {
            sources.push_edge(span.clone());
        }
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
        (nodes, self.edges, self.clusters, sources, self.diagnostics)
    }
}

/// A graph builder that also tracks the flow frontier while walking activity
/// statements.
///
/// Activity source order implies control flow: every emitted action connects
/// from the nodes that have no successor yet. A decision and a fork keep one
/// frontier entry per branch, so the node after `endif` or `end fork` receives
/// an edge from every branch instead of only the last one.
#[derive(Default)]
struct Flow {
    graph: Graph,
    tails: Vec<String>,
    pending: Option<String>,
    counter: usize,
}

impl Flow {
    fn fresh(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}:{}", self.counter);
        self.counter += 1;
        id
    }

    fn emit(
        &mut self,
        id: String,
        label: Option<String>,
        shape: NodeShape,
        ancestors: &[String],
        span: Option<Range<usize>>,
    ) {
        self.graph.upsert(&id, label, Some(shape), ancestors, span.clone());
        self.connect(&id, span);
        self.tails = vec![id];
    }

    fn connect(&mut self, to: &str, span: Option<Range<usize>>) {
        let label = self.pending.take();
        for tail in std::mem::take(&mut self.tails) {
            self.graph.push_edge(tail, to.to_string(), label.clone(), EdgeStyle::Arrow, span.clone());
        }
    }

    fn walk(&mut self, nodes: &[ActivityNode], ancestors: &[String]) -> Result<(), DiagramError> {
        for node in nodes {
            match node {
                ActivityNode::Start => {
                    let id = self.fresh("start");
                    self.emit(id, Some("start".to_string()), NodeShape::Circle, ancestors, None);
                }
                ActivityNode::Stop => {
                    let id = self.fresh("stop");
                    self.emit(id, Some("stop".to_string()), NodeShape::Circle, ancestors, None);
                }
                ActivityNode::Action(action) => {
                    let id = format!("action:{}", action.span.start);
                    let text = action.text.trim();
                    let label = (!text.is_empty()).then(|| text.to_string());
                    self.emit(id, label, NodeShape::Box, ancestors, Some(oak_span(&action.span)));
                }
                ActivityNode::Decision(decision) => self.decision(decision, ancestors)?,
                ActivityNode::Fork(fork) => self.fork(fork, ancestors)?,
                ActivityNode::Swimlane(lane) => self.swimlane(lane, ancestors)?,
                ActivityNode::Transition(transition) => self.transition(transition)?,
            }
        }
        Ok(())
    }

    fn decision(&mut self, decision: &ActivityDecision, ancestors: &[String]) -> Result<(), DiagramError> {
        let id = format!("decision:{}", decision.span.start);
        let label = decision
            .branches
            .first()
            .and_then(|branch| branch.condition.clone().or_else(|| branch.label.clone()))
            .filter(|text| !text.trim().is_empty());
        self.graph.upsert(&id, label, Some(NodeShape::Rhombus), ancestors, Some(oak_span(&decision.span)));
        self.connect(&id, Some(oak_span(&decision.span)));

        let mut tails = Vec::new();
        for (index, branch) in decision.branches.iter().enumerate() {
            let label = if index == 0 {
                branch.label.as_ref()
            }
            else {
                branch.condition.as_ref().or(branch.label.as_ref())
            }
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
            self.tails = vec![id.clone()];
            self.pending = label;
            self.walk(&branch.nodes, ancestors)?;
            tails.extend(std::mem::take(&mut self.tails));
            self.pending = None;
        }
        self.tails = tails;
        Ok(())
    }

    fn fork(&mut self, fork: &ActivityFork, ancestors: &[String]) -> Result<(), DiagramError> {
        let id = format!("fork:{}", fork.span.start);
        self.graph.upsert(&id, Some("fork".to_string()), Some(NodeShape::Box), ancestors, Some(oak_span(&fork.span)));
        self.connect(&id, Some(oak_span(&fork.span)));

        let mut tails = Vec::new();
        for branch in &fork.branches {
            self.tails = vec![id.clone()];
            self.walk(&branch.nodes, ancestors)?;
            tails.extend(std::mem::take(&mut self.tails));
        }
        self.tails = tails;
        Ok(())
    }

    fn swimlane(&mut self, lane: &ActivitySwimlane, ancestors: &[String]) -> Result<(), DiagramError> {
        let cluster = self.graph.register_cluster(Some(&format!("lane:{}", lane.span.start)), &lane.name);
        if lane.color.is_some() {
            self.graph.diagnostics.push(approximated_at(
                "an activity swimlane colour is not carried by the spec, the frame keeps the theme colour",
                oak_span(&lane.span),
            ));
        }
        let mut nested = ancestors.to_vec();
        nested.push(cluster);
        self.walk(&lane.nodes, &nested)
    }

    fn transition(&mut self, transition: &ActivityTransition) -> Result<(), DiagramError> {
        let from = self.endpoint(&transition.from, Some(oak_span(&transition.span)))?;
        let to = self.endpoint(&transition.to, Some(oak_span(&transition.span)))?;
        let label = transition.label.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
        self.graph.push_edge(from, to.clone(), label, EdgeStyle::Arrow, Some(oak_span(&transition.span)));
        self.tails = vec![to];
        Ok(())
    }

    fn endpoint(&mut self, endpoint: &ActivityEndpoint, span: Option<Range<usize>>) -> Result<String, DiagramError> {
        Ok(match endpoint {
            ActivityEndpoint::Start => {
                let id = self.fresh("(*):start");
                self.graph.upsert(&id, Some("(*)".to_string()), Some(NodeShape::Circle), &[], None);
                id
            }
            ActivityEndpoint::Stop => {
                let id = self.fresh("(*):stop");
                self.graph.upsert(&id, Some("(*)".to_string()), Some(NodeShape::Circle), &[], None);
                id
            }
            ActivityEndpoint::Named(name) => {
                let id = require_id(name)?;
                self.graph.upsert(&id, Some(id.clone()), Some(NodeShape::Box), &[], span);
                id
            }
        })
    }
}

/// Lower one activity diagram into the layered graph spec.
///
/// An action is a box, `start` and `stop` are circles, a decision is a rhombus
/// labelled with the `if` condition, and a fork is a box every parallel branch
/// leaves from. Swimlanes become cluster frames and their nodes keep the source
/// order of the flow across lane boundaries. Branch conditions ride the edge
/// that leaves the decision because the spec carries one label per edge and no
/// gate primitive.
fn activity_spec(
    diagram: &ActivityDiagram,
    source: &str,
) -> Result<(DiagramSpec, Vec<Diagnostic>, SourceMap), DiagramError> {
    refuse_dropped(source, DROPPED_ACTIVITY_STATEMENTS, false, "activity")?;
    let mut flow = Flow::default();
    flow.walk(&diagram.nodes, &[])?;

    if flow.graph.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("activity diagram contains no actions or transitions"));
    }

    let (nodes, edges, clusters, sources, diagnostics) = flow.graph.into_parts();
    let mut spec = DiagramSpec::new(Direction::TopDown);
    spec.nodes = nodes;
    spec.edges = edges;
    spec.clusters = clusters;
    Ok((spec, diagnostics, sources))
}

/// Lower one state diagram into the layered graph spec.
///
/// A state keeps its stereotype outline, a transition keeps its event text, and
/// `[*]` keeps a circle per scope so every concurrent region owns its initial
/// and final pseudo state. A composite state stays a node with one cluster frame
/// per region, and nested states join every enclosing frame.
fn state_spec(diagram: &StateDiagram, source: &str) -> Result<(DiagramSpec, Vec<Diagnostic>, SourceMap), DiagramError> {
    refuse_dropped(source, DROPPED_STATE_STATEMENTS, false, "state")?;
    if let Some((range, token)) = locate_unsupported_stereotype(source) {
        return Err(DiagramError::unsupported(format!(
            "state stereotype `<<{token}>>` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }

    let mut graph = Graph::default();
    collect_states(&mut graph, &diagram.elements, &[], "")?;
    if graph.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("state diagram contains no states or transitions"));
    }

    let (nodes, edges, clusters, sources, diagnostics) = graph.into_parts();
    let mut spec = DiagramSpec::new(Direction::TopDown);
    spec.nodes = nodes;
    spec.edges = edges;
    spec.clusters = clusters;
    Ok((spec, diagnostics, sources))
}

fn collect_states(
    graph: &mut Graph,
    elements: &[StateElement],
    ancestors: &[String],
    scope: &str,
) -> Result<(), DiagramError> {
    for element in elements {
        match element {
            StateElement::State(state) => {
                let id = require_id(&state.id)?;
                let label = state
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string)
                    .or_else(|| Some(id.clone()));
                graph.upsert(&id, label, Some(state_shape(state.kind)), ancestors, None);
                for (index, region) in state.regions.iter().enumerate() {
                    let frame = graph.register_cluster(Some(&format!("{id}:region{index}")), "");
                    let mut nested = ancestors.to_vec();
                    nested.push(frame.clone());
                    collect_states(graph, &region.elements, &nested, &frame)?;
                }
            }
            StateElement::Transition(transition) => {
                let from = state_endpoint(graph, &transition.from, ancestors, scope)?;
                let to = state_endpoint(graph, &transition.to, ancestors, scope)?;
                let label = transition.label.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
                graph.push_edge(from, to, label, EdgeStyle::Arrow, None);
            }
        }
    }
    Ok(())
}

fn state_endpoint(
    graph: &mut Graph,
    endpoint: &StateEndpoint,
    ancestors: &[String],
    scope: &str,
) -> Result<String, DiagramError> {
    match endpoint {
        StateEndpoint::State(id) => {
            let id = require_id(id)?;
            graph.upsert(&id, None, None, ancestors, None);
            Ok(id)
        }
        StateEndpoint::Start | StateEndpoint::End => {
            let role = if matches!(endpoint, StateEndpoint::Start) { "start" } else { "end" };
            let id = if scope.is_empty() { format!("[*]:{role}") } else { format!("[*]:{scope}:{role}") };
            graph.upsert(&id, Some("[*]".to_string()), Some(NodeShape::Circle), ancestors, None);
            Ok(id)
        }
    }
}

fn state_shape(kind: StateKind) -> NodeShape {
    match kind {
        StateKind::Normal => NodeShape::Round,
        StateKind::Fork | StateKind::Join => NodeShape::Box,
        StateKind::Choice => NodeShape::Rhombus,
        StateKind::Start | StateKind::End | StateKind::EntryPoint | StateKind::ExitPoint => NodeShape::Circle,
    }
}

/// Lower one component diagram into the layered graph spec.
///
/// A component or interface becomes a box, a package becomes a cluster frame,
/// and a port becomes a standalone round node. Relations reuse the class
/// relation styles; both multiplicities and the authored label fold into the
/// single edge label the spec carries.
fn component_spec(
    diagram: &ComponentDiagram,
    source: &str,
) -> Result<(DiagramSpec, Vec<Diagnostic>, SourceMap), DiagramError> {
    refuse_dropped(source, DROPPED_COMPONENT_STATEMENTS, false, "component")?;
    let mut graph = Graph::default();
    collect_components(&mut graph, &diagram.elements, &[], 0)?;

    for relation in &diagram.relations {
        let left = require_id(&relation.left)?;
        let right = require_id(&relation.right)?;
        for name in [&left, &right] {
            graph.upsert(name, None, None, &[], Some(oak_span(&relation.span)));
        }
        let (from, to) = if relation.marker_at_left { (right, left) } else { (left, right) };
        let label = folded_label(
            relation.left_multiplicity.as_ref(),
            relation.right_multiplicity.as_ref(),
            relation.label.as_ref(),
        );
        graph.push_edge(from, to, label, class_edge_style(&relation.kind), Some(oak_span(&relation.span)));
    }

    if graph.nodes.is_empty() {
        return Err(DiagramError::empty_diagram("component diagram contains no components or relations"));
    }

    let (nodes, edges, clusters, sources, diagnostics) = graph.into_parts();
    let mut spec = DiagramSpec::new(Direction::BottomUp);
    spec.nodes = nodes;
    spec.edges = edges;
    spec.clusters = clusters;
    Ok((spec, diagnostics, sources))
}

fn collect_components(
    graph: &mut Graph,
    elements: &[ComponentElement],
    ancestors: &[String],
    depth: usize,
) -> Result<(), DiagramError> {
    for element in elements {
        match element {
            ComponentElement::Component(node) => {
                let id = require_id(&node.name)?;
                graph.upsert(&id, Some(component_label(node, false)), Some(NodeShape::Box), ancestors, Some(oak_span(&node.span)));
            }
            ComponentElement::Interface(node) => {
                let id = require_id(&node.name)?;
                graph.upsert(&id, Some(component_label(node, true)), Some(NodeShape::Box), ancestors, Some(oak_span(&node.span)));
            }
            ComponentElement::Port(port) => {
                let id = require_id(&port.name)?;
                let label = format!("{} ({})", port.name.trim(), port_direction(port.direction));
                graph.upsert(&id, Some(label), Some(NodeShape::Round), ancestors, Some(oak_span(&port.span)));
                graph.diagnostics.push(approximated_at(
                    "a component port is drawn as a standalone node because the AST records no owning component",
                    oak_span(&port.span),
                ));
            }
            ComponentElement::Package(package) => {
                let cluster =
                    graph.register_cluster(Some(&format!("package:{}", package.span.start)), &package_label(package));
                if depth > 0 {
                    graph.diagnostics.push(approximated_at(
                        "a nested component package is flattened into its enclosing frame",
                        oak_span(&package.span),
                    ));
                }
                let mut nested = ancestors.to_vec();
                nested.push(cluster);
                collect_components(graph, &package.elements, &nested, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn component_label(node: &ComponentNode, interface: bool) -> String {
    let name = node
        .display
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| node.name.trim());
    if interface { format!("«interface» {name}") } else { name.to_string() }
}

fn package_label(package: &ComponentPackage) -> String {
    package
        .display
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| package.name.trim())
        .to_string()
}

fn port_direction(direction: PortDirection) -> &'static str {
    match direction {
        PortDirection::InOut => "port",
        PortDirection::In => "portin",
        PortDirection::Out => "portout",
    }
}

/// Refuse the first authored line that the family builder drops.
fn refuse_dropped(source: &str, keywords: &[&str], skip_braced: bool, family: &str) -> Result<(), DiagramError> {
    if let Some((range, keyword)) = locate_dropped_statement(source, keywords, skip_braced) {
        return Err(DiagramError::unsupported(format!(
            "{family} statement `{keyword}` is not lowered, the diagram falls back to its source"
        ))
        .with_span(range));
    }
    Ok(())
}

/// The first authored state stereotype `oak-plantuml` does not model.
///
/// The builder maps only the fork, join, choice, start, end, entry, and exit
/// stereotypes and silently downgrades the rest to a plain state, so an unknown
/// `<<Name>>` is refused before that loss reaches the spec.
fn locate_unsupported_stereotype(source: &str) -> Option<(Range<usize>, String)> {
    const SUPPORTED: &[&str] = &["fork", "join", "choice", "start", "end", "entrypoint", "entry", "exitpoint", "exit"];
    for (range, line) in source_lines(source) {
        if line.starts_with('\'') || line.starts_with("/'") {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find("<<") {
            let after = &rest[open + 2..];
            let Some(close) = after.find(">>")
            else {
                break;
            };
            let token = after[..close].trim().to_ascii_lowercase();
            if !SUPPORTED.contains(&token.as_str()) {
                return Some((range, token));
            }
            rest = &after[close + 2..];
        }
    }
    None
}

/// The first authored statement that the frontend parses and then discards.
///
/// The statement token is the first word of a trimmed line with a trailing `:`
/// removed. When `skip_braced` is set, lines inside `{ ... }` blocks are
/// ignored because a class member may start with the same word. Each hit
/// reports the trimmed line range, so the refusal names authored bytes instead
/// of an AST position the builder does not keep. Comments are skipped so a
/// commented-out statement is never refused.
fn locate_dropped_statement(source: &str, keywords: &[&str], skip_braced: bool) -> Option<(Range<usize>, String)> {
    let mut depth = 0usize;
    let mut in_block_comment = false;
    for (range, line) in source_lines(source) {
        if in_block_comment {
            if line.contains("'/") {
                in_block_comment = false;
            }
            continue;
        }
        if line.starts_with("/'") {
            in_block_comment = !line[2..].contains("'/");
            continue;
        }
        if line.is_empty() || line.starts_with('\'') {
            continue;
        }
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

/// Every non-empty line with its trimmed byte range.
fn source_lines(source: &str) -> Vec<(Range<usize>, &str)> {
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

fn oak_span(span: &oak_core::Range<usize>) -> Range<usize> {
    span.start..span.end
}

fn require_id(id: &str) -> Result<String, DiagramError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(DiagramError::unsupported("diagram contains a node or edge endpoint with an empty id"));
    }
    Ok(trimmed.to_string())
}

fn folded_label(left: Option<&String>, right: Option<&String>, label: Option<&String>) -> Option<String> {
    let mut parts = Vec::new();
    for part in [left, right, label].into_iter().flatten() {
        let part = part.trim();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn class_label(class: &ClassDeclaration, source: &str) -> String {
    let authored = authored_declaration(class, source);
    let declared = declared_identifier(&authored).unwrap_or_else(|| class.name.trim().to_string());
    let display = class.display.as_deref().map(str::trim).filter(|display| !display.is_empty());
    let name = display.unwrap_or(declared.as_str());
    let mut label = match class.keyword {
        ClassKeyword::Class => name.to_string(),
        ClassKeyword::Interface => format!("«interface» {name}"),
        ClassKeyword::Abstract => format!("«abstract» {name}"),
        ClassKeyword::Enum => format!("«enum» {name}"),
    };
    for stereotype in authored_stereotypes(&authored) {
        label.push('\n');
        label.push('«');
        label.push_str(&stereotype);
        label.push('»');
    }
    let mut members: Vec<String> = class.members.iter().map(member_text).filter(|text| !text.is_empty()).collect();
    if members.is_empty() {
        members = authored_members(&authored);
    }
    for text in members {
        label.push('\n');
        label.push_str(&text);
    }
    label
}

/// The authored bytes of one class declaration.
///
/// `oak-plantuml` drops a generic parameter list, a user stereotype, and an
/// `enum` body from the AST node, so the label is rebuilt from the declaration
/// span rather than losing authored text that PlantUML draws inside the box.
fn authored_declaration(class: &ClassDeclaration, source: &str) -> String {
    let range: Range<usize> = class.span.clone().into();
    source.get(range).unwrap_or_default().to_string()
}

/// The declared identifier, including a generic parameter list when present.
fn declared_identifier(authored: &str) -> Option<String> {
    let header = authored.lines().next()?.trim();
    let rest = header.strip_prefix("abstract").map(str::trim_start).unwrap_or(header);
    let (_, after_keyword) = rest.split_once(char::is_whitespace)?;
    let token = after_keyword.trim_start().split_whitespace().next()?.trim_end_matches('{');
    (!token.is_empty()).then(|| token.to_string())
}

/// Every `<<Name>>` token on the declaration header.
fn authored_stereotypes(authored: &str) -> Vec<String> {
    let header = authored.lines().next().unwrap_or_default();
    let mut stereotypes = Vec::new();
    let mut rest = header;
    while let Some(open) = rest.find("<<") {
        let after = &rest[open + 2..];
        let Some(close) = after.find(">>")
        else {
            break;
        };
        let token = after[..close].trim();
        if !token.is_empty() {
            stereotypes.push(token.to_string());
        }
        rest = &after[close + 2..];
    }
    stereotypes
}

/// Author-declared body lines, used when the AST keeps no member entry.
///
/// `enum Color { RED GREEN }` reaches the AST with an empty member list, so the
/// authored constants are read back; separator lines such as `--` are layout,
/// not members, and are skipped.
fn authored_members(authored: &str) -> Vec<String> {
    let Some((_, body)) = authored.split_once('{')
    else {
        return Vec::new();
    };
    let body = body.rsplit_once('}').map(|(body, _)| body).unwrap_or(body);
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.chars().all(|character| matches!(character, '-' | '.' | '=' | '#')))
        .map(str::to_string)
        .collect()
}

/// Rebuild the authored member line, including the visibility marker.
///
/// The Mermaid class lowering keeps the marker in the member text, so keeping
/// it here lets both frontends produce the same node label for the same class.
fn member_text(member: &ClassMember) -> String {
    let marker = match member.visibility {
        MemberVisibility::Public => '+',
        MemberVisibility::Private => '-',
        MemberVisibility::Protected => '#',
        MemberVisibility::Package => '~',
    };
    let text = member.text.trim();
    if text.is_empty() { String::new() } else { format!("{marker}{text}") }
}

/// Fold multiplicities and the authored label into the one edge label.
fn relation_label(relation: &ClassRelation) -> Option<String> {
    folded_label(
        relation.left_multiplicity.as_ref(),
        relation.right_multiplicity.as_ref(),
        relation.label.as_ref(),
    )
}

fn class_edge_style(kind: &ClassRelationKind) -> EdgeStyle {
    match kind {
        ClassRelationKind::Association => EdgeStyle::Arrow,
        ClassRelationKind::Inheritance => EdgeStyle::HollowArrow,
        ClassRelationKind::Composition => EdgeStyle::FilledDiamond,
        ClassRelationKind::Aggregation => EdgeStyle::HollowDiamond,
        ClassRelationKind::Dependency => EdgeStyle::DottedArrow,
        ClassRelationKind::Link => EdgeStyle::Line,
    }
}

fn message_style(arrow: &MessageArrow) -> EdgeStyle {
    match arrow {
        MessageArrow::Solid => EdgeStyle::Line,
        MessageArrow::Dotted => EdgeStyle::DottedLine,
        MessageArrow::SolidArrow => EdgeStyle::Arrow,
        MessageArrow::DottedArrow => EdgeStyle::DottedArrow,
    }
}

fn approximated_at(message: impl Into<String>, span: Range<usize>) -> Diagnostic {
    let message = message.into();
    let record = diagnostic("diagram", codes::APPROXIMATED, DiagnosticSeverity::Warning, message.clone());
    let Ok(source) = SourceRef::new(NAMESPACE, SOURCE)
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

/// Reject everything outside the accepted PlantUML subset with the line span.
///
/// `oak-plantuml` keeps reusable tokens generic, so a rejected directive would
/// otherwise reach the AST as identifiers and lose the authored range. Scanning
/// lines before the builder runs is the same strategy the DOT frontend uses for
/// HTML labels and is not a second grammar.
fn reject_unsupported(source: &str) -> Result<(), DiagramError> {
    let mut start: Option<Range<usize>> = None;
    let mut end: Option<Range<usize>> = None;
    let mut in_block_comment = false;
    let mut offset = 0;

    for raw in source.split_inclusive('\n') {
        let trimmed_start = raw.len() - raw.trim_start().len();
        let line_start = offset + trimmed_start;
        let line_end = offset + raw.trim_end().len();
        let line = raw.trim();
        offset += raw.len();

        if in_block_comment {
            in_block_comment = !line.contains("'/");
            continue;
        }
        if line.starts_with("/'") {
            in_block_comment = !line[2..].contains("'/");
            continue;
        }
        if line.is_empty() || line.starts_with('\'') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("@start") {
            if lower.starts_with("@startuml") {
                let rest = &lower["@startuml".len()..];
                if !rest.is_empty() && !rest.starts_with(char::is_whitespace) && !rest.starts_with('(') {
                    return Err(DiagramError::unsupported(format!("unknown PlantUML marker `{line}`"))
                        .with_span(line_start..line_end));
                }
                if start.is_some() {
                    return Err(DiagramError::unsupported("a PlantUML source declares more than one `@startuml`")
                        .with_span(line_start..line_end));
                }
                start = Some(line_start..line_end);
                continue;
            }
            return Err(DiagramError::unsupported("only `@startuml` is supported, other PlantUML start markers are refused")
                .with_span(line_start..line_end));
        }
        if lower.starts_with("@enduml") {
            if end.is_some() {
                return Err(DiagramError::unsupported("a PlantUML source declares more than one `@enduml`")
                    .with_span(line_start..line_end));
            }
            end = Some(line_start..line_end);
            continue;
        }
        if lower.starts_with('@') {
            return Err(DiagramError::unsupported(format!("unknown PlantUML marker `{line}`")).with_span(line_start..line_end));
        }
        if start.is_none() {
            return Err(DiagramError::unsupported("PlantUML content must be wrapped by `@startuml` and `@enduml`")
                .with_span(line_start..line_end));
        }
        if end.is_some() {
            return Err(DiagramError::unsupported("PlantUML content must not follow `@enduml`").with_span(line_start..line_end));
        }
        if line.starts_with('!') {
            return Err(DiagramError::unsupported(
                "PlantUML preprocessor directives such as `!include` and `!define` are refused",
            )
            .with_span(line_start..line_end));
        }
        let keyword = first_word(&lower);
        if OTHER_DIAGRAM_KEYWORDS.contains(&keyword.as_str()) {
            return Err(DiagramError::unsupported(format!(
                "PlantUML `{keyword}` diagrams carry no AST and are refused, the class, sequence, activity, state, and component families are lowered"
            ))
            .with_span(line_start..line_end));
        }
    }

    let Some(start) = start
    else {
        return Err(DiagramError::unsupported("PlantUML source is missing `@startuml`"));
    };
    if end.is_none() {
        return Err(DiagramError::unsupported("PlantUML source is missing `@enduml`").with_span(start));
    }
    Ok(())
}

fn first_word(line: &str) -> String {
    line.split(|character: char| character.is_whitespace() || character == '{' || character == '(')
        .next()
        .unwrap_or("")
        .to_string()
}
