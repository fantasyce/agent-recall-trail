use std::{
    fs,
    io::Write as _,
    process::{Command, Stdio},
    time::Instant,
};

use art_cli::{AutoMemorySignal, classify_correction_prompt, classify_stop_message};
use serde_json::{Value, json};
use tempfile::tempdir;

fn ordinary_request(
    agent: art_domain::agent::AgentId,
    attribution: art_agent_store::IntakeAttribution,
) -> art_agent_store::MemoryIntakeRequest {
    use art_domain::{
        anchor::{AnchorKind, SourceAnchor},
        memory::{MemoryArtifact, MemoryPayload, MemoryScope, SemanticPayload, Sensitivity},
    };
    let memory = MemoryArtifact::new(
        agent.clone(),
        "Ordinary memory",
        "A bounded reusable conclusion",
        MemoryPayload::Semantic(SemanticPayload {
            statement: "Retain bounded evidence".into(),
            applicability: "Future local maintenance tasks".into(),
            exceptions: vec![],
        }),
        MemoryScope::User("*".into()),
        Sensitivity::Internal,
        chrono::Utc::now(),
    )
    .unwrap();
    let anchor = SourceAnchor::new_with_source(
        agent,
        AnchorKind::UserStatement,
        "user:preference",
        None,
        None,
        Some("Retain bounded evidence".into()),
        json!({}),
        Sensitivity::Internal,
        chrono::Utc::now(),
    )
    .unwrap();
    art_agent_store::MemoryIntakeRequest {
        capture_origin: None,
        memory,
        anchors: vec![anchor],
        idempotency_key: "ordinary".into(),
        attribution,
        request_basis: None,
        value_reason: Some("Reuse the stable evidence requirement".into()),
        target_memory_id: None,
        expected_revision: None,
        hook_trigger_receipt_id: None,
    }
}

#[test]
fn missing_host_identity_uses_shared_persistent_fallback_without_synthetic_trust() {
    let root = tempdir().unwrap();
    let config = art_agent_store::AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let event = json!({"hook_event_name":"Stop","last_assistant_message":"Decision: retain a bounded source for future tasks."});
    let first = run_hook(root.path(), &event);
    let response: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(response["decision"], "block");
    let retry = run_hook(root.path(), &event);
    assert_eq!(
        serde_json::from_slice::<Value>(&retry.stdout).unwrap(),
        json!({"continue":true})
    );
    let second = run_hook(
        root.path(),
        &json!({"hook_event_name":"Stop","last_assistant_message":"Decision: use a different reusable procedure."}),
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&second.stdout).unwrap(),
        json!({"continue":true})
    );
    let agent = "codex-primary".parse().unwrap();
    let vault = art_agent_store::AgentVault::open(
        root.path()
            .join("data/art/agents/codex-primary/art.sqlite3"),
        agent,
    )
    .unwrap();
    let conn = rusqlite::Connection::open(vault.path()).unwrap();
    let (bucket, session, turn): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT bucket,session_id,turn_id FROM memory_intake_budget",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(bucket.starts_with("agent-day:"));
    assert_eq!(session, None);
    assert_eq!(turn, None);
    let trigger: String = conn
        .query_row(
            "SELECT receipt_id FROM auto_memory_trigger_receipts WHERE outcome='accepted'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let ordinary = vault
        .intake(
            &config,
            ordinary_request(
                vault.agent_id().clone(),
                art_agent_store::IntakeAttribution::agent_asserted(None, None),
            ),
        )
        .unwrap();
    assert_eq!(
        ordinary.disposition,
        art_agent_store::IntakeDisposition::RateLimited
    );
    let mut hook = ordinary_request(
        vault.agent_id().clone(),
        art_agent_store::IntakeAttribution::agent_asserted(None, None),
    );
    hook.capture_origin = Some(art_agent_store::IntakeOrigin::HookTriggered);
    hook.idempotency_key = format!("hook:{trigger}:0");
    hook.hook_trigger_receipt_id = Some(trigger);
    let submitted = vault.intake(&config, hook).unwrap();
    assert_eq!(
        submitted.disposition,
        art_agent_store::IntakeDisposition::PendingReview
    );
    assert!(!submitted.receipt.attribution.host_supplied);
    assert!(submitted.receipt.attribution.degraded);
    assert_eq!(submitted.receipt.attribution.session_id, None);
    assert_eq!(submitted.receipt.budget_bucket, bucket);
}

#[test]
fn stop_hook_suppresses_a_reliably_linked_ordinary_admission_without_diagnostic_error() {
    let root = tempdir().unwrap();
    let config = art_agent_store::AutoMemoryConfigStore::new(root.path());
    config.set_enabled(true, "human:governance-ui").unwrap();
    let agent = "codex-primary".parse().unwrap();
    let vault = art_agent_store::AgentVault::open(
        root.path()
            .join("data/art/agents/codex-primary/art.sqlite3"),
        agent,
    )
    .unwrap();
    let result = vault
        .intake(
            &config,
            ordinary_request(
                vault.agent_id().clone(),
                art_agent_store::IntakeAttribution::host_supplied(
                    "linked".into(),
                    Some("turn".into()),
                ),
            ),
        )
        .unwrap();
    assert_eq!(
        result.disposition,
        art_agent_store::IntakeDisposition::PendingReview
    );
    let output = run_hook(
        root.path(),
        &json!({"hook_event_name":"Stop","session_id":"linked","turn_id":"turn","last_assistant_message":"Decision: keep the verified operating procedure."}),
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"continue":true})
    );
    assert_eq!(
        vault
            .auto_memory_diagnostics()
            .unwrap()
            .last_hook_outcome
            .as_deref(),
        Some("turn_already_handled")
    );
}

#[test]
fn bilingual_classifier_covers_at_least_sixty_outcomes_without_storing_conversation() {
    let positives = [
        "Decision: keep the global switch disabled by default. Verified by 12 passing tests.",
        "We decided to use SQLite receipts; the focused test suite passed.",
        "Resolved: the 403 was caused by stale CSRF state. Regression tests pass.",
        "Fixed the migration and verified backup restoration successfully.",
        "The final decision is to bind candidates to the current Agent; tests passed.",
        "Root cause confirmed and fixed; cargo test completed successfully.",
        "Implemented the approved policy and all targeted checks passed.",
        "Verified repair: the service now rejects stale source digests.",
        "Accepted decision: retain explicit capture while auto memory is off.",
        "The correction is applied and verified by the browser journey.",
        "决定：全局开关默认关闭，相关测试已经通过。",
        "已修复 403，根因是旧会话，回归测试通过。",
        "最终采用候选优先策略，并完成验证。",
        "用户纠正已落实，浏览器验收成功。",
        "确认根因并完成修复，全部针对性检查通过。",
        "我们决定保留显式录入，自动录入受全局开关控制。",
        "迁移恢复已验证成功，结论可以复用。",
        "已完成实现并通过单元测试和端到端测试。",
        "验证结果：关闭期间没有任何自动候选写入。",
        "修复完成；重新启动后配置仍然生效，测试通过。",
    ];
    let negatives = [
        "Hello, how are you?",
        "Thanks!",
        "What is ART?",
        "I am still working on it.",
        "Progress update: 2 of 8 tests are running.",
        "Maybe we should use SQLite.",
        "I have not verified this yet.",
        "The tests are failing and the issue is unresolved.",
        "Here is a quote from an old memory: Decision: use JSON.",
        "I recalled a previous decision but did not check it.",
        "Could you clarify the requirement?",
        "This might be fixed, but I am not sure.",
        "No changes were made.",
        "The build has started.",
        "I will investigate next.",
        "你好",
        "谢谢",
        "ART 是什么？",
        "还在处理中，目前没有结论。",
        "进度：正在跑测试。",
        "也许可以改成 SQLite。",
        "尚未验证。",
        "测试失败，问题还没有解决。",
        "引用旧记忆：决定使用 JSON。",
        "我召回了以前的结论，但没有复核。",
        "请补充更多信息。",
        "可能修好了，但不确定。",
        "没有进行任何修改。",
        "构建刚刚开始。",
        "下一步继续排查。",
    ];
    let rejected = [
        "Fixed it. password = hunter2 and tests passed.",
        concat!(
            "Decision: use Authorization:",
            " Bearer abcdef and verified."
        ),
        concat!("Stored api_", "key=secret after the successful test."),
        concat!("BEGIN OPENSSH", " PRIVATE KEY fixed the deployment."),
        "Cookie: session=secret; the repair passed.",
        "Saved the full transcript and tests passed.",
        "修复完成，password=秘密，测试通过。",
        "决定保存 api-key: secret，验证成功。",
        "完整会话 transcript 已保存，测试通过。",
        "",
        "   ",
        "{}",
        "null",
        "[object Object]",
        "✅",
    ];

    assert!(positives.len() + negatives.len() + rejected.len() >= 60);
    for sample in positives {
        assert_ne!(
            classify_stop_message(sample),
            AutoMemorySignal::None,
            "{sample}"
        );
    }
    for sample in negatives.into_iter().chain(rejected) {
        assert_eq!(
            classify_stop_message(sample),
            AutoMemorySignal::None,
            "{sample}"
        );
    }
}

#[test]
fn prompt_classifier_returns_only_a_bounded_signal_code() {
    assert_eq!(
        classify_correction_prompt("不是这样，请改成默认关闭。"),
        Some("user_correction")
    );
    assert_eq!(
        classify_correction_prompt("That is incorrect; keep explicit capture enabled."),
        Some("user_correction")
    );
    assert_eq!(classify_correction_prompt("继续"), None);
    assert_eq!(classify_correction_prompt("password=secret 这不对"), None);
}

fn run_hook(home: &std::path::Path, input: &Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_art"))
        .args([
            "--home",
            home.to_str().unwrap(),
            "auto-memory",
            "hook",
            "--agent",
            "codex-primary",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), input).unwrap();
    child.wait_with_output().unwrap()
}

fn run_raw_hook(home: &std::path::Path, input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_art"))
        .args([
            "--home",
            home.to_str().unwrap(),
            "auto-memory",
            "hook",
            "--agent",
            "codex-primary",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn malformed_or_unavailable_hook_state_fails_open_without_leaking_details() {
    let root = tempdir().unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let malformed = run_raw_hook(root.path(), b"{");
    assert!(malformed.status.success());
    let response: Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(response["continue"], true);
    assert_eq!(
        response["systemMessage"],
        "ART automatic memory failed safely; no automatic memory was written."
    );
    assert!(!String::from_utf8_lossy(&malformed.stdout).contains("expected"));
}

#[test]
fn default_off_stop_and_prompt_hooks_do_no_work_and_fail_open() {
    let root = tempdir().unwrap();
    let stop = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"Stop","session_id":"s1","turn_id":"t1","cwd":root.path(),
            "stop_hook_active":false,"last_assistant_message":"Fixed the issue and all tests passed.",
            "prompt":"must-not-be-used","transcript_path":"/must/not/be/read"
        }),
    );
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&stop.stdout).unwrap(),
        json!({"continue":true})
    );
    assert!(
        !root
            .path()
            .join("data/art/agents/codex-primary/art.sqlite3")
            .exists()
    );

    let prompt = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t2","cwd":root.path(),
            "prompt":"No, that is wrong; default it off.","transcript_path":"/must/not/be/read"
        }),
    );
    assert!(prompt.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&prompt.stdout).unwrap(),
        json!({"continue":true})
    );
}

#[test]
fn enabled_prompt_hook_persists_only_a_bounded_correction_signal() {
    let root = tempdir().unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let unique_prompt = "不是这样，请改成默认关闭。RAW_PROMPT_MUST_NOT_PERSIST_7fc2";
    let output = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"signal-session",
            "turn_id":"signal-turn",
            "cwd":root.path(),
            "prompt":unique_prompt,
            "transcript_path":"/must/not/be/read"
        }),
    );
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"continue":true})
    );
    let database = fs::read(
        root.path()
            .join("data/art/agents/codex-primary/art.sqlite3"),
    )
    .unwrap();
    let bytes = String::from_utf8_lossy(&database);
    assert!(bytes.contains("user_correction"));
    assert!(!bytes.contains("RAW_PROMPT_MUST_NOT_PERSIST_7fc2"));
}

#[test]
fn a_bounded_prompt_correction_can_trigger_after_the_correction_is_applied() {
    let root = tempdir().unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let prompt = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"UserPromptSubmit","session_id":"correction-session",
            "turn_id":"correction-turn","prompt":"不是这样，请改成默认关闭。"
        }),
    );
    assert!(prompt.status.success());
    let stop = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"Stop","session_id":"correction-session",
            "turn_id":"correction-turn","cwd":root.path(),"stop_hook_active":false,
            "last_assistant_message":"Done. The requested change was applied successfully."
        }),
    );
    let response: Value = serde_json::from_slice(&stop.stdout).unwrap();
    assert_eq!(response["decision"], "block");
}

#[test]
fn enabled_stop_hook_requests_at_most_one_structured_candidate_continuation() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("config/art/agents")).unwrap();
    fs::write(root.path().join("config/art/agents/codex-primary.json"), br#"{"schema":"art.agent-profile.v1","id":"codex-primary","host":"codex","schema_version":1}"#).unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let event = json!({
        "hook_event_name":"Stop","session_id":"s1","turn_id":"t1","cwd":root.path(),
        "stop_hook_active":false,"last_assistant_message":"Decision: default the switch off. Verified by 12 passing tests."
    });
    let first = run_hook(root.path(), &event);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_json["decision"], "block");
    let reason = first_json["reason"].as_str().unwrap();
    assert!(reason.contains("art_memory_candidate_submit"));
    assert!(reason.contains("no worth recording"));
    assert!(!reason.contains("transcript_path"));

    let duplicate = run_hook(root.path(), &event);
    assert_eq!(
        serde_json::from_slice::<Value>(&duplicate.stdout).unwrap(),
        json!({"continue":true})
    );
    let active = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"Stop","session_id":"s1","turn_id":"t2","cwd":root.path(),
            "stop_hook_active":true,"last_assistant_message":"Decision: another change. Tests passed."
        }),
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&active.stdout).unwrap(),
        json!({"continue":true})
    );
}

#[test]
fn hook_continuation_treats_trigger_as_non_evidence_and_uses_a_balanced_check() {
    let root = tempdir().unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let output = run_hook(
        root.path(),
        &json!({
            "hook_event_name":"Stop","session_id":"objective-session","turn_id":"objective-turn",
            "cwd":root.path(),"stop_hook_active":false,
            "last_assistant_message":"Decision: tests passed and the task is complete."
        }),
    );
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    let reason = response["reason"].as_str().unwrap();

    assert!(reason.contains("The trigger is not evidence that the turn is worth remembering"));
    assert!(reason.contains("Evaluate the conclusion as if no Hook had fired"));
    assert!(reason.contains("Do not default to either outcome"));
    assert!(reason.contains("Count evidence for and against recording"));
    assert!(reason.contains("one bounded lexical art_recall"));
    assert!(reason.contains("already covers the same conclusion in the same scope"));
    assert!(!reason.contains("Default to not recording"));
    assert!(!reason.contains("admitted this completed turn"));
}

#[test]
fn enabled_non_trigger_hook_is_fast_and_requests_zero_continuations() {
    let root = tempdir().unwrap();
    art_agent_store::AutoMemoryConfigStore::new(root.path())
        .set_enabled(true, "human:governance-ui")
        .unwrap();
    let mut micros = Vec::new();
    for index in 0..100 {
        let started = Instant::now();
        let output = run_hook(
            root.path(),
            &json!({
                "hook_event_name":"Stop","session_id":"perf","turn_id":format!("t{index}"),"cwd":root.path(),
                "stop_hook_active":false,"last_assistant_message":"Thanks, happy to help."
            }),
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            json!({"continue":true})
        );
        micros.push(started.elapsed().as_micros());
    }
    micros.sort_unstable();
    assert!(micros[94] <= 100_000, "p95={}us", micros[94]);
    eprintln!(
        "auto-memory non-trigger subprocess runs=100 continuations=0 p50={}us p95={}us max={}us",
        micros[49], micros[94], micros[99]
    );
    let agent = "codex-primary".parse().unwrap();
    let vault = art_agent_store::AgentVault::open(
        root.path()
            .join("data/art/agents/codex-primary/art.sqlite3"),
        agent,
    )
    .unwrap();
    let diagnostics = vault.auto_memory_diagnostics().unwrap();
    assert!(diagnostics.last_hook_at.is_some());
    assert_eq!(
        diagnostics.last_hook_outcome.as_deref(),
        Some("not_admitted")
    );
    assert!(diagnostics.last_capture_at.is_none());
    assert_eq!(vault.count().unwrap(), 0);
    assert!(vault.intake_receipts().unwrap().is_empty());
}
