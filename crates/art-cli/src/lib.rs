use std::path::Path;

use art_agent_store::{AgentVault, AutoMemoryConfigStore};
use art_domain::{
    ArtError, ArtResult,
    agent::{AgentId, ArtPaths},
    memory::canonical_json_hash,
};
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoMemorySignal {
    None,
    Decision,
    VerifiedRepair,
    AppliedCorrection,
}

fn sensitive_text(value: &str) -> bool {
    Regex::new(r"(?i)(authorization\s*:\s*bearer|begin\s+(rsa|openssh|ec)\s+private\s+key|api[_-]?key\s*[:=]|password\s*[:=]|cookie\s*[:=]|recovery\s+code|full\s+transcript|完整会话\s*transcript)")
        .is_ok_and(|pattern| pattern.is_match(value))
}

pub fn classify_correction_prompt(prompt: &str) -> Option<&'static str> {
    let value = prompt.trim();
    if value.is_empty() || value.len() > 16_384 || sensitive_text(value) {
        return None;
    }
    let lower = value.to_lowercase();
    let correction = [
        "that is incorrect",
        "that's incorrect",
        "not correct",
        "no, ",
        "wrong",
        "please correct",
        "不是这样",
        "不正确",
        "不对",
        "错了",
        "请改成",
        "应该是",
    ];
    correction
        .iter()
        .any(|marker| lower.contains(marker))
        .then_some("user_correction")
}

pub fn classify_stop_message(message: &str) -> AutoMemorySignal {
    let value = message.trim();
    if value.len() < 12 || value.len() > 64 * 1024 || sensitive_text(value) {
        return AutoMemorySignal::None;
    }
    let lower = value.to_lowercase();
    let excluded = [
        "still working",
        "progress update",
        "not verified",
        "not sure",
        "tests are failing",
        "unresolved",
        "might be",
        "maybe",
        "could clarify",
        "no changes",
        "has started",
        "will investigate",
        "quote from an old memory",
        "recalled a previous",
        "did not check",
        "还在处理中",
        "进度",
        "尚未验证",
        "没有解决",
        "不确定",
        "也许",
        "可能修好",
        "引用旧记忆",
        "召回了以前",
        "没有复核",
        "请补充",
        "没有进行任何修改",
        "刚刚开始",
        "下一步继续",
    ];
    if excluded.iter().any(|marker| lower.contains(marker)) {
        return AutoMemorySignal::None;
    }
    let correction = [
        "correction is applied",
        "correction was applied",
        "corrected behavior",
        "用户纠正已落实",
        "纠正已应用",
    ];
    if correction.iter().any(|marker| lower.contains(marker)) {
        return AutoMemorySignal::AppliedCorrection;
    }
    let repair = [
        "fixed",
        "resolved",
        "repair",
        "root cause",
        "implemented",
        "migration restoration",
        "修复",
        "根因",
        "已完成实现",
        "迁移恢复",
        "验证结果",
        "重新启动",
    ];
    let proof = [
        "verified",
        "tests passed",
        "tests pass",
        "test passed",
        "successfully",
        "checks passed",
        "验收成功",
        "验证结果",
        "测试通过",
        "通过单元测试",
        "通过端到端测试",
        "验证成功",
        "检查通过",
        "已验证",
    ];
    if repair.iter().any(|marker| lower.contains(marker))
        && proof.iter().any(|marker| lower.contains(marker))
    {
        return AutoMemorySignal::VerifiedRepair;
    }
    let decision = [
        "decision:",
        "we decided",
        "final decision",
        "accepted decision",
        "决定：",
        "我们决定",
        "最终采用",
        "确认采用",
    ];
    if decision.iter().any(|marker| lower.contains(marker)) {
        return AutoMemorySignal::Decision;
    }
    AutoMemorySignal::None
}

fn reports_applied_result(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "applied successfully",
        "completed successfully",
        "change was applied",
        "已完成",
        "已落实",
        "修改完成",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    cwd: Option<String>,
    prompt: Option<String>,
    stop_hook_active: Option<bool>,
    last_assistant_message: Option<String>,
}

pub fn run_auto_memory_hook(paths: &ArtPaths, agent_id: AgentId, input: Value) -> ArtResult<Value> {
    let config = AutoMemoryConfigStore::new(paths.root());
    let status = config.status();
    if !status.enabled {
        return Ok(json!({"continue":true}));
    }
    let input: HookInput = serde_json::from_value(input)
        .map_err(|error| ArtError::InvalidInput(format!("invalid Codex hook input: {error}")))?;
    let session_id = input.session_id.as_deref().unwrap_or_default();
    let turn_id = input.turn_id.as_deref().unwrap_or_default();
    match input.hook_event_name.as_str() {
        "UserPromptSubmit" => {
            let vault = AgentVault::open(paths.agent_vault(&agent_id), agent_id)?;
            if let Some(signal) = input.prompt.as_deref().and_then(classify_correction_prompt)
                && !session_id.trim().is_empty()
                && !turn_id.trim().is_empty()
            {
                vault.record_auto_correction_signal(
                    &config,
                    session_id,
                    turn_id,
                    signal,
                    Utc::now(),
                )?;
                vault.record_auto_memory_hook_run(&config, "correction_signal", Utc::now())?;
            } else {
                vault.record_auto_memory_hook_run(&config, "not_admitted", Utc::now())?;
            }
            Ok(json!({"continue":true}))
        }
        "Stop" => {
            if input.stop_hook_active.unwrap_or(false) {
                return Ok(json!({"continue":true}));
            }
            let message = input.last_assistant_message.as_deref().unwrap_or_default();
            let mut signal = classify_stop_message(message);
            let correction_might_apply = signal == AutoMemorySignal::None
                && reports_applied_result(message)
                && !sensitive_text(message);
            if signal == AutoMemorySignal::None && !correction_might_apply {
                let vault = AgentVault::open(paths.agent_vault(&agent_id), agent_id)?;
                vault.record_auto_memory_hook_run(&config, "not_admitted", Utc::now())?;
                return Ok(json!({"continue":true}));
            }
            let vault = AgentVault::open(paths.agent_vault(&agent_id), agent_id)?;
            if correction_might_apply {
                if !session_id.trim().is_empty()
                    && !turn_id.trim().is_empty()
                    && vault.has_auto_correction_signal(session_id, turn_id)?
                {
                    signal = AutoMemorySignal::AppliedCorrection;
                } else {
                    vault.record_auto_memory_hook_run(&config, "not_admitted", Utc::now())?;
                    return Ok(json!({"continue":true}));
                }
            }
            let event_hash = canonical_json_hash(&json!({
                "session_id":session_id,
                "turn_id":turn_id,
                "signal":format!("{signal:?}"),
                "message_sha256":canonical_json_hash(&json!(message)),
            }));
            let attribution = if session_id.trim().is_empty() {
                art_agent_store::IntakeAttribution::agent_asserted(None, None)
            } else {
                art_agent_store::IntakeAttribution::host_supplied(
                    session_id.into(),
                    (!turn_id.trim().is_empty()).then(|| turn_id.into()),
                )
            };
            let claim = vault.claim_auto_memory_trigger_with_attribution(
                &config,
                &attribution,
                &event_hash,
                Utc::now(),
            )?;
            vault.record_auto_memory_hook_run(&config, &claim.reason, Utc::now())?;
            if !claim.accepted || claim.replayed {
                return Ok(json!({"continue":true}));
            }
            let scope = input
                .cwd
                .as_deref()
                .map(Path::new)
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("current-task");
            Ok(json!({
                "decision":"block",
                "reason":format!(
                    "ART's cheap reminder filter fired for this completed turn (trigger receipt {}). The trigger is not evidence that the turn is worth remembering. Evaluate the conclusion as if no Hook had fired, using one bounded pass only for scope '{}'. Do not default to either outcome and do not infer value from conclusive wording. Apply the same value standard as ordinary Agent work:\n{}\nCount evidence for and against recording. Check independently whether there is a durable conclusion, likely future reuse, current verifiable evidence that supports the full claim, bounded scope and material uncertainty, and no disqualifying transience, duplication, speculation, or sensitive content. Before submitting, run one bounded lexical art_recall using the proposed conclusion's distinctive terms. If an existing private memory already covers the same conclusion in the same scope, do not submit a paraphrase. If the positive requirements are actually supported and no disqualifier applies, record it; otherwise do not. Do not read the transcript or copy the final response. If the requirements are not met or nothing is worth recording, respond exactly 'no worth recording'. Otherwise call art_memory_candidate_submit with this trigger_receipt_id and candidate_index 0, bounded source anchors and no status/reviewer fields. Then finish the user response without another extraction pass.",
                    claim.receipt_id, scope, include_str!("../../../plugin/agent-recall-trail/skills/agent-recall-trail/references/private-memory-value-v1.md")
                )
            }))
        }
        _ => Ok(json!({"continue":true})),
    }
}
