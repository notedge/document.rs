use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crawler_types::{BodyStorage, Result, TransportProvenance};
use url::Url;

use super::{FetchRequest, Transport, build_artifact, map_transport_error};

/// Read local files referenced by `file://` URLs.
#[derive(Debug, Clone, Default)]
pub struct FileTransport {
    /// Optional redirect map for tests and fixtures.
    redirects: BTreeMap<String, String>,
}

impl FileTransport {
    /// Empty file transport.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a redirect from one file URL string to another.
    pub fn with_redirect(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.redirects.insert(from.into(), to.into());
        self
    }

    /// Register a redirect in place.
    pub fn insert_redirect(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.redirects.insert(from.into(), to.into());
    }
}

impl Transport for FileTransport {
    fn fetch(&self, request: &FetchRequest) -> Result<crawler_types::FetchArtifact> {
        if request.url.scheme() != "file" {
            return Err(map_transport_error(format!("file transport cannot fetch `{}`", request.url)));
        }

        let mut request_url = request.url.clone();
        let mut redirect_chain = Vec::new();
        for _ in 0..8 {
            let key = request_url.to_string();
            if let Some(target) = self.redirects.get(&key) {
                let next = Url::parse(target).map_err(|error| map_transport_error(error.to_string()))?;
                redirect_chain.push(crawler_types::RedirectHop { url: request_url.clone(), status: 302 });
                request_url = next;
                continue;
            }
            break;
        }

        let path = url_to_path(&request_url)?;
        let bytes = fs::read(&path).map_err(|error| map_transport_error(format!("read `{}`: {error}", path.display())))?;
        let headers = BTreeMap::from([("content-type".into(), infer_file_content_type(&path))]);

        Ok(build_artifact(
            request.url.clone(),
            request_url,
            200,
            headers,
            BodyStorage::inline(bytes),
            redirect_chain,
            TransportProvenance::File,
            None,
        ))
    }
}

fn url_to_path(url: &Url) -> Result<PathBuf> {
    let path = url.to_file_path().map_err(|_| map_transport_error(format!("invalid file URL `{}`", url)))?;
    Ok(path)
}

fn infer_file_content_type(path: &Path) -> String {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html".into(),
        "css" => "text/css".into(),
        "json" => "application/json".into(),
        _ => "application/octet-stream".into(),
    }
}
