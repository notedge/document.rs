use blake3::Hash;
use crawler_fetch::{FetchClient, FetchRequest, Transport};
use crawler_types::Result;
use document_ir::{DocumentGraph, LossMarker, SemanticStatus};
use std::collections::BTreeMap;
use url::Url;

/// Download and materialize unresolved document assets through the fetch layer.
pub fn hydrate_assets<T: Transport>(
    document: &mut DocumentGraph,
    client: &mut FetchClient<T>,
    max_assets: usize,
    max_bytes: u64,
) -> Result<usize> {
    let sources = document
        .assets
        .iter()
        .filter(|asset| asset.status == SemanticStatus::Unresolved)
        .filter_map(|asset| asset.source.clone().map(|source| (asset.id, source)))
        .collect::<Vec<_>>();
    if sources.len() > max_assets {
        document.push_loss(asset_loss(
            "budget_exhausted",
            "document assets",
            format!("asset count budget exceeded: {} candidates, {max_assets} allowed", sources.len()),
        ));
    }
    let mut used_bytes = 0u64;
    let mut hydrated = 0;

    for (asset_id, source) in sources.into_iter().take(max_assets) {
        let url = match Url::parse(&source) {
            Ok(url) => url,
            Err(error) => {
                document.push_loss(asset_loss("invalid_url", &source, error.to_string()));
                continue;
            }
        };
        let artifact = match client.fetch(&FetchRequest { url, headers: BTreeMap::new() }) {
            Ok(artifact) => artifact,
            Err(error) => {
                document.push_loss(asset_loss("fetch_failed", &source, error.to_string()));
                continue;
            }
        };
        if !(200..300).contains(&artifact.status) {
            document.push_loss(asset_loss("http_status", &source, format!("asset response returned HTTP {}", artifact.status)));
            continue;
        }
        let Some(bytes) = artifact.body_bytes()
        else {
            document.push_loss(asset_loss("body_unavailable", &source, "fetch body is out-of-line"));
            continue;
        };
        let size = bytes.len() as u64;
        if used_bytes.saturating_add(size) > max_bytes {
            document.push_loss(asset_loss("budget_exhausted", &source, "asset byte budget exceeded"));
            continue;
        }
        let fingerprint = fingerprint(bytes);
        if let Some(asset) = document.assets.iter_mut().find(|asset| asset.id == asset_id) {
            asset.bytes = Some(bytes.to_vec());
            asset.content_identity = Some(fingerprint);
            asset.media_type = artifact.content_type.or(asset.media_type.clone());
            asset.status = SemanticStatus::Resolved;
            used_bytes += size;
            hydrated += 1;
        }
    }
    Ok(hydrated)
}

/// Blake3 hex fingerprint for asset bytes.
pub fn fingerprint(bytes: &[u8]) -> String {
    let hash: Hash = blake3::hash(bytes);
    hash.to_hex().to_string()
}

fn asset_loss(code: &str, source: &str, message: impl Into<String>) -> LossMarker {
    LossMarker {
        code: format!("crawler.asset.{code}"),
        message: format!("{source}: {}", message.into()),
        status: SemanticStatus::Partial,
    }
}
