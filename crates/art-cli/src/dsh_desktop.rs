use std::{collections::BTreeMap, fs, path::Path};

use art_domain::{ArtError, ArtResult, agent::ArtPaths};
use serde_json::json;

use super::{IntegrationArgs, internal_error, io_error, print_json, write_private_new};

const SKILL: &str = include_str!("../../../integrations/dsh/art-recall/SKILL.md");
const VALUE_STANDARD: &str = include_str!(
    "../../../plugin/agent-recall-trail/skills/agent-recall-trail/references/private-memory-value-v1.md"
);

pub(super) fn export(paths: &ArtPaths, binary: &Path, args: &IntegrationArgs) -> ArtResult<()> {
    if args.apply && (args.dry_run || args.print) {
        return Err(ArtError::InvalidInput(
            "--apply cannot be combined with --dry-run or --print".into(),
        ));
    }
    let binary = binary
        .to_str()
        .ok_or_else(|| ArtError::InvalidInput("desktop executable path must be UTF-8".into()))?;
    let home = paths
        .root()
        .to_str()
        .ok_or_else(|| ArtError::InvalidInput("desktop ART home must be UTF-8".into()))?;
    let binary = serde_json::to_string(binary).map_err(internal_error)?;
    let home = serde_json::to_string(home).map_err(internal_error)?;
    let agent = serde_json::to_string(&args.agent).map_err(internal_error)?;
    let patch = format!(
        "- insert:\n    - id: art-memory\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: art\n        transport: stdio\n        command: {binary}\n        args: [\"--home\", {home}, \"mcp\", \"serve\", \"--agent\", {agent}]\n        env: {{}}\n        failOnStartupError: true\n        toolCallTimeoutMs: 30000\n        reconnect:\n          enabled: true\n          initialDelayMs: 500\n          maxDelayMs: 30000\n          maxAttempts: 10\n"
    );
    let skill = SKILL.replace(
        "../../../plugin/agent-recall-trail/skills/agent-recall-trail/references/private-memory-value-v1.md",
        "references/private-memory-value-v1.md",
    );
    let files = BTreeMap::from([
        ("art.overlay.yml", patch),
        ("art-recall/SKILL.md", skill),
        (
            "art-recall/references/private-memory-value-v1.md",
            VALUE_STANDARD.to_owned(),
        ),
    ]);
    if !args.apply {
        return print_json(&json!({
            "schema":"art.cli.v1", "profile":"desktop", "applied":false,
            "files":files,
        }));
    }
    let output = args.output.as_ref().ok_or_else(|| {
        ArtError::InvalidInput("--apply requires an explicit --output directory".into())
    })?;
    // Exclusive creation reserves only the requested new export, never a host profile.
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            ArtError::DuplicateConflict
        } else {
            io_error(error)
        }
    })?;
    let result = (|| {
        for (name, content) in &files {
            write_private_new(&output.join(name), content.as_bytes())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(output);
        return Err(error);
    }
    print_json(&json!({
        "schema":"art.cli.v1", "profile":"desktop", "applied":true,
        "output":output, "created_new":true, "host_config":"not_modified",
        "files":files.keys().collect::<Vec<_>>(),
    }))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn desktop_export_rejects_lossy_executable_paths() {
        let root = tempfile::tempdir().unwrap();
        let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
        let binary =
            std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/runtime/art-\xff".to_vec()));
        let args = IntegrationArgs {
            agent: "dsh-primary".into(),
            dry_run: true,
            apply: false,
            print: false,
            output: None,
        };
        assert!(matches!(
            export(&paths, &binary, &args),
            Err(ArtError::InvalidInput(_))
        ));
    }
}
