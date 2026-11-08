use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter, Result as FmtResult},
};

use crate::{TemplateError, codes};

/// Canonical template identifier, `{root}:{posix relative path}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TemplateId {
    root: String,
    path: String,
}

impl TemplateId {
    /// Create a canonical id from a root name and a normalized relative path.
    pub fn new(root: impl Into<String>, path: impl Into<String>) -> Self {
        Self { root: root.into(), path: path.into() }
    }

    /// Root name this id belongs to.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Normalized posix path inside the root.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Display for TemplateId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}:{}", self.root, self.path)
    }
}

/// One named template root with in-memory files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRoot {
    name: String,
    priority: u16,
    files: BTreeMap<String, String>,
}

impl TemplateRoot {
    /// Create an empty root.
    pub fn new(name: impl Into<String>, priority: u16) -> Self {
        Self { name: name.into(), priority, files: BTreeMap::new() }
    }

    /// Add one file. The path is normalized best-effort before insertion.
    #[must_use]
    pub fn with_file(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        let path = path.into();
        let key = normalize_path(&path).unwrap_or(path);
        self.files.insert(key, source.into());
        self
    }

    /// Root name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Root priority; higher wins for bare references.
    #[must_use]
    pub fn priority(&self) -> u16 {
        self.priority
    }

    /// One file's source, looked up by normalized path.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }
}

/// Template reference resolution and source lookup.
pub trait TemplateLoader: std::fmt::Debug {
    /// Resolve one author reference to a canonical id.
    ///
    /// `from` is the current template id for `./` and `../` references.
    /// `extensions` is the engine's extension probe list; the empty string
    /// means the concrete path itself.
    fn resolve(&self, reference: &str, from: Option<&TemplateId>, extensions: &[&str]) -> Result<TemplateId, TemplateError>;

    /// Return the source registered under a canonical id.
    fn source(&self, id: &TemplateId) -> Result<&str, TemplateError>;
}

/// In-memory multi-root loader.
#[derive(Debug, Clone, Default)]
pub struct MapTemplateLoader {
    roots: Vec<TemplateRoot>,
}

impl MapTemplateLoader {
    /// Create an empty loader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one root.
    #[must_use]
    pub fn with_root(mut self, root: TemplateRoot) -> Self {
        self.roots.push(root);
        self
    }

    /// Registered roots in insertion order.
    #[must_use]
    pub fn roots(&self) -> &[TemplateRoot] {
        &self.roots
    }

    fn root(&self, name: &str) -> Option<&TemplateRoot> {
        self.roots.iter().find(|root| root.name() == name)
    }

    fn probe(&self, root: &TemplateRoot, path: &str, extensions: &[&str]) -> Option<TemplateId> {
        let key = normalize_path(path)?;
        let concrete = extensions.iter().any(|extension| !extension.is_empty() && key.ends_with(extension));
        if concrete {
            return root.file(&key).map(|_| TemplateId::new(root.name(), key));
        }
        extensions.iter().find_map(|extension| {
            let candidate = format!("{key}{extension}");
            root.file(&candidate).map(|_| TemplateId::new(root.name(), candidate))
        })
    }

    fn searched(&self) -> String {
        self.roots.iter().map(|root| root.name()).collect::<Vec<_>>().join(",")
    }
}

impl TemplateLoader for MapTemplateLoader {
    fn resolve(&self, reference: &str, from: Option<&TemplateId>, extensions: &[&str]) -> Result<TemplateId, TemplateError> {
        let raw = reference.trim().replace('\\', "/");
        if raw.is_empty() {
            return Err(TemplateError::new(codes::INVALID_REFERENCE, "template reference is empty"));
        }

        if let Some((scheme, path)) = split_scheme(&raw) {
            if path.is_empty() {
                return Err(TemplateError::with_args(
                    codes::INVALID_REFERENCE,
                    format!("scheme reference `{raw}` has an empty path"),
                    vec![("ref", raw)],
                ));
            }
            let Some(root) = self.root(scheme)
            else {
                return Err(TemplateError::with_args(
                    codes::UNKNOWN_SCHEME,
                    format!("no template root is bound to scheme `{scheme}`"),
                    vec![("ref", raw.clone()), ("scheme", scheme.to_string())],
                ));
            };
            return self.probe(root, path, extensions).ok_or_else(|| {
                TemplateError::with_args(
                    codes::NOT_FOUND,
                    format!("no template in root `{scheme}` matched `{path}`"),
                    vec![("ref", raw.clone()), ("searched", scheme.to_string())],
                )
            });
        }

        if raw.starts_with("./") || raw.starts_with("../") {
            let Some(from) = from
            else {
                return Err(TemplateError::with_args(
                    codes::RELATIVE_WITHOUT_FROM,
                    format!("relative reference `{raw}` needs a canonical template id"),
                    vec![("ref", raw)],
                ));
            };
            let Some(root) = self.root(from.root())
            else {
                return Err(TemplateError::with_args(
                    codes::UNKNOWN_SCHEME,
                    format!("current template root `{}` is not registered", from.root()),
                    vec![("ref", raw), ("from", from.to_string())],
                ));
            };
            let base_dir = from.path().rsplit_once('/').map_or("", |(directory, _)| directory);
            let Ok(joined) = resolve_join(base_dir, &raw)
            else {
                return Err(TemplateError::with_args(
                    codes::RELATIVE_ESCAPE_ROOT,
                    format!("reference `{raw}` escapes root `{}`", from.root()),
                    vec![("ref", raw), ("from", from.to_string())],
                ));
            };
            return self.probe(root, &joined, extensions).ok_or_else(|| {
                TemplateError::with_args(
                    codes::NOT_FOUND,
                    format!("no template in root `{}` matched `{joined}`", from.root()),
                    vec![("ref", raw), ("from", from.to_string()), ("searched", from.root().to_string())],
                )
            });
        }

        let mut roots = self.roots.iter().collect::<Vec<_>>();
        roots.sort_by(|left, right| right.priority().cmp(&left.priority()).then_with(|| left.name().cmp(right.name())));
        for root in roots {
            if let Some(id) = self.probe(root, &raw, extensions) {
                return Ok(id);
            }
        }
        Err(TemplateError::with_args(
            codes::NOT_FOUND,
            format!("no template matched `{raw}`"),
            vec![("ref", raw), ("searched", self.searched())],
        ))
    }

    fn source(&self, id: &TemplateId) -> Result<&str, TemplateError> {
        let Some(root) = self.root(id.root())
        else {
            return Err(TemplateError::with_args(
                codes::UNKNOWN_SCHEME,
                format!("no template root is bound to scheme `{}`", id.root()),
                vec![("ref", id.to_string()), ("scheme", id.root().to_string())],
            ));
        };
        let key = normalize_path(id.path()).ok_or_else(|| {
            TemplateError::with_args(
                codes::NOT_FOUND,
                format!("template id `{id}` has an invalid path"),
                vec![("ref", id.to_string())],
            )
        })?;
        root.file(&key).ok_or_else(|| {
            TemplateError::with_args(
                codes::NOT_FOUND,
                format!("template `{id}` is not registered"),
                vec![("ref", id.to_string())],
            )
        })
    }
}

/// Split `scheme:path` when the head is a valid scheme.
fn split_scheme(reference: &str) -> Option<(&str, &str)> {
    let (head, rest) = reference.split_once(':')?;
    let first = head.chars().next()?;
    if !first.is_ascii_alphabetic()
        || !head.chars().all(|character| character.is_ascii_alphanumeric() || "_+-".contains(character))
    {
        return None;
    }
    Some((head, rest))
}

/// Normalize one path to `a/b/c` segments, rejecting `..`.
fn normalize_path(path: &str) -> Option<String> {
    let normalized = path.trim().replace('\\', "/");
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Join a base directory with a `./` or `../` reference, rejecting root escapes.
fn resolve_join(base_dir: &str, raw: &str) -> Result<String, ()> {
    let mut segments = base_dir.split('/').filter(|segment| !segment.is_empty()).collect::<Vec<_>>();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(());
                }
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() { Err(()) } else { Ok(segments.join("/")) }
}
