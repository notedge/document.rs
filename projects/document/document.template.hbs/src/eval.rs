use std::borrow::Cow;

use document_template::{
    RenderBudget, RenderOptions, RenderOutput, RenderRequest, TemplateError, TemplateId, TemplateLoader, codes,
};
use serde_json::Value;

use crate::{
    ast::{Expr, ExprKind, Node, PathBase, PathExpr},
    parser::parse,
};

pub(crate) const EXTENSIONS: &[&str] = &["", ".hbs", ".html"];

#[derive(Debug)]
struct Frame {
    index: usize,
    key: Option<String>,
    first: bool,
    last: bool,
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
    let mut context = vec![Cow::Borrowed(request.data)];
    let mut frames = Vec::new();
    let mut output = String::new();
    renderer.nodes(nodes, &mut context, &mut frames, &mut output, request.entry.cloned(), 0)?;
    Ok(RenderOutput { text: output, dependencies: renderer.dependencies })
}

struct Renderer<'a> {
    loader: &'a dyn TemplateLoader,
    options: &'a RenderOptions,
    budget: RenderBudget,
    dependencies: Vec<TemplateId>,
}

impl<'a> Renderer<'a> {
    fn nodes<'v>(
        &mut self,
        nodes: &[Node],
        context: &mut Vec<Cow<'v, Value>>,
        frames: &mut Vec<Frame>,
        output: &mut String,
        current: Option<TemplateId>,
        depth: u32,
    ) -> Result<(), TemplateError> {
        for node in nodes {
            self.budget.step()?;
            match node {
                Node::Text(text) => self.budget.write(output, text)?,
                Node::Output { expression, escape } => {
                    let value = self.resolve(expression, context, frames)?;
                    let text = value.map_or_else(String::new, |value| stringify(value.as_ref()));
                    let text = if *escape { escape_html(&text) } else { text };
                    self.budget.write(output, &text)?;
                }
                Node::If { condition, invert, positive, negative } => {
                    let value = self.resolve(condition, context, frames)?;
                    let truthy = value.as_ref().is_some_and(|value| truthy(value.as_ref()));
                    let branch = if truthy == *invert { negative } else { positive };
                    self.nodes(branch, context, frames, output, current.clone(), depth)?;
                }
                Node::Each { source, body, otherwise } => {
                    let value = self.resolve(source, context, frames)?;
                    let entries = iteration(value);
                    if entries.is_empty() {
                        self.nodes(otherwise, context, frames, output, current.clone(), depth)?;
                    }
                    else {
                        let length = entries.len();
                        for (index, (key, item)) in entries.into_iter().enumerate() {
                            frames.push(Frame { index, key, first: index == 0, last: index + 1 == length });
                            context.push(item);
                            let result = self.nodes(body, context, frames, output, current.clone(), depth);
                            context.pop();
                            frames.pop();
                            result?;
                        }
                    }
                }
                Node::With { source, body, otherwise } => {
                    let value = self.resolve(source, context, frames)?;
                    if value.as_ref().is_some_and(|value| truthy(value.as_ref())) {
                        context.push(value.expect("checked above"));
                        let result = self.nodes(body, context, frames, output, current.clone(), depth);
                        context.pop();
                        result?;
                    }
                    else {
                        self.nodes(otherwise, context, frames, output, current.clone(), depth)?;
                    }
                }
                Node::Partial { reference } => {
                    self.partial(reference, context, frames, output, current.as_ref(), depth)?;
                }
            }
        }
        Ok(())
    }

    fn partial<'v>(
        &mut self,
        reference: &str,
        context: &mut Vec<Cow<'v, Value>>,
        frames: &mut Vec<Frame>,
        output: &mut String,
        current: Option<&TemplateId>,
        depth: u32,
    ) -> Result<(), TemplateError> {
        if depth >= self.options.limits.max_include_depth {
            return Err(TemplateError::with_args(
                codes::INCLUDE_DEPTH,
                format!("partial `{reference}` exceeds the include depth limit"),
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
        self.nodes(&nodes, context, frames, output, Some(id), depth + 1)
    }

    fn resolve<'v>(
        &self,
        expression: &Expr,
        context: &[Cow<'v, Value>],
        frames: &[Frame],
    ) -> Result<Option<Cow<'v, Value>>, TemplateError> {
        let resolved = match &expression.kind {
            ExprKind::Literal(value) => Some(Cow::Owned(value.clone())),
            ExprKind::Path(path) => path_value(path, context, frames),
        };
        if resolved.is_none() && self.options.strict_undefined {
            return Err(undefined(expression));
        }
        Ok(resolved)
    }
}

fn undefined(expression: &Expr) -> TemplateError {
    TemplateError::with_args(
        codes::UNDEFINED_VARIABLE,
        format!("`{}` is undefined", expression.text),
        vec![("path", expression.text.clone())],
    )
}

fn path_value<'v>(path: &PathExpr, context: &[Cow<'v, Value>], frames: &[Frame]) -> Option<Cow<'v, Value>> {
    let mut current = match &path.base {
        PathBase::Root => reborrow(context.first()?),
        PathBase::Current(up) => {
            let index = context.len().checked_sub(1 + up)?;
            reborrow(context.get(index)?)
        }
        PathBase::Data(name) => data_value(name, frames)?,
    };
    for step in &path.steps {
        current = step_value(current, step)?;
    }
    Some(current)
}

fn reborrow<'v>(value: &Cow<'v, Value>) -> Cow<'v, Value> {
    match value {
        Cow::Borrowed(inner) => Cow::Borrowed(*inner),
        Cow::Owned(inner) => Cow::Owned(inner.clone()),
    }
}

fn data_value<'v>(name: &str, frames: &[Frame]) -> Option<Cow<'v, Value>> {
    let frame = frames.last()?;
    let value = match name {
        "index" => Value::from(frame.index as u64),
        "key" => frame.key.as_ref().map_or_else(|| Value::from(frame.index as u64), |key| Value::String(key.clone())),
        "first" => Value::Bool(frame.first),
        "last" => Value::Bool(frame.last),
        _ => return None,
    };
    Some(Cow::Owned(value))
}

fn step_value<'v>(value: Cow<'v, Value>, key: &str) -> Option<Cow<'v, Value>> {
    match value {
        Cow::Borrowed(value) => member(value, key).map(Cow::Borrowed),
        Cow::Owned(value) => member(&value, key).cloned().map(Cow::Owned),
    }
}

fn member<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map.get(key),
        Value::Array(items) => key.parse::<usize>().ok().and_then(|index| items.get(index)),
        _ => None,
    }
}

fn iteration<'v>(value: Option<Cow<'v, Value>>) -> Vec<(Option<String>, Cow<'v, Value>)> {
    match value {
        Some(Cow::Borrowed(Value::Array(items))) => items.iter().map(|item| (None, Cow::Borrowed(item))).collect(),
        Some(Cow::Borrowed(Value::Object(map))) => {
            map.iter().map(|(key, item)| (Some(key.clone()), Cow::Borrowed(item))).collect()
        }
        Some(Cow::Owned(Value::Array(items))) => items.into_iter().map(|item| (None, Cow::Owned(item))).collect(),
        Some(Cow::Owned(Value::Object(map))) => map.into_iter().map(|(key, item)| (Some(key), Cow::Owned(item))).collect(),
        _ => Vec::new(),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(items) => items.iter().map(stringify).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

fn escape_html(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        let escaped = match character {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&#x27;",
            '`' => "&#x60;",
            '=' => "&#x3D;",
            other => {
                output.push(other);
                continue;
            }
        };
        output.push_str(escaped);
    }
    output
}
