use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use art_domain::{ArtError, ArtResult};
use reqwest::{Certificate, Url, blocking::Client};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_DOCUMENTS: usize = 32;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointFile {
    schema: String,
    protocol: String,
    endpoint: String,
    model: String,
    #[serde(default)]
    revision: Option<String>,
    dimensions: usize,
    normalized: bool,
    timeout_ms: u64,
    #[serde(default)]
    token_file: Option<PathBuf>,
    #[serde(default)]
    ca_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingEndpoint {
    protocol: String,
    endpoint: Url,
    model: String,
    revision: Option<String>,
    dimensions: usize,
    normalized: bool,
    timeout: Duration,
    token_file: Option<PathBuf>,
    ca_file: Option<PathBuf>,
    fingerprint: ProviderFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFingerprint {
    pub value: String,
    pub dimensions: usize,
    pub normalized: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum EmbeddingInput<'a> {
    Query(&'a str),
    Documents(&'a [String]),
}

pub trait EmbeddingProvider: std::fmt::Debug + Send + Sync {
    fn fingerprint(&self) -> ProviderFingerprint;
    fn embed(&self, input: EmbeddingInput<'_>) -> ArtResult<Vec<Vec<f32>>>;
}

impl EmbeddingEndpoint {
    pub fn load(path: &Path) -> ArtResult<Self> {
        validate_private_file(path, "embedding endpoint")?;
        let bytes = fs::read(path).map_err(safe_file_error)?;
        let raw: EndpointFile = serde_json::from_slice(&bytes)
            .map_err(|_| invalid_endpoint("invalid embedding endpoint JSON"))?;
        let endpoint = Url::parse(&raw.endpoint)
            .map_err(|_| invalid_endpoint("invalid embedding endpoint URL"))?;
        if raw.schema != "art.embedding.endpoint.v1"
            || raw.protocol != "openai_compatible"
            || endpoint.scheme() != "https"
            || endpoint.username() != ""
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || raw.model.trim().is_empty()
            || raw
                .revision
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || !(1..=65_536).contains(&raw.dimensions)
            || !(50..=30_000).contains(&raw.timeout_ms)
        {
            return Err(invalid_endpoint("invalid embedding endpoint contract"));
        }
        for file in [raw.token_file.as_deref(), raw.ca_file.as_deref()]
            .into_iter()
            .flatten()
        {
            if !file.is_absolute() {
                return Err(invalid_endpoint("embedding runtime paths must be absolute"));
            }
            validate_private_file(file, "embedding runtime file")?;
        }
        let fingerprint_value = hex::encode(Sha256::digest(
            format!(
                "{}\n{}\n{}\n{}\n{}\n{}",
                raw.protocol,
                endpoint,
                raw.model,
                raw.revision.as_deref().unwrap_or(""),
                raw.dimensions,
                raw.normalized
            )
            .as_bytes(),
        ));
        Ok(Self {
            protocol: raw.protocol,
            endpoint,
            model: raw.model,
            revision: raw.revision,
            dimensions: raw.dimensions,
            normalized: raw.normalized,
            timeout: Duration::from_millis(raw.timeout_ms),
            token_file: raw.token_file,
            ca_file: raw.ca_file,
            fingerprint: ProviderFingerprint {
                value: fingerprint_value,
                dimensions: raw.dimensions,
                normalized: raw.normalized,
            },
        })
    }

    pub fn protocol(&self) -> &str {
        &self.protocol
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }
    pub const fn dimensions(&self) -> usize {
        self.dimensions
    }
    pub const fn normalized(&self) -> bool {
        self.normalized
    }
    pub fn fingerprint(&self) -> ProviderFingerprint {
        self.fingerprint.clone()
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleEmbeddingProvider {
    endpoint: EmbeddingEndpoint,
    client: Option<Client>,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
    dimensions: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    #[serde(default)]
    model: Option<String>,
    data: Vec<EmbeddingDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    index: usize,
    embedding: Vec<f32>,
}

impl OpenAiCompatibleEmbeddingProvider {
    pub fn new(endpoint: EmbeddingEndpoint) -> ArtResult<Self> {
        let ca_file = endpoint.ca_file.clone();
        let timeout = endpoint.timeout;
        let client = std::thread::spawn(move || {
            let mut builder = Client::builder()
                .timeout(timeout)
                .connect_timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true);
            if let Some(path) = ca_file {
                let pem = fs::read(path).map_err(safe_file_error)?;
                let certificate = Certificate::from_pem(&pem)
                    .map_err(|_| invalid_endpoint("invalid embedding CA"))?;
                builder = builder.add_root_certificate(certificate);
            }
            builder.build().map_err(|_| provider_unavailable())
        })
        .join()
        .map_err(|_| provider_unavailable())??;
        Ok(Self {
            endpoint,
            client: Some(client),
        })
    }

    fn embed_inner(&self, input: EmbeddingInput<'_>) -> ArtResult<Vec<Vec<f32>>> {
        let texts: Vec<String> = match input {
            EmbeddingInput::Query(query) => vec![query.to_owned()],
            EmbeddingInput::Documents(documents) => documents.to_vec(),
        };
        if texts.is_empty()
            || texts.len() > MAX_DOCUMENTS
            || texts
                .iter()
                .any(|text| text.is_empty() || text.len() > MAX_INPUT_BYTES)
        {
            return Err(ArtError::InvalidInput(
                "embedding input requires 1..=32 non-empty bounded texts".into(),
            ));
        }
        let url = self
            .endpoint
            .endpoint
            .join("/v1/embeddings")
            .map_err(|_| provider_unavailable())?;
        let mut request = self
            .client
            .as_ref()
            .ok_or_else(provider_unavailable)?
            .post(url)
            .json(&EmbeddingRequest {
                model: &self.endpoint.model,
                input: &texts,
                dimensions: self.endpoint.dimensions,
            });
        if let Some(path) = &self.endpoint.token_file {
            validate_private_file(path, "embedding token")?;
            let token = fs::read_to_string(path).map_err(safe_file_error)?;
            let token = token.trim();
            if token.is_empty() || token.chars().any(char::is_whitespace) {
                return Err(invalid_endpoint("invalid embedding token"));
            }
            request = request.bearer_auth(token);
        }
        let response = request.send().map_err(|_| provider_unavailable())?;
        if !response.status().is_success() {
            return Err(provider_unavailable());
        }
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| provider_unavailable())?;
        if u64::try_from(body.len()).unwrap_or(u64::MAX) > MAX_RESPONSE_BYTES {
            return Err(provider_unavailable());
        }
        let payload: EmbeddingResponse =
            serde_json::from_slice(&body).map_err(|_| provider_unavailable())?;
        if payload
            .model
            .as_deref()
            .is_some_and(|model| model != self.endpoint.model)
            || payload.data.len() != texts.len()
        {
            return Err(provider_unavailable());
        }
        let mut ordered: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        for datum in payload.data {
            if datum.index >= ordered.len() || ordered[datum.index].is_some() {
                return Err(provider_unavailable());
            }
            validate_vector(&datum.embedding, self.endpoint.dimensions)?;
            ordered[datum.index] = Some(if self.endpoint.normalized {
                datum.embedding
            } else {
                normalize_vector(datum.embedding)?
            });
        }
        ordered
            .into_iter()
            .map(|vector| vector.ok_or_else(provider_unavailable))
            .collect()
    }
}

impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    fn fingerprint(&self) -> ProviderFingerprint {
        self.endpoint.fingerprint()
    }

    fn embed(&self, input: EmbeddingInput<'_>) -> ArtResult<Vec<Vec<f32>>> {
        std::thread::scope(|scope| {
            scope
                .spawn(|| self.embed_inner(input))
                .join()
                .map_err(|_| provider_unavailable())?
        })
    }
}

impl Drop for OpenAiCompatibleEmbeddingProvider {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let _ = std::thread::spawn(move || drop(client)).join();
        }
    }
}

fn normalize_vector(mut vector: Vec<f32>) -> ArtResult<Vec<f32>> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err(provider_unavailable());
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok(vector)
}

fn validate_vector(vector: &[f32], dimensions: usize) -> ArtResult<()> {
    if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
        return Err(provider_unavailable());
    }
    Ok(())
}

#[cfg(unix)]
fn validate_private_file(path: &Path, label: &str) -> ArtResult<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path).map_err(safe_file_error)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() || metadata.nlink() != 1
    {
        return Err(ArtError::PermissionDenied(format!(
            "{label} must be one regular non-linked file"
        )));
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(ArtError::PermissionDenied(format!(
            "{label} must have mode 0600"
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_file(path: &Path, label: &str) -> ArtResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(safe_file_error)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(ArtError::PermissionDenied(format!(
            "{label} must be a regular file"
        )));
    }
    Ok(())
}

fn invalid_endpoint(message: &str) -> ArtError {
    ArtError::InvalidInput(message.into())
}
fn safe_file_error(_error: std::io::Error) -> ArtError {
    ArtError::Io("embedding runtime file access failed".into())
}
fn provider_unavailable() -> ArtError {
    ArtError::Internal("embedding provider unavailable".into())
}
