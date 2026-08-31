use std::{
    io::Write,
    process::{Command, Stdio},
    str::FromStr,
};

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
fn stdio_initialize_list_tools_and_eof_exit_cleanly() {
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
                "codex"
            ])
            .status()
            .unwrap()
            .success()
    );
    let mut child = art()
        .args(["--home", home, "mcp", "serve", "--agent", "codex-primary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"art-test","version":"1"}}})).unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let tools = &responses.iter().find(|value| value["id"] == 2).unwrap()["result"]["tools"];
    assert_eq!(tools.as_array().unwrap().len(), 6);
    assert!(responses.iter().all(|value| value.get("jsonrpc").is_some()));
}

#[test]
fn stdio_progressive_route_recall_and_exact_read_stay_bound_to_one_agent() {
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
    let paths = ArtPaths::from_explicit_root(root.path()).unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(paths.agent_vault(&agent), agent.clone()).unwrap();
    let mut memory = MemoryArtifact::new(
        agent.clone(),
        "Progressive release recovery",
        "STDIO_EXACT_BODY_MARKER",
        MemoryPayload::Semantic(SemanticPayload {
            statement: "recover the release process".into(),
            applicability: "progressive MCP journey".into(),
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
        AnchorKind::TestReceipt,
        "test:stdio-progressive",
        Some("passed".into()),
        json!({"exit_code":0}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault
        .capture(&memory, &[anchor], "stdio-progressive-memory")
        .unwrap();
    let subject_ref = format!("memory:{}@1", memory.id);

    let mut child = art()
        .args(["--home", home, "mcp", "serve", "--agent", "codex-primary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"art-progressive-test","version":"1"}}})).unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"art_recall","arguments":{"query":"release recovery","detail":"route","mode":"lexical"}}})).unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"art_recall","arguments":{"query":"release recovery","detail":"recall","mode":"lexical"}}})).unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"art_read","arguments":{"subject_ref":subject_ref,"include_anchors":false}}})).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let response = |id| responses.iter().find(|value| value["id"] == id).unwrap();
    let route = serde_json::to_string(response(2)).unwrap();
    assert!(route.contains("navigation_topics"));
    assert!(!route.contains("STDIO_EXACT_BODY_MARKER"));
    assert!(
        serde_json::to_string(response(3))
            .unwrap()
            .contains("private_memories")
    );
    assert!(
        serde_json::to_string(response(4))
            .unwrap()
            .contains("STDIO_EXACT_BODY_MARKER")
    );
}

#[test]
fn debug_logging_keeps_stdout_json_only_and_does_not_echo_request_bodies() {
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
    let marker = "ART_PRIVATE_QUERY_MUST_NOT_REACH_STDERR_9281";
    let mut child = art()
        .env("RUST_LOG", "debug")
        .args(["--home", home, "mcp", "serve", "--agent", "codex-primary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"art-log-test","version":"1"}}})).unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writeln!(stdin,"{}",serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"art_recall","arguments":{"query":marker,"budget_tokens":1800}}})).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
    }
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains(marker),
        "debug log exposed the private query"
    );
}

#[test]
fn legacy_mcp_client_protocol_negotiates_or_fails_with_json_rpc_not_process_corruption() {
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
    let mut child = art()
        .args(["--home", home, "mcp", "serve", "--agent", "codex-primary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"art-legacy-test","version":"1"}}})).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let response = responses.iter().find(|value| value["id"] == 1).unwrap();
    assert!(response.get("result").is_some() || response.get("error").is_some());
}

#[cfg(unix)]
#[test]
fn stdio_sigterm_exits_successfully_within_three_seconds() {
    assert_signal_shutdown("-TERM");
}

#[cfg(unix)]
#[test]
fn stdio_sigint_exits_successfully_within_three_seconds() {
    assert_signal_shutdown("-INT");
}

#[cfg(unix)]
fn assert_signal_shutdown(signal: &str) {
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
    let mut child = art()
        .args(["--home", home, "mcp", "serve", "--agent", "codex-primary"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"art-signal-test","version":"1"}}})).unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        Command::new("kill")
            .args([signal, &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let started = std::time::Instant::now();
    let status = child.wait().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(status.success(), "signal shutdown status: {status}");
}
