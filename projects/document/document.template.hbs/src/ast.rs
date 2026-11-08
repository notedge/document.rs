use serde_json::Value;

/// One parsed template node.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Text(String),
    Output { expression: Expr, escape: bool },
    If { condition: Expr, invert: bool, positive: Vec<Node>, negative: Vec<Node> },
    Each { source: Expr, body: Vec<Node>, otherwise: Vec<Node> },
    With { source: Expr, body: Vec<Node>, otherwise: Vec<Node> },
    Partial { reference: String },
}

/// One expression, with its source text kept for diagnostics.
#[derive(Debug, Clone)]
pub(crate) struct Expr {
    pub(crate) text: String,
    pub(crate) kind: ExprKind,
}

/// Expression shape.
#[derive(Debug, Clone)]
pub(crate) enum ExprKind {
    Literal(Value),
    Path(PathExpr),
}

/// A handlebars path such as `a.b`, `../a`, `@root.a`, or `@index`.
#[derive(Debug, Clone)]
pub(crate) struct PathExpr {
    pub(crate) base: PathBase,
    pub(crate) steps: Vec<String>,
}

/// Path base.
#[derive(Debug, Clone)]
pub(crate) enum PathBase {
    Root,
    Current(usize),
    Data(String),
}
