use std::str::FromStr;

use art_agent_store::AgentVault;
use art_domain::{
    agent::AgentId,
    anchor::{AnchorKind, SourceAnchor},
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, ProcedurePayload, Sensitivity,
    },
};
use art_knowledge::KnowledgeVault;
use art_retrieval::{RecallEngine, RecallOrigin, RecallRequest};
use chrono::{Duration, Utc};
use serde_json::json;
use tempfile::tempdir;

fn seed_memory(vault: &AgentVault, agent: &AgentId, title: &str, text: &str, key: &str) -> String {
    let mut memory = MemoryArtifact::new(
        agent.clone(),
        title,
        text,
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["确认进程父子关系".into()],
            steps: vec![text.into()],
            verification: vec!["FD 数量回到基线".into()],
            rollback: vec!["重新建立 stdio 会话".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        MemoryScope::Repository("agent-recall-trail".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
    let anchor = SourceAnchor::new(
        agent.clone(),
        AnchorKind::TestReceipt,
        "test:shutdown",
        Some("exit 0; output digest recorded".into()),
        json!({"exit_code":0,"output_hash":"abc"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault.capture(&memory, &[anchor], key).unwrap();
    memory.id
}

fn publish_knowledge(
    vault: &KnowledgeVault,
    agent: &AgentId,
    key: &str,
    title: &str,
    body: &str,
) -> String {
    let source = ProposalSourceLock {
        source_type: ProposalSourceType::FileSnapshot,
        owner_agent_id: None,
        source_id: format!("reviewed-{key}"),
        source_revision: Some(1),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: None,
        approved_excerpt_hash: Some("b".repeat(64)),
        use_grant_id: None,
    };
    let proposal = vault
        .propose(
            agent,
            KnowledgeDraft::minimal(key, title, body),
            vec![source],
            &format!("proposal-{key}"),
        )
        .unwrap();
    vault
        .approve(
            &proposal.id,
            proposal.revision,
            ReviewActor::Human("local-user".into()),
            "reviewed retrieval fixture",
        )
        .unwrap();
    vault
        .publish(&proposal.id, proposal.revision, true)
        .unwrap()
        .edition_id
}

#[test]
fn chinese_exact_jieba_and_bigram_recall_private_memory() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("codex.sqlite3"), agent.clone()).unwrap();
    seed_memory(
        &vault,
        &agent,
        "Codex 文件句柄恢复",
        "远端 Codex MCP 文件句柄泄漏后发送 EOF 并验证子进程退出",
        "m1",
    );
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [2; 32]).unwrap();
    let engine = RecallEngine::new(vault, knowledge);
    for query in ["文件句柄泄漏", "Codex MCP", "子进程退出", "句柄恢复"] {
        let bundle = engine.recall(RecallRequest::new(query)).unwrap();
        assert_eq!(bundle.private_memories.len(), 1, "query={query}");
        assert_eq!(bundle.private_memories[0].origin, RecallOrigin::Memory);
    }
}

#[test]
fn bm25_rank_dominates_common_term_overlap() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [32; 32]).unwrap();
    for index in 0..12 {
        publish_knowledge(
            &knowledge,
            &agent,
            &format!("retrieval.background-{index}"),
            "common filler background",
            "common filler background material",
        );
    }
    let rare_id = publish_knowledge(
        &knowledge,
        &agent,
        "retrieval.rare",
        "rareterm",
        "rareterm recovery evidence",
    );
    publish_knowledge(
        &knowledge,
        &agent,
        "retrieval.common",
        "common filler",
        "common filler recovery evidence",
    );
    let engine = RecallEngine::new(private, knowledge);

    let bundle = engine
        .recall(RecallRequest::new("rareterm common filler"))
        .unwrap();

    assert_eq!(
        bundle.knowledge_editions[0].subject_ref,
        format!("knowledge:{rare_id}")
    );
    assert!(
        bundle.knowledge_editions[0]
            .match_reasons
            .contains(&"bm25_rank".into())
    );
}

#[test]
fn configurable_result_depth_returns_ten_with_large_budget() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [33; 32]).unwrap();
    for index in 0..12 {
        publish_knowledge(
            &knowledge,
            &agent,
            &format!("retrieval.depth-{index}"),
            &format!("shared marker {index}"),
            &format!("shared marker knowledge body {index}"),
        );
    }
    let engine = RecallEngine::new(private, knowledge);

    let default_bundle = engine.recall(RecallRequest::new("shared marker")).unwrap();
    assert_eq!(default_bundle.knowledge_editions.len(), 3);
    let expanded = engine
        .recall(RecallRequest {
            budget_tokens: 6_000,
            max_private_results: Some(10),
            max_knowledge_results: Some(10),
            ..RecallRequest::new("shared marker")
        })
        .unwrap();

    assert_eq!(expanded.knowledge_editions.len(), 10);
    assert_eq!(expanded.omitted_knowledge, 2);
}

#[test]
fn invalid_result_depth_fails_before_recall() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let engine = RecallEngine::new(
        AgentVault::open(root.path().join("agent.sqlite3"), agent).unwrap(),
        KnowledgeVault::open(root.path().join("knowledge"), [34; 32]).unwrap(),
    );

    for invalid in [0, 21] {
        let error = engine
            .recall(RecallRequest {
                max_knowledge_results: Some(invalid),
                ..RecallRequest::new("marker")
            })
            .unwrap_err();
        assert!(matches!(error, art_domain::ArtError::InvalidInput(_)));
    }
}

#[test]
fn private_and_published_channels_are_separate_and_cross_agent_memory_never_leaks() {
    let root = tempdir().unwrap();
    let codex = AgentId::from_str("codex-primary").unwrap();
    let dsh = AgentId::from_str("dsh-primary").unwrap();
    let codex_vault = AgentVault::open(root.path().join("codex.sqlite3"), codex.clone()).unwrap();
    let dsh_vault = AgentVault::open(root.path().join("dsh.sqlite3"), dsh.clone()).unwrap();
    seed_memory(
        &codex_vault,
        &codex,
        "Codex 私有恢复",
        "只有 Codex 应看到的彩虹恢复步骤",
        "c1",
    );
    seed_memory(
        &dsh_vault,
        &dsh,
        "DSH 私有恢复",
        "只有 DSH 应看到的月光恢复步骤",
        "d1",
    );
    let knowledge = KnowledgeVault::open(root.path().join("knowledge"), [3; 32]).unwrap();
    let source = ProposalSourceLock {
        source_type: ProposalSourceType::FileSnapshot,
        owner_agent_id: None,
        source_id: "reviewed-doc".into(),
        source_revision: Some(1),
        source_content_hash: "a".repeat(64),
        anchor_set_hash: None,
        approved_excerpt_hash: Some("b".repeat(64)),
        use_grant_id: None,
    };
    let proposal = knowledge
        .propose(
            &codex,
            KnowledgeDraft::minimal(
                "operations.shared-recovery",
                "共享恢复",
                "共享的星河恢复流程",
            ),
            vec![source],
            "k1",
        )
        .unwrap();
    knowledge
        .approve(
            &proposal.id,
            1,
            ReviewActor::Human("local-user".into()),
            "reviewed",
        )
        .unwrap();
    knowledge.publish(&proposal.id, 1, true).unwrap();

    let codex_engine = RecallEngine::new(codex_vault, knowledge.clone());
    let dsh_engine = RecallEngine::new(dsh_vault, knowledge);
    let codex_cross = codex_engine.recall(RecallRequest::new("月光恢复")).unwrap();
    let dsh_cross = dsh_engine.recall(RecallRequest::new("彩虹恢复")).unwrap();
    assert!(
        codex_cross
            .private_memories
            .iter()
            .all(|item| !item.title.contains("DSH") && !item.excerpt.contains("月光"))
    );
    assert!(
        dsh_cross
            .private_memories
            .iter()
            .all(|item| !item.title.contains("Codex") && !item.excerpt.contains("彩虹"))
    );
    assert_eq!(
        codex_engine
            .recall(RecallRequest::new("星河恢复"))
            .unwrap()
            .knowledge_editions
            .len(),
        1
    );
    assert_eq!(
        dsh_engine
            .recall(RecallRequest::new("星河恢复"))
            .unwrap()
            .knowledge_editions
            .len(),
        1
    );
}

#[test]
fn candidate_is_filtered_before_ranking_and_bundle_never_requests_automatic_capture() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let candidate = MemoryArtifact::new(
        agent.clone(),
        "高分候选",
        "完全匹配火箭火箭火箭",
        MemoryPayload::Procedure(ProcedurePayload {
            prerequisites: vec!["x".into()],
            steps: vec!["火箭".into()],
            verification: vec!["x".into()],
            rollback: vec!["x".into()],
            do_not_use_when: vec!["outside the documented scope".into()],
        }),
        MemoryScope::User("local-user".into()),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    let anchor = SourceAnchor::new(
        agent,
        AnchorKind::FileSnapshot,
        "repo:file",
        Some("火箭".into()),
        json!({"digest":"x"}),
        Sensitivity::Private,
        Utc::now(),
    )
    .unwrap();
    vault.capture(&candidate, &[anchor], "candidate").unwrap();
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [4; 32]).unwrap(),
    );
    let filtered = engine.recall(RecallRequest::new("火箭")).unwrap();
    assert!(filtered.private_memories.is_empty());
    assert_eq!(filtered.persist_policy, "no_automatic_capture");
    let included = engine
        .recall(RecallRequest {
            include_candidates: true,
            ..RecallRequest::new("火箭")
        })
        .unwrap();
    assert_eq!(included.private_memories.len(), 1);
}

#[test]
fn sixty_four_chinese_golden_queries_keep_top_three_recall() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    for index in 0..16 {
        seed_memory(
            &vault,
            &agent,
            &format!("故障手册 {index}"),
            &format!(
                "故障代码 ART_E{index:02} 的中文恢复步骤，版本 v0.{index}.0，路径 src/module-{index}"
            ),
            &format!("g{index}"),
        );
    }
    let engine = RecallEngine::new(
        vault,
        KnowledgeVault::open(root.path().join("knowledge"), [5; 32]).unwrap(),
    );
    for index in 0..16 {
        for query in [
            format!("ART_E{index:02}"),
            format!("module-{index}"),
            format!("v0.{index}.0"),
            format!("中文恢复步骤 {index}"),
        ] {
            let bundle = engine.recall(RecallRequest::new(query.clone())).unwrap();
            assert!(!bundle.private_memories.is_empty(), "query={query}");
            assert!(bundle.private_memories.len() <= 3);
        }
    }
}

#[test]
fn expired_and_disputed_memories_are_filtered_before_ranking() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let vault = AgentVault::open(root.path().join("agent.sqlite3"), agent.clone()).unwrap();
    let expired_id = seed_memory(
        &vault,
        &agent,
        "expired exact match",
        "ART_EXPIRED_EXACT_555",
        "expired",
    );
    let mut expired = vault.read(&expired_id).unwrap();
    expired.valid_until = Some(Utc::now() - Duration::seconds(1));
    let replacement_vault =
        AgentVault::open(root.path().join("expired.sqlite3"), agent.clone()).unwrap();
    replacement_vault
        .capture(
            &expired,
            &[SourceAnchor::new(
                agent.clone(),
                AnchorKind::TestReceipt,
                "test:expired",
                Some("expired".into()),
                json!({"exit_code":0}),
                Sensitivity::Private,
                Utc::now(),
            )
            .unwrap()],
            "expired-copy",
        )
        .unwrap();
    let disputed_id = seed_memory(
        &replacement_vault,
        &agent,
        "disputed exact match",
        "ART_DISPUTED_EXACT_556",
        "disputed",
    );
    replacement_vault
        .dispute(&disputed_id, "conflicting evidence")
        .unwrap();
    let engine = RecallEngine::new(
        replacement_vault,
        KnowledgeVault::open(root.path().join("knowledge"), [6; 32]).unwrap(),
    );
    assert!(
        engine
            .recall(RecallRequest::new("ART_EXPIRED_EXACT_555"))
            .unwrap()
            .private_memories
            .is_empty()
    );
    assert!(
        engine
            .recall(RecallRequest::new("ART_DISPUTED_EXACT_556"))
            .unwrap()
            .private_memories
            .is_empty()
    );
}
