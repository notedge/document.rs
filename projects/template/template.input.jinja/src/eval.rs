use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};
use template::{RenderBudget, RenderOptions, RenderOutput, RenderRequest, TemplateError, TemplateId, TemplateLoader, codes};

use crate::{
    ast::{BinaryOp, Expr, ExprKind, Node, UnaryOp},
    codes as jinja_codes,
    parse::parse,
};

pub(crate) const EXTENSIONS: &[&str] = &["", ".jinja", ".j2", ".html"];

#[derive(Debug, Clone)]
struct Eval {
    value: Option<Value>,
    safe: bool,
}

pub(crate) fn render(request: &RenderRequest<'_>, nodes: &[Node]) -> Result<RenderOutput, TemplateError> {
    let mut renderer = Renderer {
        loader: request.loader,
        options: request.options,
        budget: RenderBudget::new(request.options.limits),
        dependencies: Vec::new(),
    };
    if let Some(entry) = request.entry {
        renderer.dependencies.push(entry.clone());
    }
    let mut scopes = vec![BTreeMap::new()];
    let mut output = String::new();
    renderer.nodes(nodes, request.data, &mut scopes, &mut output, request.entry.cloned(), 0)?;
    Ok(RenderOutput { text: output, dependencies: renderer.dependencies })
}

struct Renderer<'a> {
    loader: &'a dyn TemplateLoader,
    options: &'a RenderOptions,
    budget: RenderBudget,
    dependencies: Vec<TemplateId>,
}

impl<'a> Renderer<'a> {
    fn nodes(
        &mut self,
        nodes: &[Node],
        root: &Value,
        scopes: &mut Vec<BTreeMap<String, Value>>,
        output: &mut String,
        current: Option<TemplateId>,
        depth: u32,
    ) -> Result<(), TemplateError> {
        for node in nodes {
            self.budget.step()?;
            match node {
                Node::Text(text) => self.budget.write(output, text)?,
                Node::Output { expression } => {
                    let Eval { value, safe } = self.eval(expression, root, scopes.as_slice())?;
                    let value = self.value(value, expression)?;
                    let text = stringify(&value);
                    let text = if safe { text } else { escape_html(&text) };
                    self.budget.write(output, &text)?;
                }
                Node::If { sections, otherwise } => {
                    let mut chosen: Option<&[Node]> = None;
                    for (condition, body) in sections {
                        let Eval { value, .. } = self.eval(condition, root, scopes.as_slice())?;
                        let value = self.value(value, condition)?;
                        if truthy(&value) {
                            chosen = Some(body);
                            break;
                        }
                    }
                    let body = chosen.unwrap_or(otherwise);
                    self.nodes(body, root, scopes, output, current.clone(), depth)?;
                }
                Node::For { target, source, body, otherwise } => {
                    let Eval { value, .. } = self.eval(source, root, scopes.as_slice())?;
                    let value = self.value(value, source)?;
                    let items = iterable(&value, source)?;
                    if items.is_empty() {
                        self.nodes(otherwise, root, scopes, output, current.clone(), depth)?;
                    }
                    else {
                        let length = items.len();
                        for (index, item) in items.into_iter().enumerate() {
                            let mut scope = BTreeMap::new();
                            scope.insert(target.clone(), item);
                            scope.insert("loop".to_string(), loop_value(index, length));
                            scopes.push(scope);
                            let result = self.nodes(body, root, scopes, output, current.clone(), depth);
                            scopes.pop();
                            result?;
                        }
                    }
                }
                Node::Set { name, value: expression } => {
                    let Eval { value, .. } = self.eval(expression, root, scopes.as_slice())?;
                    let value = self.value(value, expression)?;
                    scopes.last_mut().expect("the root scope is always present").insert(name.clone(), value);
                }
                Node::Include { reference } => {
                    self.include(reference, root, scopes, output, current.as_ref(), depth)?;
                }
            }
        }
        Ok(())
    }

    fn include(
        &mut self,
        reference: &str,
        root: &Value,
        scopes: &mut Vec<BTreeMap<String, Value>>,
        output: &mut String,
        current: Option<&TemplateId>,
        depth: u32,
    ) -> Result<(), TemplateError> {
        if depth >= self.options.limits.max_include_depth {
            return Err(TemplateError::with_args(
                codes::INCLUDE_DEPTH,
                format!("include `{reference}` exceeds the include depth limit"),
                vec![("ref", reference.to_string()), ("limit", self.options.limits.max_include_depth.to_string())],
            ));
        }
        let id = self.loader.resolve(reference, current, EXTENSIONS)?;
        let source = self.loader.source(&id)?;
        self.budget.source(source)?;
        if !self.dependencies.contains(&id) {
            self.dependencies.push(id.clone());
        }
        let nodes = parse(source)?;
        self.nodes(&nodes, root, scopes, output, Some(id), depth + 1)
    }

    fn eval(&self, expression: &Expr, root: &Value, scopes: &[BTreeMap<String, Value>]) -> Result<Eval, TemplateError> {
        match &expression.kind {
            ExprKind::Literal(value) => Ok(Eval { value: Some(value.clone()), safe: false }),
            ExprKind::Name(name) => Ok(Eval { value: lookup(name, root, scopes), safe: false }),
            ExprKind::Attr { base, key } => {
                let Some(value) = self.eval(base, root, scopes)?.value
                else {
                    return Ok(Eval { value: None, safe: false });
                };
                Ok(Eval { value: member(&value, key), safe: false })
            }
            ExprKind::Index { base, key } => {
                let base = self.eval(base, root, scopes)?.value;
                let key = self.eval(key, root, scopes)?.value;
                let (Some(base), Some(key)) = (base, key)
                else {
                    return Ok(Eval { value: None, safe: false });
                };
                Ok(Eval { value: index(&base, &key), safe: false })
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.eval(operand, root, scopes)?.value;
                let value = self.value(operand, expression)?;
                let value = match op {
                    UnaryOp::Not => Value::Bool(!truthy(&value)),
                    UnaryOp::Negate => negate(&value, expression)?,
                };
                Ok(Eval { value: Some(value), safe: false })
            }
            ExprKind::Binary { op, left, right } => self.binary(*op, left, right, expression, root, scopes),
            ExprKind::Filter { input, name, args } => {
                let input = self.eval(input, root, scopes)?;
                self.apply_filter(name, args, input, expression, root, scopes)
            }
        }
    }

    fn binary(
        &self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        expression: &Expr,
        root: &Value,
        scopes: &[BTreeMap<String, Value>],
    ) -> Result<Eval, TemplateError> {
        match op {
            BinaryOp::Or => {
                let Eval { value, safe } = self.eval(left, root, scopes)?;
                let value = self.value(value, left)?;
                if truthy(&value) {
                    return Ok(Eval { value: Some(value), safe });
                }
                Ok(self.eval(right, root, scopes)?)
            }
            BinaryOp::And => {
                let Eval { value, safe } = self.eval(left, root, scopes)?;
                let value = self.value(value, left)?;
                if !truthy(&value) {
                    return Ok(Eval { value: Some(value), safe });
                }
                Ok(self.eval(right, root, scopes)?)
            }
            other => {
                let left = self.value(self.eval(left, root, scopes)?.value, left)?;
                let right = self.value(self.eval(right, root, scopes)?.value, right)?;
                let value = match other {
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        compare(other, &left, &right, expression)?
                    }
                    BinaryOp::Concat => Value::String(format!("{}{}", stringify(&left), stringify(&right))),
                    _ => arithmetic(other, &left, &right, expression)?,
                };
                Ok(Eval { value: Some(value), safe: false })
            }
        }
    }

    fn apply_filter(
        &self,
        name: &str,
        args: &[Expr],
        input: Eval,
        expression: &Expr,
        root: &Value,
        scopes: &[BTreeMap<String, Value>],
    ) -> Result<Eval, TemplateError> {
        let expected = match name {
            "upper" | "lower" | "trim" | "length" | "escape" | "safe" => 0,
            "default" | "join" => 1,
            "replace" => 2,
            _ => return Err(unknown_filter(name)),
        };
        if args.len() != expected {
            return Err(TemplateError::with_args(
                jinja_codes::FILTER_ARITY,
                format!("filter `{name}` takes {expected} argument(s), got {}", args.len()),
                vec![("filter", name.to_string()), ("expected", expected.to_string()), ("actual", args.len().to_string())],
            ));
        }
        if name == "default" {
            if matches!(&input.value, None | Some(Value::Null)) {
                let Eval { value, safe } = self.eval(&args[0], root, scopes)?;
                let value = self.value(value, &args[0])?;
                return Ok(Eval { value: Some(value), safe });
            }
            return Ok(input);
        }
        let value = self.value(input.value, expression)?;
        match name {
            "upper" => {
                let text = match value {
                    Value::String(text) => text.to_uppercase(),
                    other => return Err(filter_value(name, &other)),
                };
                Ok(Eval { value: Some(Value::String(text)), safe: false })
            }
            "lower" => {
                let text = match value {
                    Value::String(text) => text.to_lowercase(),
                    other => return Err(filter_value(name, &other)),
                };
                Ok(Eval { value: Some(Value::String(text)), safe: false })
            }
            "trim" => {
                let text = match value {
                    Value::String(text) => text.trim().to_string(),
                    other => return Err(filter_value(name, &other)),
                };
                Ok(Eval { value: Some(Value::String(text)), safe: false })
            }
            "length" => {
                let length = match &value {
                    Value::String(text) => text.chars().count(),
                    Value::Array(items) => items.len(),
                    Value::Object(map) => map.len(),
                    other => return Err(filter_value(name, other)),
                };
                Ok(Eval { value: Some(Value::from(length as u64)), safe: false })
            }
            "join" => {
                let Value::Array(items) = &value
                else {
                    return Err(filter_value(name, &value));
                };
                let separator = self.filter_arg_string(name, &args[0], root, scopes)?;
                let joined = items.iter().map(stringify).collect::<Vec<_>>().join(&separator);
                Ok(Eval { value: Some(Value::String(joined)), safe: false })
            }
            "replace" => {
                let Value::String(text) = &value
                else {
                    return Err(filter_value(name, &value));
                };
                let from = self.filter_arg_string(name, &args[0], root, scopes)?;
                let to = self.filter_arg_string(name, &args[1], root, scopes)?;
                Ok(Eval { value: Some(Value::String(text.replace(&from, &to))), safe: false })
            }
            "escape" => Ok(Eval { value: Some(Value::String(escape_html(&stringify(&value)))), safe: true }),
            "safe" => Ok(Eval { value: Some(value), safe: true }),
            other => Err(unknown_filter(other)),
        }
    }

    fn filter_arg_string(
        &self,
        name: &str,
        expression: &Expr,
        root: &Value,
        scopes: &[BTreeMap<String, Value>],
    ) -> Result<String, TemplateError> {
        let value = self.value(self.eval(expression, root, scopes)?.value, expression)?;
        match value {
            Value::String(text) => Ok(text),
            other => Err(filter_value(name, &other)),
        }
    }

    fn value(&self, value: Option<Value>, expression: &Expr) -> Result<Value, TemplateError> {
        match value {
            Some(value) => Ok(value),
            None if self.options.strict_undefined => Err(undefined(expression)),
            None => Ok(Value::Null),
        }
    }
}

fn undefined(expression: &Expr) -> TemplateError {
    TemplateError::with_args(
        codes::UNDEFINED_VARIABLE,
        format!("`{}` is undefined", expression.text),
        vec![("path", expression.text.clone())],
    )
}

fn lookup(name: &str, root: &Value, scopes: &[BTreeMap<String, Value>]) -> Option<Value> {
    for scope in scopes.iter().rev() {
        if let Some(value) = scope.get(name) {
            return Some(value.clone());
        }
    }
    if let Value::Object(map) = root {
        return map.get(name).cloned();
    }
    None
}

fn member(value: &Value, key: &str) -> Option<Value> {
    match value {
        Value::Object(map) => map.get(key).cloned(),
        Value::Array(items) => key.parse::<usize>().ok().and_then(|index| items.get(index).cloned()),
        _ => None,
    }
}

fn index(value: &Value, key: &Value) -> Option<Value> {
    match (value, key) {
        (Value::Object(map), Value::String(key)) => map.get(key).cloned(),
        (Value::Array(items), Value::Number(key)) => key.as_u64().and_then(|index| items.get(index as usize).cloned()),
        _ => None,
    }
}

fn iterable(value: &Value, expression: &Expr) -> Result<Vec<Value>, TemplateError> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::Object(map) => Ok(map.keys().map(|key| Value::String(key.clone())).collect()),
        other => Err(TemplateError::with_args(
            jinja_codes::ITERATION_TYPE,
            format!("`{}` is not iterable", expression.text),
            vec![("expression", expression.text.clone()), ("value", type_name(other).to_string())],
        )),
    }
}

fn loop_value(index: usize, length: usize) -> Value {
    Value::Object(Map::from_iter([
        ("index".to_string(), Value::from((index + 1) as u64)),
        ("index0".to_string(), Value::from(index as u64)),
        ("first".to_string(), Value::Bool(index == 0)),
        ("last".to_string(), Value::Bool(index + 1 == length)),
        ("length".to_string(), Value::from(length as u64)),
    ]))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(value) => if *value { "True" } else { "False" }.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => repr(value),
    }
}

fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(value) => if *value { "True" } else { "False" }.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => format!("[{}]", items.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!("'{}': {}", key.replace('\\', "\\\\").replace('\'', "\\'"), repr(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn escape_html(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        let escaped = match character {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&#34;",
            '\'' => "&#39;",
            other => {
                output.push(other);
                continue;
            }
        };
        output.push_str(escaped);
    }
    output
}

fn compare(op: BinaryOp, left: &Value, right: &Value, expression: &Expr) -> Result<Value, TemplateError> {
    match op {
        BinaryOp::Eq => Ok(Value::Bool(left == right)),
        BinaryOp::Ne => Ok(Value::Bool(left != right)),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let (Some(left), Some(right)) = (number_f64(left), number_f64(right))
            else {
                return Err(operator_type(expression, "ordering comparison requires numbers"));
            };
            let result = match op {
                BinaryOp::Lt => left < right,
                BinaryOp::Le => left <= right,
                BinaryOp::Gt => left > right,
                _ => left >= right,
            };
            Ok(Value::Bool(result))
        }
        _ => unreachable!("only comparison operators reach here"),
    }
}

fn arithmetic(op: BinaryOp, left: &Value, right: &Value, expression: &Expr) -> Result<Value, TemplateError> {
    match op {
        BinaryOp::Div => {
            let (Some(left), Some(right)) = (number_f64(left), number_f64(right))
            else {
                return Err(operator_type(expression, "division requires numbers"));
            };
            if right == 0.0 {
                return Err(arithmetic_error(expression, "division by zero"));
            }
            finite(left / right, expression)
        }
        BinaryOp::Mod => {
            let (Some(left), Some(right)) = (number_i64(left), number_i64(right))
            else {
                return Err(operator_type(expression, "modulo requires integers"));
            };
            if right == 0 {
                return Err(arithmetic_error(expression, "modulo by zero"));
            }
            let Some(result) = left.checked_rem(right)
            else {
                return Err(arithmetic_error(expression, "integer overflow"));
            };
            Ok(Value::from(result))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
            if let (Some(left), Some(right)) = (number_i64(left), number_i64(right)) {
                let result = match op {
                    BinaryOp::Add => left.checked_add(right),
                    BinaryOp::Sub => left.checked_sub(right),
                    _ => left.checked_mul(right),
                };
                let Some(result) = result
                else {
                    return Err(arithmetic_error(expression, "integer overflow"));
                };
                return Ok(Value::from(result));
            }
            let (Some(left), Some(right)) = (number_f64(left), number_f64(right))
            else {
                return Err(operator_type(expression, "arithmetic requires numbers"));
            };
            let value = match op {
                BinaryOp::Add => left + right,
                BinaryOp::Sub => left - right,
                _ => left * right,
            };
            finite(value, expression)
        }
        _ => unreachable!("only arithmetic operators reach here"),
    }
}

fn negate(value: &Value, expression: &Expr) -> Result<Value, TemplateError> {
    if let Some(number) = number_i64(value) {
        let Some(result) = number.checked_neg()
        else {
            return Err(arithmetic_error(expression, "integer overflow"));
        };
        return Ok(Value::from(result));
    }
    let Some(number) = number_f64(value)
    else {
        return Err(operator_type(expression, "unary `-` requires a number"));
    };
    finite(-number, expression)
}

fn finite(value: f64, expression: &Expr) -> Result<Value, TemplateError> {
    Number::from_f64(value).map(Value::Number).ok_or_else(|| arithmetic_error(expression, "non-finite result"))
}

fn number_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        _ => None,
    }
}

fn number_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        _ => None,
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn unknown_filter(name: &str) -> TemplateError {
    TemplateError::with_args(
        jinja_codes::UNKNOWN_FILTER,
        format!("filter `{name}` is not supported in v0"),
        vec![("filter", name.to_string())],
    )
}

fn filter_value(name: &str, value: &Value) -> TemplateError {
    TemplateError::with_args(
        jinja_codes::FILTER_VALUE,
        format!("filter `{name}` does not accept {}", type_name(value)),
        vec![("filter", name.to_string()), ("value", type_name(value).to_string())],
    )
}

fn operator_type(expression: &Expr, detail: &str) -> TemplateError {
    TemplateError::with_args(
        jinja_codes::OPERATOR_TYPE,
        format!("`{}` is not supported: {detail}", expression.text),
        vec![("expression", expression.text.clone())],
    )
}

fn arithmetic_error(expression: &Expr, reason: &str) -> TemplateError {
    TemplateError::with_args(
        jinja_codes::ARITHMETIC,
        format!("arithmetic failed for `{}`: {reason}", expression.text),
        vec![("expression", expression.text.clone()), ("reason", reason.to_string())],
    )
}
