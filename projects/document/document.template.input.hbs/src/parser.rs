use document_template::{TemplateError, codes};
use serde_json::Value;

use crate::{
    ast::{Expr, ExprKind, Node, PathBase, PathExpr},
    codes as hbs_codes,
};

const BLOCKS: [&str; 4] = ["if", "unless", "each", "with"];

#[derive(Debug)]
enum Token {
    Text(String),
    Comment,
    Output { expression: Expr, escape: bool },
    BlockOpen { name: String, expression: Expr },
    BlockClose { name: String },
    Else(Option<Expr>),
    Partial(String),
}

#[derive(Debug)]
enum Frame {
    If { invert: bool, sections: Vec<(Expr, Vec<Node>)>, active: Option<Expr>, in_else: bool },
    Each { source: Expr, body: Vec<Node>, in_else: bool },
    With { source: Expr, body: Vec<Node>, in_else: bool },
}

/// Parse one Handlebars source into a node tree.
pub(crate) fn parse(source: &str) -> Result<Vec<Node>, TemplateError> {
    build_tree(tokenize(source)?)
}

fn parse_failed(message: impl Into<String>) -> TemplateError {
    TemplateError::new(codes::PARSE_FAILED, message)
}

fn unsupported_construct(construct: &str, message: impl Into<String>) -> TemplateError {
    TemplateError::with_args(hbs_codes::UNSUPPORTED_CONSTRUCT, message, vec![("construct", construct.to_string())])
}

fn tokenize(source: &str) -> Result<Vec<Token>, TemplateError> {
    let mut tokens = Vec::new();
    let mut rest = source;
    let mut pending_trim = false;
    while let Some(start) = rest.find("{{") {
        let (text, tail) = rest.split_at(start);
        push_text(&mut tokens, text, &mut pending_trim);
        let after_open = &tail[2..];
        let (trim_left, after_open) = match after_open.strip_prefix('~') {
            Some(stripped) => (true, stripped),
            None => (false, after_open),
        };
        if trim_left {
            trim_last_text(&mut tokens);
        }
        let (token, trim_right, next) = parse_tag(after_open)?;
        tokens.push(token);
        pending_trim = trim_right;
        rest = next;
    }
    push_text(&mut tokens, rest, &mut pending_trim);
    Ok(tokens)
}

fn parse_tag<'a>(after_open: &'a str) -> Result<(Token, bool, &'a str), TemplateError> {
    if let Some(body) = after_open.strip_prefix('{') {
        let (content, rest) = split_tag(body, "}}}")?;
        let (content, trim_right) = strip_trailing_trim(content);
        return Ok((Token::Output { expression: parse_expression(content)?, escape: false }, trim_right, rest));
    }
    if let Some(comment) = after_open.strip_prefix("!--") {
        let (_, rest) = split_tag(comment, "--}}")?;
        return Ok((Token::Comment, false, rest));
    }
    if let Some(comment) = after_open.strip_prefix('!') {
        let (content, rest) = split_tag(comment, "}}")?;
        let (_, trim_right) = strip_trailing_trim(content);
        return Ok((Token::Comment, trim_right, rest));
    }
    if let Some(partial) = after_open.strip_prefix('>') {
        let (content, rest) = split_tag(partial, "}}")?;
        let (content, trim_right) = strip_trailing_trim(content);
        let reference = content.trim();
        if reference.is_empty() {
            return Err(parse_failed("partial reference is empty"));
        }
        if reference.chars().any(char::is_whitespace) {
            return Err(unsupported_construct(
                "partial-arguments",
                format!("partial `{reference}` uses arguments or hash, which v0 does not support"),
            ));
        }
        return Ok((Token::Partial(reference.to_string()), trim_right, rest));
    }
    if after_open.starts_with('&') {
        return Err(unsupported_construct("ampersand-escape", "`{{&expr}}` is not supported in v0"));
    }

    let (content, rest) = split_tag(after_open, "}}")?;
    let (content, trim_right) = strip_trailing_trim(content);
    let content = content.trim();
    if content.is_empty() {
        return Err(parse_failed("empty tag"));
    }
    if let Some(block) = content.strip_prefix('#') {
        let (name, expression) = split_block(block)?;
        if !BLOCKS.contains(&name) {
            return Err(TemplateError::with_args(
                hbs_codes::UNSUPPORTED_BLOCK,
                format!("block `{name}` is not supported in v0"),
                vec![("block", name.to_string())],
            ));
        }
        return Ok((Token::BlockOpen { name: name.to_string(), expression }, trim_right, rest));
    }
    if let Some(close) = content.strip_prefix('/') {
        let name = close.trim();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(parse_failed(format!("invalid block close `{content}`")));
        }
        return Ok((Token::BlockClose { name: name.to_string() }, trim_right, rest));
    }
    if content == "else" {
        return Ok((Token::Else(None), trim_right, rest));
    }
    if let Some(expression) = content.strip_prefix("else if") {
        if expression.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
            return Err(unsupported_construct("else-form", format!("`{content}` is not a supported else form")));
        }
        return Ok((Token::Else(Some(parse_expression(expression)?)), trim_right, rest));
    }
    if content.starts_with("else") {
        return Err(unsupported_construct("else-form", format!("`{content}` is not a supported else form")));
    }
    if content.starts_with('^') {
        return Err(unsupported_construct("inverse-section", "`{{^}}` inverse sections are not supported in v0"));
    }
    Ok((Token::Output { expression: parse_expression(content)?, escape: true }, trim_right, rest))
}

fn split_block(block: &str) -> Result<(&str, Expr), TemplateError> {
    let block = block.trim();
    let name_end = block.find(char::is_whitespace).unwrap_or(block.len());
    let name = &block[..name_end];
    let raw = &block[name_end..];
    if name.is_empty() {
        return Err(parse_failed("block name is empty"));
    }
    Ok((name, parse_expression(raw)?))
}

fn split_tag<'a>(text: &'a str, close: &str) -> Result<(&'a str, &'a str), TemplateError> {
    let Some(index) = text.find(close)
    else {
        return Err(parse_failed(format!("unterminated tag, expected `{close}`")));
    };
    Ok((&text[..index], &text[index + close.len()..]))
}

fn strip_trailing_trim(content: &str) -> (&str, bool) {
    match content.strip_suffix('~') {
        Some(stripped) => (stripped, true),
        None => (content, false),
    }
}

fn push_text(tokens: &mut Vec<Token>, text: &str, pending_trim: &mut bool) {
    if text.is_empty() {
        return;
    }
    let text = if *pending_trim { text.trim_start() } else { text };
    *pending_trim = false;
    if !text.is_empty() {
        tokens.push(Token::Text(text.to_string()));
    }
}

fn trim_last_text(tokens: &mut Vec<Token>) {
    if let Some(Token::Text(text)) = tokens.last_mut() {
        text.truncate(text.trim_end().len());
    }
}

fn parse_expression(raw: &str) -> Result<Expr, TemplateError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(parse_failed("expected an expression"));
    }
    let first = text.chars().next().unwrap_or_default();
    if first == '"' || first == '\'' {
        let (value, used) = parse_string_literal(text)?;
        if !text[used..].trim().is_empty() {
            return Err(unsupported_helper(text));
        }
        return Ok(Expr { text: text.to_string(), kind: ExprKind::Literal(Value::String(value)) });
    }
    if text.split_whitespace().count() > 1 {
        return Err(unsupported_helper(text));
    }
    match text {
        "true" => return Ok(literal(text, Value::Bool(true))),
        "false" => return Ok(literal(text, Value::Bool(false))),
        "null" | "undefined" => return Ok(literal(text, Value::Null)),
        _ => {}
    }
    if let Ok(value) = text.parse::<i64>() {
        return Ok(literal(text, Value::from(value)));
    }
    if let Ok(value) = text.parse::<f64>() {
        let Some(number) = serde_json::Number::from_f64(value)
        else {
            return Err(parse_failed(format!("`{text}` is not a finite number")));
        };
        return Ok(literal(text, Value::Number(number)));
    }
    parse_path(text).map(|path| Expr { text: text.to_string(), kind: ExprKind::Path(path) })
}

fn literal(text: &str, value: Value) -> Expr {
    Expr { text: text.to_string(), kind: ExprKind::Literal(value) }
}

fn unsupported_helper(text: &str) -> TemplateError {
    TemplateError::with_args(
        hbs_codes::UNSUPPORTED_HELPER,
        format!("helper call `{text}` is not supported in v0"),
        vec![("helper", text.to_string())],
    )
}

fn parse_string_literal(text: &str) -> Result<(String, usize), TemplateError> {
    let quote = text.chars().next().unwrap_or_default();
    let mut output = String::new();
    let mut cursor = text.char_indices().skip(1);
    while let Some((index, character)) = cursor.next() {
        if character == '\\' {
            let Some((_, escaped)) = cursor.next()
            else {
                return Err(parse_failed("unterminated string literal"));
            };
            output.push(match escaped {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
        }
        else if character == quote {
            return Ok((output, index + character.len_utf8()));
        }
        else {
            output.push(character);
        }
    }
    Err(parse_failed("unterminated string literal"))
}

fn parse_path(text: &str) -> Result<PathExpr, TemplateError> {
    if text == "this" || text == "." {
        return Ok(PathExpr { base: PathBase::Current(0), steps: Vec::new() });
    }
    let (base, remainder) = if let Some(rest) = text.strip_prefix('@') {
        if let Some(after_root) = rest.strip_prefix("root") {
            (PathBase::Root, after_root)
        }
        else {
            if rest.is_empty() || rest.contains('.') {
                return Err(parse_failed(format!("invalid data path `{text}`")));
            }
            return Ok(PathExpr { base: PathBase::Data(rest.to_string()), steps: Vec::new() });
        }
    }
    else {
        let mut remainder = text;
        let mut up = 0;
        while let Some(after) = remainder.strip_prefix("../") {
            up += 1;
            remainder = after;
        }
        if let Some(after) = remainder.strip_prefix("./") {
            remainder = after;
        }
        (PathBase::Current(up), remainder)
    };

    let remainder = remainder.strip_prefix('.').unwrap_or(remainder);
    let mut steps = Vec::new();
    if !remainder.is_empty() {
        for segment in remainder.split('.') {
            if segment.is_empty() || segment.contains(['[', ']', '"', '\'']) {
                return Err(parse_failed(format!("invalid path `{text}`")));
            }
            steps.push(segment.to_string());
        }
    }
    Ok(PathExpr { base, steps })
}

fn build_tree(tokens: Vec<Token>) -> Result<Vec<Node>, TemplateError> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    for token in tokens {
        match token {
            Token::Text(text) => current.push(Node::Text(text)),
            Token::Comment => {}
            Token::Output { expression, escape } => current.push(Node::Output { expression, escape }),
            Token::Partial(reference) => current.push(Node::Partial { reference }),
            Token::BlockOpen { name, expression } => {
                frames.push(match name.as_str() {
                    "if" => Frame::If { invert: false, sections: Vec::new(), active: Some(expression), in_else: false },
                    "unless" => Frame::If { invert: true, sections: Vec::new(), active: Some(expression), in_else: false },
                    "each" => Frame::Each { source: expression, body: Vec::new(), in_else: false },
                    "with" => Frame::With { source: expression, body: Vec::new(), in_else: false },
                    other => {
                        return Err(TemplateError::with_args(
                            hbs_codes::UNSUPPORTED_BLOCK,
                            format!("block `{other}` is not supported in v0"),
                            vec![("block", other.to_string())],
                        ));
                    }
                });
            }
            Token::Else(expression) => else_branch(&mut frames, &mut current, expression)?,
            Token::BlockClose { name } => {
                let frame = frames.pop().ok_or_else(|| parse_failed(format!("unexpected close `{{{{/{name}}}}}`")))?;
                let node = close_frame(frame, &name, std::mem::take(&mut current))?;
                current.push(node);
            }
        }
    }
    if !frames.is_empty() {
        return Err(parse_failed("unclosed block"));
    }
    Ok(current)
}

fn else_branch(frames: &mut Vec<Frame>, current: &mut Vec<Node>, expression: Option<Expr>) -> Result<(), TemplateError> {
    let Some(frame) = frames.last_mut()
    else {
        return Err(parse_failed("`else` outside a block"));
    };
    match frame {
        Frame::If { sections, active, in_else, .. } => {
            if *in_else {
                return Err(parse_failed("block has more than one final `else`"));
            }
            let Some(condition) = active.take()
            else {
                return Err(parse_failed("`else` follows another `else`"));
            };
            sections.push((condition, std::mem::take(current)));
            match expression {
                Some(expression) => *active = Some(expression),
                None => *in_else = true,
            }
        }
        Frame::Each { body, in_else, .. } | Frame::With { body, in_else, .. } => {
            if expression.is_some() {
                return Err(unsupported_construct("else-if", "`else if` is only valid on `if` and `unless` blocks"));
            }
            if *in_else {
                return Err(parse_failed("block has more than one `else`"));
            }
            *body = std::mem::take(current);
            *in_else = true;
        }
    }
    Ok(())
}

fn close_frame(frame: Frame, name: &str, final_nodes: Vec<Node>) -> Result<Node, TemplateError> {
    match frame {
        Frame::If { invert, sections, active, in_else } => {
            let expected = if invert { "unless" } else { "if" };
            if name != expected {
                return Err(parse_failed(format!("close `{{{{/{name}}}}}` does not match `{expected}`")));
            }
            let mut sections = sections;
            let mut tail = Vec::new();
            if in_else {
                tail = final_nodes;
            }
            else if let Some(condition) = active {
                sections.push((condition, final_nodes));
            }
            for (index, (condition, positive)) in sections.into_iter().enumerate().rev() {
                let is_first = index == 0;
                tail = vec![Node::If { condition, invert: invert && is_first, positive, negative: tail }];
            }
            tail.pop().ok_or_else(|| parse_failed("`if` block has no section"))
        }
        Frame::Each { source, body, in_else } => {
            if name != "each" {
                return Err(parse_failed(format!("close `{{{{/{name}}}}}` does not match `each`")));
            }
            let (body, otherwise) = if in_else { (body, final_nodes) } else { (final_nodes, Vec::new()) };
            Ok(Node::Each { source, body, otherwise })
        }
        Frame::With { source, body, in_else } => {
            if name != "with" {
                return Err(parse_failed(format!("close `{{{{/{name}}}}}` does not match `with`")));
            }
            let (body, otherwise) = if in_else { (body, final_nodes) } else { (final_nodes, Vec::new()) };
            Ok(Node::With { source, body, otherwise })
        }
    }
}
