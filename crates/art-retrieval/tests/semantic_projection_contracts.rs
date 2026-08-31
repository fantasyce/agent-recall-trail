use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use art_domain::{ArtError, ArtResult};
use art_retrieval::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, ProviderFingerprint, SemanticDocument,
    SemanticProjection, knowledge_semantic_path, private_semantic_path,
};
use serde_json::json;

#[cfg(unix)]
fn private_file(path: &Path, body: &[u8]) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn private_file(path: &Path, body: &[u8]) {
    fs::write(path, body).unwrap();
}

fn endpoint(root: &Path) -> EmbeddingEndpoint {
    let config = root.join("endpoint.json");
    private_file(
        &config,
        serde_json::to_vec(&json!({
            "schema":"art.embedding.endpoint.v1",
            "protocol":"openai_compatible",
            "endpoint":"https://embedding.example.test",
            "model":"test/model",
            "revision":"r1",
            "dimensions":3,
            "normalized":true,
            "timeout_ms":500
        }))
        .unwrap()
        .as_slice(),
    );
    EmbeddingEndpoint::load(&config).unwrap()
}

#[derive(Debug)]
struct FakeProvider {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    fail_after: Option<usize>,
    fingerprint: ProviderFingerprint,
}

impl EmbeddingProvider for FakeProvider {
    fn fingerprint(&self) -> ProviderFingerprint {
        self.fingerprint.clone()
    }

    fn embed(&self, input: EmbeddingInput<'_>) -> ArtResult<Vec<Vec<f32>>> {
        let texts = match input {
            EmbeddingInput::Query(query) => vec![query.to_owned()],
            EmbeddingInput::Documents(documents) => documents.to_vec(),
        };
        let mut calls = self.calls.lock().unwrap();
        if self.fail_after.is_some_and(|limit| calls.len() >= limit) {
            return Err(ArtError::Internal("synthetic interruption".into()));
        }
        calls.push(texts.clone());
        Ok(texts
            .into_iter()
            .map(|text| {
                if text.contains("beta") {
                    vec![0.0, 1.0, 0.0]
                } else {
                    vec![1.0, 0.0, 0.0]
                }
            })
            .collect())
    }
}

fn documents(count: usize) -> Vec<SemanticDocument> {
    (0..count)
        .map(|index| {
            SemanticDocument::new(
                format!("memory:artm_{index:04}@1"),
                if index == count - 1 {
                    format!("beta document {index}")
                } else {
                    format!("alpha document {index}")
                },
                format!("{index:064x}"),
            )
            .unwrap()
        })
        .collect()
}

#[test]
fn semantic_projection_is_lane_local_private_epoch_bound_and_ranked() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = endpoint(root.path());
    let agent_vault = root
        .path()
        .join("data/art/agents/codex-primary/art.sqlite3");
    let knowledge = root.path().join("data/art/knowledge-vault");
    let private_path = private_semantic_path(&agent_vault);
    let shared_path = knowledge_semantic_path(&knowledge);
    assert_ne!(private_path, shared_path);
    assert_eq!(
        private_path,
        root.path()
            .join("data/art/agents/codex-primary/retrieval/semantic.sqlite3")
    );
    assert_eq!(
        shared_path,
        root.path()
            .join("data/art/knowledge-vault/.art/retrieval/semantic.sqlite3")
    );
    let provider = FakeProvider {
        calls: Arc::new(Mutex::new(Vec::new())),
        fail_after: None,
        fingerprint: endpoint.fingerprint(),
    };

    assert_eq!(
        SemanticProjection::rebuild(
            &private_path,
            &endpoint,
            "epoch-1",
            &documents(2),
            &provider
        )
        .unwrap(),
        2
    );
    let projection = SemanticProjection::open(&private_path, &endpoint, "epoch-1").unwrap();
    assert!(SemanticProjection::open(&private_path, &endpoint, "epoch-2").is_err());
    let ranked = projection.rank(&[0.0, 1.0, 0.0], 2).unwrap();
    assert!(ranked[0].subject_ref.ends_with("0001@1"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(private_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn interrupted_rebuild_preserves_progress_and_resumes_without_reembedding_completed_batches() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = endpoint(root.path());
    let path = root.path().join("semantic.sqlite3");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let interrupted = FakeProvider {
        calls: calls.clone(),
        fail_after: Some(1),
        fingerprint: endpoint.fingerprint(),
    };
    assert!(
        SemanticProjection::rebuild(&path, &endpoint, "epoch", &documents(20), &interrupted)
            .is_err()
    );
    let completed_calls = calls.lock().unwrap().len();
    assert_eq!(completed_calls, 1);
    assert!(path.with_extension("sqlite3.staging").exists());

    let resumed = FakeProvider {
        calls: calls.clone(),
        fail_after: None,
        fingerprint: endpoint.fingerprint(),
    };
    let progress = Arc::new(Mutex::new(Vec::new()));
    let observed = progress.clone();
    assert_eq!(
        SemanticProjection::rebuild_with_progress(
            &path,
            &endpoint,
            "epoch",
            &documents(20),
            &resumed,
            &move |update| observed.lock().unwrap().push(update),
        )
        .unwrap(),
        20
    );
    let all_calls = calls.lock().unwrap();
    assert_eq!(all_calls.len(), 2);
    assert_eq!(all_calls[1].len(), 4);
    let progress = progress.lock().unwrap();
    assert_eq!(progress.first().unwrap().completed, 16);
    assert!(progress.first().unwrap().resumed);
    assert_eq!(progress.last().unwrap().completed, 20);
    assert!(SemanticProjection::open(&path, &endpoint, "epoch").is_ok());
}
