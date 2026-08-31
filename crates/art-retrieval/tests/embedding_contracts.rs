use std::{fs, path::Path};

use art_retrieval::{EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider};
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

fn endpoint_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let token = root.path().join("token");
    private_file(&token, b"test-token-value-that-is-never-serialized\n");
    let config = root.path().join("endpoint.json");
    private_file(
        &config,
        serde_json::to_vec_pretty(&json!({
            "schema":"art.embedding.endpoint.v1",
            "protocol":"openai_compatible",
            "endpoint":"https://embedding.example.test",
            "model":"operator/model-of-choice",
            "revision":"reviewed-revision",
            "dimensions":3,
            "normalized":false,
            "timeout_ms":750,
            "token_file":token
        }))
        .unwrap()
        .as_slice(),
    );
    (root, config)
}

#[test]
fn endpoint_contract_is_provider_neutral_and_fingerprinted_without_secrets() {
    let (_root, config) = endpoint_fixture();
    let endpoint = EmbeddingEndpoint::load(&config).unwrap();
    assert_eq!(endpoint.protocol(), "openai_compatible");
    assert_eq!(endpoint.model(), "operator/model-of-choice");
    assert_eq!(endpoint.revision(), Some("reviewed-revision"));
    assert_eq!(endpoint.dimensions(), 3);
    assert!(!endpoint.normalized());
    assert_eq!(endpoint.fingerprint().value.len(), 64);
    let encoded = serde_json::to_string(&endpoint.fingerprint()).unwrap();
    assert!(!encoded.contains("test-token-value"));
}

#[test]
fn endpoint_rejects_inline_secrets_unknown_fields_and_unsafe_transport() {
    let (_root, config) = endpoint_fixture();
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    for (field, value) in [
        ("token", json!("INLINE_SECRET_MUST_NOT_ECHO")),
        ("endpoint", json!("http://embedding.example.test")),
        ("dimensions", json!(0)),
        ("timeout_ms", json!(40)),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        private_file(&config, serde_json::to_vec(&invalid).unwrap().as_slice());
        let error = EmbeddingEndpoint::load(&config).unwrap_err().to_string();
        assert!(!error.contains("INLINE_SECRET_MUST_NOT_ECHO"));
    }
}

#[derive(Debug)]
struct ContractProvider;

impl EmbeddingProvider for ContractProvider {
    fn fingerprint(&self) -> art_retrieval::ProviderFingerprint {
        art_retrieval::ProviderFingerprint {
            value: "a".repeat(64),
            dimensions: 3,
            normalized: true,
        }
    }

    fn embed(&self, input: EmbeddingInput<'_>) -> art_domain::ArtResult<Vec<Vec<f32>>> {
        let count = match input {
            EmbeddingInput::Query(_) => 1,
            EmbeddingInput::Documents(documents) => documents.len(),
        };
        Ok(vec![vec![1.0, 0.0, 0.0]; count])
    }
}

#[test]
fn provider_interface_distinguishes_queries_from_document_batches() {
    let provider = ContractProvider;
    assert_eq!(
        provider
            .embed(EmbeddingInput::Query("query"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        provider
            .embed(EmbeddingInput::Documents(&["one".into(), "two".into()]))
            .unwrap()
            .len(),
        2
    );
}
