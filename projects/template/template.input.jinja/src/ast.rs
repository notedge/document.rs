use serde_json::Value;

/// One parsed template node.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    Text(String),
    Output { expression: Expr },
    If { sections: Vec<(Expr, Vec<Node>)>, otherwise: Vec<Node> },
    For { target: String, source: Expr, body: Vec<Node>, otherwise: Vec<Node> },
    Set { name: String, value: Expr },
    Include { reference: String },
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
    Name(String),
    Attr { base: Box<Expr>, key: String },
    Index { base: Box<Expr>, key: Box<Expr> },
    Unary { op: UnaryOp, operand: Box<Expr> },
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Filter { input: Box<Expr>, name: String, args: Vec<Expr> },
}

/// Unary operator.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UnaryOp {
    Not,
    Negate,
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
}
