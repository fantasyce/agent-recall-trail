use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use art_agent_store::AgentVault;
use art_domain::{
    ArtError, ArtResult,
    agent::{AgentId, AgentProfile, ArtPaths, HostKind},
    anchor::AssuranceOutcome,
    knowledge::{KnowledgeDraft, ProposalSourceLock, ProposalSourceType, ReviewActor, RiskLevel},
    memory::Sensitivity,
};
use art_knowledge::{
    KnowledgeVault,
    backup::{create_backup, restore_backup, verify_backup},
};
use art_mcp::{ArtMcpServer, run_stdio_server};
use art_retrieval::{
    EmbeddingEndpoint, OpenAiCompatibleEmbeddingProvider, RankFusionPolicy, RecallDetail,
    RecallEngine, RecallRequest, RetrievalMode, SemanticProjection, SemanticRuntime,
    knowledge_semantic_path, knowledge_semantic_snapshot, private_semantic_path,
    private_semantic_snapshot,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
struct MarkdownImportProposal {
    proposal_id: String,
    source_path: PathBuf,
    content_sha256: String,
    title: Option<String>,
    permalink: Option<String>,
    wiki_links: Vec<String>,
    warnings: Vec<String>,
    eligible: bool,
}

#[derive(Debug, Clone, Serialize)]
struct MarkdownScan {
    schema: String,
    proposals: Vec<MarkdownImportProposal>,
    non_markdown_files: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtConfig {
    schema: String,
    home: PathBuf,
}

#[derive(Debug, Parser)]
#[command(
    name = "art",
    version,
    about = "Agent Recall Trail: private Agent memory and human-reviewed shared knowledge"
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    home: Option<PathBuf>,
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        confirm: bool,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    Doctor {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        repair_preview: bool,
        #[arg(long)]
        apply: bool,
    },
    Recall {
        query: String,
        #[arg(long)]
        agent: String,
        #[arg(long, value_enum, default_value_t = RetrievalModeArg::Lexical)]
        mode: RetrievalModeArg,
        #[arg(long, value_enum, default_value_t = RecallDetailArg::Recall)]
        detail: RecallDetailArg,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 1800)]
        budget_tokens: usize,
        #[arg(long)]
        include_candidates: bool,
        #[arg(long)]
        max_private_results: Option<usize>,
        #[arg(long)]
        max_knowledge_results: Option<usize>,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Knowledge {
        #[command(subcommand)]
        command: KnowledgeCommand,
    },
    Integration {
        #[command(subcommand)]
        command: IntegrationCommand,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    Reindex {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        knowledge: bool,
        #[arg(long)]
        navigation: bool,
        #[arg(long)]
        vectors: bool,
    },
}

#[derive(Debug, Subcommand)]
enum BackupCommand {
    Create {
        #[arg(long)]
        output: PathBuf,
    },
    Verify {
        #[arg(long)]
        source: PathBuf,
    },
    Restore {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        target_home: PathBuf,
        #[arg(long)]
        commitment_key: PathBuf,
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        host: HostArg,
    },
    List,
    Show {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum HostArg {
    Codex,
    Dsh,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RetrievalModeArg {
    Lexical,
    FullScan,
    Semantic,
    Hybrid,
}

impl From<RetrievalModeArg> for RetrievalMode {
    fn from(value: RetrievalModeArg) -> Self {
        match value {
            RetrievalModeArg::Lexical => Self::Lexical,
            RetrievalModeArg::FullScan => Self::FullScan,
            RetrievalModeArg::Semantic => Self::Semantic,
            RetrievalModeArg::Hybrid => Self::Hybrid,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RecallDetailArg {
    Route,
    Recall,
}

impl From<RecallDetailArg> for RecallDetail {
    fn from(value: RecallDetailArg) -> Self {
        match value {
            RecallDetailArg::Route => Self::Route,
            RecallDetailArg::Recall => Self::Recall,
        }
    }
}
impl From<HostArg> for HostKind {
    fn from(value: HostArg) -> Self {
        match value {
            HostArg::Codex => Self::Codex,
            HostArg::Dsh => Self::Dsh,
        }
    }
}

#[derive(Debug, Subcommand)]
enum MemoryCommand {
    List {
        #[arg(long)]
        agent: String,
    },
    Read {
        memory_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        revision: Option<u32>,
        #[arg(long)]
        anchors: bool,
    },
    Assure {
        memory_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        outcome: String,
        #[arg(long)]
        reason: String,
    },
    Dispute {
        memory_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        reason: String,
    },
    Supersede {
        memory_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        reason: String,
    },
    Archive {
        memory_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Subcommand)]
enum KnowledgeCommand {
    Proposal {
        #[command(subcommand)]
        command: ProposalCommand,
    },
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    Publish {
        proposal_id: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        confirm: bool,
    },
    Revoke {
        edition_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        confirm: bool,
    },
    Supersede {
        edition_id: String,
        #[arg(long)]
        with: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        confirm: bool,
    },
    Verify {
        #[arg(long)]
        edition: Option<String>,
    },
}
#[derive(Debug, Subcommand)]
enum ProposalCommand {
    List,
    Show {
        proposal_id: String,
        #[arg(long)]
        sources: bool,
        #[arg(long)]
        diff: bool,
    },
    Submit {
        proposal_id: String,
    },
    Compose {
        #[arg(long = "from", required = true)]
        sources: Vec<String>,
        #[arg(long)]
        knowledge_key: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        applicability: String,
        #[arg(long)]
        markdown_file: PathBuf,
        #[arg(long, default_value = "internal")]
        sensitivity: String,
        #[arg(long)]
        idempotency_key: String,
    },
    ComposeFile {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        knowledge_key: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        applicability: String,
        #[arg(long)]
        markdown_file: PathBuf,
        #[arg(long)]
        source_id: String,
        #[arg(long)]
        source_sha256: String,
        #[arg(long, default_value = "internal")]
        sensitivity: String,
        #[arg(long)]
        idempotency_key: String,
    },
}
#[derive(Debug, Subcommand)]
enum ReviewCommand {
    Approve {
        proposal_id: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        reason: String,
    },
    RequestChanges {
        proposal_id: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        reason: String,
    },
    Reject {
        proposal_id: String,
        #[arg(long)]
        revision: u32,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Subcommand)]
enum IntegrationCommand {
    Codex(IntegrationArgs),
    Dsh(IntegrationArgs),
}
#[derive(Debug, Args)]
struct IntegrationArgs {
    #[arg(long)]
    agent: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    apply: bool,
    #[arg(long)]
    print: bool,
    #[arg(long)]
    output: Option<PathBuf>,
}
#[derive(Debug, Subcommand)]
enum McpCommand {
    Serve {
        #[arg(long)]
        agent: String,
    },
    Schema {
        #[arg(long)]
        agent: String,
    },
}
#[derive(Debug, Subcommand)]
enum ImportCommand {
    Markdown {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        copy_to: Option<PathBuf>,
        #[arg(long)]
        confirm: bool,
    },
    MemoryJsonl {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        confirm: bool,
    },
}
#[derive(Debug, Subcommand)]
enum ExportCommand {
    Memory {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        include_private: bool,
        #[arg(long)]
        confirm: bool,
    },
    Knowledge {
        #[arg(long)]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(safe_log_filter())
        .init();
    let cli = Cli::parse();
    if let Err(error) = run(cli).await {
        if error == ArtError::ShuttingDown {
            std::process::exit(0);
        }
        eprintln!(
            "{}",
            json!({"schema":"art.error.v1","code":error.code(),"message":error.to_string(),"retryable":error.retryable(),"details":{}})
        );
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> ArtResult<()> {
    let paths = ArtPaths::from_explicit_root(resolve_home(cli.home, cli.config.as_deref())?)?;
    match cli.command {
        Command::Init { confirm } => init(&paths, confirm),
        Command::Agent { command } => agent_command(&paths, command),
        Command::Doctor {
            agent,
            json: _,
            repair_preview,
            apply,
        } => doctor(&paths, agent.as_deref(), repair_preview, apply),
        Command::Recall {
            query,
            agent,
            mode,
            detail,
            json: _,
            budget_tokens,
            include_candidates,
            max_private_results,
            max_knowledge_results,
        } => {
            let (vault, knowledge) = runtime(&paths, &agent)?;
            print_json(
                &configured_recall_engine(&paths, &vault, &knowledge).recall(RecallRequest {
                    query,
                    mode: mode.into(),
                    detail: detail.into(),
                    include_candidates,
                    budget_tokens,
                    max_private_results,
                    max_knowledge_results,
                })?,
            )
        }
        Command::Memory { command } => memory_command(&paths, command),
        Command::Knowledge { command } => knowledge_command(&paths, command),
        Command::Integration { command } => integration_command(&paths, command),
        Command::Mcp {
            command: McpCommand::Serve { agent },
        } => {
            let (agent_id, key) = identity_and_key(&paths, &agent)?;
            run_stdio_server(ArtMcpServer::open(&paths, agent_id, key)?).await
        }
        Command::Mcp {
            command: McpCommand::Schema { agent },
        } => {
            let (agent_id, key) = identity_and_key(&paths, &agent)?;
            let server = ArtMcpServer::open(&paths, agent_id, key)?;
            let tools: serde_json::Value = serde_json::from_str(&server.tool_schema_json())
                .map_err(|error| ArtError::Internal(error.to_string()))?;
            print_json(&json!({"schema":"art.mcp.tool-schema.v1","tools":tools}))
        }
        Command::Import { command } => import_command(&paths, command),
        Command::Export { command } => export_command(&paths, command),
        Command::Backup { command } => backup_command(&paths, command),
        Command::Reindex {
            agent,
            knowledge,
            navigation,
            vectors,
        } => {
            if vectors && (agent.is_none() || !knowledge) {
                return Err(ArtError::InvalidInput(
                    "vector reindex requires --agent and --knowledge".into(),
                ));
            }
            let embedding = if vectors {
                let endpoint = EmbeddingEndpoint::load(
                    &paths.root().join("config/art/embedding/default.json"),
                )?;
                let provider = Arc::new(OpenAiCompatibleEmbeddingProvider::new(endpoint.clone())?);
                Some((endpoint, provider))
            } else {
                None
            };
            let provider_fingerprint = embedding
                .as_ref()
                .map(|(endpoint, _)| endpoint.fingerprint().value);
            let mut private_memories = None;
            let mut private_navigation = None;
            let mut private_vectors = None;
            if let Some(agent) = agent {
                let (vault, _) = runtime(&paths, &agent)?;
                if !vault.integrity_check()? {
                    return Err(ArtError::IndexDegraded);
                }
                private_memories = Some(vault.rebuild_search_index()?);
                if navigation {
                    private_navigation = Some(vault.rebuild_navigation()?);
                }
                if let Some((endpoint, provider)) = &embedding {
                    let (source_epoch, documents) = private_semantic_snapshot(&vault)?;
                    private_vectors = Some(SemanticProjection::rebuild_with_progress(
                        &private_semantic_path(vault.path()),
                        endpoint,
                        &source_epoch,
                        &documents,
                        provider.as_ref(),
                        &|progress| {
                            eprintln!(
                                "{}",
                                json!({"schema":"art.embedding.reindex.progress.v1","lane":"private","completed":progress.completed,"total":progress.total,"resumed":progress.resumed})
                            );
                        },
                    )?);
                }
                vault.checkpoint_wal()?;
            }
            let (knowledge_editions, knowledge_navigation, knowledge_vectors) = if knowledge {
                let (editions, navigation_count, vector_count) = {
                    let vault = KnowledgeVault::open(paths.knowledge_vault(), load_key(&paths)?)?;
                    let rebuilt = vault.rebuild_projection()?;
                    vault.rebuild_search_index()?;
                    let navigation_count = if navigation {
                        Some(vault.rebuild_navigation()?)
                    } else {
                        None
                    };
                    let vector_count = if let Some((endpoint, provider)) = &embedding {
                        let (source_epoch, documents) = knowledge_semantic_snapshot(&vault)?;
                        Some(SemanticProjection::rebuild_with_progress(
                            &knowledge_semantic_path(&paths.knowledge_vault()),
                            endpoint,
                            &source_epoch,
                            &documents,
                            provider.as_ref(),
                            &|progress| {
                                eprintln!(
                                    "{}",
                                    json!({"schema":"art.embedding.reindex.progress.v1","lane":"knowledge","completed":progress.completed,"total":progress.total,"resumed":progress.resumed})
                                );
                            },
                        )?)
                    } else {
                        None
                    };
                    vault.checkpoint_wal()?;
                    (rebuilt, navigation_count, vector_count)
                };
                (Some(editions), navigation_count, vector_count)
            } else {
                (None, None, None)
            };
            print_json(
                &json!({"schema":"art.cli.v1","reindexed":true,"private_memories":private_memories,"private_navigation":private_navigation,"private_vectors":private_vectors,"knowledge":knowledge,"knowledge_editions":knowledge_editions,"knowledge_navigation":knowledge_navigation,"knowledge_vectors":knowledge_vectors,"provider_fingerprint":provider_fingerprint}),
            )
        }
    }
}

fn backup_command(paths: &ArtPaths, command: BackupCommand) -> ArtResult<()> {
    match command {
        BackupCommand::Create { output } => {
            ensure_initialized(paths)?;
            let manifest =
                create_backup(&paths.knowledge_vault(), &output, env!("CARGO_PKG_VERSION"))?;
            print_json(&json!({
                "schema":"art.cli.v1",
                "status":"created",
                "output":output,
                "edition_count":manifest.edition_count,
                "event_count":manifest.event_count,
                "tree_sha256":manifest.tree_sha256
            }))
        }
        BackupCommand::Verify { source } => {
            let manifest = verify_backup(&source)?;
            print_json(&json!({
                "schema":"art.cli.v1",
                "status":"verified",
                "source":source,
                "edition_count":manifest.edition_count,
                "event_count":manifest.event_count,
                "tree_sha256":manifest.tree_sha256
            }))
        }
        BackupCommand::Restore {
            source,
            target_home,
            commitment_key,
            confirm,
        } => restore_home(&source, &target_home, &commitment_key, confirm),
    }
}

fn restore_home(
    source: &Path,
    target_home: &Path,
    source_key: &Path,
    confirm: bool,
) -> ArtResult<()> {
    if !confirm {
        return Err(ArtError::PermissionDenied(
            "backup restore requires --confirm".into(),
        ));
    }
    if target_home.exists() {
        return Err(ArtError::DuplicateConflict);
    }
    let target_paths = ArtPaths::from_explicit_root(target_home)?;
    let key_metadata = fs::symlink_metadata(source_key).map_err(io_error)?;
    if key_metadata.file_type().is_symlink() || !key_metadata.is_file() {
        return Err(ArtError::PathConflict(
            "commitment key must be a regular file".into(),
        ));
    }
    reject_hard_link(&key_metadata)?;
    if private_mode(source_key)?.is_some_and(|mode| mode != 0o600) {
        return Err(ArtError::PermissionDenied(
            "commitment key must have mode 0600".into(),
        ));
    }
    let key: [u8; 32] = fs::read(source_key)
        .map_err(io_error)?
        .try_into()
        .map_err(|_| ArtError::InvalidInput("invalid commitment key".into()))?;
    let parent = target_home
        .parent()
        .ok_or_else(|| ArtError::InvalidInput("restore target requires a parent".into()))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let name = target_home
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ArtError::InvalidInput("restore target requires a UTF-8 name".into()))?;
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(|error| ArtError::Internal(error.to_string()))?;
    let staging = parent.join(format!("{name}.restore-staging-{}", hex::encode(nonce)));
    let staging_paths = ArtPaths::from_explicit_root(&staging)?;
    fs::create_dir(&staging).map_err(io_error)?;
    set_private_dir(&staging)?;

    let result = (|| {
        write_private_new(&commitment_key_path(&staging_paths), &key)?;
        let diagnostics = restore_backup(source, &staging_paths.knowledge_vault(), key)?;
        if !diagnostics.integrity_ok || !diagnostics.search_index_aligned {
            return Err(ArtError::IndexDegraded);
        }
        fs::rename(&staging, target_home).map_err(io_error)?;
        Ok(diagnostics)
    })();
    match result {
        Ok(diagnostics) => print_json(&json!({
            "schema":"art.cli.v1",
            "status":"restored",
            "target_home":target_paths.root(),
            "edition_count":diagnostics.projection_count,
            "event_count":diagnostics.event_files_verified,
            "integrity_ok":diagnostics.integrity_ok,
            "search_index_aligned":diagnostics.search_index_aligned
        })),
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn resolve_home(explicit: Option<PathBuf>, explicit_config: Option<&Path>) -> ArtResult<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(path) = explicit_config {
        return load_art_config(path);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        ArtError::InvalidInput("--home is required when HOME is unavailable".into())
    })?;
    let built_in = PathBuf::from(home).join(".across");
    let user_config = built_in.join("config/art/config.json");
    if user_config.exists() {
        load_art_config(&user_config)
    } else {
        Ok(built_in)
    }
}

fn load_art_config(path: &Path) -> ArtResult<PathBuf> {
    let config: ArtConfig = serde_json::from_slice(&fs::read(path).map_err(io_error)?)
        .map_err(|error| ArtError::InvalidInput(format!("invalid ART config: {error}")))?;
    if config.schema != "art.config.v1" || !config.home.is_absolute() {
        return Err(ArtError::InvalidInput(
            "ART config requires schema art.config.v1 and an absolute home".into(),
        ));
    }
    Ok(config.home)
}

fn init(paths: &ArtPaths, confirm: bool) -> ArtResult<()> {
    if !confirm {
        return Err(ArtError::PermissionDenied("init requires --confirm".into()));
    }
    for dir in [
        paths.profiles_dir(),
        paths.agents_dir(),
        paths.knowledge_vault(),
        paths
            .control_db()
            .parent()
            .unwrap_or(paths.root())
            .to_path_buf(),
        paths
            .knowledge_index()
            .parent()
            .unwrap_or(paths.root())
            .to_path_buf(),
    ] {
        fs::create_dir_all(&dir).map_err(io_error)?;
        set_private_dir(&dir)?;
    }
    let key_path = commitment_key_path(paths);
    if !key_path.exists() {
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(|error| ArtError::Internal(error.to_string()))?;
        write_private_new(&key_path, &key)?;
    }
    KnowledgeVault::open(paths.knowledge_vault(), load_key(paths)?)?;
    print_json(&json!({"schema":"art.cli.v1","status":"initialized","root":paths.root()}))
}

fn agent_command(paths: &ArtPaths, command: AgentCommand) -> ArtResult<()> {
    match command {
        AgentCommand::Create { id, host } => {
            ensure_initialized(paths)?;
            let agent_id = AgentId::from_str(&id)?;
            let profile_path = paths.agent_profile(&agent_id);
            if profile_path.exists() {
                return Err(ArtError::DuplicateConflict);
            }
            let profile = AgentProfile {
                agent_id: agent_id.clone(),
                host: host.into(),
                schema_version: 1,
            };
            write_private_new(
                &profile_path,
                &serde_json::to_vec_pretty(&profile).map_err(internal_error)?,
            )?;
            AgentVault::open(paths.agent_vault(&agent_id), agent_id.clone())?;
            print_json(&json!({"schema":"art.cli.v1","created":agent_id,"host":profile.host}))
        }
        AgentCommand::List => {
            let mut profiles = Vec::new();
            if paths.profiles_dir().exists() {
                for entry in fs::read_dir(paths.profiles_dir()).map_err(io_error)? {
                    let path = entry.map_err(io_error)?.path();
                    if path.extension().and_then(|value| value.to_str()) == Some("json") {
                        profiles.push(
                            serde_json::from_slice::<AgentProfile>(
                                &fs::read(path).map_err(io_error)?,
                            )
                            .map_err(internal_error)?,
                        );
                    }
                }
            }
            print_json(&json!({"schema":"art.cli.v1","agents":profiles}))
        }
        AgentCommand::Show { id } => {
            let agent = AgentId::from_str(&id)?;
            print_json(&load_profile(paths, &agent)?)
        }
    }
}

fn doctor(
    paths: &ArtPaths,
    agent: Option<&str>,
    repair_preview: bool,
    apply: bool,
) -> ArtResult<()> {
    ensure_initialized(paths)?;
    if apply {
        return Err(ArtError::PermissionDenied(
            "doctor never repairs implicitly; use the exact recovery command from --repair-preview"
                .into(),
        ));
    }
    if let Some(agent) = agent {
        let agent_id = AgentId::from_str(agent)?;
        let profile = load_profile(paths, &agent_id)?;
        let profile_mode = private_mode(&paths.agent_profile(&agent_id))?;
        let (vault, knowledge) = runtime(paths, agent)?;
        let private = vault.diagnostics()?;
        let shared = knowledge.diagnostics()?;
        let vector_status = configured_recall_engine(paths, &vault, &knowledge)
            .vector_status()
            .to_owned();
        let status = if private.integrity_ok
            && private.foreign_key_violations == 0
            && private.bound_agent_id == agent
            && private.file_mode.is_none_or(|mode| mode == 0o600)
            && profile_mode.is_none_or(|mode| mode == 0o600)
            && shared.integrity_ok
            && shared.foreign_key_violations == 0
            && shared.pending_publish_intents == 0
            && shared.projection_hashes_ok
            && shared.control_file_mode.is_none_or(|mode| mode == 0o600)
        {
            "ok"
        } else {
            "degraded"
        };
        let recovery = if repair_preview && shared.pending_publish_intents > 0 {
            vec![
                json!({"action":"inspect quarantined publish intent","target":paths.knowledge_vault().join(".art/recovery"),"automatic":false}),
            ]
        } else {
            Vec::new()
        };
        return print_json(&json!({
            "schema":"art.cli.v1",
            "status":status,
            "binary_version":env!("CARGO_PKG_VERSION"),
            "agent":agent,
            "host":profile.host,
            "profile_mode":profile_mode,
            "agent_vault":private,
            "knowledge":shared,
            "process":{"fd_count":current_fd_count(),"active_requests":0,"task_queue":0},
            "integration_config":{"codex":"not_modified","dsh":"not_modified"},
            "repair_preview":recovery,
            "vector_status":vector_status
        }));
    }
    let knowledge = KnowledgeVault::open(paths.knowledge_vault(), load_key(paths)?)?;
    let shared = knowledge.diagnostics()?;
    let status = if shared.integrity_ok
        && shared.foreign_key_violations == 0
        && shared.pending_publish_intents == 0
        && shared.projection_hashes_ok
    {
        "ok"
    } else {
        "degraded"
    };
    print_json(
        &json!({"schema":"art.cli.v1","status":status,"binary_version":env!("CARGO_PKG_VERSION"),"agent":null,"agent_vault":"not_checked","knowledge":shared,"process":{"fd_count":current_fd_count(),"active_requests":0,"task_queue":0}}),
    )
}

fn memory_command(paths: &ArtPaths, command: MemoryCommand) -> ArtResult<()> {
    match command {
        MemoryCommand::List { agent } => {
            let (vault, _) = runtime(paths, &agent)?;
            print_json(&json!({"schema":"art.cli.v1","memories":vault.list()?}))
        }
        MemoryCommand::Read {
            memory_id,
            agent,
            revision,
            anchors: _,
        } => {
            let (vault, _) = runtime(paths, &agent)?;
            let memory = vault.read(&memory_id)?;
            if revision.is_some_and(|value| value != memory.current_revision) {
                return Err(ArtError::NotFound);
            }
            print_json(&memory)
        }
        MemoryCommand::Assure {
            memory_id,
            agent,
            revision,
            outcome,
            reason,
        } => {
            let (vault, _) = runtime(paths, &agent)?;
            let decision = vault.assure(
                &memory_id,
                revision,
                parse_assurance_outcome(&outcome)?,
                "human:local-user",
                &reason,
            )?;
            print_json(&decision)
        }
        MemoryCommand::Dispute {
            memory_id,
            agent,
            reason,
        } => {
            let (vault, _) = runtime(paths, &agent)?;
            vault.dispute(&memory_id, &reason)?;
            print_json(&json!({"schema":"art.cli.v1","disputed":memory_id}))
        }
        MemoryCommand::Supersede {
            memory_id,
            agent,
            by,
            reason,
        } => {
            let (vault, _) = runtime(paths, &agent)?;
            vault.supersede(&memory_id, &by, &reason)?;
            print_json(&json!({"schema":"art.cli.v1","superseded":memory_id,"by":by}))
        }
        MemoryCommand::Archive {
            memory_id,
            agent,
            reason,
        } => {
            let (vault, _) = runtime(paths, &agent)?;
            vault.archive(&memory_id, &reason)?;
            print_json(&json!({"schema":"art.cli.v1","archived":memory_id}))
        }
    }
}

fn knowledge_command(paths: &ArtPaths, command: KnowledgeCommand) -> ArtResult<()> {
    let vault = KnowledgeVault::open(paths.knowledge_vault(), load_key(paths)?)?;
    match command {
        KnowledgeCommand::Proposal { command } => match command {
            ProposalCommand::List => {
                print_json(&json!({"schema":"art.cli.v1","proposals":vault.list_proposals()?}))
            }
            ProposalCommand::Show { proposal_id, .. } => print_json(&vault.proposal(&proposal_id)?),
            ProposalCommand::Submit { proposal_id } => {
                let proposal = vault.proposal(&proposal_id)?;
                print_json(
                    &json!({"schema":"art.cli.v1","proposal_id":proposal.id,"status":proposal.status}),
                )
            }
            ProposalCommand::Compose {
                sources,
                knowledge_key,
                title,
                applicability,
                markdown_file,
                sensitivity,
                idempotency_key,
            } => {
                let mut locks = Vec::new();
                let mut author = None;
                for reference in sources {
                    let (agent, memory_reference) = reference.split_once(':').ok_or_else(|| {
                        ArtError::InvalidInput(
                            "source must be <agent>:<memory-id>@<revision>".into(),
                        )
                    })?;
                    let (memory_id, revision) =
                        memory_reference.rsplit_once('@').ok_or_else(|| {
                            ArtError::InvalidInput("source must include an exact revision".into())
                        })?;
                    let revision: u32 = revision
                        .parse()
                        .map_err(|_| ArtError::InvalidInput("invalid source revision".into()))?;
                    let (agent_id, _) = identity_and_key(paths, agent)?;
                    let private_vault =
                        AgentVault::open(paths.agent_vault(&agent_id), agent_id.clone())?;
                    let (memory, anchor_set_hash) =
                        private_vault.read_source_revision(memory_id, revision)?;
                    author.get_or_insert_with(|| agent_id.clone());
                    locks.push(ProposalSourceLock {
                        source_type: ProposalSourceType::PrivateMemory,
                        owner_agent_id: Some(agent_id),
                        source_id: memory.id,
                        source_revision: Some(revision),
                        source_content_hash: memory.current_hash,
                        anchor_set_hash: Some(anchor_set_hash),
                        approved_excerpt_hash: None,
                        use_grant_id: None,
                    });
                }
                let proposal = vault.propose(
                    &author.ok_or(ArtError::SourceRequired)?,
                    KnowledgeDraft {
                        knowledge_key,
                        title,
                        applicability,
                        markdown: fs::read_to_string(markdown_file).map_err(io_error)?,
                        sensitivity: parse_sensitivity(&sensitivity)?,
                        risk: RiskLevel::Normal,
                    },
                    locks,
                    &idempotency_key,
                )?;
                print_json(&proposal)
            }
            ProposalCommand::ComposeFile {
                agent,
                knowledge_key,
                title,
                applicability,
                markdown_file,
                source_id,
                source_sha256,
                sensitivity,
                idempotency_key,
            } => {
                let metadata = fs::symlink_metadata(&markdown_file).map_err(io_error)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ArtError::InvalidInput(
                        "Markdown source must be a regular non-symbolic-link file".into(),
                    ));
                }
                reject_hard_link(&metadata)?;
                if markdown_file.extension().and_then(|value| value.to_str()) != Some("md") {
                    return Err(ArtError::InvalidInput(
                        "external knowledge source must be Markdown".into(),
                    ));
                }
                if source_id.trim().is_empty()
                    || Path::new(&source_id).is_absolute()
                    || Path::new(&source_id)
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
                {
                    return Err(ArtError::InvalidInput(
                        "source id must be a non-empty relative identifier without traversal"
                            .into(),
                    ));
                }
                let markdown = fs::read_to_string(&markdown_file).map_err(io_error)?;
                let actual_sha256 = hex::encode(Sha256::digest(markdown.as_bytes()));
                if source_sha256 != actual_sha256 {
                    return Err(ArtError::InvalidInput(
                        "external source digest does not match the reviewed file".into(),
                    ));
                }
                let (author, _) = identity_and_key(paths, &agent)?;
                let proposal = vault.propose(
                    &author,
                    KnowledgeDraft {
                        knowledge_key,
                        title,
                        applicability,
                        markdown,
                        sensitivity: parse_sensitivity(&sensitivity)?,
                        risk: RiskLevel::Normal,
                    },
                    vec![ProposalSourceLock {
                        source_type: ProposalSourceType::FileSnapshot,
                        owner_agent_id: None,
                        source_id,
                        source_revision: None,
                        source_content_hash: actual_sha256.clone(),
                        anchor_set_hash: None,
                        approved_excerpt_hash: Some(actual_sha256),
                        use_grant_id: None,
                    }],
                    &idempotency_key,
                )?;
                print_json(&proposal)
            }
        },
        KnowledgeCommand::Review { command } => match command {
            ReviewCommand::Approve {
                proposal_id,
                revision,
                reason,
            } => {
                vault.approve(
                    &proposal_id,
                    revision,
                    ReviewActor::Human("local-user".into()),
                    &reason,
                )?;
                print_json(
                    &json!({"schema":"art.cli.v1","approved":proposal_id,"revision":revision}),
                )
            }
            ReviewCommand::RequestChanges {
                proposal_id,
                revision,
                reason,
            } => {
                vault.review(
                    &proposal_id,
                    revision,
                    ReviewActor::Human("local-user".into()),
                    "changes_requested",
                    &reason,
                )?;
                print_json(
                    &json!({"schema":"art.cli.v1","changes_requested":proposal_id,"revision":revision}),
                )
            }
            ReviewCommand::Reject {
                proposal_id,
                revision,
                reason,
            } => {
                vault.review(
                    &proposal_id,
                    revision,
                    ReviewActor::Human("local-user".into()),
                    "rejected",
                    &reason,
                )?;
                print_json(
                    &json!({"schema":"art.cli.v1","rejected":proposal_id,"revision":revision}),
                )
            }
        },
        KnowledgeCommand::Publish {
            proposal_id,
            revision,
            confirm,
        } => print_json(&vault.publish(&proposal_id, revision, confirm)?),
        KnowledgeCommand::Revoke {
            edition_id,
            reason,
            confirm,
        } => {
            vault.revoke(&edition_id, &reason, confirm)?;
            print_json(&json!({"schema":"art.cli.v1","revoked":edition_id}))
        }
        KnowledgeCommand::Supersede {
            edition_id,
            with,
            reason,
            confirm,
        } => {
            vault.supersede(&edition_id, &with, &reason, confirm)?;
            print_json(&json!({"schema":"art.cli.v1","superseded":edition_id,"with":with}))
        }
        KnowledgeCommand::Verify { edition } => {
            if let Some(id) = edition {
                vault.read(&id)?;
            } else {
                for edition in vault.list_current()? {
                    vault.read(&edition.edition_id)?;
                }
            }
            print_json(&json!({"schema":"art.cli.v1","verified":true}))
        }
    }
}

fn integration_command(paths: &ArtPaths, command: IntegrationCommand) -> ArtResult<()> {
    let binary = std::env::current_exe().map_err(io_error)?;
    let (rendered, args) = match command {
        IntegrationCommand::Codex(args) => {
            load_profile(paths, &AgentId::from_str(&args.agent)?)?;
            let name = format!("art_{}", args.agent.replace('-', "_"));
            let binary =
                serde_json::to_string(&binary.to_string_lossy()).map_err(internal_error)?;
            let home =
                serde_json::to_string(&paths.root().to_string_lossy()).map_err(internal_error)?;
            let agent = serde_json::to_string(&args.agent).map_err(internal_error)?;
            (
                format!(
                    "[mcp_servers.{name}]\ncommand = {binary}\nargs = [\"--home\", {home}, \"mcp\", \"serve\", \"--agent\", {agent}]\nstartup_timeout_sec = 10\ntool_timeout_sec = 30\n"
                ),
                args,
            )
        }
        IntegrationCommand::Dsh(args) => {
            load_profile(paths, &AgentId::from_str(&args.agent)?)?;
            let binary =
                serde_json::to_string(&binary.to_string_lossy()).map_err(internal_error)?;
            let home =
                serde_json::to_string(&paths.root().to_string_lossy()).map_err(internal_error)?;
            let agent = serde_json::to_string(&args.agent).map_err(internal_error)?;
            (
                format!(
                    "- insert:\n    - id: art-memory\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: art\n        transport: stdio\n        command: {binary}\n        args: [\"--home\", {home}, \"mcp\", \"serve\", \"--agent\", {agent}]\n        env: {{}}\n        failOnStartupError: true\n"
                ),
                args,
            )
        }
    };
    if args.apply {
        let output = args.output.ok_or_else(|| {
            ArtError::InvalidInput("--apply requires an explicit --output".into())
        })?;
        write_private_new(&output, rendered.as_bytes())?;
        return print_json(
            &json!({"schema":"art.cli.v1","applied":true,"output":output,"created_new":true}),
        );
    }
    print!("{rendered}");
    Ok(())
}

fn import_command(paths: &ArtPaths, command: ImportCommand) -> ArtResult<()> {
    match command {
        ImportCommand::Markdown {
            source,
            dry_run,
            copy_to,
            confirm,
        } => {
            if !source.exists() {
                return Err(ArtError::NotFound);
            }
            if !dry_run && (copy_to.is_none() || !confirm) {
                return Err(ArtError::PermissionDenied(
                    "import writes require --copy-to and --confirm".into(),
                ));
            }
            let scan = scan_markdown(&source)?;
            let markdown_files = scan.proposals.len();
            if !dry_run && scan.proposals.iter().any(|item| !item.eligible) {
                return Err(ArtError::PermissionDenied(
                    "blocked Markdown findings must be resolved before copy".into(),
                ));
            }
            let copied_files = if dry_run {
                0
            } else {
                copy_markdown(&source, copy_to.as_deref().expect("validated copy target"))?
            };
            print_json(
                &json!({"schema":"art.cli.v1","source":source,"markdown_files":markdown_files,"copied_files":copied_files,"copy_to":copy_to,"dry_run":dry_run,"source_modified":false,"knowledge_import":scan}),
            )
        }
        ImportCommand::MemoryJsonl {
            source,
            agent,
            confirm,
        } => {
            if !confirm {
                return Err(ArtError::PermissionDenied(
                    "memory import requires --confirm".into(),
                ));
            }
            let (vault, _) = runtime(paths, &agent)?;
            let content = fs::read_to_string(&source).map_err(io_error)?;
            let mut imported = 0_u64;
            for (index, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
                    ArtError::InvalidInput(format!("invalid JSONL at line {}: {error}", index + 1))
                })?;
                if value["schema"] == "art.memory.export.header.v1" {
                    continue;
                }
                let record: art_agent_store::MemoryExportRecord =
                    serde_json::from_value(value).map_err(internal_error)?;
                vault.import_record(&record)?;
                imported += 1;
            }
            print_json(
                &json!({"schema":"art.cli.v1","imported":imported,"source":source,"agent":agent}),
            )
        }
    }
}

fn export_command(paths: &ArtPaths, command: ExportCommand) -> ArtResult<()> {
    match command {
        ExportCommand::Memory {
            agent,
            output,
            include_private,
            confirm,
        } => {
            if include_private && !confirm {
                return Err(ArtError::PermissionDenied(
                    "private export requires --confirm".into(),
                ));
            }
            let (vault, _) = runtime(paths, &agent)?;
            let memories = vault.list()?;
            let records = if include_private {
                memories
                    .iter()
                    .map(|memory| vault.export_record(&memory.id))
                    .collect::<ArtResult<Vec<_>>>()?
            } else {
                Vec::new()
            };
            let mut bytes = serde_json::to_vec(&json!({"schema":"art.memory.export.header.v1","agent":agent,"record_count":records.len(),"excluded_private_count":memories.len()-records.len()})).map_err(internal_error)?;
            bytes.push(b'\n');
            for record in records {
                bytes.extend(serde_json::to_vec(&record).map_err(internal_error)?);
                bytes.push(b'\n');
            }
            write_private_new(&output, &bytes)?;
            print_json(
                &json!({"schema":"art.cli.v1","exported":memories.len(),"included_private":include_private,"output":output}),
            )
        }
        ExportCommand::Knowledge { output } => {
            let vault = KnowledgeVault::open(paths.knowledge_vault(), load_key(paths)?)?;
            let editions = vault.list_current()?;
            copy_knowledge_tree(&paths.knowledge_vault(), &output)?;
            write_private_new(
                &output.join("art-export.json"),
                &serde_json::to_vec_pretty(
                    &json!({"schema":"art.knowledge.export.v1","edition_count":editions.len()}),
                )
                .map_err(internal_error)?,
            )?;
            print_json(
                &json!({"schema":"art.cli.v1","exported_editions":editions.len(),"output":output}),
            )
        }
    }
}

fn runtime(paths: &ArtPaths, agent: &str) -> ArtResult<(AgentVault, KnowledgeVault)> {
    let (agent, key) = identity_and_key(paths, agent)?;
    Ok((
        AgentVault::open(paths.agent_vault(&agent), agent)?,
        KnowledgeVault::open(paths.knowledge_vault(), key)?,
    ))
}

fn configured_recall_engine(
    paths: &ArtPaths,
    private: &AgentVault,
    knowledge: &KnowledgeVault,
) -> RecallEngine {
    let engine = RecallEngine::new(private.clone(), knowledge.clone());
    let config = paths.root().join("config/art/embedding/default.json");
    let engine = configured_semantic(paths, private, knowledge, engine, &config);
    configured_rank_fusion(paths, engine)
}

fn configured_semantic(
    paths: &ArtPaths,
    private: &AgentVault,
    knowledge: &KnowledgeVault,
    engine: RecallEngine,
    config: &Path,
) -> RecallEngine {
    if !config.exists() {
        return engine;
    }
    let Ok(endpoint) = EmbeddingEndpoint::load(config) else {
        return engine.with_semantic_unavailable("degraded", "semantic_configuration_invalid");
    };
    let Ok(provider) = OpenAiCompatibleEmbeddingProvider::new(endpoint.clone()) else {
        return engine.with_semantic_unavailable("degraded", "semantic_provider_unavailable");
    };
    let (Ok(private_epoch), Ok(knowledge_epoch)) = (private.index_epoch(), knowledge.index_epoch())
    else {
        return engine.with_semantic_unavailable("degraded", "semantic_epoch_unavailable");
    };
    match SemanticRuntime::open(
        &endpoint,
        Arc::new(provider),
        &private_semantic_path(private.path()),
        &private_epoch,
        &knowledge_semantic_path(&paths.knowledge_vault()),
        &knowledge_epoch,
    ) {
        Ok(runtime) => engine.with_semantic(runtime),
        Err(_) => engine.with_semantic_unavailable("stale", "semantic_projection_unavailable"),
    }
}

fn configured_rank_fusion(paths: &ArtPaths, engine: RecallEngine) -> RecallEngine {
    let config = paths.root().join("config/art/retrieval/fusion.json");
    if !config.exists() {
        return engine;
    }
    match RankFusionPolicy::load(&config) {
        Ok(policy) => match engine.clone().with_rank_fusion_policy(policy) {
            Ok(configured) => configured,
            Err(_) => {
                engine.with_semantic_unavailable("degraded", "rank_fusion_configuration_invalid")
            }
        },
        Err(_) => engine.with_semantic_unavailable("degraded", "rank_fusion_configuration_invalid"),
    }
}
fn identity_and_key(paths: &ArtPaths, agent: &str) -> ArtResult<(AgentId, [u8; 32])> {
    let id = AgentId::from_str(agent)?;
    load_profile(paths, &id)?;
    Ok((id, load_key(paths)?))
}
fn load_profile(paths: &ArtPaths, id: &AgentId) -> ArtResult<AgentProfile> {
    let bytes = fs::read(paths.agent_profile(id)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ArtError::NotFound
        } else {
            io_error(error)
        }
    })?;
    let profile: AgentProfile = serde_json::from_slice(&bytes).map_err(internal_error)?;
    if profile.agent_id != *id {
        return Err(ArtError::IdentityMismatch);
    }
    Ok(profile)
}
fn ensure_initialized(paths: &ArtPaths) -> ArtResult<()> {
    if commitment_key_path(paths).exists() {
        Ok(())
    } else {
        Err(ArtError::NotFound)
    }
}
fn commitment_key_path(paths: &ArtPaths) -> PathBuf {
    paths.root().join("config/art/commitment.key")
}
fn load_key(paths: &ArtPaths) -> ArtResult<[u8; 32]> {
    fs::read(commitment_key_path(paths))
        .map_err(io_error)?
        .try_into()
        .map_err(|_| ArtError::InvalidInput("invalid commitment key".into()))
}
fn write_private_new(path: &Path, bytes: &[u8]) -> ArtResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ArtError::DuplicateConflict
            } else {
                io_error(error)
            }
        })?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    set_private(path)
}
#[cfg(unix)]
fn set_private(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)
}
#[cfg(not(unix))]
fn set_private(_path: &Path) -> ArtResult<()> {
    Ok(())
}
fn scan_markdown(source: &Path) -> ArtResult<MarkdownScan> {
    let mut files = Vec::new();
    let mut non_markdown_files = 0;
    collect_import_files(source, source, &mut files, &mut non_markdown_files)?;
    let secret = Regex::new(
        r"(?i)(authorization:\s*bearer|begin (rsa|openssh|ec) private key|api[_-]?key\s*=|password\s*=)",
    )
    .map_err(internal_error)?;
    let wiki = Regex::new(r"\[\[([^\]|#]+)").map_err(internal_error)?;
    let mut proposals = Vec::new();
    for (relative, content) in files {
        let mut title = None;
        let mut permalink = None;
        if content.starts_with("---\n") {
            for line in content.lines().skip(1).take_while(|line| *line != "---") {
                if let Some((key, value)) = line.split_once(':') {
                    let value = value.trim().trim_matches(['\'', '"']).to_owned();
                    match key.trim() {
                        "title" if !value.is_empty() => title = Some(value),
                        "permalink" if !value.is_empty() => permalink = Some(value),
                        _ => {}
                    }
                }
            }
        }
        if title.is_none() {
            title = content
                .lines()
                .find_map(|line| line.strip_prefix("# ").map(str::trim).map(str::to_owned));
        }
        let wiki_links: Vec<_> = wiki
            .captures_iter(&content)
            .filter_map(|capture| capture.get(1).map(|value| value.as_str().trim().to_owned()))
            .collect();
        let mut warnings = Vec::new();
        if title.is_none() {
            warnings.push("missing_title".into());
        }
        let eligible = !secret.is_match(&content);
        if !eligible {
            warnings.push("secret_like_content".into());
        }
        let digest = hex::encode(Sha256::digest(content.as_bytes()));
        proposals.push(MarkdownImportProposal {
            proposal_id: format!(
                "artip_{}",
                &hex::encode(Sha256::digest(
                    format!("{}:{digest}", relative.display()).as_bytes()
                ))[..24]
            ),
            source_path: relative,
            content_sha256: digest,
            title,
            permalink,
            wiki_links,
            warnings,
            eligible,
        });
    }
    let mut targets = std::collections::BTreeSet::new();
    for proposal in &proposals {
        if let Some(title) = &proposal.title {
            targets.insert(title.to_lowercase());
        }
        if let Some(permalink) = &proposal.permalink {
            targets.insert(permalink.to_lowercase());
        }
        if let Some(stem) = proposal
            .source_path
            .file_stem()
            .and_then(|value| value.to_str())
        {
            targets.insert(stem.to_lowercase());
        }
    }
    let mut permalink_counts = std::collections::BTreeMap::new();
    for proposal in &proposals {
        if let Some(permalink) = &proposal.permalink {
            *permalink_counts
                .entry(permalink.to_lowercase())
                .or_insert(0) += 1;
        }
    }
    for proposal in &mut proposals {
        if proposal.permalink.as_ref().is_some_and(|value| {
            permalink_counts
                .get(&value.to_lowercase())
                .copied()
                .unwrap_or(0)
                > 1
        }) {
            proposal.warnings.push("duplicate_permalink".into());
            proposal.eligible = false;
        }
        if proposal
            .wiki_links
            .iter()
            .any(|target| !targets.contains(&target.to_lowercase()))
        {
            proposal.warnings.push("dangling_wiki_link".into());
        }
    }
    Ok(MarkdownScan {
        schema: "art.knowledge.import.scan.v1".into(),
        proposals,
        non_markdown_files,
    })
}

fn collect_import_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(PathBuf, String)>,
    non_markdown_files: &mut usize,
) -> ArtResult<()> {
    let metadata = fs::symlink_metadata(current).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(ArtError::InvalidInput(
            "symbolic links are not imported".into(),
        ));
    }
    if metadata.is_file() {
        reject_hard_link(&metadata)?;
        if current.extension().and_then(|value| value.to_str()) == Some("md") {
            let relative = if root.is_file() {
                current
                    .file_name()
                    .map(PathBuf::from)
                    .ok_or_else(|| ArtError::InvalidInput("source file has no name".into()))?
            } else {
                current
                    .strip_prefix(root)
                    .map_err(internal_error)?
                    .to_path_buf()
            };
            files.push((relative, fs::read_to_string(current).map_err(io_error)?));
        } else {
            *non_markdown_files += 1;
        }
        return Ok(());
    }
    for entry in fs::read_dir(current).map_err(io_error)? {
        collect_import_files(
            root,
            &entry.map_err(io_error)?.path(),
            files,
            non_markdown_files,
        )?;
    }
    Ok(())
}

fn copy_markdown(source: &Path, target: &Path) -> ArtResult<usize> {
    if target.exists() {
        return Err(ArtError::DuplicateConflict);
    }
    let source = source.canonicalize().map_err(io_error)?;
    let target_parent = target.parent().unwrap_or_else(|| Path::new("."));
    let target_parent = target_parent.canonicalize().map_err(io_error)?;
    let resolved_target = target_parent.join(
        target
            .file_name()
            .ok_or_else(|| ArtError::InvalidInput("copy target requires a name".into()))?,
    );
    if resolved_target.starts_with(&source) {
        return Err(ArtError::InvalidInput(
            "copy target must be outside the source".into(),
        ));
    }
    fs::create_dir(&resolved_target).map_err(io_error)?;
    set_private_dir(&resolved_target)?;
    let result = copy_markdown_tree(&source, &source, &resolved_target);
    if result.is_err() {
        let _ = fs::remove_dir_all(&resolved_target);
    }
    result
}

fn copy_knowledge_tree(source: &Path, target: &Path) -> ArtResult<()> {
    if target.exists() {
        return Err(ArtError::DuplicateConflict);
    }
    fs::create_dir(target).map_err(io_error)?;
    set_private_dir(target)?;
    for relative in [Path::new("editions"), Path::new(".art/events")] {
        let from = source.join(relative);
        let to = target.join(relative);
        if from.exists() {
            copy_public_tree(&from, &to)?;
        }
    }
    Ok(())
}

fn copy_public_tree(source: &Path, target: &Path) -> ArtResult<()> {
    let metadata = fs::symlink_metadata(source).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(ArtError::PathConflict(
            "symbolic links are not exported".into(),
        ));
    }
    if metadata.is_dir() {
        fs::create_dir(target).map_err(io_error)?;
        set_private_dir(target)?;
        for entry in fs::read_dir(source).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            copy_public_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    reject_hard_link(&metadata)?;
    if !matches!(
        source.extension().and_then(|value| value.to_str()),
        Some("md" | "json")
    ) {
        return Err(ArtError::InvalidInput(
            "knowledge export contains an unsupported file".into(),
        ));
    }
    fs::copy(source, target).map_err(io_error)?;
    set_private(target)
}

fn copy_markdown_tree(source_root: &Path, current: &Path, target: &Path) -> ArtResult<usize> {
    let metadata = fs::symlink_metadata(current).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        return Err(ArtError::InvalidInput(
            "symbolic links are not imported".into(),
        ));
    }
    if metadata.is_file() {
        reject_hard_link(&metadata)?;
        if current.extension().and_then(|value| value.to_str()) != Some("md") {
            return Ok(0);
        }
        let destination = if source_root.is_file() {
            target.join(
                current
                    .file_name()
                    .ok_or_else(|| ArtError::InvalidInput("source file has no name".into()))?,
            )
        } else {
            target.join(current.strip_prefix(source_root).map_err(internal_error)?)
        };
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
            set_private_dir(parent)?;
        }
        fs::copy(current, &destination).map_err(io_error)?;
        set_private(&destination)?;
        return Ok(1);
    }
    let mut count = 0;
    for entry in fs::read_dir(current).map_err(io_error)? {
        count += copy_markdown_tree(source_root, &entry.map_err(io_error)?.path(), target)?;
    }
    Ok(count)
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> ArtResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}
#[cfg(not(unix))]
fn set_private_dir(_path: &Path) -> ArtResult<()> {
    Ok(())
}

fn parse_assurance_outcome(value: &str) -> ArtResult<AssuranceOutcome> {
    match value {
        "corroborated" => Ok(AssuranceOutcome::Corroborated),
        "partially_corroborated" => Ok(AssuranceOutcome::PartiallyCorroborated),
        "disputed" => Ok(AssuranceOutcome::Disputed),
        "invalidated" => Ok(AssuranceOutcome::Invalidated),
        "needs_review" => Ok(AssuranceOutcome::NeedsReview),
        _ => Err(ArtError::InvalidInput("invalid assurance outcome".into())),
    }
}

fn parse_sensitivity(value: &str) -> ArtResult<Sensitivity> {
    match value {
        "private" => Ok(Sensitivity::Private),
        "internal" => Ok(Sensitivity::Internal),
        "public" => Ok(Sensitivity::Public),
        _ => Err(ArtError::InvalidInput("invalid sensitivity".into())),
    }
}

#[cfg(unix)]
fn reject_hard_link(metadata: &fs::Metadata) -> ArtResult<()> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() > 1 {
        Err(ArtError::InvalidInput(
            "hard-linked files are not imported".into(),
        ))
    } else {
        Ok(())
    }
}
#[cfg(not(unix))]
fn reject_hard_link(_metadata: &fs::Metadata) -> ArtResult<()> {
    Ok(())
}
fn print_json(value: &impl serde::Serialize) -> ArtResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(internal_error)?
    );
    Ok(())
}

fn safe_log_filter() -> tracing_subscriber::EnvFilter {
    let requested = std::env::var("RUST_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let level = if requested.contains("trace") {
        "trace"
    } else if requested.contains("debug") {
        "debug"
    } else if requested.contains("info") {
        "info"
    } else {
        "warn"
    };
    tracing_subscriber::EnvFilter::new(format!(
        "warn,art_cli={level},art_mcp={level},art_retrieval={level},art_agent_store={level},art_knowledge={level}"
    ))
}

#[cfg(unix)]
fn private_mode(path: &Path) -> ArtResult<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    Ok(Some(
        fs::metadata(path).map_err(io_error)?.permissions().mode() & 0o777,
    ))
}

#[cfg(not(unix))]
fn private_mode(_path: &Path) -> ArtResult<Option<u32>> {
    Ok(None)
}

fn current_fd_count() -> Option<usize> {
    ["/dev/fd", "/proc/self/fd"]
        .into_iter()
        .find_map(|path| fs::read_dir(path).ok().map(Iterator::count))
}
#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> ArtError {
    ArtError::Io(error.to_string())
}
fn internal_error(error: impl std::fmt::Display) -> ArtError {
    ArtError::Internal(error.to_string())
}
