use std::{
    fmt::Write as _,
    fs,
    io::{Read, Write as _},
    net::TcpListener,
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use art_retrieval::{
    EmbeddingEndpoint, EmbeddingInput, EmbeddingProvider, OpenAiCompatibleEmbeddingProvider,
};
use rcgen::generate_simple_self_signed;
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
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

#[derive(Debug)]
struct StubResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

fn https_stub(
    response: StubResponse,
) -> (
    String,
    String,
    mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let certified = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate = certified.cert.der().clone();
    let certificate_pem = certified.cert.pem();
    let private_key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private_key.into())
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = mpsc::channel();
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let connection = ServerConnection::new(Arc::new(config)).unwrap();
        let mut stream = StreamOwned::new(connection, socket);
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4_096];
        let header_end = loop {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "request ended before headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or_default();
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0, "request ended before body");
            request.extend_from_slice(&buffer[..read]);
        }
        sent.send(String::from_utf8(request).unwrap()).unwrap();
        thread::sleep(response.delay);
        let reason = match response.status {
            200 => "OK",
            302 => "Found",
            401 => "Unauthorized",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Test",
        };
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            response.status,
            reason,
            response.body.len()
        );
        for (name, value) in response.headers {
            write!(&mut head, "{name}: {value}\r\n").unwrap();
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&response.body);
        let _ = stream.flush();
    });
    (
        format!("https://localhost:{}/api", address.port()),
        certificate_pem,
        received,
        server,
    )
}

fn provider_for_stub(
    root: &Path,
    endpoint_url: &str,
    ca_pem: &str,
    timeout_ms: u64,
) -> OpenAiCompatibleEmbeddingProvider {
    let token = root.join("stub-token");
    private_file(&token, b"stub-bearer-token\n");
    let ca = root.join("stub-ca.pem");
    private_file(&ca, ca_pem.as_bytes());
    let config = root.join("stub-endpoint.json");
    private_file(
        &config,
        serde_json::to_vec(&json!({
            "schema":"art.embedding.endpoint.v1",
            "protocol":"openai_compatible",
            "endpoint":endpoint_url,
            "model":"stub/model",
            "revision":"stub-r1",
            "dimensions":3,
            "normalized":false,
            "timeout_ms":timeout_ms,
            "token_file":token,
            "ca_file":ca
        }))
        .unwrap()
        .as_slice(),
    );
    OpenAiCompatibleEmbeddingProvider::new(EmbeddingEndpoint::load(&config).unwrap()).unwrap()
}

#[test]
fn openai_adapter_uses_private_https_ca_and_preserves_response_index_order() {
    let body = serde_json::to_vec(&json!({
        "model":"stub/model",
        "data":[
            {"index":1,"embedding":[0.0,4.0,0.0]},
            {"index":0,"embedding":[3.0,0.0,0.0]}
        ]
    }))
    .unwrap();
    let (url, ca, request, server) = https_stub(StubResponse {
        status: 200,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body,
        delay: Duration::ZERO,
    });
    let root = tempfile::tempdir().unwrap();
    let provider = provider_for_stub(root.path(), &url, &ca, 2_000);
    let vectors = provider
        .embed(EmbeddingInput::Documents(&[
            "first".into(),
            "second".into(),
        ]))
        .unwrap();
    assert_eq!(vectors, vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]]);
    let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(request.starts_with("POST /v1/embeddings HTTP/1.1\r\n"));
    assert!(request.contains("authorization: Bearer stub-bearer-token\r\n"));
    let payload: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(payload["model"], "stub/model");
    assert_eq!(payload["input"], json!(["first", "second"]));
    assert_eq!(payload["dimensions"], 3);
    server.join().unwrap();
}

#[test]
fn openai_adapter_rejects_redirects_statuses_timeouts_and_malformed_vectors_safely() {
    let cases = [
        (
            302,
            vec![("Location".into(), "https://example.test/elsewhere".into())],
            b"redirect secret".to_vec(),
            Duration::ZERO,
        ),
        (
            401,
            Vec::new(),
            b"credential rejected secret".to_vec(),
            Duration::ZERO,
        ),
        (
            429,
            Vec::new(),
            b"quota detail secret".to_vec(),
            Duration::ZERO,
        ),
        (
            500,
            Vec::new(),
            b"stack trace secret".to_vec(),
            Duration::ZERO,
        ),
        (200, Vec::new(), b"not-json secret".to_vec(), Duration::ZERO),
        (
            200,
            Vec::new(),
            serde_json::to_vec(
                &json!({"model":"stub/model","data":[{"index":0,"embedding":[1.0,0.0]}]}),
            )
            .unwrap(),
            Duration::ZERO,
        ),
        (
            200,
            Vec::new(),
            serde_json::to_vec(
                &json!({"model":"stub/model","data":[{"index":0,"embedding":[1.0,0.0,0.0]}]}),
            )
            .unwrap(),
            Duration::from_millis(150),
        ),
    ];
    for (status, headers, body, delay) in cases {
        let (url, ca, _request, server) = https_stub(StubResponse {
            status,
            headers,
            body,
            delay,
        });
        let root = tempfile::tempdir().unwrap();
        let provider = provider_for_stub(root.path(), &url, &ca, 75);
        let error = provider
            .embed(EmbeddingInput::Query("safe query"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "internal error: embedding provider unavailable");
        assert!(!error.contains("secret"));
        server.join().unwrap();
    }
}

#[test]
fn openai_adapter_rejects_responses_over_the_bounded_body_limit() {
    let (url, ca, _request, server) = https_stub(StubResponse {
        status: 200,
        headers: Vec::new(),
        body: vec![b'x'; 16 * 1024 * 1024 + 1],
        delay: Duration::ZERO,
    });
    let root = tempfile::tempdir().unwrap();
    let provider = provider_for_stub(root.path(), &url, &ca, 2_000);
    assert!(provider.embed(EmbeddingInput::Query("bounded")).is_err());
    server.join().unwrap();
}
