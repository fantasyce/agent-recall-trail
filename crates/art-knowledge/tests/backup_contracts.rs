use art_domain::{
    agent::AgentId,
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor},
};
use art_knowledge::{
    KnowledgeVault,
    backup::{create_backup, restore_backup, verify_backup},
};
use std::{fs, str::FromStr};
use tempfile::tempdir;

fn published_vault() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = KnowledgeVault::open(root.path(), [19_u8; 32]).unwrap();
    let proposal = vault
        .propose(
            &agent,
            KnowledgeDraft::minimal(
                "operations.backup",
                "ART backup",
                "Verified immutable knowledge.",
            ),
            vec![ProposalSourceLock {
                source_type: ProposalSourceType::ExternalDocument,
                owner_agent_id: None,
                source_id: "design-spec".into(),
                source_revision: Some(1),
                source_content_hash:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                anchor_set_hash: None,
                approved_excerpt_hash: None,
                use_grant_id: None,
            }],
            "backup-proposal",
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    vault
        .publish(&proposal.id, proposal.revision, true)
        .unwrap();
    root
}

#[test]
fn deterministic_backup_is_path_and_time_independent() {
    let source = published_vault();
    let first_root = tempdir().unwrap();
    let second_root = tempdir().unwrap();
    let first = first_root.path().join("snapshot-a");
    let second = second_root.path().join("snapshot-b");

    let first_manifest = create_backup(source.path(), &first, "art 0.1.1").unwrap();
    let second_manifest = create_backup(source.path(), &second, "art 0.1.1").unwrap();

    assert_eq!(first_manifest, second_manifest);
    assert_eq!(first_manifest.schema, "art.knowledge.backup.v1");
    assert_eq!(first_manifest.generator, "art 0.1.1");
    assert_eq!(first_manifest.edition_count, 1);
    assert_eq!(first_manifest.event_count, 0);
    assert_eq!(first_manifest.files.len(), 2);
    assert!(
        first_manifest
            .files
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
    );
    assert!(
        first_manifest
            .files
            .iter()
            .all(|item| item.path.starts_with("knowledge/editions/"))
    );
    assert_eq!(
        fs::read(first.join("art-backup.json")).unwrap(),
        fs::read(second.join("art-backup.json")).unwrap()
    );
}

fn one_backup() -> (tempfile::TempDir, std::path::PathBuf) {
    let source = published_vault();
    let output_root = tempdir().unwrap();
    let output = output_root.path().join("snapshot");
    create_backup(source.path(), &output, "art 0.1.1").unwrap();
    (output_root, output)
}

#[test]
fn verify_backup_accepts_only_the_exact_hashed_inventory() {
    let (_root, output) = one_backup();
    let manifest = verify_backup(&output).unwrap();
    assert_eq!(manifest.edition_count, 1);
    assert_eq!(manifest.files.len(), 2);

    let markdown = manifest
        .files
        .iter()
        .find(|item| {
            std::path::Path::new(&item.path).extension() == Some(std::ffi::OsStr::new("md"))
        })
        .unwrap();
    fs::write(output.join(&markdown.path), "corrupted").unwrap();
    assert!(verify_backup(&output).is_err());
}

#[test]
fn verify_backup_rejects_unlisted_and_malformed_content() {
    let (_root, output) = one_backup();
    fs::write(output.join("knowledge/unlisted.txt"), "not canonical").unwrap();
    assert!(verify_backup(&output).is_err());

    fs::remove_file(output.join("knowledge/unlisted.txt")).unwrap();
    let manifest_path = output.join("art-backup.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["files"].as_array_mut().unwrap().reverse();
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    assert!(verify_backup(&output).is_err());
}

#[test]
fn create_backup_rejects_a_corrupt_source_and_removes_the_target() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(source.join("editions/corrupt")).unwrap();
    fs::create_dir_all(source.join(".art/events")).unwrap();
    fs::write(source.join("editions/corrupt/1-bad.json"), b"not-json").unwrap();
    fs::write(source.join("editions/corrupt/1-bad.md"), b"not-an-edition").unwrap();
    let target = root.path().join("backup");
    assert!(create_backup(&source, &target, "art 0.1.1").is_err());
    assert!(!target.exists());
}

#[cfg(unix)]
#[test]
fn verify_backup_rejects_symbolic_and_hard_link_aliases() {
    use std::os::unix::fs::symlink;

    let (_root, output) = one_backup();
    let manifest = create_backup(
        published_vault().path(),
        &output.with_extension("other"),
        "art 0.1.1",
    )
    .unwrap();
    let canonical = output.with_extension("other").join(&manifest.files[0].path);
    let outside = tempdir().unwrap();
    let alias = outside.path().join("alias");
    fs::hard_link(&canonical, &alias).unwrap();
    assert!(verify_backup(&output.with_extension("other")).is_err());

    let (_root, output) = one_backup();
    let manifest = verify_backup(&output).unwrap();
    let canonical = output.join(&manifest.files[0].path);
    let bytes = fs::read(&canonical).unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("target");
    fs::write(&target, bytes).unwrap();
    fs::remove_file(&canonical).unwrap();
    symlink(&target, &canonical).unwrap();
    assert!(verify_backup(&output).is_err());
}

#[test]
fn restore_backup_rebuilds_an_exact_healthy_projection() {
    let (_backup_root, backup) = one_backup();
    let target_root = tempdir().unwrap();
    let target = target_root.path().join("restored-vault");

    let diagnostics = restore_backup(&backup, &target, [19_u8; 32]).unwrap();

    assert!(diagnostics.integrity_ok);
    assert!(diagnostics.projection_hashes_ok);
    assert!(diagnostics.search_index_aligned);
    assert_eq!(diagnostics.projection_count, 1);
    assert_eq!(diagnostics.current_edition_count, 1);
    assert_eq!(diagnostics.manifest_files_verified, 1);
    let restored = KnowledgeVault::open(&target, [19_u8; 32]).unwrap();
    assert_eq!(
        restored.current("operations.backup").unwrap().title,
        "ART backup"
    );
}

#[test]
fn restore_backup_refuses_overwrite_and_cleans_failed_staging() {
    let (_backup_root, backup) = one_backup();
    let target_root = tempdir().unwrap();
    let existing = target_root.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("owner-data"), "preserve").unwrap();
    assert!(restore_backup(&backup, &existing, [19_u8; 32]).is_err());
    assert_eq!(
        fs::read_to_string(existing.join("owner-data")).unwrap(),
        "preserve"
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("art-backup.json")).unwrap()).unwrap();
    let markdown = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| {
            let path = item["path"].as_str()?;
            (std::path::Path::new(path).extension() == Some(std::ffi::OsStr::new("md")))
                .then_some(path)
        })
        .unwrap();
    fs::write(backup.join(markdown), "corrupted").unwrap();
    let absent = target_root.path().join("absent");
    assert!(restore_backup(&backup, &absent, [19_u8; 32]).is_err());
    assert!(!absent.exists());
    assert!(fs::read_dir(target_root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("absent.restore-staging-")
    }));
}
