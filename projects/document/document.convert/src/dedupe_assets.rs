//! Content-addressable asset deduplication for Markdown projects.
//!
//! After `notedown-formats` exports a [`MarkdownProject`], the asset names
//! are derived from source file names and may contain duplicates (two images
//! with identical bytes but different origins). This module:
//!
//! 1. Computes a BLAKE3 fingerprint for every asset payload.
//! 2. Collapses assets that share identical bytes into a single canonical entry.
//! 3. Rewrites each asset's `relative_path` to a content-addressed name
//!    (`assets/{blake3_prefix}.{ext}`) preserving the original file extension.
//! 4. Applies the same path rewrite to every image reference in the Markdown
//!    body (`index.md` plus chapter bodies).
//!
//! The caller invokes [`dedupe_markdown_project`] after export but before
//! publication.

use std::collections::HashMap;

use blake3::Hasher;
use document_formats::export::markdown_project::{MarkdownProject, MarkdownProjectAsset};

/// Length of the BLAKE3 hex prefix used in content-addressed file names.
const FINGERPRINT_PREFIX_LEN: usize = 16;

/// Collapse duplicate assets and rewrite the Markdown project to use
/// content-addressed file names.
///
/// Returns the number of **unique** assets that remain after deduplication.
pub fn dedupe_markdown_project(project: &mut MarkdownProject) -> usize {
    let mut fingerprint_to_path: HashMap<blake3::Hash, String> = HashMap::new();
    let mut old_to_new: HashMap<String, String> = HashMap::new();
    let mut rewritten_assets: Vec<_> = Vec::new();

    for asset in project.assets.drain(..) {
        let fingerprint = blake3_hash(&asset.bytes);
        let is_first = !fingerprint_to_path.contains_key(&fingerprint);
        let canonical_path = fingerprint_to_path
            .entry(fingerprint)
            .or_insert_with(|| content_address_path(&asset.relative_path, &fingerprint))
            .clone();

        old_to_new.insert(asset.relative_path, canonical_path.clone());

        // Only keep the first occurrence of each fingerprint.
        if is_first {
            rewritten_assets.push(MarkdownProjectAsset {
                relative_path: canonical_path,
                bytes: asset.bytes,
                asset_id: asset.asset_id,
            });
        }
    }

    project.assets = rewritten_assets;

    // Rewrite image references in the Markdown bodies.
    rewrite_markdown_refs(&mut project.index_markdown, &old_to_new);
    for chapter in &mut project.chapters {
        rewrite_markdown_refs(&mut chapter.markdown, &old_to_new);
    }

    fingerprint_to_path.len()
}

/// Compute a BLAKE3 hash over `bytes`.
fn blake3_hash(bytes: &[u8]) -> blake3::Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

/// Build a content-addressed path like `assets/a1b2c3d4e5f6g7h8.png`.
fn content_address_path(old_path: &str, fingerprint: &blake3::Hash) -> String {
    let ext = old_path.rsplit('.').next().filter(|e| !e.is_empty() && e.len() <= 8).unwrap_or("bin");
    let hex = fingerprint.to_hex();
    let prefix = hex.as_str().split_at(FINGERPRINT_PREFIX_LEN).0;
    format!("assets/{prefix}.{ext}")
}

/// Replace Markdown image/link targets that match an old asset path
/// with their content-addressed replacement.
///
/// Handles four common Markdown reference forms:
///   `![alt](assets/logo.png)`
///   `![alt](./assets/logo.png)`
///   `![alt](assets/logo.png "title")`
///   `[label](assets/logo.png)`
///
/// Only paths present in `old_to_new` are rewritten.
fn rewrite_markdown_refs(body: &mut String, old_to_new: &HashMap<String, String>) {
    if old_to_new.is_empty() {
        return;
    }

    let mut replaced = body.clone();
    for (old, new) in old_to_new {
        // Replace exact occurrences within Markdown link/image targets.
        // We match the path as it appears inside parentheses — either bare
        // `(assets/x.png)` or `(./assets/x.png)` or with a title suffix.
        let patterns = [format!("]({old})"), format!("](./{old})"), format!("]({old} "), format!("](./{old} ")];
        for pattern in &patterns {
            let replacement = format!("]({new})");
            let replacement_with_space = format!("]({new} ");
            replaced = replaced.replace(pattern, if pattern.ends_with(' ') { &replacement_with_space } else { &replacement });
        }
    }
    *body = replaced;
}
