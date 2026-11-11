//! Handlebars-specific diagnostic codes.

/// A helper call such as `{{helper x}}` or a block helper.
pub const UNSUPPORTED_HELPER: &str = "template.input.hbs.unsupported-helper";
/// A block whose name is not the supported set.
pub const UNSUPPORTED_BLOCK: &str = "template.input.hbs.unsupported-block";
/// A recognized but unsupported construct, such as partial arguments.
pub const UNSUPPORTED_CONSTRUCT: &str = "template.input.hbs.unsupported-construct";
