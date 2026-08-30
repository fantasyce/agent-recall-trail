use std::{str::FromStr, sync::Arc, thread, time::Instant};

use art_agent_store::AgentVault;
use art_domain::{
    agent::AgentId,
    anchor::{AnchorKind, SourceAnchor},
    memory::{
        MemoryArtifact, MemoryPayload, MemoryScope, MemoryStatus, SemanticPayload, Sensitivity,
    },
};
use art_knowledge::KnowledgeVault;
use art_retrieval::{RecallEngine, RecallRequest};
use chrono::Utc;
use rusqlite::{Connection, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn percentile_millis(values: &mut [u128], percentile: usize) -> u128 {
    values.sort_unstable();
    values[(values.len() * percentile / 100).min(values.len() - 1)]
}

#[test]
#[ignore = "release acceptance dataset: 10k private memories and 5k shared editions"]
fn target_mac_release_performance_contract() {
    let root = tempdir().unwrap();
    let agent = AgentId::from_str("codex-primary").unwrap();
    let private_path = root.path().join("agent.sqlite3");
    let private_fixture = AgentVault::open(&private_path, agent.clone()).unwrap();
    let knowledge_root = root.path().join("knowledge");
    let knowledge_fixture = KnowledgeVault::open(&knowledge_root, [21_u8; 32]).unwrap();

    let mut connection = Connection::open(&private_path).unwrap();
    let transaction = connection.transaction().unwrap();
    for index in 0..10_000_u32 {
        let marker = if index == 9_999 {
            "ART_PERF_TARGET_9999"
        } else {
            "普通基准记忆"
        };
        let mut memory = MemoryArtifact::new(
            agent.clone(),
            format!("记忆 {index}"),
            format!("{marker} 编号 {index}"),
            MemoryPayload::Semantic(SemanticPayload {
                statement: format!("{marker} claim {index}"),
                applicability: "performance acceptance".into(),
                exceptions: vec![],
            }),
            MemoryScope::Repository("agent-recall-trail".into()),
            Sensitivity::Private,
            Utc::now(),
        )
        .unwrap();
        memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
        transaction.execute(
            "INSERT INTO memory_artifacts(id,agent_id,kind,status,title,summary,scope_type,scope_key,sensitivity,current_revision,current_hash,artifact_json,created_at,updated_at) VALUES (?1,?2,'semantic','active',?3,?4,'repository','agent-recall-trail','private',1,?5,?6,?7,?7)",
            params![memory.id,agent.as_str(),memory.title,memory.summary,memory.current_hash,serde_json::to_string(&memory).unwrap(),memory.created_at.to_rfc3339()],
        ).unwrap();
    }
    transaction.commit().unwrap();
    drop(connection);
    private_fixture.rebuild_search_index().unwrap();

    let control_path = knowledge_root.join("art-control.sqlite3");
    let mut control = Connection::open(&control_path).unwrap();
    let transaction = control.transaction().unwrap();
    for index in 0..5_000_u32 {
        let marker = if index == 4_999 {
            "ART_KNOWLEDGE_PERF_4999"
        } else {
            "普通共享知识"
        };
        let edition_id = format!("arke_perf_{index:05}");
        let key = format!("performance.key-{index}");
        let directory = knowledge_root.join("editions").join(&key);
        std::fs::create_dir(&directory).unwrap();
        let body = format!("{marker} knowledge {index}");
        let body_hash = hex::encode(Sha256::digest(body.as_bytes()));
        let manifest = json!({
            "schema":"art.knowledge.edition.v1",
            "edition_id":edition_id,
            "knowledge_key":key,
            "edition_number":1,
            "title":format!("知识 {index}"),
            "markdown_body_sha256":body_hash,
            "source_set_hash":"a".repeat(64),
            "source_commitments":["b".repeat(64)],
            "review_receipt_hash":"c".repeat(64),
            "published_at":"2026-08-30T00:00:00Z",
            "generator":"0.1.0"
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = hex::encode(Sha256::digest(&manifest_bytes));
        let stem = format!("1-{edition_id}");
        let manifest_path = directory.join(format!("{stem}.json"));
        let markdown_path = directory.join(format!("{stem}.md"));
        let markdown =
            format!("---\nmanifest_sha256: {manifest_hash}\n---\n\n## Knowledge\n\n{body}\n");
        std::fs::write(&manifest_path, &manifest_bytes).unwrap();
        std::fs::write(&markdown_path, markdown.as_bytes()).unwrap();
        transaction.execute(
            "INSERT INTO edition_projections(edition_id,knowledge_key,edition_number,title,markdown_path,manifest_path,markdown_sha256,manifest_sha256,published_at,revoked,current) VALUES (?1,?2,1,?3,?4,?5,?6,?7,'2026-08-30T00:00:00Z',0,1)",
            params![edition_id,key,format!("知识 {index}"),markdown_path.to_string_lossy(),manifest_path.to_string_lossy(),hex::encode(Sha256::digest(markdown.as_bytes())),manifest_hash],
        ).unwrap();
    }
    transaction.commit().unwrap();
    drop(control);
    knowledge_fixture.rebuild_search_index().unwrap();

    let startup = Instant::now();
    let private = AgentVault::open(&private_path, agent.clone()).unwrap();
    let knowledge = KnowledgeVault::open(&knowledge_root, [21_u8; 32]).unwrap();
    let startup_ms = startup.elapsed().as_millis();
    assert!(startup_ms < 500, "startup exceeded 500 ms");

    let mut capture_ms = Vec::new();
    for index in 0..100_u32 {
        let mut memory = MemoryArtifact::new(
            agent.clone(),
            format!("capture {index}"),
            "capture performance",
            MemoryPayload::Semantic(SemanticPayload {
                statement: "capture performance".into(),
                applicability: "acceptance".into(),
                exceptions: vec![],
            }),
            MemoryScope::User("local-user".into()),
            Sensitivity::Private,
            Utc::now(),
        )
        .unwrap();
        memory.transition(MemoryStatus::Active, Utc::now()).unwrap();
        let anchor = SourceAnchor::new(
            agent.clone(),
            AnchorKind::TestReceipt,
            format!("perf:capture-{index}"),
            Some("capture benchmark".into()),
            json!({"exit_code":0,"output_hash":format!("{index}")}),
            Sensitivity::Private,
            Utc::now(),
        )
        .unwrap();
        let started = Instant::now();
        private
            .capture(&memory, &[anchor], &format!("perf-capture-{index}"))
            .unwrap();
        capture_ms.push(started.elapsed().as_millis());
    }
    let capture_p95 = percentile_millis(&mut capture_ms, 95);
    assert!(capture_p95 < 100, "capture p95 exceeded 100 ms");

    let engine = Arc::new(RecallEngine::new(private, knowledge));
    let cold_started = Instant::now();
    let first = engine
        .recall(RecallRequest::new("ART_PERF_TARGET_9999"))
        .unwrap();
    let cold_recall_ms = cold_started.elapsed().as_millis();
    assert!(cold_recall_ms < 150, "cold recall exceeded 150 ms");
    assert_eq!(first.private_memories.len(), 1);
    let mut recall_ms = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        engine
            .recall(RecallRequest::new("ART_KNOWLEDGE_PERF_4999"))
            .unwrap();
        recall_ms.push(started.elapsed().as_millis());
    }
    let recall_p50 = percentile_millis(&mut recall_ms, 50);
    let recall_p95 = percentile_millis(&mut recall_ms, 95);
    let recall_p99 = percentile_millis(&mut recall_ms, 99);
    assert!(recall_p95 < 150, "recall p95 exceeded 150 ms");

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                let started = Instant::now();
                engine
                    .recall(RecallRequest::new("ART_PERF_TARGET_9999"))
                    .unwrap();
                started.elapsed().as_millis()
            })
        })
        .collect();
    let mut concurrent_ms = Vec::new();
    for handle in handles {
        let elapsed = handle.join().unwrap();
        assert!(elapsed < 500, "concurrent recall exceeded 500 ms");
        concurrent_ms.push(elapsed);
    }
    println!(
        "startup_ms={startup_ms} cold_recall_ms={cold_recall_ms} capture_p95_ms={capture_p95} recall_p50_ms={recall_p50} recall_p95_ms={recall_p95} recall_p99_ms={recall_p99} concurrent_max_ms={}",
        concurrent_ms.into_iter().max().unwrap_or(0)
    );
}
