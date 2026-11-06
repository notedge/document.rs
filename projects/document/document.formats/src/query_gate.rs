//! Selector execution gates shared by the XML and HTML importers.
//!
//! Oak returns partial matches together with a [`SelectorOutcome`]. A caller
//! that only inspects the matched nodes cannot tell a complete query from one
//! that stopped at `max_matches` or hit an unsupported selector shape. This
//! module turns every non-`Matched` outcome into a hard format error, so a
//! truncated selector result never becomes silent document loss.

use oak_core::query::{QueryBudget, SelectorOutcome};

use crate::FormatError;

/// Limits applied to one selector execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryLimits {
    /// Maximum number of matched nodes returned.
    pub max_matches: usize,
    /// Maximum traversal depth from the query root.
    pub max_depth: usize,
    /// Maximum predicate evaluations per query.
    pub max_predicates: usize,
    /// Maximum bytes materialized for text extraction.
    pub max_text_bytes: usize,
}

impl Default for QueryLimits {
    fn default() -> Self {
        let budget = QueryBudget::default();
        Self {
            max_matches: budget.max_matches,
            max_depth: budget.max_depth,
            max_predicates: budget.max_predicates,
            max_text_bytes: budget.max_text_bytes,
        }
    }
}

impl QueryLimits {
    /// Builds limits sized for one OPC package member.
    #[must_use]
    pub fn package_member() -> Self {
        let budget = QueryBudget::package_member();
        Self {
            max_matches: budget.max_matches,
            max_depth: budget.max_depth,
            max_predicates: budget.max_predicates,
            max_text_bytes: budget.max_text_bytes,
        }
    }

    /// Converts these limits into the Oak query budget.
    #[must_use]
    pub fn budget(self) -> QueryBudget {
        QueryBudget {
            max_matches: self.max_matches,
            max_depth: self.max_depth,
            max_predicates: self.max_predicates,
            max_text_bytes: self.max_text_bytes,
        }
    }
}

/// Rejects a selector execution that did not complete.
///
/// `format` names the importer (`docx`, `epub`, `html`) and `selector` is the
/// authored selector, so a budget failure points at the exact query instead of
/// a generic reader error.
pub fn ensure_complete(format: &str, selector: &str, outcome: SelectorOutcome, matched: usize) -> Result<(), FormatError> {
    match outcome {
        SelectorOutcome::Matched => Ok(()),
        SelectorOutcome::BudgetExceeded => Err(FormatError::budget(
            format,
            format!("selector `{selector}` stopped after {matched} matches at the query budget"),
        )),
        SelectorOutcome::Unsupported => {
            Err(FormatError::unsupported(format, format!("selector `{selector}` is not supported by the active executor")))
        }
        SelectorOutcome::Ambiguous => {
            Err(FormatError::invalid_input(format!("selector `{selector}` matched more than one node where one was required")))
        }
        SelectorOutcome::StaleSnapshot => {
            Err(FormatError::invalid_input(format!("selector `{selector}` was executed against a stale document snapshot")))
        }
    }
}
