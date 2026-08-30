use std::{fs, process::Command, str::FromStr};

use art_agent_store::AgentVault;
use art_domain::{
    agent::{AgentId, ArtPaths},
    anchor::{AnchorKind, SourceAnchor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, SemanticPayload, Sensitivity,
    },
};
use chrono::Utc;
use serde_json::json;
use tempfile::tempdir;

fn art() -> Command {
    Command::new(env!("CARGO_BIN_EXE_art"))
}

#[test]
fn help_exposes_operator_and_mcp_surfaces() {
    let output = art().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for command in [
        "init",
        "agent",
        "doctor",
        "recall",
        "memory",
        "knowledge",
        "integration",
        "mcp",
        "import",
        "export",
        "reindex",
    ] {
        assert!(help.contains(command), "missing {command}");
    }
}

#[test]
fn init_and_agent_create_are_explicit_and_private() {
    let root = tempdir().unwrap();
    let denied = art()
        .args(["--home", root.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    let initialized = art()
        .args(["--home", root.path().to_str().unwrap(), "init", "--confirm"])
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let created = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "agent",
            "create",
            "--id",
            "codex-primary",
            "--host",
            "codex",
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let profile = root.path().join("config/art/agents/codex-primary.json");
    assert!(profile.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(profile).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let duplicate = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "agent",
            "create",
            "--id",
            "codex-primary",
            "--host",
            "dsh",
        ])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
}

#[test]
fn configuration_priority_is_cli_then_explicit_config_and_unknown_secret_fields_fail() {
    let root = tempdir().unwrap();
    let configured_home = root.path().join("configured-home");
    let cli_home = root.path().join("cli-home");
    let config = root.path().join("art-config.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({"schema":"art.config.v1","home":configured_home})).unwrap(),
    )
    .unwrap();
    assert!(
        art()
            .args(["--config", config.to_str().unwrap(), "init", "--confirm"])
            .status()
            .unwrap()
            .success()
    );
    assert!(configured_home.join("config/art/commitment.key").exists());
    assert!(
        art()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--home",
                cli_home.to_str().unwrap(),
                "init",
                "--confirm",
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(cli_home.join("config/art/commitment.key").exists());

    let unsafe_config = root.path().join("unsafe-config.json");
    fs::write(
        &unsafe_config,
        serde_json::to_vec(&json!({"schema":"art.config.v1","home":root.path().join("unsafe-home"),"api_key":"must-not-be-supported"})).unwrap(),
    )
    .unwrap();
    assert!(
        !art()
            .args([
                "--config",
                unsafe_config.to_str().unwrap(),
                "init",
                "--confirm",
            ])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn doctor_and_integrations_are_machine_readable_without_applying_config() {
    let root = tempdir().unwrap();
    assert!(
        art()
            .args(["--home", root.path().to_str().unwrap(), "init", "--confirm"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        art()
            .args([
                "--home",
                root.path().to_str().unwrap(),
                "agent",
                "create",
                "--id",
                "dsh-primary",
                "--host",
                "dsh"
            ])
            .status()
            .unwrap()
            .success()
    );
    let doctor = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "doctor",
            "--agent",
            "dsh-primary",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(doctor.status.success());
    let value: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(value["schema"], "art.cli.v1");
    assert_eq!(value["status"], "ok");
    let overlay = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "integration",
            "dsh",
            "--agent",
            "dsh-primary",
            "--print",
        ])
        .output()
        .unwrap();
    let overlay = String::from_utf8(overlay.stdout).unwrap();
    assert!(overlay.contains("transport: stdio"));
    assert!(overlay.contains("--agent"));
    assert!(overlay.contains("dsh-primary"));

    let generated = root.path().join("art-dsh.overlay.yml");
    let applied = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "integration",
            "dsh",
            "--agent",
            "dsh-primary",
            "--apply",
            "--output",
            generated.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(applied.status.success());
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("serverName: art")
    );
    let duplicate = art()
        .args([
            "--home",
            root.path().to_str().unwrap(),
            "integration",
            "dsh",
            "--agent",
            "dsh-primary",
            "--apply",
            "--output",
            generated.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
}

#[test]
fn markdown_import_is_read_only_until_confirmed_and_copies_only_to_new_target() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("one.md"), "# One\n").unwrap();
    fs::write(source.join("ignored.txt"), "ignore\n").unwrap();
    let target = root.path().join("copy");
    let art_home = root.path().join("art-home");

    let dry_run = art()
        .args([
            "--home",
            art_home.to_str().unwrap(),
            "import",
            "markdown",
            "--source",
            source.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(dry_run.status.success());
    assert!(!target.exists());

    let copied = art()
        .args([
            "--home",
            art_home.to_str().unwrap(),
            "import",
            "markdown",
            "--source",
            source.to_str().unwrap(),
            "--copy-to",
            target.to_str().unwrap(),
            "--confirm",
        ])
        .output()
        .unwrap();
    assert!(
        copied.status.success(),
        "{}",
        String::from_utf8_lossy(&copied.stderr)
    );
    assert_eq!(
        fs::read_to_string(source.join("one.md")).unwrap(),
        "# One\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("one.md")).unwrap(),
        "# One\n"
    );
    assert!(!target.join("ignored.txt").exists());
}

#[test]
fn markdown_scan_emits_reviewable_knowledge_proposals_and_blocks_unsafe_copy() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("one.md"),
        "---\ntitle: One\npermalink: duplicate\n---\nSee [[Missing]].\n",
    )
    .unwrap();
    fs::write(
        source.join("two.md"),
        "---\ntitle: Two\npermalink: duplicate\n---\npassword=not-allowed\n",
    )
    .unwrap();
    fs::write(source.join("data.bin"), "ignored").unwrap();
    let home = root.path().join("home");
    let output = art()
        .args([
            "--home",
            home.to_str().unwrap(),
            "import",
            "markdown",
            "--source",
            source.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["knowledge_import"]["schema"],
        "art.knowledge.import.scan.v1"
    );
    assert_eq!(report["knowledge_import"]["non_markdown_files"], 1);
    let proposals = report["knowledge_import"]["proposals"].as_array().unwrap();
    assert_eq!(proposals.len(), 2);
    assert!(
        proposals
            .iter()
            .all(|proposal| !proposal["eligible"].as_bool().unwrap())
    );
    assert!(proposals.iter().any(|proposal| {
        proposal["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning == "dangling_wiki_link")
    }));
    let target = root.path().join("copy");
    let copy = art()
        .args([
            "--home",
            home.to_str().unwrap(),
            "import",
            "markdown",
            "--source",
            source.to_str().unwrap(),
            "--copy-to",
            target.to_str().unwrap(),
            "--confirm",
        ])
        .output()
        .unwrap();
    assert!(!copy.status.success());
    assert!(!target.exists());
}

#[cfg(unix)]
#[test]
fn markdown_import_rejects_hard_linked_sources() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let original = source.join("original.md");
    fs::write(&original, "# Original\n").unwrap();
    fs::hard_link(&original, source.join("alias.md")).unwrap();
    let result = art()
        .args([
            "--home",
            root.path().join("art-home").to_str().unwrap(),
            "import",
            "markdown",
            "--source",
            source.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(original).unwrap(), "# Original\n");
}

#[test]
fn human_cli_can_compose_a_proposal_from_two_agent_vaults() {
    let root = tempdir().unwrap();
    let home = root.path().to_str().unwrap();
    assert!(
        art()
            .args(["--home", home, "init", "--confirm"])
            .status()
            .unwrap()
            .success()
    );
    for (agent, host) in [("codex-primary", "codex"), ("dsh-primary", "dsh")] {
        assert!(
            art()
                .args([
                    "--home", home, "agent", "create", "--id", agent, "--host", host,
                ])
                .status()
                .unwrap()
                .success()
        );
    }
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let mut refs = Vec::new();
    for (index, agent_name) in ["codex-primary", "dsh-primary"].into_iter().enumerate() {
        let agent = AgentId::from_str(agent_name).unwrap();
        let vault = AgentVault::open(paths.agent_vault(&agent), agent.clone()).unwrap();
        let mut memory = MemoryArtifact::new(
            agent.clone(),
            format!("source {index}"),
            format!("shared evidence {index}"),
            MemoryPayload::Semantic(SemanticPayload {
                statement: format!("shared evidence {index}"),
                applicability: "multi-agent proposal".into(),
                exceptions: vec![],
            }),
            MemoryScope::Repository("agent-recall-trail".into()),
            Sensitivity::Private,
            Utc::now(),
        )
        .unwrap();
        memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
        let anchor = SourceAnchor::new(
            agent,
            AnchorKind::UserStatement,
            format!("test:{agent_name}"),
            Some(format!("shared evidence {index}")),
            json!({}),
            Sensitivity::Private,
            Utc::now(),
        )
        .unwrap();
        vault
            .capture(&memory, &[anchor], &format!("compose-{index}"))
            .unwrap();
        refs.push(format!("{agent_name}:{}@1", memory.id));
    }
    let draft = root.path().join("draft.md");
    fs::write(&draft, "Reviewed combined evidence.").unwrap();
    let output = art()
        .args([
            "--home",
            home,
            "knowledge",
            "proposal",
            "compose",
            "--from",
            &refs[0],
            "--from",
            &refs[1],
            "--knowledge-key",
            "combined.evidence",
            "--title",
            "Combined evidence",
            "--applicability",
            "local agents",
            "--markdown-file",
            draft.to_str().unwrap(),
            "--idempotency-key",
            "combined-evidence",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proposal: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proposal["sources"].as_array().unwrap().len(), 2);
    assert_ne!(
        proposal["sources"][0]["owner_agent_id"],
        proposal["sources"][1]["owner_agent_id"]
    );
}

#[test]
fn compose_file_locks_one_exact_external_snapshot_without_creating_private_memory() {
    let root = tempdir().unwrap();
    let home = root.path().to_str().unwrap();
    assert!(
        art()
            .args(["--home", home, "init", "--confirm"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        art()
            .args([
                "--home",
                home,
                "agent",
                "create",
                "--id",
                "codex-primary",
                "--host",
                "codex",
            ])
            .status()
            .unwrap()
            .success()
    );
    let markdown = root.path().join("migration.md");
    fs::write(&markdown, "# Migrated knowledge\nA reviewed body.\n").unwrap();
    let output = art()
        .args([
            "--home",
            home,
            "knowledge",
            "proposal",
            "compose-file",
            "--agent",
            "codex-primary",
            "--knowledge-key",
            "migration.reviewed",
            "--title",
            "Migrated knowledge",
            "--applicability",
            "local coding agents",
            "--markdown-file",
            markdown.to_str().unwrap(),
            "--source-id",
            "30-operations/migration.md",
            "--source-sha256",
            "53a5cc4350d2cbdbc365f00c998b6e986c368b95e67c6d4fda931c7debae8743",
            "--idempotency-key",
            "migration-reviewed",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proposal: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proposal["status"], "submitted");
    assert_eq!(
        proposal["draft"]["markdown"],
        "# Migrated knowledge\nA reviewed body.\n"
    );
    assert_eq!(proposal["sources"].as_array().unwrap().len(), 1);
    assert_eq!(proposal["sources"][0]["source_type"], "file_snapshot");
    assert!(proposal["sources"][0]["owner_agent_id"].is_null());
    assert_eq!(
        proposal["sources"][0]["source_content_hash"],
        "53a5cc4350d2cbdbc365f00c998b6e986c368b95e67c6d4fda931c7debae8743"
    );

    let memories = art()
        .args(["--home", home, "memory", "list", "--agent", "codex-primary"])
        .output()
        .unwrap();
    assert!(memories.status.success());
    let memories: serde_json::Value = serde_json::from_slice(&memories.stdout).unwrap();
    assert_eq!(memories["memories"].as_array().unwrap().len(), 0);
}

#[test]
fn compose_file_rejects_a_snapshot_when_the_operator_digest_does_not_match() {
    let root = tempdir().unwrap();
    let home = root.path().to_str().unwrap();
    assert!(
        art()
            .args(["--home", home, "init", "--confirm"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        art()
            .args([
                "--home",
                home,
                "agent",
                "create",
                "--id",
                "codex-primary",
                "--host",
                "codex",
            ])
            .status()
            .unwrap()
            .success()
    );
    let markdown = root.path().join("migration.md");
    fs::write(&markdown, "# Migrated knowledge\nA reviewed body.\n").unwrap();
    let output = art()
        .args([
            "--home",
            home,
            "knowledge",
            "proposal",
            "compose-file",
            "--agent",
            "codex-primary",
            "--knowledge-key",
            "migration.reviewed",
            "--title",
            "Migrated knowledge",
            "--applicability",
            "local coding agents",
            "--markdown-file",
            markdown.to_str().unwrap(),
            "--source-id",
            "30-operations/migration.md",
            "--source-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--idempotency-key",
            "migration-reviewed",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());

    let proposals = art()
        .args(["--home", home, "knowledge", "proposal", "list"])
        .output()
        .unwrap();
    assert!(proposals.status.success());
    let proposals: serde_json::Value = serde_json::from_slice(&proposals.stdout).unwrap();
    assert_eq!(proposals["proposals"].as_array().unwrap().len(), 0);
}
