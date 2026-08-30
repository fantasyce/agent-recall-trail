use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use art_domain::{ArtError, ArtResult, memory::canonical_json_hash};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema: String,
    pub generator: String,
    pub edition_count: u64,
    pub event_count: u64,
    pub tree_sha256: String,
    pub files: Vec<BackupFile>,
}

pub fn create_backup(source: &Path, target: &Path, generator: &str) -> ArtResult<BackupManifest> {
    if target.exists() {
        return Err(ArtError::DuplicateConflict);
    }
    fs::create_dir(target).map_err(io_error)?;
    set_private_dir(target)?;

    let result = create_backup_inner(source, target, generator);
    if result.is_err() {
        let _ = fs::remove_dir_all(target);
    }
    result
}

fn create_backup_inner(source: &Path, target: &Path, generator: &str) -> ArtResult<BackupManifest> {
    let mut files = Vec::new();
    copy_tree(
        &source.join("editions"),
        &source.join("editions"),
        &target.join("knowledge/editions"),
        "knowledge/editions",
        &mut files,
    )?;
    copy_tree(
        &source.join(".art/events"),
        &source.join(".art/events"),
        &target.join("knowledge/events"),
        "knowledge/events",
        &mut files,
    )?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let edition_count = files
        .iter()
        .filter(|file| {
            file.path.starts_with("knowledge/editions/") && extension_is(&file.path, "json")
        })
        .count() as u64;
    let event_count = files
        .iter()
        .filter(|file| {
            file.path.starts_with("knowledge/events/") && extension_is(&file.path, "json")
        })
        .count() as u64;
    let tree_sha256 = digest(&serde_json::to_vec(&files).map_err(internal_error)?);
    let manifest = BackupManifest {
        schema: "art.knowledge.backup.v1".into(),
        generator: generator.into(),
        edition_count,
        event_count,
        tree_sha256,
        files,
    };
    write_private(
        &target.join("art-backup.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(internal_error)?,
    )?;
    Ok(manifest)
}

fn copy_tree(
    source_root: &Path,
    current: &Path,
    target_root: &Path,
    target_prefix: &str,
    files: &mut Vec<BackupFile>,
) -> ArtResult<()> {
    if !current.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(current).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(ArtError::PathConflict(
            "symbolic links are not backed up".into(),
        ));
    }
    if metadata.is_dir() {
        if current == source_root {
            fs::create_dir_all(target_root).map_err(io_error)?;
            set_private_dir(target_root)?;
        }
        let mut entries = fs::read_dir(current)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            copy_tree(
                source_root,
                &entry.path(),
                target_root,
                target_prefix,
                files,
            )?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(ArtError::PathConflict(
            "backup source must be a regular file".into(),
        ));
    }
    reject_hard_link(&metadata)?;
    let relative = current
        .strip_prefix(source_root)
        .map_err(|_| ArtError::PathConflict("backup path escaped its source root".into()))?;
    let extension = current.extension().and_then(|value| value.to_str());
    if !matches!(extension, Some("md" | "json")) {
        return Err(ArtError::InvalidInput(
            "knowledge backup contains an unsupported file".into(),
        ));
    }
    let destination = target_root.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
        set_private_dir(parent)?;
    }
    let bytes = fs::read(current).map_err(io_error)?;
    write_private(&destination, &bytes)?;
    let relative_text = relative
        .to_str()
        .ok_or_else(|| ArtError::InvalidInput("backup path must be UTF-8".into()))?
        .replace('\\', "/");
    files.push(BackupFile {
        path: format!("{target_prefix}/{relative_text}"),
        bytes: bytes.len() as u64,
        sha256: digest(&bytes),
    });
    Ok(())
}

pub fn verify_backup(root: &Path) -> ArtResult<BackupManifest> {
    let root_metadata = fs::symlink_metadata(root).map_err(io_error)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(ArtError::PathConflict(
            "backup root must be a regular directory".into(),
        ));
    }
    let manifest_path = root.join("art-backup.json");
    let manifest_metadata = fs::symlink_metadata(&manifest_path).map_err(io_error)?;
    if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
        return Err(ArtError::PathConflict(
            "backup manifest must be a regular file".into(),
        ));
    }
    reject_hard_link(&manifest_metadata)?;
    let manifest: BackupManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(io_error)?)
            .map_err(internal_error)?;
    if manifest.schema != "art.knowledge.backup.v1" || manifest.generator.trim().is_empty() {
        return Err(ArtError::InvalidInput(
            "invalid knowledge backup manifest".into(),
        ));
    }

    let declared: Vec<&str> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    if declared.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ArtError::InvalidInput(
            "backup inventory must be strictly sorted and unique".into(),
        ));
    }
    let mut actual = collect_inventory(root)?;
    for repository_metadata in [
        "README.md",
        "recovery/recovery-manifest.json",
        "recovery/control-and-key.tar.age",
    ] {
        actual.remove(repository_metadata);
    }
    let declared_set: BTreeSet<&str> = declared.iter().copied().collect();
    if actual.len() != declared_set.len()
        || actual
            .iter()
            .any(|path| !declared_set.contains(path.as_str()))
    {
        return Err(ArtError::InvalidInput(
            "backup inventory does not match the manifest".into(),
        ));
    }

    for file in &manifest.files {
        validate_relative_path(&file.path)?;
        if !is_allowlisted(&file.path) {
            return Err(ArtError::PathConflict(
                "backup manifest contains an unsupported path".into(),
            ));
        }
        let path = root.join(&file.path);
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ArtError::PathConflict(
                "backup entries must be regular files".into(),
            ));
        }
        reject_hard_link(&metadata)?;
        let bytes = fs::read(&path).map_err(io_error)?;
        if bytes.len() as u64 != file.bytes || digest(&bytes) != file.sha256 {
            return Err(ArtError::IndexDegraded);
        }
        if file.path.starts_with("knowledge/editions/") && extension_is(&file.path, "json") {
            verify_edition(root, &file.path, &bytes)?;
        } else if file.path.starts_with("knowledge/events/") {
            verify_event(&bytes)?;
        }
    }

    let edition_count = manifest
        .files
        .iter()
        .filter(|file| {
            file.path.starts_with("knowledge/editions/") && extension_is(&file.path, "json")
        })
        .count() as u64;
    let event_count = manifest
        .files
        .iter()
        .filter(|file| file.path.starts_with("knowledge/events/"))
        .count() as u64;
    let tree_sha256 = digest(&serde_json::to_vec(&manifest.files).map_err(internal_error)?);
    if edition_count != manifest.edition_count
        || event_count != manifest.event_count
        || tree_sha256 != manifest.tree_sha256
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(manifest)
}

pub fn restore_backup(
    source: &Path,
    target_vault: &Path,
    commitment_key: [u8; 32],
) -> ArtResult<super::KnowledgeDiagnostics> {
    if target_vault.exists() {
        return Err(ArtError::DuplicateConflict);
    }
    let manifest = verify_backup(source)?;
    let parent = target_vault
        .parent()
        .ok_or_else(|| ArtError::InvalidInput("restore target requires a parent".into()))?;
    let name = target_vault
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ArtError::InvalidInput("restore target requires a UTF-8 name".into()))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let staging = parent.join(format!("{name}.restore-staging-{}", Ulid::new()));
    fs::create_dir(&staging).map_err(io_error)?;
    set_private_dir(&staging)?;

    let result = restore_backup_inner(source, &staging, commitment_key, &manifest);
    match result {
        Ok(diagnostics) => match fs::rename(&staging, target_vault) {
            Ok(()) => Ok(diagnostics),
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                Err(io_error(error))
            }
        },
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn restore_backup_inner(
    source: &Path,
    staging: &Path,
    commitment_key: [u8; 32],
    manifest: &BackupManifest,
) -> ArtResult<super::KnowledgeDiagnostics> {
    let mut copied = Vec::new();
    copy_tree(
        &source.join("knowledge/editions"),
        &source.join("knowledge/editions"),
        &staging.join("editions"),
        "editions",
        &mut copied,
    )?;
    copy_tree(
        &source.join("knowledge/events"),
        &source.join("knowledge/events"),
        &staging.join(".art/events"),
        ".art/events",
        &mut copied,
    )?;
    let vault = super::KnowledgeVault::open(staging, commitment_key)?;
    vault.rebuild_projection()?;
    let diagnostics = vault.diagnostics()?;
    if !diagnostics.integrity_ok
        || diagnostics.foreign_key_violations != 0
        || !diagnostics.search_index_aligned
        || !diagnostics.projection_hashes_ok
        || diagnostics.projection_count != manifest.edition_count
        || diagnostics.manifest_files_verified != manifest.edition_count
        || diagnostics.event_files_verified != manifest.event_count
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(diagnostics)
}

fn collect_inventory(root: &Path) -> ArtResult<BTreeSet<String>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeSet<String>) -> ArtResult<()> {
        let metadata = fs::symlink_metadata(current).map_err(io_error)?;
        if current == root.join(".git") {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(ArtError::PathConflict(
                    "backup Git metadata must be a regular directory".into(),
                ));
            }
            return Ok(());
        }
        if metadata.file_type().is_symlink() {
            return Err(ArtError::PathConflict(
                "symbolic links are not allowed".into(),
            ));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(current).map_err(io_error)? {
                visit(root, &entry.map_err(io_error)?.path(), files)?;
            }
            return Ok(());
        }
        if !metadata.is_file() {
            return Err(ArtError::PathConflict(
                "backup contains a non-regular file".into(),
            ));
        }
        reject_hard_link(&metadata)?;
        let relative = current
            .strip_prefix(root)
            .map_err(|_| ArtError::PathConflict("backup inventory escaped its root".into()))?;
        let relative = relative
            .to_str()
            .ok_or_else(|| ArtError::InvalidInput("backup path must be UTF-8".into()))?
            .replace('\\', "/");
        if relative != "art-backup.json" {
            files.insert(relative);
        }
        Ok(())
    }

    let mut files = BTreeSet::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn validate_relative_path(path: &str) -> ArtResult<()> {
    let parsed = Path::new(path);
    if path.is_empty()
        || parsed.is_absolute()
        || parsed
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArtError::PathConflict("invalid backup path".into()));
    }
    Ok(())
}

fn is_allowlisted(path: &str) -> bool {
    (path.starts_with("knowledge/editions/")
        && matches!(
            Path::new(path).extension().and_then(|value| value.to_str()),
            Some("md" | "json")
        ))
        || (path.starts_with("knowledge/events/") && extension_is(path, "json"))
}

fn extension_is(path: &str, expected: &str) -> bool {
    Path::new(path).extension() == Some(std::ffi::OsStr::new(expected))
}

fn verify_edition(root: &Path, manifest_relative: &str, manifest_bytes: &[u8]) -> ArtResult<()> {
    let manifest: super::SharedManifest =
        serde_json::from_slice(manifest_bytes).map_err(internal_error)?;
    if manifest.schema != "art.knowledge.edition.v1" {
        return Err(ArtError::IndexDegraded);
    }
    let markdown_relative = Path::new(manifest_relative).with_extension("md");
    let markdown_path = root.join(&markdown_relative);
    let markdown = fs::read_to_string(markdown_path).map_err(io_error)?;
    let knowledge_body = markdown
        .split_once("## Knowledge\n\n")
        .map(|(_, body)| body.strip_suffix('\n').unwrap_or(body))
        .ok_or(ArtError::IndexDegraded)?;
    let manifest_sha256 = digest(manifest_bytes);
    if digest(knowledge_body.as_bytes()) != manifest.markdown_body_sha256
        || !markdown.contains(&format!("manifest_sha256: {manifest_sha256}"))
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(())
}

fn verify_event(bytes: &[u8]) -> ArtResult<()> {
    let mut event: serde_json::Value = serde_json::from_slice(bytes).map_err(internal_error)?;
    let stored_hash = event
        .as_object_mut()
        .and_then(|object| object.remove("event_hash"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or(ArtError::IndexDegraded)?;
    let schema = event["schema"].as_str().ok_or(ArtError::IndexDegraded)?;
    if !matches!(
        schema,
        "art.knowledge.revocation.v1" | "art.knowledge.supersession.v1"
    ) || canonical_json_hash(&event) != stored_hash
    {
        return Err(ArtError::IndexDegraded);
    }
    Ok(())
}

#[cfg(unix)]
fn reject_hard_link(metadata: &fs::Metadata) -> ArtResult<()> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() > 1 {
        Err(ArtError::PathConflict("hard links are not allowed".into()))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn reject_hard_link(_metadata: &fs::Metadata) -> ArtResult<()> {
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn write_private(path: &Path, bytes: &[u8]) -> ArtResult<()> {
    fs::write(path, bytes).map_err(io_error)?;
    set_private_file(path)
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> ArtResult<()> {
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> ArtError {
    ArtError::Io(error.to_string())
}

fn internal_error(error: impl std::fmt::Display) -> ArtError {
    ArtError::Internal(error.to_string())
}
