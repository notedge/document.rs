//! Shared helpers for the Y-1..Y-8 store contract suite.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crawler_store::frontier_item_from_contract;

/// Host partition used by every contract case.
pub const HOST: &str = "example.com";

/// Lease used by contract cases that do not exercise expiry.
pub const TEST_LEASE: Duration = Duration::from_millis(400);

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
    std::env::temp_dir().join(format!("crawler-store-{label}-{pid}-{nonce}.yydb"))
}

/// Remove the single-file database and its journal siblings.
pub fn cleanup_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
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
