use std::fs;
use std::sync::{Arc, Barrier};

use art_agent_store::AutoMemoryConfigStore;
use tempfile::tempdir;

#[test]
fn missing_or_malformed_global_config_fails_closed_without_blocking_explicit_state() {
    let root = tempdir().unwrap();
    let store = AutoMemoryConfigStore::new(root.path());

    let missing = store.status();
    assert!(!missing.enabled);
    assert!(!missing.configured);
    assert_eq!(missing.reason.as_deref(), Some("configuration_missing"));
    assert_eq!(missing.max_captures_per_session, 3);
    assert_eq!(missing.cooldown_seconds, 600);

    fs::create_dir_all(root.path().join("config/art")).unwrap();
    fs::write(store.path(), b"not-json").unwrap();
    let malformed = store.status();
    assert!(!malformed.enabled);
    assert!(malformed.configured);
    assert_eq!(malformed.reason.as_deref(), Some("configuration_invalid"));
}

#[test]
fn setting_is_machine_wide_persistent_and_separate_from_agent_identity() {
    let root = tempdir().unwrap();
    let first = AutoMemoryConfigStore::new(root.path());
    let enabled = first.set_enabled(true, "human:governance-ui").unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.updated_by.as_deref(), Some("human:governance-ui"));

    let after_restart = AutoMemoryConfigStore::new(root.path()).status();
    let another_agent_view = AutoMemoryConfigStore::new(root.path()).status();
    assert!(after_restart.enabled);
    assert!(another_agent_view.enabled);
    assert_eq!(after_restart.config_version, 1);
    assert_eq!(after_restart.policy_version, "art.auto-memory.policy.v1");

    let disabled = first.set_enabled(false, "human:governance-ui").unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.config_version, 2);
    assert!(!AutoMemoryConfigStore::new(root.path()).status().enabled);
}

#[cfg(unix)]
#[test]
fn persisted_global_config_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    let store = AutoMemoryConfigStore::new(root.path());
    store.set_enabled(true, "human:governance-ui").unwrap();
    assert_eq!(
        fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(store.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[cfg(unix)]
#[test]
fn unreadable_config_fails_closed() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    let store = AutoMemoryConfigStore::new(root.path());
    store.set_enabled(true, "human:governance-ui").unwrap();
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o000)).unwrap();
    let status = store.status();
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!status.enabled);
    assert_eq!(status.reason.as_deref(), Some("configuration_unreadable"));
}

#[test]
fn invalid_limits_fail_closed() {
    let root = tempdir().unwrap();
    let store = AutoMemoryConfigStore::new(root.path());
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(
        store.path(),
        br#"{"schema":"art.auto-memory.config.v1","enabled":true,"max_captures_per_session":0,"cooldown_seconds":600,"policy_version":"art.auto-memory.policy.v1","config_version":1,"updated_at":"2026-09-15T00:00:00Z","updated_by":"human:governance-ui"}"#,
    )
    .unwrap();

    let status = store.status();
    assert!(!status.enabled);
    assert_eq!(status.reason.as_deref(), Some("configuration_invalid"));
}

#[test]
fn concurrent_human_updates_are_serialized_without_losing_audit_versions() {
    let root = tempdir().unwrap();
    let store = AutoMemoryConfigStore::new(root.path());
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|index| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .set_enabled(index % 2 == 0, "human:governance-ui")
                    .unwrap()
                    .config_version
            })
        })
        .collect();
    let mut versions: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    versions.sort_unstable();
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(store.status().config_version, 8);
}
