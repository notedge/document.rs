//! Typed document metadata entries.
//!
//! Fixed metadata fields cover the common author-facing attributes. Values
//! outside that fixed set stay typed in a stable ordered list so importers do
//! not have to flatten maps, lists, numbers, or booleans into strings.

use serde::{Deserialize, Serialize};

/// One typed metadata entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// Metadata key.
    pub key: String,
    /// Typed value bound to the key.
    pub value: MetadataValue,
}

/// Typed metadata value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataValue {
    /// Plain text value.
    Text(String),
    /// Signed integer value.
    Integer(i64),
    /// Finite floating-point value.
    Float(f64),
    /// Boolean value.
    Boolean(bool),
    /// Ordered value list.
    List(Vec<MetadataValue>),
    /// Ordered key/value map with unique keys.
    Map(Vec<MetadataEntry>),
}

impl MetadataValue {
    /// Whether every floating-point value in this payload is finite.
    ///
    /// Non-finite floats cannot be represented in the wire schema and must be
    /// rejected before serialization.
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::List(values) => values.iter().all(Self::is_finite),
            Self::Map(entries) => entries.iter().all(|entry| entry.value.is_finite()),
            Self::Text(_) | Self::Integer(_) | Self::Boolean(_) => true,
        }
    }
}
