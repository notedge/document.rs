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

    /// Parse a canonical `{root}:{posix relative path}` identifier.
    ///
    /// A form aliasing, padding or re-normalizing the stored shape is rejected
    /// with `template.invalid-reference`, the same failure the loader uses for
    /// malformed references. This only moves the existing judgment earlier and
    /// does not add a new code.
    pub fn parse(id: &str) -> Result<Self, TemplateError> {
        let trimmed = id.trim();
        if trimmed != id {
            return Err(TemplateError::with_args(
                codes::INVALID_REFERENCE,
                format!("template id `{id}` has leading or trailing whitespace"),
                vec![("ref", id.to_string())],
            ));
        }
        let Some((root, path)) = split_scheme(trimmed)
        else {
            return Err(TemplateError::with_args(
                codes::INVALID_REFERENCE,
                format!("template id `{id}` has no `root:path` separator"),
                vec![("ref", id.to_string())],
            ));
        };
        if path.is_empty() {
            return Err(TemplateError::with_args(
                codes::INVALID_REFERENCE,
                format!("template id `{id}` has an empty path"),
                vec![("ref", id.to_string())],
            ));
        }
        let Some(normalized) = normalize_path(path)
        else {
            return Err(TemplateError::with_args(
                codes::INVALID_REFERENCE,
                format!("template id `{id}` has a path that escapes its root"),
                vec![("ref", id.to_string())],
            ));
        };
        if normalized != path {
            return Err(TemplateError::with_args(
                codes::INVALID_REFERENCE,
                format!("template id `{id}` is not in canonical `root:path` form"),
                vec![("ref", id.to_string())],
            ));
        }
        Ok(Self::new(root, normalized))
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
    scheme: String,
    priority: u16,
    files: BTreeMap<String, String>,
}

impl TemplateRoot {
    /// Create an empty root. Its scheme defaults to its name.
    pub fn new(name: impl Into<String>, priority: u16) -> Self {
        let name = name.into();
        Self { scheme: name.clone(), name, priority, files: BTreeMap::new() }
    }

    /// Set the scheme a `scheme:` reference matches.
    ///
    /// The canonical id keeps the root name, so aliasing a root never changes
    /// the identity a loader returns.
    #[must_use]
    pub fn with_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.scheme = scheme.into();
        self
    }

    /// Add one file. The path is normalized best-effort before insertion.
    #[must_use]
    pub fn with_file(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        let path = path.into();
        let key = normalize_path(&path).unwrap_or(path);
        self.files.insert(key, source.into());
        self
    }

    /// Root name, used as the canonical id prefix.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Scheme matched by `scheme:` references, defaulting to the name.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
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

    fn root_by_scheme(&self, scheme: &str) -> Option<&TemplateRoot> {
        self.roots.iter().find(|root| root.scheme() == scheme)
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
            let Some(root) = self.root_by_scheme(scheme)
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
                    format!("no template in root `{}` matched `{path}`", root.name()),
                    vec![("ref", raw.clone()), ("scheme", scheme.to_string()), ("searched", root.name().to_string())],
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
///
/// The canonical id shape uses this same normalization, so hosts can split a
/// `from` id into a root name and a normalized path without a second parser.
#[must_use]
pub fn normalize_path(path: &str) -> Option<String> {
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
