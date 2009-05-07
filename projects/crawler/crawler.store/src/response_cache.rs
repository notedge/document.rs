use std::path::Path;

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{CrawlError, FetchArtifact, Result, TransportProvenance, diagnostics as pd};
use yydb::{Connection, OpenFlags};

use crate::keyspace;

/// Schema document for the response cache table.
///
/// The document itself carries the schema version (`// @yydb-schema-version`), so the
/// initial version applies and no separate version argument is passed.
const RESPONSE_CACHE_SCHEMA: &str = "table CrawlerCache { @@key: utf8, value: utf8 }";

/// YYDB-backed response cache. Metadata and inline bodies are stored as one record.
pub struct YydbResponseCache {
    connection: Connection,
}

impl YydbResponseCache {
    /// Open a WAL-backed YYDB response cache.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_str = path.as_ref().display().to_string();
        let connection = Connection::open_with_flags(path.as_ref(), OpenFlags::wal()).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::STORE_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_store(),
                Message::new("crawler.diagnostic.storeOpen")
                    .with_arg("path", MessageArg::Text(path_str.clone()))
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("open YYDB cache `{path_str}`: {error}")),
            );
            CrawlError::from(diag)
        })?;
        connection.ensure_schema(RESPONSE_CACHE_SCHEMA).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::STORE_UNAVAILABLE),
                DiagnosticSeverity::Error,
                pd::origin_store(),
                Message::new("crawler.diagnostic.storeSchema")
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("initialize YYDB cache: {error}")),
            );
            CrawlError::from(diag)
        })?;
        Ok(Self { connection })
    }

    /// Read an artifact and mark it as a cache hit.
    pub fn get(&self, cache_key: &str) -> Result<Option<FetchArtifact>> {
        let key = keyspace::response_cache(cache_key);
        let Some(bytes) = self.connection.get(&key).map_err(map_yydb)?
        else {
            return Ok(None);
        };
        let mut artifact: FetchArtifact = serde_json::from_slice(&bytes).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::STORE_CORRUPT),
                DiagnosticSeverity::Error,
                pd::origin_store(),
                Message::new("crawler.diagnostic.storeDecode")
                    .with_arg("cache_key", MessageArg::Text(cache_key.to_string()))
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("decode YYDB cache entry: {error}")),
            );
            CrawlError::from(diag)
        })?;
        artifact.transport_provenance = TransportProvenance::CacheHit;
        Ok(Some(artifact))
    }

    /// Store an artifact in YYDB and return its cache key.
    pub fn put(&self, artifact: &FetchArtifact) -> Result<String> {
        let key = keyspace::response_cache(&artifact.cache_key);
        let bytes = serde_json::to_vec(artifact).map_err(|error| {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::STORE_TRANSACTION_FAILED),
                DiagnosticSeverity::Error,
                pd::origin_store(),
                Message::new("crawler.diagnostic.storeEncode")
                    .with_arg("cache_key", MessageArg::Text(artifact.cache_key.clone()))
                    .with_arg("error", MessageArg::Text(error.to_string()))
                    .with_fallback(format!("encode YYDB cache entry: {error}")),
            );
            CrawlError::from(diag)
        })?;
        self.connection.put(key, bytes).map_err(map_yydb)?;
        Ok(artifact.cache_key.clone())
    }

    /// Flush the YYDB write-ahead log.
    pub fn checkpoint(&self) -> Result<()> {
        self.connection.checkpoint().map_err(map_yydb)
    }
}

fn map_yydb(error: yydb::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::STORE_TRANSACTION_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_store(),
        Message::new("crawler.diagnostic.storeYydb")
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("YYDB cache error: {error}")),
    );
    CrawlError::from(diag)
}
