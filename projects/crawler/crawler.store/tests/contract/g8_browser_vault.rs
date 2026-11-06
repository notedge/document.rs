//! G-PANDARK-6：browser session + challenge，只留 vault 句柄且无 secret 落盘。

use crawler_store::{BrowserSessionPersistence, BrowserSessionRecord, BrowserSnapshotInput, ObjectKind, RunSpec};
use crawler_types::PageChallengeState;
use std::time::Duration;

use crate::contract::support::{LONG_LEASE, assert_both};

fn redacted_archive() -> Vec<u8> {
    b"From: <https://example.com/>\r\nSubject: challenge page\r\n\r\n<html><body>operator review needed</body></html>".to_vec()
}

#[test]
fn g8_browser_snapshot_roundtrips_through_the_object_store() {
    assert_both!("g8-snapshot", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        let archive = redacted_archive();
        let record = store
            .put_browser_snapshot(BrowserSnapshotInput {
                snapshot_key: "snap/thread-1".into(),
                page_url: "https://example.com/challenge".into(),
                archive: archive.clone(),
                media_type: Some("multipart/related".into()),
                transport_provenance: "browser".into(),
                captured_at_epoch: 1_760_000_000,
                ttl: Some(Duration::from_secs(3600)),
                vault_session_ref: Some("vault://pandark/session/thread-1".into()),
            })
            .expect("put snapshot");
        assert_eq!(record.snapshot_ref.kind, ObjectKind::BrowserArchive);
        assert_eq!(record.snapshot_ref.size, archive.len() as u64);
        assert_eq!(record.vault_session_ref.as_deref(), Some("vault://pandark/session/thread-1"));
        assert_eq!(store.get_object(&record.snapshot_ref.fingerprint).expect("archive bytes"), Some(archive.clone()));
        let stored = store.get_browser_snapshot("snap/thread-1").expect("snapshot record").expect("record exists");
        assert_eq!(stored.snapshot_ref.fingerprint, record.snapshot_ref.fingerprint);
        let json = serde_json::to_string(&stored).expect("snapshot json");
        assert!(!json.to_ascii_lowercase().contains("cookie"), "record must not carry cookie material: {json}");

        store.reopen().expect("reopen");
        assert_eq!(store.get_object(&record.snapshot_ref.fingerprint).expect("archive after reopen"), Some(archive));
        assert!(store.get_browser_snapshot("snap/thread-1").expect("record after reopen").is_some());
    });
}

#[test]
fn g8_browser_snapshot_rejects_credential_plaintext() {
    assert_both!("g8-redact", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        let poisoned = b"HTTP/1.1 200 OK\r\nSet-Cookie: sid=abc123\r\n\r\n<html></html>".to_vec();
        let fingerprint = blake3::hash(&poisoned).to_hex().to_string();
        let error = store
            .put_browser_snapshot(BrowserSnapshotInput {
                snapshot_key: "snap/poisoned".into(),
                page_url: "https://example.com/challenge".into(),
                archive: poisoned,
                media_type: Some("multipart/related".into()),
                transport_provenance: "browser".into(),
                captured_at_epoch: 1_760_000_000,
                ttl: None,
                vault_session_ref: None,
            })
            .expect_err("cookie plaintext must fail closed");
        let message = error.to_string();
        assert!(message.contains("credential"), "rejection must name the credential rule: {message}");
        assert!(store.get_browser_snapshot("snap/poisoned").expect("no record").is_none());
        assert!(store.object_ref(&fingerprint).expect("no object metadata").is_none());
        assert!(store.get_object(&fingerprint).expect("no object bytes").is_none());

        let error = store
            .put_browser_snapshot(BrowserSnapshotInput {
                snapshot_key: "snap/inline-secret".into(),
                page_url: "https://example.com/challenge".into(),
                archive: redacted_archive(),
                media_type: None,
                transport_provenance: "browser".into(),
                captured_at_epoch: 1_760_000_000,
                ttl: None,
                vault_session_ref: Some("cookie=sid=abc123".into()),
            })
            .expect_err("inlined cookie must not pass as a vault handle");
        assert!(error.to_string().contains("opaque"));
        assert!(store.get_browser_snapshot("snap/inline-secret").expect("no record").is_none());
    });
}

#[test]
fn g8_challenge_pause_lands_in_run_state() {
    assert_both!("g8-challenge", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        let pause = store
            .record_challenge_pause(
                "https://example.com/login",
                "https://example.com/login",
                PageChallengeState::ChallengeRequired,
            )
            .expect("record pause");
        assert_eq!(pause.state, PageChallengeState::ChallengeRequired);
        assert_eq!(pause.page_url, "https://example.com/login");
        let stored = store.challenge_pause("https://example.com/login").expect("pause lookup").expect("pause exists");
        assert_eq!(stored, pause);
        let json = serde_json::to_string(&stored).expect("pause json");
        assert!(!json.to_ascii_lowercase().contains("cookie"), "pause record must not carry cookie material: {json}");

        assert_eq!(store.event_count().expect("event count"), 1, "pause appends one report event");
        assert_eq!(store.save_checkpoint_view().expect("save view"), 1);
        assert_eq!(store.doctor().expect("doctor").checkpoint_lag_records, 0);

        store.reopen().expect("reopen");
        assert_eq!(store.challenge_pause("https://example.com/login").expect("pause after reopen"), Some(stored));
    });
}

#[test]
fn g8_browser_session_keeps_only_an_external_vault_handle() {
    assert_both!("g8-session", LONG_LEASE, |store| {
        store.begin_run(RunSpec { seed_url: "https://example.com/".into() }).expect("begin run");
        let record = BrowserSessionRecord {
            persistence: BrowserSessionPersistence::ExternalVault,
            vault_session_ref: Some("vault://pandark/session/run-1".into()),
            expires_at_ms: Some(1_760_003_600_000),
            allowed_origins: vec!["example.com".into()],
        };
        store.put_browser_session(&record).expect("put session");
        let stored = store.browser_session().expect("session read").expect("session exists");
        assert_eq!(stored, record);
        let json = serde_json::to_string(&stored).expect("session json");
        assert!(!json.to_ascii_lowercase().contains("cookie"), "session record must not carry cookie material: {json}");
        assert!(!json.contains('='), "the vault handle stays opaque: {json}");

        let cookie = BrowserSessionRecord { vault_session_ref: Some("cookie: sid=abc".into()), ..record.clone() };
        let error = store.put_browser_session(&cookie).expect_err("inlined cookie must be rejected");
        assert!(error.to_string().contains("opaque"));

        let no_vault = BrowserSessionRecord { vault_session_ref: None, ..record };
        let error = store.put_browser_session(&no_vault).expect_err("external vault without a handle must be rejected");
        assert!(error.to_string().contains("requires a vault session reference"));

        store.reopen().expect("reopen");
        assert_eq!(store.browser_session().expect("session after reopen"), Some(stored));
    });
}
