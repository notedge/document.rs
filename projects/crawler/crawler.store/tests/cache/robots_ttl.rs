//! robots 缓存 TTL。

use std::{thread, time::Duration};

#[cfg(feature = "yydb")]
use crawler_store::YydbStore;
use crawler_store::{MemoryStore, Store, StoreConfig};

const SITE_KEY: &str = "example.com";
const ROBOTS_BODY: &[u8] = b"User-agent: *\nDisallow: /private";
// Backends persist state synchronously on write, so a millisecond-scale TTL can
// expire before the immediate read on a loaded machine. Keep a wide margin.
const TTL: Duration = Duration::from_millis(1_000);
const EXPIRY_WAIT: Duration = Duration::from_millis(1_100);

fn assert_robots_ttl<S: Store>(make_store: impl FnOnce() -> S) {
    let mut store = make_store();
    store.put_robots_cache(SITE_KEY, ROBOTS_BODY, TTL).expect("put robots cache");
    assert_eq!(store.get_robots_cache(SITE_KEY).expect("get robots cache").as_deref(), Some(ROBOTS_BODY));
    thread::sleep(EXPIRY_WAIT);
    assert!(store.get_robots_cache(SITE_KEY).expect("get robots cache after ttl").is_none(), "expired robots cache must miss");
}

#[test]
fn robots_ttl_memory() {
    assert_robots_ttl(|| MemoryStore::open(StoreConfig::memory()).expect("open"));
}

#[cfg(feature = "yydb")]
mod yydb_helpers {
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    pub fn temp_db_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        std::env::temp_dir().join(format!("crawler-store-{label}-{nonce}.yydb"))
    }

    pub fn cleanup_db(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}

#[cfg(feature = "yydb")]
#[test]
fn robots_ttl_yydb() {
    let path = yydb_helpers::temp_db_path("robots-ttl");
    assert_robots_ttl(|| YydbStore::open(StoreConfig::yydb(&path)).expect("open"));
    yydb_helpers::cleanup_db(&path);
}
