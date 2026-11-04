//! Math-list / noad normalization layer.
//!
//! TeX's math engine lowers the syntax tree into a flat list of **noads**,
//! each carrying a role (the 8 spacing classes from Appendix G), an optional
//! nucleus, sub/superscripts, and the math style that governs it.  VuTeX's
//! [`FormulaNode`] tree currently mixes syntax (grouping), semantic role,
//! style commands and layout hints in one enum, which makes the spacing,
//! demotion and style-propagation rules hard to reason about locally.
//!
//! This module introduces the **skeleton** of that normalization:
//!
//! * [`NoadKind`] — the 8 Appendix G classes plus a few structural kinds.
//! * [`Noad`] — a single normalized atom with nucleus, scripts, style and
//!   a `cramped` flag that TeX uses for radicals and denominators.
//! * [`MathItem`] — one slot of the list: an atom or an explicit glue item.
//! * [`MathList`] — a newtype over `Vec<MathItem>`.
//! * [`MathStyleState`] — the recursive `(MathStyle, cramped)` pair that
//!   descends into subtrees.
//! * [`normalize_math_list`] — flatten transparent groups, demote `Bin` atoms
//!   per TeX Appendix G and merge consecutive text atoms in one left-to-right
//!   sweep.
//! * [`From<&[FormulaNode]>` for `MathList`] — a shallow projector that
//!   lowers a FormulaNode sequence into a MathList.
//!
//! The layout engine consumes this normalization: every math list it lays out
//! is projected and normalized first, so inter-atom spacing only ever reads an
//! already-resolved [`NoadKind`] and never re-derives a class from the source
//! [`SymbolRole`].  There is no second demotion rule anywhere downstream.

use serde::{Deserialize, Serialize};

use super::{
    formula::{Fixed, FontStyle, FormulaNode, MathStyle, SymbolRole},
    operator::LimitsPolicy,
};

/// TeX Appendix G spacing classes + structural extensions.
///
/// The 8 classes control which glue TeX inserts between two adjacent atoms
/// (Ordinary, Binary, Relation, Large Operator, Closing, Opening,
/// Punctuation, Inner).  [`NoadKind::Fence`], [`NoadKind::Radical`],
/// [`NoadKind::Vcent`], [`NoadKind::TableCell`] and
/// [`NoadKind::Environment`] carry additional structural information that the
/// spacing table does not need to know about; they sit alongside the 8 base
/// classes because every noad has exactly one kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NoadKind {
    /// Ordinary atom (the default for most symbols and text).
    Atom,
    /// Binary operator like `+`, `-`, `\times`.
    Bin,
    /// Relation like `=`, `<`, `\subset`.
    Rel,
    /// Large operator like `\sum`, `\int`.
    Op,
    /// Closing delimiter like `)`, `]`, `\rangle`.
    Close,
    /// Opening delimiter like `(`, `[`, `\langle`.
    Open,
    /// Punctuation like `,`, `;`.
    Punct,
    /// Inner atom produced by `\left…\right`, fractions, etc.
    Inner,
    /// Structural kind: `\left…\right` fence pair.
    Fence,
    /// Structural kind: `\sqrt`, `\root n of`.
    Radical,
    /// Structural kind: `\vcenter` box.
    Vcent,
    /// Structural kind: matrix cell.
    TableCell,
    /// Structural kind: an AMS environment block (`aligned`, `align`,
    /// `gather`, `multline`, `split`, `equation`).
    ///
    /// The block is one atom of the surrounding list — its rows, anchors and
    /// numbering stay inside the nucleus — and takes Inner spacing like the
    /// other structural containers.
    Environment,
}

impl NoadKind {
    /// True if this class participates in the Appendix G 9×9 spacing table.
    ///
    /// Fence / Radical / Vcent / TableCell are structural containers; the
    /// spacing engine treats them as `Inner` for the purpose of inter-atom
    /// glue, which is what [`spacing_class`] returns.
    pub fn is_table_class(self) -> bool {
        matches!(
            self,
            NoadKind::Atom
                | NoadKind::Bin
                | NoadKind::Rel
                | NoadKind::Op
                | NoadKind::Close
                | NoadKind::Open
                | NoadKind::Punct
                | NoadKind::Inner
        )
    }

    /// Collapse structural kinds to their effective spacing class.
    ///
    /// TeX treats fences, radicals and `\vcenter` boxes as Inner for glue
    /// purposes.  Table cells are flattened away before spacing is applied
    /// so they also resolve to Inner.
    pub fn spacing_class(self) -> NoadKind {
        if self.is_table_class() { self } else { NoadKind::Inner }
    }

    /// Map from [`SymbolRole`] to the base spacing kind.
    pub fn from_role(role: SymbolRole) -> Self {
        match role {
            SymbolRole::Ordinary => NoadKind::Atom,
            SymbolRole::Operator | SymbolRole::LargeOperator => NoadKind::Op,
            SymbolRole::Binary => NoadKind::Bin,
            SymbolRole::Relation => NoadKind::Rel,
            SymbolRole::Opening => NoadKind::Open,
            SymbolRole::Closing => NoadKind::Close,
            SymbolRole::Punctuation => NoadKind::Punct,
            SymbolRole::Inner => NoadKind::Inner,
        }
    }
}

/// A single normalized math atom.
///
/// Every noad has exactly one [`NoadKind`] plus an optional nucleus, an
/// optional sub/superscript pair, the math style that governs glyph metrics
/// and placement, and TeX's `cramped` flag (raised inside radicals, in
/// fraction denominators and in subscripts, inherited everywhere else).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Noad {
    /// The spacing / structural class for this noad.
    pub kind: NoadKind,
    /// The nucleus — a shallow reference to the original [`FormulaNode`]
    /// subtree that produced this noad, so downstream consumers still have
    /// full access to the parsed form before a deeper nucleus enum replaces
    /// it.
    pub nucleus: Option<Box<FormulaNode>>,
    /// Attached sub/superscripts, if any.
    pub scripts: Option<Scripts>,
    /// An explicit `\limits` / `\nolimits` override, if any.  Only meaningful
    /// when `kind == NoadKind::Op`.
    pub limits: Option<LimitsPolicy>,
    /// Math style that applies to this noad's own nucleus.
    pub style: MathStyle,
    /// TeX `cramped` flag for the style that governs this noad's nucleus.
    /// Raised inside radicals, in fraction denominators and in subscripts.
    pub cramped: bool,
    /// True when this `Open` / `Close` noad is one half of a `\left…\right`
    /// pair rather than a literal delimiter typed into the source.
    ///
    /// Pairs stay two noads so the Appendix G table still sees an Open and a
    /// Close atom, but the layout engine must size both delimiters against the
    /// whole body between them, the way TeX does.  A literal `(` or `)` is not
    /// marked and keeps its own glyph design at any body size.
    #[serde(default)]
    pub fence: bool,
}

/// Sub- and/or superscript attached to a [`Noad`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scripts {
    pub sub: Option<Box<FormulaNode>>,
    pub sup: Option<Box<FormulaNode>>,
}

impl Noad {
    /// Constructor for the most common case: a plain atom of the given kind.
    pub fn new(kind: NoadKind, style: MathStyle, cramped: bool) -> Self {
        Self { kind, nucleus: None, scripts: None, limits: None, style, cramped, fence: false }
    }

    /// True when this noad should participate in Appendix G spacing glue.
    pub fn has_spacing_class(&self) -> bool {
        self.kind.is_table_class()
    }

    /// Effective spacing kind after structural resolution.
    pub fn effective_kind(&self) -> NoadKind {
        self.kind.spacing_class()
    }

    /// True when this noad's nucleus is a `FormulaNode::Text` (not a
    /// Symbol, not any other variant).
    ///
    /// Text noads come from `FormulaNode::Text { text, font_style }` while
    /// Symbol noads come from `FormulaNode::Symbol { glyph_key, role }`.
    /// We only merge adjacent Text atoms — Symbol atoms (even when both
    /// happen to be Ordinary) must remain distinct because TeX keeps a
    /// demoted `Bin→Atom` as its own spacing entity.
    pub fn is_text_atom(&self) -> bool {
        matches!(self.nucleus.as_deref(), Some(FormulaNode::Text { .. }))
    }

    /// The font face of this noad's nucleus when it is a
    /// [`FormulaNode::Text`], and `None` for every other nucleus.
    ///
    /// The face is part of a Text atom's identity: `\text{a}` and `\textbf{b}`
    /// are two atoms, not one, because the bold run must keep its own face.
    pub fn text_font_style(&self) -> Option<FontStyle> {
        match self.nucleus.as_deref() {
            Some(FormulaNode::Text { font_style, .. }) => Some(*font_style),
            _ => None,
        }
    }
}

/// One slot of a [`MathList`]: an atom, or explicit glue between atoms.
///
/// TeX's math list is not a pure atom sequence.  `\,`, `\:`, `\;`, `\ `,
/// `\quad` and `\qquad` are `\mskip` glue items: they advance the line without
/// joining the atom sequence, so they neither occupy a slot of the Appendix G
/// spacing table nor change which atoms are neighbours.  Keeping them out of
/// [`Noad`] is what makes the two contributions independent — the spacing
/// table reads the atoms on either side, the glue adds only its own width.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MathItem {
    /// A spacing atom.
    Noad(Noad),
    /// Explicit glue inserted between atoms.
    Glue(MathGlue),
}

impl MathItem {
    /// The atom this item carries, if it is one.
    pub fn as_noad(&self) -> Option<&Noad> {
        match self {
            MathItem::Noad(noad) => Some(noad),
            MathItem::Glue(_) => None,
        }
    }
}

/// Explicit glue `\,` and friends insert into a math list.
///
/// The width is stored in em of the math style that wrote it (`\,` = 3mu =
/// 3/18 em), so layout scales it by that style's em exactly the way a kern in
/// the same style would be scaled.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MathGlue {
    /// Width in em of [`style`](Self::style).
    pub width: Fixed,
    /// Math style whose em the width is measured in.
    pub style: MathStyle,
}

/// A flat, normalized math list: atoms interleaved with explicit glue.
///
/// The newtype exists so downstream crates can depend on a named type rather
/// than `Vec<MathItem>` everywhere, and so we can attach helper methods and
/// trait implementations to it without polluting `Vec`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct MathList(pub Vec<MathItem>);

impl MathList {
    /// Create an empty list.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Number of items in the list, explicit glue included.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when the list holds neither atoms nor glue.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over every item, explicit glue included.
    pub fn iter(&self) -> std::slice::Iter<'_, MathItem> {
        self.0.iter()
    }

    /// Iterate over the atoms, skipping the glue between them.
    pub fn noads(&self) -> impl Iterator<Item = &Noad> {
        self.0.iter().filter_map(MathItem::as_noad)
    }

    /// Number of atoms in the list, explicit glue excluded.
    pub fn noad_count(&self) -> usize {
        self.noads().count()
    }

    /// Append an item to the end.
    pub fn push(&mut self, item: MathItem) {
        self.0.push(item);
    }

    /// Append an atom to the end.
    pub fn push_noad(&mut self, noad: Noad) {
        self.0.push(MathItem::Noad(noad));
    }

    /// Append explicit glue to the end.
    pub fn push_glue(&mut self, glue: MathGlue) {
        self.0.push(MathItem::Glue(glue));
    }
}

impl FromIterator<MathItem> for MathList {
    fn from_iter<T: IntoIterator<Item = MathItem>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for MathList {
    type Item = MathItem;
    type IntoIter = std::vec::IntoIter<MathItem>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Recursive style state: `(MathStyle, cramped)`.
///
/// TeX's `mlist_to_hlist` tracks the current math style together with the
/// `cramped` flag.  `cramped` is not derivable from `MathStyle` alone: it is
/// raised by the structure we enter (fraction denominator, radical radicand,
/// subscript) and inherited by structures that keep the parent's crampedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathStyleState {
    pub style: MathStyle,
    pub cramped: bool,
}

impl MathStyleState {
    /// Default: Display, not cramped.
    pub const DISPLAY: Self = Self { style: MathStyle::Display, cramped: false };

    /// Display style inside a radical or a fraction denominator.
    pub const DISPLAY_CRAMPED: Self = Self { style: MathStyle::Display, cramped: true };

    /// Create a state from any outer math style, not cramped.
    pub const fn from_style(style: MathStyle) -> Self {
        Self { style, cramped: false }
    }

    /// Push into a subscript.
    ///
    /// TeX demotes the style one script level (display/text → script,
    /// script/scriptscript → scriptscript) and sets `cramped`: scripts take
    /// the *cramped* shifts so the subscript hugs the base instead of keeping
    /// the uncramped ascender clearance.
    pub fn sub(self) -> Self {
        Self { style: demote_script(self.style), cramped: true }
    }

    /// Push into a superscript.
    ///
    /// Style demotes like [`sub`](Self::sub), but `cramped` keeps the parent's
    /// value — nothing in a superscript raises it.
    pub fn sup(self) -> Self {
        Self { style: demote_script(self.style), cramped: self.cramped }
    }

    /// Fraction numerator.
    ///
    /// TeX steps the style down one level (display → text, text → script,
    /// script → scriptscript, scriptscript stays put) and keeps the parent's
    /// `cramped` flag.
    pub fn frac_num(self) -> Self {
        Self { style: demote_text(self.style), cramped: self.cramped }
    }

    /// Fraction denominator: the same step as [`frac_num`](Self::frac_num),
    /// but always cramped, which is why fractions use the *cramped* shifts.
    pub fn frac_den(self) -> Self {
        Self { style: demote_text(self.style), cramped: true }
    }

    /// Enter a radical's radicand: style unchanged, cramped forced.
    pub fn cramp(self) -> Self {
        Self { style: self.style, cramped: true }
    }
}

/// One script level down: display and text both land in script style.
fn demote_script(s: MathStyle) -> MathStyle {
    match s {
        MathStyle::Display | MathStyle::Text => MathStyle::Script,
        MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
    }
}

/// One *text* level down, the step fractions use: display → text, then
/// text → script, then script → scriptscript.
fn demote_text(s: MathStyle) -> MathStyle {
    match s {
        MathStyle::Display => MathStyle::Text,
        MathStyle::Text => MathStyle::Script,
        MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
    }
}

/// Shallow projection from a `FormulaNode` slice into a [`MathList`].
///
/// This is intentionally a **first pass**: we flatten `Sequence` groups,
/// honour `SetMathStyle` style switches, and project individual atoms to
/// their [`NoadKind`].  Fractions, radicals, delimiters, matrices and
/// other structural nodes still produce a single `Inner` noad whose
/// nucleus points back to the original `FormulaNode` — a deeper nucleus
/// decomposition will come later.
///
/// Transparent flattening rules:
///
/// * `FormulaNode::Sequence` — descend, append children (flat).
/// * `FormulaNode::SetMathStyle(s)` — push `s` as the current style for
///   subsequent noads.  Does **not** itself emit a noad.  Each `Sequence` is
///   a style scope: the style in force at its end is restored when the group
///   closes, so `\scriptstyle` inside `{…}` cannot leak to later siblings.
/// * `FormulaNode::LimitsSpec(policy)` — sets `limits` on the next `Op` noad.
///
/// `base_style` is the outer formula context math style (Display / Text /
/// Script / ScriptScript).  Imperative nodes still override `state.style`
/// in place, so subsequent noads carry the correct per-node style.
pub fn project_formula(nodes: &[FormulaNode], base_style: MathStyle) -> MathList {
    project_formula_with_state(nodes, MathStyleState::from_style(base_style))
}

/// Project from an already-known recursive style state.
///
/// Layout uses this entry point when it enters a structure that pins `cramped`
/// (a fraction denominator, a radical radicand or a sequence body); the noads
/// then carry the flag so their own scripts and nested structures inherit it.
pub fn project_formula_with_state(nodes: &[FormulaNode], initial: MathStyleState) -> MathList {
    let mut state = initial;
    let mut list = MathList::new();
    let mut pending_limits: Option<LimitsPolicy> = None;

    for node in nodes {
        project_one(node, &mut state, &mut list, &mut pending_limits);
    }

    list
}

fn project_one(node: &FormulaNode, state: &mut MathStyleState, list: &mut MathList, pending_limits: &mut Option<LimitsPolicy>) {
    match node {
        FormulaNode::Sequence(children) => {
            // A braced group is a style scope; `\scriptstyle` and friends
            // inside it must not leak to the siblings after the group.
            let outer = *state;
            for c in children {
                project_one(c, state, list, pending_limits);
            }
            *state = outer;
        }
        FormulaNode::SetMathStyle(s) => {
            state.style = *s;
        }
        FormulaNode::LimitsSpec(policy) => {
            *pending_limits = Some(*policy);
        }
        FormulaNode::Symbol { role, .. } => {
            let kind = NoadKind::from_role(*role);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        // A number is one ordinary atom: the same spacing class a digit symbol
        // carries, so Appendix G reads it as `Ord` whatever its digit count.
        FormulaNode::Number { .. } | FormulaNode::Text { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Space { width } => {
            // Explicit TeX math spaces (`\,`, `\:`, `\;`, `\ `, `\quad`,
            // `\qquad`) are `\mskip` glue, not atoms: they advance the line by
            // their own width and leave the Appendix G table to the atoms on
            // either side.  `\limits` describes an atom, so a queued policy
            // cannot attach to glue and must not survive it either.
            *pending_limits = None;
            list.push_glue(MathGlue { width: *width, style: state.style });
        }
        FormulaNode::Subscript { base, sub } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.sub();
            noad.scripts = Some(Scripts { sub: Some(Box::new(with_style(sub, sub_state.style))), sup: None });
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Superscript { base, sup } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sup_state = state.sup();
            noad.scripts = Some(Scripts { sub: None, sup: Some(Box::new(with_style(sup, sup_state.style))) });
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::SubSuperscript { base, sub, sup } => {
            let kind = kind_of_node(base);
            let mut noad = Noad::new(kind, state.style, state.cramped);
            noad.nucleus = Some(base.clone());
            let sub_state = state.sub();
            let sup_state = state.sup();
            noad.scripts = Some(Scripts {
                sub: Some(Box::new(with_style(sub, sub_state.style))),
                sup: Some(Box::new(with_style(sup, sup_state.style))),
            });
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Fraction { num, den, line } => {
            // The fraction itself is an Inner atom in the current style, but
            // TeX sets its numerator and denominator one text level down, and
            // the denominator cramped.  Stamping the derived styles into the
            // nucleus keeps the nested scripts on that state instead of the
            // outer one.
            let num_state = state.frac_num();
            let den_state = state.frac_den();
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(FormulaNode::Fraction {
                num: Box::new(with_style(num, num_state.style)),
                den: Box::new(with_style(den, den_state.style)),
                line: *line,
            }));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Radical { .. } => {
            // A radical's argument is typeset cramped at the current style.
            let inner = state.cramp();
            let mut noad = Noad::new(NoadKind::Radical, inner.style, inner.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Delimited { left, right, content, left_span, right_span } => {
            // A `\left…\right` pair is one fence split into its two spacing
            // atoms.  Every pair produces both atoms, even when one side is a
            // null delimiter (`\left.` / `\right.`) or an environment supplies
            // only its opening brace (`cases`): the missing half is a
            // zero-width Open / Close atom without a nucleus, not a missing
            // fence.  That is what lets the layout engine pair the halves back
            // up and size the delimiters against the body, which the flattened
            // list no longer exposes on its own.  Emitting only the side that
            // carries a glyph left `cases` with a text-size brace that never
            // covered its two rows.
            // Left delimiter → Open noad with a synthetic Symbol nucleus
            // so the spacing engine can read its glyph for metrics.
            let mut open = Noad::new(NoadKind::Open, state.style, state.cramped);
            if let Some(gk) = left {
                open.nucleus =
                    Some(Box::new(FormulaNode::Symbol { glyph_key: gk.clone(), role: SymbolRole::Opening, span: *left_span }));
            }
            open.fence = true;
            push_noad(list, open, pending_limits);

            // Content → recursive projection, preserving its internal spacing
            // classes (Atom / Bin / Rel / ...).  When the body is completely
            // empty (e.g. `\left(\right)`), TeX still emits an Inner atom so
            // the fence pair participates in spacing; we replicate that here.
            let before = list.len();
            project_one(content, state, list, pending_limits);
            if list.len() == before {
                push_noad(list, Noad::new(NoadKind::Inner, state.style, state.cramped), pending_limits);
            }

            // Right delimiter → Close noad, or the null `\right.` close.
            let mut close = Noad::new(NoadKind::Close, state.style, state.cramped);
            if let Some(gk) = right {
                close.nucleus =
                    Some(Box::new(FormulaNode::Symbol { glyph_key: gk.clone(), role: SymbolRole::Closing, span: *right_span }));
            }
            close.fence = true;
            push_noad(list, close, pending_limits);
        }
        FormulaNode::Accent { .. } => {
            let mut noad = Noad::new(NoadKind::Atom, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::OverSet { .. } | FormulaNode::UnderSet { .. } => {
            // A stacked expression is one Inner atom: TeX treats composed
            // atoms like `\overset` / `\underset` as Inner for Appendix G
            // spacing purposes, so spacing flows around the whole block.
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Relation(inner) => {
            // `\mathrel` / `\stackrel` — force Relation spacing on the body.
            let mut noad = Noad::new(NoadKind::Rel, state.style, state.cramped);
            noad.nucleus = Some(inner.clone());
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::HorizontalBox { content, .. } | FormulaNode::Paragraph { content, .. } => {
            // Flattened: descend and emit children as if they were in a Sequence.
            for c in content {
                project_one(c, state, list, pending_limits);
            }
        }
        FormulaNode::VerticalBox { content, .. } => {
            // Vertical box is Inner.
            let mut noad = Noad::new(NoadKind::Inner, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
            let _ = content; // deeper decomposition deferred
        }
        FormulaNode::Matrix { .. } => {
            let mut noad = Noad::new(NoadKind::TableCell, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
        FormulaNode::Environment(_) => {
            // An AMS environment is one Inner atom of the surrounding list: its
            // rows, alignment anchors and numbering intent stay inside the
            // nucleus so the block still knows what kind of environment it is.
            let mut noad = Noad::new(NoadKind::Environment, state.style, state.cramped);
            noad.nucleus = Some(Box::new(node.clone()));
            push_noad(list, noad, pending_limits);
        }
    }
}

/// Append `noad`, binding the `\limits` / `\nolimits` policy queued before it.
///
/// `\limits` describes the atom it follows, so the queued policy is consumed
/// here whatever that atom is.  It is only stored when the atom is an
/// operator: a policy authored after an ordinary atom is reported by the
/// parser as a misuse and must not travel on to the next operator.
fn push_noad(list: &mut MathList, mut noad: Noad, pending_limits: &mut Option<LimitsPolicy>) {
    let policy = pending_limits.take();
    if noad.kind == NoadKind::Op {
        noad.limits = policy;
    }
    list.push_noad(noad);
}

/// Extract the [`NoadKind`] a formula node contributes to the flat math
/// list.  Mirrors [`LayoutEngine::role_of`] but only covers the projector's
/// own needs: Symbols map through `NoadKind::from_role`, everything else
/// defaults to Atom.  Fractions / radicals / delimiters are handled by
/// their own `project_one` arms and never reach here.
fn kind_of_node(node: &FormulaNode) -> NoadKind {
    match node {
        FormulaNode::Symbol { role, .. } => NoadKind::from_role(*role),
        _ => NoadKind::Atom,
    }
}

/// Wrap a node with a matching `SetMathStyle` when the target style differs
/// from the node's natural style.  In the shallow projection the sub/sup
/// nodes still live as raw `FormulaNode`s so we push the style imperatively
/// in front of them.
fn with_style(node: &FormulaNode, target: MathStyle) -> FormulaNode {
    // For the first pass we don't introspect inside the sub/sup subtree —
    // the style command at the head is a hint for the layout engine.
    FormulaNode::Sequence(vec![FormulaNode::SetMathStyle(target), node.clone()])
}

/// Normalize a projected [`MathList`] with TeX Appendix G passes.
///
/// The list interleaves atoms with explicit glue.  Every pass below reads atoms
/// as the neighbours of atoms and leaves glue where the author wrote it:
///
/// 1. **Boundary trim** — strip leading `Open`/`Close`/`Punct` atoms
///    that have no nucleus (bare grouping atoms with no glyph of their
///    own).  Fence pairs emitted by the `Delimited` projector carry a
///    proper `Symbol` nucleus for their delimiters and are therefore
///    preserved; the trim only catches artifact atoms from future
///    sources that have neither a glyph nor a sub/superscript.  Glue at
///    either end is authored spacing and stays.
/// 2. **`Bin` demotion** — downgrade binary-operator atoms per TeXbook
///    Chapter 17 / Appendix G, evaluated in one left-to-right pass (each noad
///    sees its original neighbours; TeX does not re-resolve).  Glue in between
///    two atoms does not hide them from each other: TeX resolves the class from
///    the surrounding atoms, so `a=\,\;+b` demotes `+` exactly like `a=+b`
///    does rather than treating the space as an atom neighbour.  `Bin` is the
///    only class TeX converts: relations keep their `Rel` class and operators
///    keep their `Op` class whatever surrounds them, so a relation never turns
///    into a binary operator and a large operator is never downgraded to
///    `Ord`.
/// 3. **Consecutive Atom merge** — fold adjacent `Atom` atoms into a
///    single one; later scripts override earlier ones.  Glue breaks the run:
///    `\text{a}\,\text{b}` stays two words.
pub fn normalize_math_list(mut list: MathList) -> MathList {
    // ── Pass 1: boundary trim ────────────────────────────────────────
    trim_boundary(&mut list);

    // ── Pass 2: Bin demote per TeXbook Chapter 17 ─────────────────────
    let v = demote_pass(&list.0);

    // ── Pass 3: consecutive **Text** Atom merge ───────────────────────
    // Only merges Text-derived atoms; Symbol atoms stay distinct (TeX
    // semantics — a demoted Bin→Atom remains its own spacing entity).
    let v = merge_atoms_pass(v);

    MathList(v)
}

// ────────────────────────────────────────────────────────────────────────
// Pass 1 — boundary trim
// ────────────────────────────────────────────────────────────────────────

/// Strip leading and trailing `Open`/`Close`/`Punct` atoms that have no
/// nucleus.
///
/// Open/Close atoms at the boundaries never have a useful neighbour on
/// the outer side, so their spacing contribution is zero.  Bare atoms of
/// those kinds that carry no glyph (no nucleus) and no fence are pure
/// artifacts and can be dropped safely.  Atoms projected from a
/// [`FormulaNode::Delimited`] call themselves a fence and always stay,
/// glyph or not: the layout engine pairs the two halves to size the
/// delimiter.  Explicit glue is never a placeholder: `\,x` really is
/// spaced, so the glue at either end stays.
fn trim_boundary(list: &mut MathList) {
    /// True when the atom is a bare grouping artifact with nothing to set.
    ///
    /// A fence half is never an artifact even when it carries no glyph: the
    /// null side of `\left.` / `\right.` and the missing close of `cases` are
    /// zero-width atoms the layout engine pairs with their opposite half to
    /// size the delimiter, so trimming them would leave the surviving half
    /// unpaired and drawn at text size.
    fn droppable(noad: &Noad) -> bool {
        noad.nucleus.is_none() && !noad.fence && matches!(noad.kind, NoadKind::Open | NoadKind::Close | NoadKind::Punct)
    }

    // Trim leading.
    while let Some(index) = list.0.iter().position(|item| item.as_noad().is_some()) {
        if list.0[index].as_noad().is_some_and(droppable) {
            list.0.remove(index);
        }
        else {
            break;
        }
    }
    // Trim trailing.
    while let Some(index) = list.0.iter().rposition(|item| item.as_noad().is_some()) {
        if list.0[index].as_noad().is_some_and(droppable) {
            list.0.remove(index);
        }
        else {
            break;
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Pass 2 — demote
// ────────────────────────────────────────────────────────────────────────

/// TeXbook Chapter 17 demote pass — one left-to-right sweep.
///
/// Each atom's **left** neighbour is read from the already-demoted `out`
/// list (so a `Bin→Atom` demotion is what the next atom sees), while the
/// **right** neighbour is still the raw `input`.  This is the single place
/// where atom classes are resolved: downstream spacing reads the resulting
/// [`NoadKind`] and never re-applies these rules.
///
/// Both neighbours are read through [`adjacent_kind`], which skips the glue
/// items: TeX's math list holds `\mskip` items, and `mlist_to_hlist` walks past
/// them to find the next atom, so an explicit space can neither shield a `Bin`
/// from a cancelling neighbour nor create one.
///
/// Only `Bin` is converted.  TeX leaves `Rel` and `Op` alone: `(` followed by
/// `=` keeps a relation (it never becomes a binary operator) and `∑` followed
/// by a closing delimiter is still a large operator, so its scripts keep
/// stacking and its Appendix G glue stays that of an `Op`.
///
/// Neighbours are read via [`Noad::effective_kind`] so structural kinds
/// (`Fence`, `Radical`, `Vcent`, `TableCell`) collapse to `Inner` before
/// the demote rules fire — they participate as an Inner neighbour exactly
/// the way TeX treats fences and radicals for spacing.
fn demote_pass(input: &[MathItem]) -> Vec<MathItem> {
    let mut out: Vec<MathItem> = Vec::with_capacity(input.len());
    let mut left: Option<NoadKind> = None;
    for (i, item) in input.iter().enumerate() {
        let MathItem::Noad(noad) = item
        else {
            // Glue keeps its place; it is not an atom and never a neighbour.
            out.push(item.clone());
            continue;
        };
        // right = still the original input; TeX evaluates demotion rules
        // before any neighbour has changed.
        let right = adjacent_kind(input, i + 1);
        let mut noad = noad.clone();
        if noad.kind == NoadKind::Bin && bin_demotes(left, right) {
            noad.kind = NoadKind::Atom;
        }
        left = Some(noad.effective_kind());
        out.push(MathItem::Noad(noad));
    }
    out
}

/// Effective spacing class of the first atom at or after `from`, skipping glue.
fn adjacent_kind(items: &[MathItem], from: usize) -> Option<NoadKind> {
    items[from..].iter().find_map(|item| item.as_noad().map(Noad::effective_kind))
}

/// TeXbook Chapter 17 — a `Bin` that cannot bind becomes `Ord`.
///
/// Demotes when the **left** neighbour is "strong" enough to absorb the
/// binary operator (or Bin is first), **or** the **right** neighbour is
/// likewise strong enough (or Bin is last).
///
/// ```text
/// Bin → Atom if:
///   first (no left)  or left ∈ {Bin, Open, Rel, Op, Punct}
///   OR
///   last  (no right) or right ∈ {Rel, Close, Punct}
/// ```
///
/// `Close` is *not* a Bin-left canceller and `Open` is *not* a Bin-right
/// canceller — `(a+b)` still keeps its Bin with thick glue on both sides.
fn bin_demotes(left: Option<NoadKind>, right: Option<NoadKind>) -> bool {
    use NoadKind::*;
    let left_strong = matches!(left, None | Some(Bin | Open | Rel | Op | Punct));
    let right_strong = matches!(right, None | Some(Rel | Close | Punct));
    left_strong || right_strong
}

// ────────────────────────────────────────────────────────────────────────
// Pass 3 — merge consecutive Text atoms only
// ────────────────────────────────────────────────────────────────────────

/// Fold adjacent **Text-derived** Atom atoms into a single Atom.
///
/// TeX does **not** merge Symbol atoms even if both sides happen to be
/// Ordinary — each remains a distinct atom with its own glyph and spacing
/// context.  In particular, a Bin demoted to Atom still participates in
/// spacing as a separate Ordinary atom.  But multiple consecutive `Text`
/// fragments (e.g. a word split into several `FormulaNode::Text` during
/// markdown import) should fold into a single atom so the layout engine
/// sees one "word" instead of N single-letter atoms with zero glue between
/// them.
///
/// Explicit glue breaks the run: `\text{a}\,\text{b}` is two words separated by
/// authored space, not one word whose letters happen to be spaced.
///
/// Two Text atoms are merged only when:
/// * both are `NoadKind::Atom`,
/// * both carry a `FormulaNode::Text` nucleus (never Symbol),
/// * they carry the same `FontStyle` face (a bold run never folds into the
///   plain run before it),
/// * they share the same `MathStyle` and `cramped` flag,
/// * neither carries sub/superscripts (scripts break the run),
/// * nothing — in particular no glue — sits between them.
fn merge_atoms_pass(input: Vec<MathItem>) -> Vec<MathItem> {
    let mut out: Vec<MathItem> = Vec::with_capacity(input.len());
    for item in input {
        let noad = match item {
            MathItem::Glue(glue) => {
                out.push(MathItem::Glue(glue));
                continue;
            }
            MathItem::Noad(noad) => noad,
        };
        let mergeable = |last: &Noad| {
            last.kind == NoadKind::Atom
                && noad.kind == NoadKind::Atom
                && last.is_text_atom()
                && noad.is_text_atom()
                && last.text_font_style() == noad.text_font_style()
                && last.style == noad.style
                && last.cramped == noad.cramped
                && last.scripts.is_none()
                && noad.scripts.is_none()
        };
        match out.last_mut() {
            Some(MathItem::Noad(last)) if mergeable(last) => {
                last.nucleus = Some(merge_text_nodes(last.nucleus.as_ref(), noad.nucleus.as_ref()));
            }
            _ => out.push(MathItem::Noad(noad)),
        }
    }
    out
}

/// Concatenate two Text nuclei into a single `FormulaNode::Text`.
///
/// Only succeeds when **both** are `FormulaNode::Text`; otherwise returns
/// `b` verbatim as a safe fallback (the caller gate keeps Symbol/other
/// nuclei out of this branch, so we only hit the fallback in defensive
/// paths).  `font_style` is taken from `a` — the merge gate already requires
/// both fragments to share the face, so the concatenation keeps it.
fn merge_text_nodes(a: Option<&Box<FormulaNode>>, b: Option<&Box<FormulaNode>>) -> Box<FormulaNode> {
    let a_text = a.and_then(|x| match x.as_ref() {
        FormulaNode::Text { text, font_style, span } => Some((text.clone(), *font_style, *span)),
        _ => None,
    });
    let b_text = b.and_then(|x| match x.as_ref() {
        FormulaNode::Text { text, span, .. } => Some((text.clone(), *span)),
        _ => None,
    });
    match (a_text, b_text) {
        (Some((ta, fa, sa)), Some((tb, sb))) => {
            // The merged run keeps a source span only when both fragments
            // carry contiguous ones; otherwise the concatenation no longer
            // maps to one authored range.
            let span = match (sa, sb) {
                (Some(left), Some(right)) if left.end == right.start => {
                    Some(crate::diagnostic::SourceSpan { start: left.start, end: right.end })
                }
                _ => None,
            };
            Box::new(FormulaNode::Text { text: format!("{ta}{tb}"), font_style: fa, span })
        }
        // Defensive fallback: keep b (caller already filtered).
        (_, Some(_)) => b.unwrap().clone(),
        (Some(_), None) => a.unwrap().clone(),
        (None, None) => {
            Box::new(FormulaNode::Text { text: String::new(), font_style: crate::math::FontStyle::Math, span: None })
        }
    }
}

// ---------------------------------------------------------------------------
// From<&[FormulaNode]> for MathList
// ---------------------------------------------------------------------------

impl From<&[FormulaNode]> for MathList {
    fn from(nodes: &[FormulaNode]) -> Self {
        project_formula(nodes, MathStyle::Display)
    }
}

impl From<&Vec<FormulaNode>> for MathList {
    fn from(nodes: &Vec<FormulaNode>) -> Self {
        project_formula(nodes, MathStyle::Display)
    }
}
