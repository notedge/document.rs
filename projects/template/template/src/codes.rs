//! 合同层稳定诊断码。
//!
//! 只登记当前实现真有触发路径的码；引擎专有码在各自 crate 的 `codes` 模块。

/// Reference is empty or otherwise malformed.
pub const INVALID_REFERENCE: &str = "template.invalid-reference";
/// A `scheme:` reference names a root that the loader does not have.
pub const UNKNOWN_SCHEME: &str = "template.unknown-scheme";
/// A `./` or `../` reference has no canonical `from` id.
pub const RELATIVE_WITHOUT_FROM: &str = "template.relative-without-from";
/// A relative reference climbs above its root.
pub const RELATIVE_ESCAPE_ROOT: &str = "template.relative-escape-root";
/// No root and extension candidate matched the reference.
pub const NOT_FOUND: &str = "template.not-found";
/// The engine rejected the source while parsing.
pub const PARSE_FAILED: &str = "template.parse-failed";
/// `strict_undefined` turned a missing name or member into an error.
pub const UNDEFINED_VARIABLE: &str = "template.undefined-variable";
/// One template source exceeds `RenderLimits::max_source_bytes`.
pub const SOURCE_BUDGET: &str = "template.source-budget";
/// Rendered output exceeds `RenderLimits::max_output_bytes`.
pub const OUTPUT_BUDGET: &str = "template.output-budget";
/// Node evaluation exceeds `RenderLimits::max_steps`.
pub const STEP_BUDGET: &str = "template.step-budget";
/// Include recursion exceeds `RenderLimits::max_include_depth`.
pub const INCLUDE_DEPTH: &str = "template.include-depth";
