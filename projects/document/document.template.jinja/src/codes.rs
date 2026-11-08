//! Jinja-specific diagnostic codes.

/// A recognized Jinja tag outside the supported subset.
pub const UNSUPPORTED_TAG: &str = "template.jinja.unsupported-tag";
/// A recognized construct outside the supported subset, such as an `is` test.
pub const UNSUPPORTED_CONSTRUCT: &str = "template.jinja.unsupported-construct";
/// A filter name the engine does not know.
pub const UNKNOWN_FILTER: &str = "template.jinja.unknown-filter";
/// A filter called with the wrong number of arguments.
pub const FILTER_ARITY: &str = "template.jinja.filter-arity";
/// A filter applied to a value of the wrong type.
pub const FILTER_VALUE: &str = "template.jinja.filter-value";
/// An operator applied to values of the wrong type.
pub const OPERATOR_TYPE: &str = "template.jinja.operator-type";
/// Division by zero, modulo zero, or integer overflow.
pub const ARITHMETIC: &str = "template.jinja.arithmetic";
/// A `for` source that is not iterable.
pub const ITERATION_TYPE: &str = "template.jinja.iteration-type";
