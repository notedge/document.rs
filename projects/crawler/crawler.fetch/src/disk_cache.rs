use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use boost_diagnostic::{DiagnosticSeverity, Message, MessageArg};
use crawler_types::{BodyStorage, CrawlError, FetchArtifact, Result, TransportProvenance, diagnostics as pd};

use super::ResponseCache;

/// On-disk metadata for one cached response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DiskCacheEntry {
    artifact: FetchArtifact,
    etag: Option<String>,
    last_modified: Option<String>,
    body_bytes: u64,
}

/// Persistent response cache with atomic writes and a byte quota.
#[derive(Debug, Clone)]
pub struct DiskResponseCache {
    root: PathBuf,
    max_bytes: u64,
    used_bytes: u64,
}

impl DiskResponseCache {
    /// Open or create a cache directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|error| map_io_error("create cache dir", &root, error))?;
        fs::create_dir_all(root.join("entries"))
            .map_err(|error| map_io_error("create cache entries dir", &root.join("entries"), error))?;
        let used_bytes = measure_used_bytes(&root.join("entries"))?;
        Ok(Self { root, max_bytes: 64 * 1024 * 1024, used_bytes })
    }

    /// Override the total byte quota for stored bodies.
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Lookup a cached artifact by request URL.
    pub fn get(&self, request_url: &str) -> Result<Option<FetchArtifact>> {
        let entry_dir = self.entry_dir(request_url);
        let meta_path = entry_dir.join("meta.json");
        if !meta_path.is_file() {
            return Ok(None);
        }
        let meta_bytes = fs::read(&meta_path).map_err(|error| map_io_error("read cache meta", &meta_path, error))?;
        let entry: DiskCacheEntry = serde_json::from_slice(&meta_bytes).map_err(|error| map_serde_read(&meta_path, error))?;
        let body_path = entry_dir.join("body.bin");
        let mut body = Vec::new();
        if body_path.is_file() {
            let mut reader = fs::File::open(&body_path).map_err(|error| map_io_error("open cache body", &body_path, error))?;
            reader.read_to_end(&mut body).map_err(|error| map_io_error("read cache body", &body_path, error))?;
        }
        let mut artifact = entry.artifact;
        artifact.body = BodyStorage::inline(body);
        artifact.transport_provenance = TransportProvenance::CacheHit;
        Ok(Some(artifact))
    }

    /// Store an artifact and return its cache key.
    pub fn put(&mut self, artifact: FetchArtifact) -> Result<String> {
        let cache_key = artifact.cache_key.clone();
        let body = artifact
            .body_bytes()
            .ok_or_else(|| {
                let diag = boost_diagnostic::Diagnostic::new(
                    pd::code(pd::CACHE_WRITE_FAILED),
                    DiagnosticSeverity::Error,
                    pd::origin_fetch(),
                    Message::new("crawler.diagnostic.cacheBodyReference")
                        .with_arg("cache_key", MessageArg::Text(cache_key.clone()))
                        .with_fallback("disk cache requires an inline body; reference bodies cannot be stored"),
                );
                CrawlError::from(diag)
            })?
            .to_vec();
        let body_bytes = body.len() as u64;
        self.evict_for_insert(body_bytes)?;

        let entry_dir = self.entry_dir(&artifact.request_url.to_string());
        if entry_dir.exists() {
            self.used_bytes = self.used_bytes.saturating_sub(entry_size(&entry_dir)?);
            let _ = fs::remove_dir_all(&entry_dir);
        }
        fs::create_dir_all(&entry_dir).map_err(|error| map_io_error("create cache entry dir", &entry_dir, error))?;

        let etag = artifact.response_headers.get("etag").cloned();
        let last_modified = artifact.response_headers.get("last-modified").cloned();
        let stored = FetchArtifact {
            body: BodyStorage::Reference { cache_key: cache_key.clone() },
            transport_provenance: TransportProvenance::Network,
            ..artifact
        };
        let entry = DiskCacheEntry { artifact: stored, etag, last_modified, body_bytes };
        let meta_json = serde_json::to_vec_pretty(&entry).map_err(|error| map_serde_write(error))?;
        write_atomically(&entry_dir.join("meta.json"), &meta_json)?;
        write_atomically(&entry_dir.join("body.bin"), &body)?;
        self.used_bytes += body_bytes;
        Ok(cache_key)
    }

    /// Number of entries currently on disk.
    pub fn len(&self) -> Result<usize> {
        let entries = self.root.join("entries");
        if !entries.is_dir() {
            return Ok(0);
        }
        Ok(fs::read_dir(&entries)
            .map_err(|error| map_io_error("read cache entries dir", &entries, error))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .count())
    }

    /// Cached ETag and Last-Modified pair for a request URL, if both were stored.
    pub fn validators_for_url(&self, request_url: &str) -> Result<Option<(String, String)>> {
        let entry_dir = self.entry_dir(request_url);
        let meta_path = entry_dir.join("meta.json");
        if !meta_path.is_file() {
            return Ok(None);
        }
        let meta_bytes = fs::read(&meta_path).map_err(|error| map_io_error("read cache meta", &meta_path, error))?;
        let entry: DiskCacheEntry = serde_json::from_slice(&meta_bytes).map_err(|error| map_serde_read(&meta_path, error))?;
        Ok(match (entry.etag, entry.last_modified) {
            (Some(etag), Some(last_modified)) => Some((etag, last_modified)),
            _ => None,
        })
    }

    fn entry_dir(&self, cache_key: &str) -> PathBuf {
        self.root.join("entries").join(hash_cache_key(cache_key))
    }

    fn evict_for_insert(&mut self, incoming_bytes: u64) -> Result<()> {
        if incoming_bytes > self.max_bytes {
            let diag = boost_diagnostic::Diagnostic::new(
                pd::code(pd::CACHE_WRITE_FAILED),
                DiagnosticSeverity::Error,
                pd::origin_fetch(),
                Message::new("crawler.diagnostic.cacheQuota")
                    .with_arg("incoming_bytes", MessageArg::U64(incoming_bytes))
                    .with_arg("max_bytes", MessageArg::U64(self.max_bytes))
                    .with_fallback(format!("response body {incoming_bytes} bytes exceeds disk cache quota {}", self.max_bytes)),
            );
            return Err(CrawlError::from(diag));
        }
        while self.used_bytes.saturating_add(incoming_bytes) > self.max_bytes {
            let Some(oldest) = oldest_entry_dir(&self.root.join("entries"))?
            else {
                break;
            };
            self.used_bytes = self.used_bytes.saturating_sub(entry_size(&oldest)?);
            fs::remove_dir_all(&oldest).map_err(|error| map_io_error("evict cache entry", &oldest, error))?;
        }
        Ok(())
    }
}

/// Layer memory and disk response caches behind one lookup surface.
#[derive(Debug, Default)]
pub struct LayeredResponseCache {
    memory: ResponseCache,
    disk: Option<DiskResponseCache>,
}

impl LayeredResponseCache {
    /// Memory-only cache.
    pub fn memory_only() -> Self {
        Self::default()
    }

    /// Memory cache backed by optional disk persistence.
    pub fn with_disk(disk: DiskResponseCache) -> Self {
        Self { memory: ResponseCache::new(), disk: Some(disk) }
    }

    /// Lookup a cached artifact.
    pub fn get(&mut self, cache_key: &str) -> Result<Option<FetchArtifact>> {
        if let Some(cached) = self.memory.get(cache_key).cloned() {
            return Ok(Some(cached));
        }
        if let Some(disk) = self.disk.as_ref() {
            if let Some(cached) = disk.get(cache_key)? {
                let key = self.memory.put(cached.clone());
                return Ok(self.memory.get(&key).cloned());
            }
        }
        Ok(None)
    }

    /// Lookup a cached artifact by request URL through disk storage.
    pub fn get_by_request_url(&mut self, request_url: &str) -> Result<Option<FetchArtifact>> {
        if let Some(cached) = self.memory.get(request_url).cloned() {
            return Ok(Some(cached));
        }
        if let Some(disk) = self.disk.as_ref() {
            if let Some(cached) = disk.get(request_url)? {
                let key = self.memory.put(cached.clone());
                return Ok(self.memory.get(&key).cloned());
            }
        }
        Ok(None)
    }

    /// Store an artifact in memory and on disk when configured.
    pub fn put(&mut self, artifact: FetchArtifact) -> Result<String> {
        if let Some(disk) = self.disk.as_mut() {
            let _ = disk.put(artifact.clone())?;
        }
        Ok(self.memory.put(artifact))
    }

    /// Access the in-memory cache for tests.
    pub fn memory(&self) -> &ResponseCache {
        &self.memory
    }

    /// Mutable in-memory cache access for tests.
    pub fn memory_mut(&mut self) -> &mut ResponseCache {
        &mut self.memory
    }

    /// Validators from the memory layer by cache key.
    pub fn validators_for_cache_key(&self, cache_key: &str) -> Option<(String, String)> {
        self.memory.validators_for(cache_key)
    }

    /// Validators from the disk layer by request URL.
    pub fn validators_for_url(&self, request_url: &str) -> Result<Option<(String, String)>> {
        if let Some(disk) = self.disk.as_ref() {
            return disk.validators_for_url(request_url);
        }
        Ok(None)
    }
}

fn hash_cache_key(cache_key: &str) -> String {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let mut hasher = DefaultHasher::new();
    cache_key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp).map_err(|error| map_io_error("create cache temp file", &temp, error))?;
        file.write_all(bytes).map_err(|error| map_io_error("write cache temp file", &temp, error))?;
        file.sync_all().map_err(|error| map_io_error("sync cache temp file", &temp, error))?;
    }
    fs::rename(&temp, path).map_err(|error| map_io_error("commit cache file", path, error))
}

fn map_read_dir_entry(path: &Path, error: std::io::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CACHE_READ_FAILED),
        DiagnosticSeverity::Warning,
        pd::origin_fetch(),
        Message::new("crawler.diagnostic.cacheEntryRead")
            .with_arg("dir", MessageArg::Text(path.display().to_string()))
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("cache dir iteration error in `{}`: {error}", path.display())),
    );
    CrawlError::from(diag)
}

fn measure_used_bytes(entries_root: &Path) -> Result<u64> {
    if !entries_root.is_dir() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(entries_root).map_err(|error| map_io_error("read cache entries dir", entries_root, error))? {
        let entry = entry.map_err(|error| map_read_dir_entry(entries_root, error))?;
        if entry.path().is_dir() {
            total += entry_size(&entry.path())?;
        }
    }
    Ok(total)
}

fn entry_size(entry_dir: &Path) -> Result<u64> {
    let body_path = entry_dir.join("body.bin");
    if !body_path.is_file() {
        return Ok(0);
    }
    Ok(fs::metadata(&body_path).map_err(|error| map_io_error("stat cache body", &body_path, error))?.len())
}

fn oldest_entry_dir(entries_root: &Path) -> Result<Option<PathBuf>> {
    if !entries_root.is_dir() {
        return Ok(None);
    }
    let mut oldest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(entries_root).map_err(|error| map_io_error("read cache entries dir", entries_root, error))? {
        let entry = entry.map_err(|error| map_read_dir_entry(entries_root, error))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let modified = fs::metadata(&path)
            .map_err(|error| map_io_error("stat cache entry dir", &path, error))?
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &oldest {
            Some((current, _)) if modified >= *current => {}
            _ => oldest = Some((modified, path)),
        }
    }
    Ok(oldest.map(|(_, path)| path))
}

fn map_io_error(action: &str, path: &Path, error: std::io::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CACHE_READ_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_fetch(),
        Message::new("crawler.diagnostic.cacheIo")
            .with_arg("action", MessageArg::Text(action.to_string()))
            .with_arg("path", MessageArg::Text(path.display().to_string()))
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("{action} `{}`: {error}", path.display())),
    );
    CrawlError::from(diag)
}

fn map_serde_read(path: &Path, error: serde_json::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CACHE_READ_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_fetch(),
        Message::new("crawler.diagnostic.cacheMetaParse")
            .with_arg("path", MessageArg::Text(path.display().to_string()))
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("parse cache meta `{}`: {error}", path.display())),
    );
    CrawlError::from(diag)
}

fn map_serde_write(error: serde_json::Error) -> CrawlError {
    let diag = boost_diagnostic::Diagnostic::new(
        pd::code(pd::CACHE_WRITE_FAILED),
        DiagnosticSeverity::Error,
        pd::origin_fetch(),
        Message::new("crawler.diagnostic.cacheMetaSerialize")
            .with_arg("error", MessageArg::Text(error.to_string()))
            .with_fallback(format!("serialize cache meta: {error}")),
    );
    CrawlError::from(diag)
}
