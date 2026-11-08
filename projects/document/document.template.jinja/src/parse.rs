use document_template::{TemplateError, codes};
use serde_json::{Number, Value};

use crate::{
    ast::{BinaryOp, Expr, ExprKind, Node, UnaryOp},
    codes as jinja_codes,
};

#[derive(Debug)]
enum Token {
    Text(String),
    Output(String),
    Tag(String),
}

#[derive(Debug)]
enum Frame {
    If { sections: Vec<(Expr, Vec<Node>)>, active: Option<Expr>, in_else: bool },
    For { target: String, source: Expr, body: Vec<Node>, in_else: bool },
}

/// Parse one Jinja source into a node tree.
pub(crate) fn parse(source: &str) -> Result<Vec<Node>, TemplateError> {
    build_tree(tokenize(source)?)
}

fn parse_failed(message: impl Into<String>) -> TemplateError {
    TemplateError::new(codes::PARSE_FAILED, message)
}

fn unsupported_construct(construct: &str, message: impl Into<String>) -> TemplateError {
    TemplateError::with_args(jinja_codes::UNSUPPORTED_CONSTRUCT, message, vec![("construct", construct.to_string())])
}

fn unsupported_tag(tag: &str) -> TemplateError {
    TemplateError::with_args(
        jinja_codes::UNSUPPORTED_TAG,
        format!("tag `{tag}` is not supported in v0"),
        vec![("construct", tag.to_string())],
    )
}

fn tokenize(source: &str) -> Result<Vec<Token>, TemplateError> {
    let mut tokens = Vec::new();
    let mut rest = source;
    let mut pending_trim = false;
    loop {
        let Some((index, open)) = find_open(rest)
        else {
            push_text(&mut tokens, rest, &mut pending_trim);
            break;
        };
        let (text, tail) = rest.split_at(index);
        push_text(&mut tokens, text, &mut pending_trim);
        let body = &tail[2..];
        let (trim_left, body) = strip_left_marker(body);
        if trim_left {
            trim_last_text(&mut tokens);
        }
        let (content, trim_right, next) = match open {
            "{{" => split_delim(body, "}}")?,
            "{%" => split_delim(body, "%}")?,
            _ => split_delim(body, "#}")?,
        };
        match open {
            "{{" => tokens.push(Token::Output(content.trim().to_string())),
            "{%" => tokens.push(Token::Tag(content.trim().to_string())),
            _ => {}
        }
        pending_trim = trim_right;
        rest = next;
    }
    Ok(tokens)
}

fn find_open(text: &str) -> Option<(usize, &'static str)> {
    [
        text.find("{{").map(|index| (index, "{{")),
        text.find("{%").map(|index| (index, "{%")),
        text.find("{#").map(|index| (index, "{#")),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(index, _)| *index)
}

fn strip_left_marker(body: &str) -> (bool, &str) {
    if let Some(stripped) = body.strip_prefix('-') {
        (true, stripped)
    }
    else if let Some(stripped) = body.strip_prefix('+') {
        (false, stripped)
    }
    else {
        (false, body)
    }
}

fn split_delim<'a>(body: &'a str, close: &str) -> Result<(&'a str, bool, &'a str), TemplateError> {
    let Some(index) = body.find(close)
    else {
        return Err(parse_failed(format!("unterminated tag, expected `{close}`")));
    };
    let mut content = &body[..index];
    let mut trim_right = false;
    if let Some(stripped) = content.strip_suffix('-') {
        trim_right = true;
        content = stripped;
    }
    else if let Some(stripped) = content.strip_suffix('+') {
        content = stripped;
    }
    Ok((content, trim_right, &body[index + close.len()..]))
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

fn split_head(content: &str) -> (&str, &str) {
    let content = content.trim();
    let end = content.find(char::is_whitespace).unwrap_or(content.len());
    (&content[..end], content[end..].trim_start())
}

fn build_tree(tokens: Vec<Token>) -> Result<Vec<Node>, TemplateError> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    for token in tokens {
        match token {
            Token::Text(text) => current.push(Node::Text(text)),
            Token::Output(content) => current.push(Node::Output { expression: parse_expression(&content)? }),
            Token::Tag(content) => tag(&content, &mut frames, &mut current)?,
        }
    }
    if !frames.is_empty() {
        return Err(parse_failed("unclosed tag"));
    }
    Ok(current)
}

fn tag(content: &str, frames: &mut Vec<Frame>, current: &mut Vec<Node>) -> Result<(), TemplateError> {
    let (name, rest) = split_head(content);
    match name {
        "" => Err(parse_failed("empty tag")),
        "if" => {
            if rest.is_empty() {
                return Err(parse_failed("`if` requires a condition"));
            }
            frames.push(Frame::If { sections: Vec::new(), active: Some(parse_expression(rest)?), in_else: false });
            Ok(())
        }
        "elif" => {
            if rest.is_empty() {
                return Err(parse_failed("`elif` requires a condition"));
            }
            let Some(frame) = frames.last_mut()
            else {
                return Err(parse_failed("`elif` outside an `if`"));
            };
            let Frame::If { sections, active, in_else } = frame
            else {
                return Err(parse_failed("`elif` outside an `if`"));
            };
            if *in_else {
                return Err(parse_failed("`elif` after `else`"));
            }
            let Some(previous) = active.take()
            else {
                return Err(parse_failed("`elif` has no preceding condition"));
            };
            sections.push((previous, std::mem::take(current)));
            *active = Some(parse_expression(rest)?);
            Ok(())
        }
        "else" => {
            if !rest.is_empty() {
                return Err(parse_failed("`else` takes no expression"));
            }
            let Some(frame) = frames.last_mut()
            else {
                return Err(parse_failed("`else` outside a block"));
            };
            match frame {
                Frame::If { sections, active, in_else } => {
                    if *in_else {
                        return Err(parse_failed("block has more than one `else`"));
                    }
                    let Some(previous) = active.take()
                    else {
                        return Err(parse_failed("`else` has no preceding condition"));
                    };
                    sections.push((previous, std::mem::take(current)));
                    *in_else = true;
                }
                Frame::For { body, in_else, .. } => {
                    if *in_else {
                        return Err(parse_failed("block has more than one `else`"));
                    }
                    *body = std::mem::take(current);
                    *in_else = true;
                }
            }
            Ok(())
        }
        "endif" => {
            if !rest.is_empty() {
                return Err(parse_failed("`endif` takes no expression"));
            }
            let Some(frame) = frames.pop()
            else {
                return Err(parse_failed("`endif` outside an `if`"));
            };
            let Frame::If { sections, active, in_else } = frame
            else {
                return Err(parse_failed("`endif` does not close an `if`"));
            };
            let tail = std::mem::take(current);
            current.push(close_if(sections, active, in_else, tail)?);
            Ok(())
        }
        "for" => {
            let (target, source) = parse_for(rest)?;
            frames.push(Frame::For { target, source, body: Vec::new(), in_else: false });
            Ok(())
        }
        "endfor" => {
            if !rest.is_empty() {
                return Err(parse_failed("`endfor` takes no expression"));
            }
            let Some(frame) = frames.pop()
            else {
                return Err(parse_failed("`endfor` outside a `for`"));
            };
            let Frame::For { target, source, body, in_else } = frame
            else {
                return Err(parse_failed("`endfor` does not close a `for`"));
            };
            let (body, otherwise) =
                if in_else { (body, std::mem::take(current)) } else { (std::mem::take(current), Vec::new()) };
            current.push(Node::For { target, source, body, otherwise });
            Ok(())
        }
        "set" => {
            let (name, value) = parse_set(rest)?;
            current.push(Node::Set { name, value });
            Ok(())
        }
        "include" => {
            let reference = parse_include(rest)?;
            current.push(Node::Include { reference });
            Ok(())
        }
        other => Err(unsupported_tag(other)),
    }
}

fn close_if(
    sections: Vec<(Expr, Vec<Node>)>,
    active: Option<Expr>,
    in_else: bool,
    final_nodes: Vec<Node>,
) -> Result<Node, TemplateError> {
    let mut sections = sections;
    let otherwise = if in_else {
        final_nodes
    }
    else {
        let Some(condition) = active
        else {
            return Err(parse_failed("`if` block has no condition"));
        };
        sections.push((condition, final_nodes));
        Vec::new()
    };
    Ok(Node::If { sections, otherwise })
}

fn parse_for(body: &str) -> Result<(String, Expr), TemplateError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(parse_failed("`for` requires `name in expression`"));
    }
    let (target, rest) = split_head(body);
    if target.contains([',', '(', ')', '[', ']']) {
        return Err(unsupported_construct(
            "destructuring",
            format!("`for {target} in …` destructuring is not supported in v0"),
        ));
    }
    if !target.chars().all(|character| character.is_alphanumeric() || character == '_') || target.is_empty() {
        return Err(parse_failed(format!("invalid `for` target `{target}`")));
    }
    let Some(after_in) = rest.strip_prefix("in")
    else {
        return Err(parse_failed("`for` requires `in` after the target"));
    };
    if after_in.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
        return Err(parse_failed("`for` requires whitespace after `in`"));
    }
    let source = after_in.trim();
    if source.is_empty() {
        return Err(parse_failed("`for` requires a source expression"));
    }
    let mut parser = ExprParser::new(source)?;
    let expression = parser.parse_or()?;
    if parser.index < parser.tokens.len() {
        if matches!(parser.tokens.get(parser.index), Some(Lex::Ident(name)) if name == "if") {
            return Err(unsupported_construct("loop-filter", "`for ... if ...` loop filters are not supported in v0"));
        }
        parser.expect_end()?;
    }
    Ok((target.to_string(), expression))
}

fn parse_set(body: &str) -> Result<(String, Expr), TemplateError> {
    let body = body.trim();
    let end = body.find(|character: char| !(character.is_alphanumeric() || character == '_')).unwrap_or(body.len());
    let name = &body[..end];
    if name.is_empty() {
        return Err(parse_failed("`set` requires a name"));
    }
    let rest = body[end..].trim_start();
    if rest.starts_with(',') {
        return Err(unsupported_construct("set-destructuring", "`set` destructuring is not supported in v0"));
    }
    let Some(rest) = rest.strip_prefix('=')
    else {
        return Err(parse_failed("`set` requires `name = expression`"));
    };
    Ok((name.to_string(), parse_expression(rest)?))
}

fn parse_include(body: &str) -> Result<String, TemplateError> {
    if body.trim().is_empty() {
        return Err(parse_failed("`include` requires a reference"));
    }
    match parse_expression(body)?.kind {
        ExprKind::Literal(Value::String(reference)) => Ok(reference),
        _ => Err(unsupported_construct("include-expression", "`include` requires a string literal in v0")),
    }
}

/// One expression token.
#[derive(Debug, Clone)]
enum Lex {
    Ident(String),
    Number(Value),
    Str(String),
    Op(&'static str),
}

impl Lex {
    fn render(&self) -> String {
        match self {
            Lex::Ident(name) => name.clone(),
            Lex::Number(value) => value.to_string(),
            Lex::Str(value) => format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
            Lex::Op(op) => (*op).to_string(),
        }
    }
}

fn lex_expression(raw: &str) -> Result<Vec<Lex>, TemplateError> {
    let mut tokens = Vec::new();
    let mut characters = raw.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character.is_whitespace() {
            continue;
        }
        if character == '"' || character == '\'' {
            let value = lex_string(raw, index, &mut characters)?;
            tokens.push(Lex::Str(value));
            continue;
        }
        if character.is_ascii_digit() {
            let (value, end) = lex_number(raw, index)?;
            for _ in raw[index + 1..end].chars() {
                characters.next();
            }
            tokens.push(Lex::Number(value));
            continue;
        }
        if character.is_alphanumeric() || character == '_' {
            let mut end = index + character.len_utf8();
            while let Some((position, next)) = characters.peek().copied() {
                if next.is_alphanumeric() || next == '_' {
                    end = position + next.len_utf8();
                    characters.next();
                }
                else {
                    break;
                }
            }
            tokens.push(Lex::Ident(raw[index..end].to_string()));
            continue;
        }
        let two = raw.get(index..index + 2);
        let op = match two {
            Some("==") => {
                characters.next();
                "=="
            }
            Some("!=") => {
                characters.next();
                "!="
            }
            Some("<=") => {
                characters.next();
                "<="
            }
            Some(">=") => {
                characters.next();
                ">="
            }
            _ => match character {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '%' => "%",
                '~' => "~",
                '|' => "|",
                '(' => "(",
                ')' => ")",
                '[' => "[",
                ']' => "]",
                '.' => ".",
                ',' => ",",
                '=' => "=",
                '<' => "<",
                '>' => ">",
                other => return Err(parse_failed(format!("unexpected character `{other}` in an expression"))),
            },
        };
        tokens.push(Lex::Op(op));
    }
    Ok(tokens)
}

fn lex_string(
    raw: &str,
    start: usize,
    characters: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, TemplateError> {
    let quote = raw[start..].chars().next().unwrap_or_default();
    let mut output = String::new();
    while let Some((_, character)) = characters.next() {
        if character == '\\' {
            let Some((_, escaped)) = characters.next()
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
            return Ok(output);
        }
        else {
            output.push(character);
        }
    }
    Err(parse_failed("unterminated string literal"))
}

fn lex_number(raw: &str, start: usize) -> Result<(Value, usize), TemplateError> {
    let bytes = raw.as_bytes();
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end + 1 < bytes.len() && bytes[end] == b'.' && bytes[end + 1].is_ascii_digit() {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    let text = &raw[start..end];
    if text.contains('.') {
        let value = text.parse::<f64>().map_err(|_| parse_failed(format!("`{text}` is not a number")))?;
        let Some(number) = Number::from_f64(value)
        else {
            return Err(parse_failed(format!("`{text}` is not a finite number")));
        };
        Ok((Value::Number(number), end))
    }
    else {
        let value = text.parse::<i64>().map_err(|_| parse_failed(format!("`{text}` is not an integer")))?;
        Ok((Value::from(value), end))
    }
}

fn parse_expression(raw: &str) -> Result<Expr, TemplateError> {
    ExprParser::new(raw)?.parse_all()
}

struct ExprParser {
    tokens: Vec<Lex>,
    index: usize,
}

impl ExprParser {
    fn new(raw: &str) -> Result<Self, TemplateError> {
        let tokens = lex_expression(raw)?;
        if tokens.is_empty() {
            return Err(parse_failed("expected an expression"));
        }
        Ok(Self { tokens, index: 0 })
    }

    fn parse_all(mut self) -> Result<Expr, TemplateError> {
        let expression = self.parse_or()?;
        self.expect_end()?;
        Ok(expression)
    }

    fn expect_end(&self) -> Result<(), TemplateError> {
        if self.index == self.tokens.len() {
            return Ok(());
        }
        if self.peek_ident("is") {
            return Err(unsupported_construct("is-test", "`is` tests are not supported in v0"));
        }
        Err(parse_failed(format!("unexpected token `{}`", self.tokens[self.index].render())))
    }

    fn peek(&self) -> Option<&Lex> {
        self.tokens.get(self.index)
    }

    fn peek_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Lex::Ident(current)) if current == name)
    }

    fn take_ident(&mut self, name: &str) -> bool {
        if self.peek_ident(name) {
            self.index += 1;
            true
        }
        else {
            false
        }
    }

    fn take_op(&mut self, op: &str) -> bool {
        if matches!(self.peek(), Some(Lex::Op(current)) if *current == op) {
            self.index += 1;
            true
        }
        else {
            false
        }
    }

    fn make(&self, start: usize, kind: ExprKind) -> Expr {
        let text = self.tokens[start..self.index].iter().map(Lex::render).collect::<Vec<_>>().join(" ");
        Expr { text, kind }
    }

    fn parse_or(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let mut left = self.parse_and()?;
        while self.take_ident("or") {
            let right = self.parse_and()?;
            left = self.make(start, ExprKind::Binary { op: BinaryOp::Or, left: Box::new(left), right: Box::new(right) });
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let mut left = self.parse_not()?;
        while self.take_ident("and") {
            let right = self.parse_not()?;
            left = self.make(start, ExprKind::Binary { op: BinaryOp::And, left: Box::new(left), right: Box::new(right) });
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        if self.take_ident("not") {
            let operand = self.parse_not()?;
            return Ok(self.make(start, ExprKind::Unary { op: UnaryOp::Not, operand: Box::new(operand) }));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let left = self.parse_additive()?;
        let op = match self.peek() {
            Some(Lex::Op("==")) => Some(BinaryOp::Eq),
            Some(Lex::Op("!=")) => Some(BinaryOp::Ne),
            Some(Lex::Op("<=")) => Some(BinaryOp::Le),
            Some(Lex::Op(">=")) => Some(BinaryOp::Ge),
            Some(Lex::Op("<")) => Some(BinaryOp::Lt),
            Some(Lex::Op(">")) => Some(BinaryOp::Gt),
            _ => None,
        };
        let Some(op) = op
        else {
            if self.peek_ident("is") {
                return Err(unsupported_construct("is-test", "`is` tests are not supported in v0"));
            }
            return Ok(left);
        };
        self.index += 1;
        let right = self.parse_additive()?;
        if self.peek_ident("is") {
            return Err(unsupported_construct("is-test", "`is` tests are not supported in v0"));
        }
        Ok(self.make(start, ExprKind::Binary { op, left: Box::new(left), right: Box::new(right) }))
    }

    fn parse_additive(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = if self.take_op("+") {
                BinaryOp::Add
            }
            else if self.take_op("-") {
                BinaryOp::Sub
            }
            else if self.take_op("~") {
                BinaryOp::Concat
            }
            else {
                break;
            };
            let right = self.parse_multiplicative()?;
            left = self.make(start, ExprKind::Binary { op, left: Box::new(left), right: Box::new(right) });
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.take_op("*") {
                BinaryOp::Mul
            }
            else if self.take_op("/") {
                BinaryOp::Div
            }
            else if self.take_op("%") {
                BinaryOp::Mod
            }
            else {
                break;
            };
            let right = self.parse_unary()?;
            left = self.make(start, ExprKind::Binary { op, left: Box::new(left), right: Box::new(right) });
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        if self.take_op("-") {
            let operand = self.parse_unary()?;
            return Ok(self.make(start, ExprKind::Unary { op: UnaryOp::Negate, operand: Box::new(operand) }));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let mut expression = self.parse_primary()?;
        loop {
            if self.take_op(".") {
                let Some(Lex::Ident(key)) = self.peek().cloned()
                else {
                    return Err(parse_failed("expected an attribute name after `.`"));
                };
                self.index += 1;
                expression = self.make(start, ExprKind::Attr { base: Box::new(expression), key });
                continue;
            }
            if self.take_op("[") {
                let key = self.parse_or()?;
                if !self.take_op("]") {
                    return Err(parse_failed("missing `]` in an index expression"));
                }
                expression = self.make(start, ExprKind::Index { base: Box::new(expression), key: Box::new(key) });
                continue;
            }
            if self.take_op("|") {
                let Some(Lex::Ident(name)) = self.peek().cloned()
                else {
                    return Err(parse_failed("expected a filter name after `|`"));
                };
                self.index += 1;
                let mut args = Vec::new();
                if self.take_op("(") {
                    if !self.take_op(")") {
                        loop {
                            args.push(self.parse_or()?);
                            if self.take_op(",") {
                                continue;
                            }
                            if self.take_op(")") {
                                break;
                            }
                            return Err(parse_failed("missing `,` or `)` in filter arguments"));
                        }
                    }
                }
                expression = self.make(start, ExprKind::Filter { input: Box::new(expression), name, args });
                continue;
            }
            break;
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<Expr, TemplateError> {
        let start = self.index;
        let Some(token) = self.peek().cloned()
        else {
            return Err(parse_failed("expected an expression"));
        };
        match token {
            Lex::Number(value) => {
                self.index += 1;
                Ok(self.make(start, ExprKind::Literal(value)))
            }
            Lex::Str(value) => {
                self.index += 1;
                Ok(self.make(start, ExprKind::Literal(Value::String(value))))
            }
            Lex::Ident(name) => {
                self.index += 1;
                let kind = match name.as_str() {
                    "true" | "True" => ExprKind::Literal(Value::Bool(true)),
                    "false" | "False" => ExprKind::Literal(Value::Bool(false)),
                    "none" | "None" => ExprKind::Literal(Value::Null),
                    "is" => return Err(unsupported_construct("is-test", "`is` tests are not supported in v0")),
                    _ => ExprKind::Name(name),
                };
                Ok(self.make(start, kind))
            }
            Lex::Op("(") => {
                self.index += 1;
                let inner = self.parse_or()?;
                if !self.take_op(")") {
                    return Err(parse_failed("missing `)`"));
                }
                Ok(inner)
            }
            other => Err(parse_failed(format!("expected an expression, found `{}`", other.render()))),
        }
    }
}
