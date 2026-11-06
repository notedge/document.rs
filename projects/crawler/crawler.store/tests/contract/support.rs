//! Shared helpers for the Y-1..Y-8 store contract suite.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crawler_store::frontier_item_from_contract;

/// Host partition used by every contract case.
pub const HOST: &str = "example.com";

/// Lease used by contract cases that exercise expiry.
///
/// Short enough to expire inside a crash fixture and long enough that a
/// parallel test run cannot expire it between `claim_next` and
/// `begin_page_tx`.
pub const TEST_LEASE: Duration = Duration::from_secs(2);

/// Lease used by long claim loops so a loaded machine cannot expire mid-case.
pub const LONG_LEASE: Duration = Duration::from_secs(30);

/// Build a frontier item on the shared host partition.
pub fn item(path: &str) -> crawler_types::FrontierItem {
    frontier_item_from_contract(&format!("https://example.com{path}"), 1).expect("frontier item")
}

/// Unique database path for one case.
pub fn temp_db_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let pid = std::process::id();
    std::env::temp_dir().join(format!("crawler-store-{label}-{pid}-{nonce}.yydx"))
}

/// Remove the database, sidecars and the sibling `<stem>-objects/` CAS root.
///
/// The blob root is drained file-by-file so cleanup never deletes a whole
/// directory tree recursively. Both the file and the root must stay inside the
/// OS temp directory.
pub fn cleanup_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    for sidecar in ["-wal", "-shm", "-lock"] {
        let _ = std::fs::remove_file(format!("{}{sidecar}", path.display()));
    }
    let root = object_root(path);
    if !root.starts_with(std::env::temp_dir()) {
        return;
    }
    let Some(name) = root.file_name().and_then(|name| name.to_str())
    else {
        return;
    };
    if !name.ends_with("-objects") {
        return;
    }
    remove_temp_tree(&root);
}

/// `<stem>-objects/` beside a `.yydx` main file.
fn object_root(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("crawler");
    path.with_file_name(format!("{stem}-objects"))
}

/// Delete a verified temp tree file-by-file, deepest directory last.
fn remove_temp_tree(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir)
        else {
            continue;
        };
        dirs.push(dir);
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                pending.push(entry_path);
            }
            else {
                files.push(entry_path);
            }
        }
    }
    for file in files {
        let _ = std::fs::remove_file(file);
    }
    for dir in dirs.into_iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Run the same assertion block against `MemoryStore` and, when enabled, `YydbStore`.
macro_rules! assert_both {
    ($label:expr, $lease:expr, |$store:ident| $body:block) => {
        assert_both!($label, $lease, |$store, _unused_backend| $body);
    };
    ($label:expr, $lease:expr, |$store:ident, $backend:ident| $body:block) => {{
        use crawler_store::Store as _;
        {
            let mut opened = crawler_store::MemoryStore::open(
                crawler_store::StoreConfig::memory().with_claim_lease($lease),
            )
            .expect("open memory store");
            let $store = &mut opened;
            let $backend = "memory";
            $body
        }
        #[cfg(feature = "yydb")]
        {
            let path = crate::contract::support::temp_db_path($label);
            let mut opened = crawler_store::YydbStore::open(
                crawler_store::StoreConfig::yydb(&path).with_claim_lease($lease),
            )
            .expect("open yydb store");
            let $store = &mut opened;
            let $backend = "yydb";
            $body
            drop(opened);
            crate::contract::support::cleanup_db(&path);
        }
    }};
}

pub(crate) use assert_both;
