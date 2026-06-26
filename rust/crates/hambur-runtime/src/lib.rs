use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use hambur_db::{
    AppSnapshot, AttachmentRecord, HamburDatabase, MarkdownBlockPayloadRecord, MessageRecord,
    ModelRouteSnapshot, NewAttachment, NewFileRecord, NewMarkdownBlockPayload, NewTimelineItem,
    NewToolCall, NewToolResult, NewTraceSpan, ProviderModelOverride, ProviderModelUpsert,
    ProviderUpsert, SessionReviewRecord, SessionSummary, SettingsSnapshot, TimelineItemSnapshot,
};
use hambur_filestore::FileStore;
use hambur_llm::{
    CompleteToolCall, FallbackPolicy, ModelCapabilities, ModelMessage, ModelRequest, ModelRouter,
    OPENAI_COMPATIBLE_PROTOCOL, OpenAiCompatibleAdapter, ProviderConfig, ProviderModel,
    ProviderStreamEvent, ProviderTarget, ReasoningMode, RoutePlan, RouteRequirements,
    RoutingStrategy, SseDecoder, ToolCallAccumulator, scripted_openai_sse_chunks, should_fallback,
};
use hambur_markdown::{MarkdownPipeline, MarkdownRenderUpdate};
use hambur_sandbox::{SandboxAccess, SandboxService};
use hambur_tools::{
    MAX_TOOL_ITERATIONS_PER_TURN, RawToolOutput, ToolCallBatch, ToolExecutionRecord,
    ToolInvocation, ToolResult, ToolScheduler,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, sleep, timeout};

#[derive(Debug, Clone)]
pub struct AppBootstrap {
    pub app_files_dir: String,
    pub native_library_dir: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeCommand {
    pub command_id: String,
    pub idempotency_key: String,
    pub created_at_ms: u64,
    pub kind: String,
    pub session_id: String,
    pub turn_id: String,
    pub title: String,
    pub message_id: String,
    pub chunk: String,
    pub content: String,
    pub reasoning: String,
    pub provider_id: String,
    pub model_id: String,
    pub source_message_id: String,
    pub payload_json: String,
    pub finalize: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PlatformRequest {
    pub request_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub kind: String,
    pub payload_json: String,
    pub timeout_ms: u64,
    pub cancellable: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeCommandAck {
    pub command_id: String,
    pub idempotency_key: String,
    pub accepted: bool,
    pub duplicate: bool,
    pub rejection_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSessionListSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub sessions: Vec<SessionSummary>,
    pub selected_session_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeFileResolution {
    pub sandbox_path: String,
    pub host_path: String,
    pub relative_path: String,
    pub root: String,
    pub writable: bool,
    pub exists: bool,
    pub is_file: bool,
    pub mime_type: String,
    pub byte_size: u64,
    pub file_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeRootfsStatus {
    pub rootfs_installed: bool,
    pub proot_available: bool,
    pub root_available: bool,
    pub chroot_available: bool,
    pub backend: String,
    pub version: String,
    pub rootfs_size_bytes: u64,
    pub rootfs_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSkillSummary {
    pub name: String,
    pub description: String,
    pub path: String,
    pub category: String,
    pub tags: Vec<String>,
    pub built_in: bool,
    pub enabled: bool,
    pub created_at_ms: u64,
    pub modified_at_ms: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSkillDetail {
    pub summary: RuntimeSkillSummary,
    pub content: String,
    pub raw_content: String,
    pub skill_dir_path: String,
    pub linked_files_json: String,
    pub selected_file_path: String,
    pub selected_file_content: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeMemoryFileSummary {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at_ms: u64,
    pub entry_count: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeMemoryFileDetail {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at_ms: u64,
    pub entry_count: u32,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSessionSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session: Option<SessionSummary>,
    pub timeline_items: Vec<TimelineItemSnapshot>,
    pub markdown_block_payloads: Vec<MarkdownBlockPayloadRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeTimelinePage {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session_id: String,
    pub items: Vec<TimelineItemSnapshot>,
    pub markdown_block_payloads: Vec<MarkdownBlockPayloadRecord>,
    pub next_before_cursor: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeMessageSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub message: Option<MessageRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSearchSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub query: String,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSettingsSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub settings: SettingsSnapshot,
}

#[derive(Debug, Clone)]
pub enum RuntimeEventKind {
    RuntimeReady,
    SessionCreated,
    SessionOpened,
    SessionDeleted,
    ModelsUpdated,
    MessageUpserted,
    TurnStarted,
    TurnStateChanged,
    AssistantMessageStarted,
    AssistantContentDelta,
    AssistantReasoningDelta,
    MarkdownRenderUpdate,
    AssistantMessageFinished,
    ToolCallStarted,
    ToolCallDelta,
    ToolCallFinished,
    ToolCallFailed,
    PlatformRequest,
    PlatformRequestCancelled,
    PlatformRequestTimedOut,
    AttachmentImported,
    PendingAttachmentRemoved,
    PendingAttachmentsCleaned,
    SessionRenamed,
    SessionPinnedChanged,
    SettingsChanged,
    TurnFinished,
    TurnFailed,
    TurnCancelled,
    RuntimeClosed,
    RuntimeError,
}

impl RuntimeEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RuntimeReady => "RuntimeReady",
            Self::SessionCreated => "SessionCreated",
            Self::SessionOpened => "SessionOpened",
            Self::SessionDeleted => "SessionDeleted",
            Self::ModelsUpdated => "ModelsUpdated",
            Self::MessageUpserted => "MessageUpserted",
            Self::TurnStarted => "TurnStarted",
            Self::TurnStateChanged => "TurnStateChanged",
            Self::AssistantMessageStarted => "AssistantMessageStarted",
            Self::AssistantContentDelta => "AssistantContentDelta",
            Self::AssistantReasoningDelta => "AssistantReasoningDelta",
            Self::MarkdownRenderUpdate => "MarkdownRenderUpdate",
            Self::AssistantMessageFinished => "AssistantMessageFinished",
            Self::ToolCallStarted => "ToolCallStarted",
            Self::ToolCallDelta => "ToolCallDelta",
            Self::ToolCallFinished => "ToolCallFinished",
            Self::ToolCallFailed => "ToolCallFailed",
            Self::PlatformRequest => "PlatformRequest",
            Self::PlatformRequestCancelled => "PlatformRequestCancelled",
            Self::PlatformRequestTimedOut => "PlatformRequestTimedOut",
            Self::AttachmentImported => "AttachmentImported",
            Self::PendingAttachmentRemoved => "PendingAttachmentRemoved",
            Self::PendingAttachmentsCleaned => "PendingAttachmentsCleaned",
            Self::SessionRenamed => "SessionRenamed",
            Self::SessionPinnedChanged => "SessionPinnedChanged",
            Self::SettingsChanged => "SettingsChanged",
            Self::TurnFinished => "TurnFinished",
            Self::TurnFailed => "TurnFailed",
            Self::TurnCancelled => "TurnCancelled",
            Self::RuntimeClosed => "RuntimeClosed",
            Self::RuntimeError => "RuntimeError",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeEvent {
    pub event_id: String,
    pub schema_version: u32,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub kind: RuntimeEventKind,
    pub session_id: String,
    pub turn_id: String,
    pub snapshot: AppSnapshot,
    pub markdown_render_update: MarkdownRenderUpdate,
    pub platform_request: PlatformRequest,
    pub error_code: String,
    pub message: String,
}

pub struct RuntimeEngine {
    tokio: Runtime,
    self_ref: Mutex<Weak<RuntimeEngine>>,
    bootstrap: AppBootstrap,
    database: HamburDatabase,
    filestore: FileStore,
    sandbox: SandboxService,
    markdown_streams: Mutex<HashMap<String, MarkdownPipeline>>,
    active_turns: Mutex<HashMap<String, ActiveTurn>>,
    platform_requests: Mutex<HashMap<String, oneshot::Sender<PlatformResultPayload>>>,
    delegate_tasks: Mutex<HashMap<String, DelegateTaskState>>,
    delegate_sessions: Mutex<HashSet<String>>,
    process_sessions: Mutex<HashMap<String, BackgroundProcessSession>>,
    memory_review_sessions: Mutex<HashSet<String>>,
    router: Mutex<ModelRouter>,
    tools: ToolScheduler,
    idempotency: Mutex<HashMap<String, RuntimeCommandAck>>,
    sender: mpsc::Sender<RuntimeEvent>,
    receiver: Mutex<mpsc::Receiver<RuntimeEvent>>,
    sequence: AtomicU64,
    shutdown: AtomicBool,
}

#[derive(Debug, Clone)]
struct ActiveTurn {
    turn_id: String,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
struct PlatformResultPayload {
    is_error: bool,
    payload_json: String,
    error_code: String,
    message: String,
}

struct DelegateTaskState {
    parent_session_id: String,
    parent_turn_id: String,
    child_session_id: String,
    trace_id: String,
    sender: oneshot::Sender<DelegateCompletionPayload>,
}

#[derive(Debug, Clone)]
struct DelegateCompletionPayload {
    is_error: bool,
    content: Value,
    summary: String,
}

#[derive(Debug, Clone)]
struct DelegateTaskSnapshot {
    parent_session_id: String,
    child_session_id: String,
}

impl DelegateTaskState {
    fn snapshot(&self) -> DelegateTaskSnapshot {
        DelegateTaskSnapshot {
            parent_session_id: self.parent_session_id.clone(),
            child_session_id: self.child_session_id.clone(),
        }
    }
}

struct PreparedDelegateTurn {
    turn_id: String,
    assistant_message_id: String,
    route_candidates: Vec<ModelRouteSnapshot>,
    fallback_policy: FallbackPolicy,
    cancel: Arc<AtomicBool>,
    stream_sources_by_route: Vec<RouteStreamSource>,
}

struct BackgroundProcessSession {
    session_id: String,
    process_session_id: String,
    backend: String,
    command: String,
    cwd: String,
    started_at_ms: u64,
    pid: u32,
    pid_file: Option<PathBuf>,
    child: Child,
    output: Arc<Mutex<ProcessOutputBuffer>>,
    exit_code: Option<i32>,
    finished_at_ms: u64,
}

#[derive(Debug, Default)]
struct ProcessOutputBuffer {
    stdout: VecDeque<u8>,
    stderr: VecDeque<u8>,
    stdout_total_bytes: u64,
    stderr_total_bytes: u64,
}

impl ProcessOutputBuffer {
    fn push_stdout(&mut self, bytes: &[u8]) {
        push_ring(&mut self.stdout, bytes);
        self.stdout_total_bytes += bytes.len() as u64;
    }

    fn push_stderr(&mut self, bytes: &[u8]) {
        push_ring(&mut self.stderr, bytes);
        self.stderr_total_bytes += bytes.len() as u64;
    }

    fn snapshot(&self) -> ProcessOutputSnapshot {
        ProcessOutputSnapshot {
            stdout: String::from_utf8_lossy(&self.stdout.iter().copied().collect::<Vec<_>>())
                .to_string(),
            stderr: String::from_utf8_lossy(&self.stderr.iter().copied().collect::<Vec<_>>())
                .to_string(),
            stdout_total_bytes: self.stdout_total_bytes,
            stderr_total_bytes: self.stderr_total_bytes,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ProcessOutputSnapshot {
    stdout: String,
    stderr: String,
    stdout_total_bytes: u64,
    stderr_total_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct MemorySnapshot {
    memory_block: String,
    user_block: String,
}

impl MemorySnapshot {
    fn is_empty(&self) -> bool {
        self.memory_block.trim().is_empty() && self.user_block.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default)]
struct MemoryReviewAssistantMessage {
    content: String,
    tool_calls: Vec<CompleteToolCall>,
}

enum StreamAttemptResult {
    Completed,
    Cancelled,
    Continue(ToolContinuation),
    Failed {
        error: HamburError,
        semantic_delta_started: bool,
    },
}

#[derive(Debug, Clone)]
struct ToolContinuation {
    assistant_message_id: String,
    route: ModelRouteSnapshot,
    stream_source: RouteStreamSource,
    tool_iteration: u32,
}

#[derive(Debug, Clone)]
enum RouteStreamSource {
    Scripted {
        request: ModelRequest,
        chunks: Vec<Vec<u8>>,
        continuation_sse: Vec<String>,
    },
    Provider(ModelRequest),
}

#[derive(Debug, Default)]
struct StreamAttemptState {
    decoder: SseDecoder,
    content: String,
    reasoning: String,
    semantic_delta_started: bool,
    finish_reason: String,
    native_finish_reason: String,
    saw_tool_delta: bool,
    tool_accumulator: ToolCallAccumulator,
    complete_tool_calls: Vec<CompleteToolCall>,
}

impl RuntimeEngine {
    pub fn create(bootstrap: AppBootstrap) -> HamburResult<Arc<Self>> {
        if bootstrap.app_files_dir.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "app_files_dir must not be empty".to_string(),
            ));
        }

        let tokio = Runtime::new()
            .map_err(|error| HamburError::Internal(format!("tokio runtime: {error}")))?;
        let database_path = database_path(&bootstrap);
        let database = tokio.block_on(HamburDatabase::open(database_path))?;
        let filestore = FileStore::new(&bootstrap.app_files_dir)?;
        let sandbox = SandboxService::new_with_native_library_dir(
            &bootstrap.app_files_dir,
            &bootstrap.native_library_dir,
        )?;
        seed_bundled_skills(
            &PathBuf::from(&bootstrap.app_files_dir)
                .join("sandbox")
                .join("global")
                .join("skills"),
        )?;
        let jobs = tokio.block_on(database.cleanup_pending_attachments(0))?;
        for job in jobs {
            let _ = filestore.delete_relative_if_exists(&job.relative_path);
            let _ = tokio.block_on(database.mark_file_cleanup_done(&job.id));
        }
        let snapshot = tokio.block_on(database.bootstrap_snapshot())?;
        for session in &snapshot.sessions {
            let _ = sandbox.prepare_session(&session.id);
        }
        let tools = ToolScheduler::new(PathBuf::from(&bootstrap.app_files_dir).join("offloads"))?;
        let (sender, receiver) = mpsc::channel(64);
        let engine = Arc::new(Self {
            tokio,
            self_ref: Mutex::new(Weak::new()),
            bootstrap,
            database,
            filestore,
            sandbox,
            markdown_streams: Mutex::new(HashMap::new()),
            active_turns: Mutex::new(HashMap::new()),
            platform_requests: Mutex::new(HashMap::new()),
            delegate_tasks: Mutex::new(HashMap::new()),
            delegate_sessions: Mutex::new(HashSet::new()),
            process_sessions: Mutex::new(HashMap::new()),
            memory_review_sessions: Mutex::new(HashSet::new()),
            router: Mutex::new(ModelRouter::default()),
            tools,
            idempotency: Mutex::new(HashMap::new()),
            sender,
            receiver: Mutex::new(receiver),
            sequence: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });

        if let Ok(mut self_ref) = engine.self_ref.lock() {
            *self_ref = Arc::downgrade(&engine);
        }
        engine.emit(RuntimeEventKind::RuntimeReady, snapshot, None)?;
        engine.schedule_startup_memory_review_check();
        Ok(engine)
    }

    fn safe_block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| self.tokio.block_on(future))
        } else {
            self.tokio.block_on(future)
        }
    }

    pub fn dispatch(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        let command = normalize_command(command);
        let validation = validate_command(&command);
        if let Err(error) = validation {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let key = command.idempotency_key.clone();
        if let Some(previous) = self
            .idempotency
            .lock()
            .ok()
            .and_then(|registry| registry.get(&key).cloned())
        {
            return RuntimeCommandAck {
                duplicate: true,
                ..previous
            };
        }

        let ack = self.handle_command(command);
        if let Ok(mut registry) = self.idempotency.lock() {
            registry.insert(key, ack.clone());
        }
        ack
    }

    pub fn next_event(&self) -> Option<RuntimeEvent> {
        let mut receiver = self.receiver.lock().ok()?;
        receiver.blocking_recv()
    }

    pub fn create_session(&self, title: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "CreateSession".to_string(),
            title,
            idempotency_key: new_id("idem_session_create"),
            ..RuntimeCommand::default()
        })
    }

    pub fn open_session(&self, session_id: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "OpenSession".to_string(),
            session_id: session_id.clone(),
            idempotency_key: format!("{session_id}:open"),
            ..RuntimeCommand::default()
        })
    }

    pub fn delete_session(&self, session_id: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "SoftDeleteSession".to_string(),
            session_id: session_id.clone(),
            idempotency_key: format!("{session_id}:soft-delete:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        })
    }

    pub fn append_markdown_delta(
        &self,
        session_id: String,
        message_id: String,
        chunk: String,
        finalize: bool,
    ) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "AppendMarkdownDelta".to_string(),
            session_id,
            message_id: message_id.clone(),
            chunk,
            finalize,
            idempotency_key: format!("markdown:{message_id}:{}", new_id("delta")),
            ..RuntimeCommand::default()
        })
    }

    fn handle_command(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        match command.kind.as_str() {
            "Initialize" => accepted_ack(command.command_id, command.idempotency_key),
            "Shutdown" => {
                self.shutdown();
                accepted_ack(command.command_id, command.idempotency_key)
            }
            "CreateSession" => self.execute_create_session(command),
            "OpenSession" => self.execute_open_session(command),
            "RenameSession" | "UpdateSessionTitle" => self.execute_rename_session(command),
            "SetSessionPinned" | "PinSession" | "UnpinSession" => {
                self.execute_set_session_pinned(command)
            }
            "DeleteSession" | "SoftDeleteSession" | "HardPurgeSession" => {
                self.execute_delete_session(command)
            }
            "UpdateProvider" => self.execute_update_provider(command),
            "DeleteProvider" => self.execute_delete_provider(command),
            "RefreshProviderModels" => self.execute_refresh_provider_models(command),
            "UpdateModelOverride" | "UpdateModelDetail" => {
                self.execute_update_model_override(command)
            }
            "UpdateModelGroup" => self.execute_update_model_group(command),
            "UpdateModelGroupMember" => self.execute_update_model_group_member(command),
            "SetDefaultModelGroup" => self.execute_set_default_model_group(command),
            "DeleteModelGroup" => self.execute_delete_model_group(command),
            "DeleteModelGroupMember" => self.execute_delete_model_group_member(command),
            "UpdateDefaultModelGroups" => self.execute_update_default_model_groups(command),
            "UpdateToolSettings"
            | "UpdateSkills"
            | "UpdateMemoryProjections"
            | "UpdateStartupTasks"
            | "UpdateRootfsSettings"
            | "UpdateAppearance"
            | "UpdateLogs"
            | "UpdateTokenUsage"
            | "UpdatePersona"
            | "UpdateEnvironmentVariables"
            | "UpdateAppSetting"
            | "UpdateBrowserToolSettings"
            | "UpdateSkillEnabled"
            | "UpdateStartupTask"
            | "DeleteStartupTask"
            | "UpdateRootfsSetting" => self.execute_update_app_setting(command),
            "DeleteSkill" => self.execute_delete_skill(command),
            "ImportAttachmentFromUri" => self.execute_import_attachment(command),
            "RemovePendingAttachment" => self.execute_remove_pending_attachment(command),
            "ClearPendingAttachments" => self.execute_clear_pending_attachments(command),
            "SendMessage" => self.execute_send_message(command, "SendMessage"),
            "RetryTurn" => self.execute_send_message(command, "RetryTurn"),
            "RegenerateMessage" => self.execute_send_message(command, "RegenerateMessage"),
            "EditMessage" => self.execute_send_message(command, "EditMessage"),
            "CancelTurn" => self.execute_cancel_turn(command),
            "SubmitPlatformResult" => self.execute_submit_platform_result(command),
            "RunRootfsWarmup" | "ResetRootfs" => self.execute_rootfs_lifecycle(command),
            "AppendMarkdownDelta" | "MarkdownRenderUpdate" => {
                self.execute_append_markdown_delta(command)
            }
            _ => rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand(format!("unsupported command kind: {}", command.kind)),
            ),
        }
    }

    fn execute_create_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let previous_session_id = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self
            .tokio
            .block_on(self.database.create_session(&command.title))
        {
            Ok(snapshot) => {
                if !snapshot.selected_session_id.trim().is_empty()
                    && let Err(error) = self.sandbox.prepare_session(&snapshot.selected_session_id)
                {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
                let new_session_id = snapshot.selected_session_id.clone();
                let _ = self.emit(RuntimeEventKind::SessionCreated, snapshot, None);
                self.spawn_memory_review_if_session_changed(
                    previous_session_id,
                    new_session_id,
                    "session_switch",
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_open_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let previous_session_id = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self
            .tokio
            .block_on(self.database.open_session(&command.session_id))
        {
            Ok(snapshot) => {
                let _ = self.emit(RuntimeEventKind::SessionOpened, snapshot, None);
                self.spawn_memory_review_if_session_changed(
                    previous_session_id,
                    command.session_id.clone(),
                    "session_switch",
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_rename_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let title = command
            .title
            .clone()
            .if_blank(config_payload_string(&command.payload_json, "title"))
            .if_blank(command.content.clone())
            .if_blank(command.chunk.clone());
        match self
            .tokio
            .block_on(self.database.rename_session(&command.session_id, &title))
        {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SessionRenamed,
                    command.session_id,
                    String::new(),
                    snapshot,
                    "Session renamed".to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_set_session_pinned(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let pinned = if command.kind == "PinSession" {
            true
        } else if command.kind == "UnpinSession" {
            false
        } else {
            config_bool(&payload, "pinned", false)
        };
        match self.tokio.block_on(
            self.database
                .set_session_pinned(&command.session_id, pinned),
        ) {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SessionPinnedChanged,
                    command.session_id,
                    String::new(),
                    snapshot,
                    pinned.to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_delete_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        match self
            .tokio
            .block_on(self.database.delete_session(&command.session_id))
        {
            Ok(snapshot) => {
                self.kill_processes_for_session(&command.session_id);
                let _ = self.emit(RuntimeEventKind::SessionDeleted, snapshot, None);
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_update_provider(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let result = self.tokio.block_on(async {
            let provider = self
                .database
                .upsert_provider(ProviderUpsert {
                    id: provider_id.clone(),
                    name: command.title.clone(),
                    icon_name: config_payload_string(&command.payload_json, "iconName")
                        .if_blank("sparkles".to_string()),
                    api_type: OPENAI_COMPATIBLE_PROTOCOL.to_string(),
                    base_url: command.chunk.clone(),
                    secret_ref: provider_secret_ref_from_payload(&command.payload_json),
                    enabled: config_payload_bool(&command.payload_json, "enabled", true),
                })
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateProvider",
                    "provider",
                    &provider.id,
                    &format!(
                        "Provider '{}' saved with redacted secret ({})",
                        provider.name,
                        redacted_secret_label(&provider.secret_ref)
                    ),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    "Provider updated".to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_delete_provider(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if let Err(error) = require_approval(&command, "delete-provider") {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let result = self.tokio.block_on(async {
            self.database.delete_provider(&provider_id).await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteProvider",
                    "provider",
                    &provider_id,
                    "Provider deleted after explicit approval",
                    true,
                    &approval_token_from_payload(&command.payload_json),
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    "Provider deleted".to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_refresh_provider_models(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let models_response = if command.payload_json.trim().is_empty() {
            default_models_response(&command.model_id)
        } else {
            command.payload_json.clone()
        };
        let models =
            match OpenAiCompatibleAdapter::parse_models_response(&provider_id, &models_response) {
                Ok(models) => models,
                Err(error) => {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
        let upserts = models
            .into_iter()
            .map(|model| ProviderModelUpsert {
                model_id: model.model_id,
                display_name: model.display_name,
                supports_tool_call: model.capabilities.supports_tool_call,
                supports_reasoning: model.capabilities.supports_reasoning,
                supports_image_input: model.capabilities.supports_image_input,
                supports_structured_output: model.capabilities.supports_structured_output,
                supports_temperature: model.capabilities.supports_temperature,
                context_limit: model.capabilities.context_limit,
                output_limit: model.capabilities.output_limit,
                reasoning_field: model.capabilities.reasoning_field,
                metadata_json: model.metadata_json,
            })
            .collect::<Vec<_>>();

        match self
            .tokio
            .block_on(self.database.replace_provider_models(&provider_id, upserts))
        {
            Ok(models) => {
                for (index, model) in models.iter().enumerate() {
                    let _ = self
                        .tokio
                        .block_on(self.database.upsert_primary_chat_member(
                            &provider_id,
                            &model.model_id,
                            index as u32,
                        ));
                }
                let _ = self.tokio.block_on(self.database.insert_config_audit(
                    &command.command_id,
                    "user",
                    "RefreshProviderModels",
                    "provider",
                    &provider_id,
                    &format!("{} provider models refreshed", models.len()),
                    false,
                    "",
                ));
                let snapshot = self
                    .tokio
                    .block_on(self.database.bootstrap_snapshot())
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::ModelsUpdated,
                    String::new(),
                    String::new(),
                    snapshot,
                    format!("{} models refreshed", models.len()),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_update_model_override(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(config_string(&payload, "providerId").if_blank(command.message_id.clone()));
        let model_id = command
            .model_id
            .clone()
            .if_blank(config_string(&payload, "modelId"));
        let result = self.tokio.block_on(async {
            let model = self
                .database
                .upsert_provider_model_override(ProviderModelOverride {
                    provider_id: provider_id.clone(),
                    model_id: model_id.clone(),
                    display_name: command
                        .title
                        .clone()
                        .if_blank(config_string(&payload, "displayName")),
                    supports_tool_call: config_bool(&payload, "supportsToolCall", true),
                    supports_reasoning: config_bool(&payload, "supportsReasoning", true),
                    supports_image_input: config_bool(&payload, "supportsImageInput", false),
                    supports_structured_output: config_bool(
                        &payload,
                        "supportsStructuredOutput",
                        false,
                    ),
                    supports_temperature: config_bool(&payload, "supportsTemperature", true),
                    context_limit: config_u32(&payload, "contextLimit", 32000),
                    output_limit: config_u32(&payload, "outputLimit", 4096),
                    reasoning_field: config_string(&payload, "reasoningField"),
                    metadata_json: config_object_string(&payload, "metadataJson"),
                })
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelOverride",
                    "provider_model",
                    &format!("{}/{}", model.provider_id, model.model_id),
                    &format!("Model '{}' overrides saved", model.display_name),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model overrides updated")
    }

    fn execute_update_model_group(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            let group = self
                .database
                .upsert_model_group(
                    &group_id,
                    &command
                        .title
                        .clone()
                        .if_blank(config_string(&payload, "name")),
                    &config_string(&payload, "routingStrategy"),
                    &config_string(&payload, "fallbackPolicy"),
                )
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelGroup",
                    "model_group",
                    &group.id,
                    &format!("Model group '{}' saved", group.name),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group updated")
    }

    fn execute_update_model_group_member(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"))
            .if_blank("grp_primary_chat".to_string());
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(config_string(&payload, "providerId"));
        let model_id = command
            .model_id
            .clone()
            .if_blank(config_string(&payload, "modelId"));
        let position = config_u32(&payload, "position", 0);
        let enabled = config_bool(&payload, "enabled", true);
        let result = self.tokio.block_on(async {
            let member = self
                .database
                .upsert_model_group_member(&group_id, &provider_id, &model_id, position, enabled)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelGroupMember",
                    "model_group_member",
                    &format!(
                        "{}/{}/{}",
                        member.group_id, member.provider_id, member.model_id
                    ),
                    &format!("Model group member enabled={}", member.enabled),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group member updated")
    }

    fn execute_set_default_model_group(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let key = command
            .chunk
            .clone()
            .if_blank(config_string(&payload, "key"));
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            let default = self
                .database
                .set_default_model_group(&key, &group_id)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "SetDefaultModelGroup",
                    "default_model_group",
                    &default.key,
                    &format!("Default group '{}' -> '{}'", default.key, default.group_id),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Default model group updated")
    }

    fn execute_update_default_model_groups(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let payload = config_payload_value(&command.payload_json);
        let primary_group_id = config_string(&payload, "primaryGroupId")
            .if_blank(config_string(&payload, "primary_group_id"))
            .if_blank(config_string(&payload, "primary"))
            .if_blank(command.message_id.clone());
        let secondary_group_id = config_string(&payload, "secondaryGroupId")
            .if_blank(config_string(&payload, "secondary_group_id"))
            .if_blank(config_string(&payload, "secondary"));
        let result = self.tokio.block_on(async {
            if !primary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("primary", &primary_group_id)
                    .await?;
            }
            if !secondary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("secondary", &secondary_group_id)
                    .await?;
            }
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateDefaultModelGroups",
                    "default_model_groups",
                    "default_model_groups",
                    "Default model groups updated",
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Default model groups updated")
    }

    fn execute_delete_model_group(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if let Err(error) = require_approval(&command, "delete-model-group") {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let payload = config_payload_value(&command.payload_json);
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            self.database.delete_model_group(&group_id).await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteModelGroup",
                    "model_group",
                    &group_id,
                    "Model group deleted after explicit approval",
                    true,
                    &approval_token_from_payload(&command.payload_json),
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group deleted")
    }

    fn execute_delete_model_group_member(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let group_id = command.message_id.clone();
        let provider_id = command.provider_id.clone();
        let model_id = command.model_id.clone();
        let result = self.tokio.block_on(async {
            self.database
                .delete_model_group_member(&group_id, &provider_id, &model_id)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteModelGroupMember",
                    "model_group_member",
                    &format!("{}/{}/{}", group_id, provider_id, model_id),
                    "Model group member deleted",
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group member deleted")
    }

    fn execute_update_app_setting(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let setting_key = match setting_key_for_command(&command) {
            Ok(setting_key) => setting_key,
            Err(error) => {
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        if setting_requires_approval(&setting_key)
            && let Err(error) = require_approval(&command, &setting_key)
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let setting_value = setting_value_for_command(&command);
        let result = self.tokio.block_on(async {
            let setting = if command.kind == "DeleteStartupTask" {
                self.database.delete_app_setting(&setting_key).await?;
                hambur_db::AppSettingRecord {
                    key: setting_key.clone(),
                    value: String::new(),
                    updated_at_ms: now_ms(),
                }
            } else {
                self.database
                    .upsert_app_setting(&setting_key, &setting_value)
                    .await?
            };
            let approval_token = approval_token_from_payload(&command.payload_json);
            let approval_required = setting_requires_approval(&setting.key);
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    command.kind.as_str(),
                    "app_setting",
                    &setting.key,
                    &setting_audit_summary(&command.kind, &setting.key),
                    approval_required,
                    &approval_token,
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Setting updated")
    }

    fn finish_settings_command(
        &self,
        command: RuntimeCommand,
        result: HamburResult<AppSnapshot>,
        message: &'static str,
    ) -> RuntimeCommandAck {
        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    message.to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_delete_skill(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let identifier = command
            .message_id
            .clone()
            .if_blank(config_payload_string(&command.payload_json, "skillId"))
            .if_blank(config_payload_string(&command.payload_json, "skillPath"));
        let result = self
            .delete_skill_internal(&identifier)
            .and_then(|deleted_path| {
                self.tokio.block_on(async {
                    self.database
                        .delete_app_setting(&format!("skill_enabled:{deleted_path}"))
                        .await
                        .ok();
                    self.database
                        .insert_config_audit(
                            &command.command_id,
                            "user",
                            "DeleteSkill",
                            "skill",
                            &deleted_path,
                            "Skill directory deleted",
                            false,
                            "",
                        )
                        .await?;
                    self.database.bootstrap_snapshot().await
                })
            });
        self.finish_settings_command(command, result, "Skill deleted")
    }

    fn execute_import_attachment(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let metadata = match AttachmentImportPayload::parse(&command.payload_json) {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let display_name = metadata.display_name.if_blank("attachment".to_string());
        let reserved = match self
            .filestore
            .reserve_session_attachment(&command.session_id, &display_name)
        {
            Ok(reserved) => reserved,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };

        if !metadata.bytes_base64.trim().is_empty() {
            let bytes = match BASE64_STANDARD.decode(metadata.bytes_base64.as_bytes()) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let error =
                        HamburError::InvalidCommand(format!("attachment bytes base64: {error}"));
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
            if let Err(error) = fs::write(&reserved.host_path, bytes) {
                let error = HamburError::Internal(format!("write attachment bytes: {error}"));
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        } else if !metadata.source_path.trim().is_empty() {
            if let Err(error) = fs::copy(&metadata.source_path, &reserved.host_path)
                .map(|_| ())
                .map_err(|error| HamburError::Internal(format!("copy attachment source: {error}")))
            {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        } else if !reserved.host_path.exists()
            && let Err(error) = fs::write(&reserved.host_path, [])
        {
            let error = HamburError::Internal(format!("create attachment placeholder: {error}"));
            let _ = self.emit_error(error.clone());
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let byte_size = if reserved.host_path.exists() {
            fs::metadata(&reserved.host_path)
                .map(|metadata| metadata.len())
                .unwrap_or(metadata.byte_size)
        } else {
            metadata.byte_size
        };
        let result = self.tokio.block_on(async {
            self.database
                .upsert_file_record(NewFileRecord {
                    id: reserved.file_id.clone(),
                    scope: "session".to_string(),
                    session_id: command.session_id.clone(),
                    relative_path: reserved.relative_path.clone(),
                    sandbox_path: reserved.sandbox_path.clone(),
                    mime_type: metadata.mime_type.clone(),
                    byte_size,
                    sha256: metadata.sha256.clone(),
                    retention_policy: "delete_with_session".to_string(),
                })
                .await?;
            let attachment = self
                .database
                .create_pending_attachment(NewAttachment {
                    id: String::new(),
                    session_id: command.session_id.clone(),
                    message_id: String::new(),
                    kind: metadata.kind.clone(),
                    display_name,
                    mime_type: metadata.mime_type.clone(),
                    byte_size,
                    origin_type: metadata.origin_type.clone(),
                    original_uri: metadata.original_uri.clone(),
                    file_id: reserved.file_id,
                    sandbox_path: reserved.sandbox_path,
                    width: metadata.width,
                    height: metadata.height,
                    sha256: metadata.sha256,
                    status: "pending".to_string(),
                })
                .await?;
            let snapshot = self.database.session_snapshot(&command.session_id).await?;
            Ok::<_, HamburError>((attachment, snapshot))
        });

        match result {
            Ok((attachment, snapshot)) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::AttachmentImported,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    attachment.display_name,
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_remove_pending_attachment(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let attachment_id = command.message_id.clone().if_blank(command.chunk.clone());
        if attachment_id.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("attachment_id must not be empty".to_string()),
            );
        }

        let result = self.tokio.block_on(async {
            let (_attachment, cleanup) = self
                .database
                .remove_pending_attachment(&command.session_id, &attachment_id)
                .await?;
            if let Some(job) = cleanup {
                let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                let _ = self.database.mark_file_cleanup_done(&job.id).await;
            }
            self.database.session_snapshot(&command.session_id).await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::PendingAttachmentRemoved,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    attachment_id,
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_clear_pending_attachments(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let result = self.tokio.block_on(async {
            let pending = self
                .database
                .pending_attachments_for_session(&command.session_id)
                .await?;
            for attachment in pending {
                let (_removed, cleanup) = self
                    .database
                    .remove_pending_attachment(&command.session_id, &attachment.id)
                    .await?;
                if let Some(job) = cleanup {
                    let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                    let _ = self.database.mark_file_cleanup_done(&job.id).await;
                }
            }
            self.database.session_snapshot(&command.session_id).await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::PendingAttachmentsCleaned,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    "Pending attachments cleared".to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    fn execute_send_message(
        &self,
        command: RuntimeCommand,
        command_kind: &'static str,
    ) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let mut content = match self.resolve_turn_content(&command, command_kind) {
            Ok(content) => content,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let send_options = SendOptions::parse(&command.payload_json);
        let attachment_ids = send_options.attachment_ids.clone();
        if content.trim().is_empty() && !attachment_ids.is_empty() {
            content = "Attached files.".to_string();
        }
        if content.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("message content must not be empty".to_string()),
            );
        }

        let pending_attachments =
            match self.load_pending_attachments(&command.session_id, &attachment_ids) {
                Ok(attachments) => attachments,
                Err(error) => {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
        let requires_image_input = pending_attachments
            .iter()
            .any(|attachment| attachment.kind == "image");

        if self.active_turn_for_session(&command.session_id).is_some() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::SessionBusy(command.session_id.clone()),
            );
        }

        let routes = match self.tokio.block_on(self.database.primary_chat_route()) {
            Ok(routes) => routes,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let mut plan = route_plan_from_records(routes);
        let requirements = RouteRequirements {
            requires_tool_protocol: send_options.search_enabled,
            requires_image_input,
            requires_structured_output: false,
        };
        plan = match self
            .router
            .lock()
            .map_err(|_| HamburError::Internal("router registry poisoned".to_string()))
            .and_then(|mut router| router.resolve(plan, requirements))
        {
            Ok(plan) => plan,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let route_snapshots = plan
            .targets
            .iter()
            .map(route_snapshot_from_target)
            .collect::<Vec<_>>();
        let Some(route) = route_snapshots.first().cloned() else {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::ModelUnavailable("route has no usable target".to_string()),
            );
        };

        let user_content = format_user_content_with_attachments(&content, &pending_attachments);
        let setup = self.tokio.block_on(async {
            let turn = self
                .database
                .create_turn_with_route(&command.session_id, "StreamingAssistant", &route)
                .await?;
            let user_message = self
                .database
                .insert_message_with_route(
                    &command.session_id,
                    "user",
                    &user_content,
                    "",
                    "completed",
                    &turn.id,
                    &route,
                )
                .await?;
            let attachment_ids = pending_attachments
                .iter()
                .map(|attachment| attachment.id.clone())
                .collect::<Vec<_>>();
            self.database
                .attach_pending_to_message(&command.session_id, &user_message.id, &attachment_ids)
                .await?;
            self.database
                .upsert_timeline_item(
                    &command.session_id,
                    NewTimelineItem {
                        stable_key: user_message.id.clone(),
                        content_type: "user_message".to_string(),
                        display_sequence: user_message.created_at_ms,
                        payload_ref: user_message.id.clone(),
                        small_summary: user_content.chars().take(160).collect(),
                        kind: if command_kind == "EditMessage" {
                            "EditedUserMessage".to_string()
                        } else {
                            "UserMessage".to_string()
                        },
                    },
                )
                .await?;
            let assistant_message = self
                .database
                .insert_message_with_route(
                    &command.session_id,
                    "assistant",
                    "",
                    "",
                    "streaming",
                    &turn.id,
                    &route,
                )
                .await?;
            let snapshot = self.database.session_snapshot(&command.session_id).await?;
            Ok::<_, HamburError>((turn, assistant_message, snapshot))
        });

        let (turn, assistant_message, snapshot) = match setup {
            Ok(value) => value,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };

        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut active_turns) = self.active_turns.lock() {
            active_turns.insert(
                command.session_id.clone(),
                ActiveTurn {
                    turn_id: turn.id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStarted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot.clone(),
            command_kind.to_string(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::MessageUpserted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot.clone(),
            user_content.clone(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::AssistantMessageStarted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot,
            route.model_display_name.clone(),
            None,
        );

        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::Internal("runtime self reference unavailable".to_string()),
            );
        };
        let fallback_policy = plan.fallback_policy;
        let tools_json = self.tools.schemas().compile_openai_tools_json();
        let skills_index_prompt = self.build_skills_index_prompt();
        let memory_system_prompt = self.build_memory_system_prompt();
        let stream_sources_by_route = route_snapshots
            .iter()
            .map(|route| {
                stream_source_for_command(
                    &command,
                    &content,
                    route,
                    &tools_json,
                    &skills_index_prompt,
                    &memory_system_prompt,
                    send_options.deep_thinking_enabled,
                    send_options.search_enabled,
                )
            })
            .collect::<Vec<_>>();
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .run_chat_turn(
                    command.session_id,
                    turn.id,
                    assistant_message.id,
                    route_snapshots,
                    fallback_policy,
                    cancel,
                    stream_sources_by_route,
                    0,
                )
                .await;
        });

        accepted_ack(command.command_id, command.idempotency_key)
    }

    fn resolve_turn_content(
        &self,
        command: &RuntimeCommand,
        command_kind: &str,
    ) -> HamburResult<String> {
        let explicit_content = command.content.clone().if_blank(command.chunk.clone());
        if command_kind == "SendMessage" || command_kind == "EditMessage" {
            return Ok(explicit_content);
        }
        if !explicit_content.trim().is_empty() {
            return Ok(explicit_content);
        }

        let source_message_id = command
            .source_message_id
            .clone()
            .if_blank(command.message_id.clone());
        if source_message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "source_message_id must not be empty".to_string(),
            ));
        }

        self.tokio
            .block_on(
                self.database
                    .source_user_message_for(&command.session_id, &source_message_id),
            )
            .map(|message| message.content_text)
    }

    fn load_pending_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> HamburResult<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = HashSet::new();
        let mut attachments = Vec::new();
        for attachment_id in attachment_ids {
            if !seen.insert(attachment_id.clone()) {
                continue;
            }
            let attachment = self
                .tokio
                .block_on(self.database.attachment_by_id(attachment_id))?;
            if attachment.session_id != session_id {
                return Err(HamburError::InvalidCommand(format!(
                    "attachment does not belong to session: {attachment_id}"
                )));
            }
            if attachment.status != "pending" || !attachment.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(format!(
                    "attachment is not pending: {attachment_id}"
                )));
            }
            attachments.push(attachment);
        }
        Ok(attachments)
    }

    fn execute_cancel_turn(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let active = self.active_turns.lock().ok().and_then(|turns| {
            turns.iter().find_map(|(session_id, active)| {
                let session_matches =
                    command.session_id.is_empty() || command.session_id == *session_id;
                let turn_matches = command.turn_id.is_empty() || command.turn_id == active.turn_id;
                if session_matches && turn_matches {
                    Some(active.clone())
                } else {
                    None
                }
            })
        });

        if let Some(active) = active {
            active.cancel.store(true, Ordering::SeqCst);
        }

        accepted_ack(command.command_id, command.idempotency_key)
    }

    fn execute_append_markdown_delta(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if command.message_id.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("message_id must not be empty".to_string()),
            );
        }

        let stream_key = format!("{}:{}", command.session_id, command.message_id);
        let result = {
            let mut streams = match self.markdown_streams.lock() {
                Ok(streams) => streams,
                Err(_) => {
                    return rejected_ack(
                        command.command_id,
                        command.idempotency_key,
                        HamburError::Internal("markdown stream registry poisoned".to_string()),
                    );
                }
            };
            let pipeline = streams
                .entry(stream_key.clone())
                .or_insert_with(|| MarkdownPipeline::new(command.message_id.clone()));
            let update = if command.chunk.is_empty() {
                MarkdownRenderUpdate {
                    message_id: command.message_id.clone(),
                    ..Default::default()
                }
            } else {
                pipeline.append(&command.chunk)
            };
            let final_update = if command.finalize {
                Some(pipeline.finalize())
            } else {
                None
            };
            if command.finalize {
                streams.remove(&stream_key);
            }
            (update, final_update)
        };

        if !result.0.committed_nodes.is_empty()
            || result.0.pending_node.is_some()
            || result.0.reset
            || !result.0.invalidated_block_ids.is_empty()
        {
            let _ = self.emit_markdown(command.session_id.clone(), result.0);
        }
        if let Some(update) = result.1
            && (!update.committed_nodes.is_empty()
                || update.pending_node.is_some()
                || update.reset
                || !update.invalidated_block_ids.is_empty())
        {
            let _ = self.emit_markdown(command.session_id.clone(), update);
        }

        accepted_ack(command.command_id, command.idempotency_key)
    }

    fn execute_submit_platform_result(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let payload = config_payload_value(&command.payload_json);
        let request_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "requestId"))
            .if_blank(config_string(&payload, "request_id"));
        let Some(sender) = self
            .platform_requests
            .lock()
            .ok()
            .and_then(|mut requests| requests.remove(&request_id))
        else {
            return accepted_ack(command.command_id, command.idempotency_key);
        };

        let result = PlatformResultPayload {
            is_error: config_bool(&payload, "isError", false),
            payload_json: config_value_string(&payload, "payloadJson"),
            error_code: config_string(&payload, "errorCode"),
            message: config_string(&payload, "message"),
        };
        let _ = sender.send(result);
        accepted_ack(command.command_id, command.idempotency_key)
    }

    async fn resolve_provider_api_key(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        cancel: &Arc<AtomicBool>,
    ) -> HamburResult<String> {
        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::Cancelled);
        }
        let secret_ref = route.secret_ref.trim();
        if let Some(env_name) = secret_ref.strip_prefix("env://") {
            let value = std::env::var(env_name).map_err(|_| {
                HamburError::ProviderUnavailable(format!(
                    "provider secret env var is not available: {env_name}"
                ))
            })?;
            if value.trim().is_empty() {
                return Err(HamburError::ProviderUnavailable(format!(
                    "provider secret env var is empty: {env_name}"
                )));
            }
            return Ok(value);
        }
        if !secret_ref.starts_with("android-secret://") {
            return Err(HamburError::ProviderUnavailable(
                "unsupported provider secret_ref".to_string(),
            ));
        }

        let request_id = new_id("platform_req");
        let timeout_ms = 30_000;
        let request = PlatformRequest {
            request_id: request_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: "ResolveSecret".to_string(),
            payload_json: json!({
                "secretRef": secret_ref,
                "providerId": route.provider_id
            })
            .to_string(),
            timeout_ms,
            cancellable: true,
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut requests) = self.platform_requests.lock() {
            requests.insert(request_id.clone(), sender);
        } else {
            return Err(HamburError::Internal(
                "platform request registry unavailable".to_string(),
            ));
        }
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        if let Err(error) = self.emit_platform_request_event(request, snapshot) {
            let _ = self
                .platform_requests
                .lock()
                .ok()
                .and_then(|mut requests| requests.remove(&request_id));
            return Err(error);
        }
        let result = match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => result,
            _ => {
                let _ = self
                    .platform_requests
                    .lock()
                    .ok()
                    .and_then(|mut requests| requests.remove(&request_id));
                return Err(HamburError::ProviderUnavailable(
                    "ResolveSecret platform request timed out".to_string(),
                ));
            }
        };
        if result.is_error {
            return Err(HamburError::ProviderUnavailable(
                result
                    .message
                    .if_blank(result.error_code)
                    .if_blank("ResolveSecret failed".to_string()),
            ));
        }
        let value = config_payload_value(&result.payload_json);
        let api_key = config_string(&value, "apiKey")
            .if_blank(config_string(&value, "secret"))
            .if_blank(config_string(&value, "value"));
        if api_key.trim().is_empty() {
            return Err(HamburError::ProviderUnavailable(
                "ResolveSecret returned an empty secret".to_string(),
            ));
        }
        Ok(api_key)
    }

    fn execute_rootfs_lifecycle(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if command.kind == "ResetRootfs"
            && let Err(error) = require_approval(&command, "rootfs_reset")
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .tokio
            .block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);

        if command.kind == "ResetRootfs" {
            let payload = config_payload_value(&command.payload_json);
            let preserve_root = config_bool(&payload, "preserveRoot", true);
            if let Err(error) = self.sandbox.reset_rootfs(preserve_root) {
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        }

        if let Err(error) = self
            .sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let status = self.sandbox.rootfs_status();
        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        let message = json!({
            "available": status.available,
            "backend": status.backend,
            "abi": status.abi,
            "reason": status.reason,
            "action": command.kind,
            "sessionIdProvided": !command.session_id.trim().is_empty()
        })
        .to_string();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStateChanged,
            command.session_id,
            command.turn_id,
            snapshot,
            message,
            None,
        );
        accepted_ack(command.command_id, command.idempotency_key)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_chat_turn(
        self: Arc<Self>,
        session_id: String,
        turn_id: String,
        assistant_message_id: String,
        routes: Vec<ModelRouteSnapshot>,
        fallback_policy: FallbackPolicy,
        cancel: Arc<AtomicBool>,
        stream_sources_by_route: Vec<RouteStreamSource>,
        tool_iteration: u32,
    ) {
        let mut current_assistant_message_id = assistant_message_id;
        let mut current_routes = routes;
        let mut current_stream_sources_by_route = stream_sources_by_route;
        let mut current_tool_iteration = tool_iteration;
        'agent_loop: loop {
            let target_count = current_routes.len();
            let route_candidates = current_routes.clone();
            let mut last_error = None;

            for (attempt_index, route) in current_routes.iter().cloned().enumerate() {
                if attempt_index > 0 {
                    if let Err(error) = self
                        .database
                        .update_turn_route_snapshot(&turn_id, &route)
                        .await
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        )
                        .await;
                        return;
                    }
                    if let Err(error) = self
                        .database
                        .update_message_route_snapshot(&current_assistant_message_id, &route)
                        .await
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        )
                        .await;
                        return;
                    }

                    let snapshot = self
                        .database
                        .session_snapshot(&session_id)
                        .await
                        .unwrap_or_default();
                    let _ = self.emit_session_event(
                        RuntimeEventKind::TurnStateChanged,
                        session_id.clone(),
                        turn_id.clone(),
                        snapshot,
                        format!("Fallback to {}", route.model_display_name),
                        None,
                    );
                }

                let stream_source = current_stream_sources_by_route
                    .get(attempt_index)
                    .cloned()
                    .unwrap_or_else(|| {
                        let tools_json = self.tools.schemas().compile_openai_tools_json();
                        provider_stream_source(
                            &session_id,
                            &turn_id,
                            "",
                            &route,
                            &tools_json,
                            "",
                            "",
                            false,
                            false,
                        )
                    });
                match self
                    .clone()
                    .run_chat_stream_attempt(
                        session_id.clone(),
                        turn_id.clone(),
                        current_assistant_message_id.clone(),
                        route,
                        route_candidates.clone(),
                        cancel.clone(),
                        stream_source,
                        current_tool_iteration,
                    )
                    .await
                {
                    StreamAttemptResult::Completed | StreamAttemptResult::Cancelled => return,
                    StreamAttemptResult::Continue(continuation) => {
                        current_assistant_message_id = continuation.assistant_message_id;
                        current_routes = vec![continuation.route];
                        current_stream_sources_by_route = vec![continuation.stream_source];
                        current_tool_iteration = continuation.tool_iteration;
                        continue 'agent_loop;
                    }
                    StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started,
                    } => {
                        let can_fallback = should_fallback(
                            fallback_policy,
                            semantic_delta_started,
                            attempt_index,
                            target_count,
                            fallback_error_code(&error),
                        );
                        if can_fallback {
                            last_error = Some(error);
                            continue;
                        }
                        if !semantic_delta_started {
                            self.finish_failed_turn(
                                &session_id,
                                &turn_id,
                                &current_assistant_message_id,
                                "",
                                "",
                                error,
                            )
                            .await;
                        }
                        return;
                    }
                }
            }

            if let Some(error) = last_error {
                self.finish_failed_turn(
                    &session_id,
                    &turn_id,
                    &current_assistant_message_id,
                    "",
                    "",
                    error,
                )
                .await;
            }
            return;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_chat_stream_attempt(
        self: Arc<Self>,
        session_id: String,
        turn_id: String,
        assistant_message_id: String,
        route: ModelRouteSnapshot,
        route_candidates: Vec<ModelRouteSnapshot>,
        cancel: Arc<AtomicBool>,
        stream_source: RouteStreamSource,
        tool_iteration: u32,
    ) -> StreamAttemptResult {
        let mut state = StreamAttemptState::default();
        let mut current_request = match &stream_source {
            RouteStreamSource::Scripted { request, .. } => request.clone(),
            RouteStreamSource::Provider(request) => request.clone(),
        };
        let continuation_sse = match &stream_source {
            RouteStreamSource::Scripted {
                continuation_sse, ..
            } => continuation_sse.clone(),
            RouteStreamSource::Provider(_) => Vec::new(),
        };
        let scripted_source = matches!(&stream_source, RouteStreamSource::Scripted { .. });
        match stream_source {
            RouteStreamSource::Scripted { chunks, .. } => {
                for chunk in chunks {
                    if let Some(result) = self
                        .clone()
                        .process_stream_chunk(
                            &mut state,
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &cancel,
                            &chunk,
                            true,
                        )
                        .await
                    {
                        return result;
                    }
                }
            }
            RouteStreamSource::Provider(request) => {
                current_request = request.clone();
                let api_key = match self
                    .resolve_provider_api_key(&session_id, &turn_id, &route, &cancel)
                    .await
                {
                    Ok(api_key) => api_key,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                let target = provider_target_from_route(route.clone());
                let spec = match OpenAiCompatibleAdapter::build_stream_request(
                    &request, &target, &api_key,
                ) {
                    Ok(spec) => spec,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                let mut response = match reqwest_stream(spec).await {
                    Ok(response) => response,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                loop {
                    if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                        self.finish_cancelled_turn(
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &state.content,
                            &state.reasoning,
                        )
                        .await;
                        return StreamAttemptResult::Cancelled;
                    }
                    let chunk = match response.chunk().await {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            let mapped = HamburError::ProviderUnavailable(format!(
                                "NetworkError: provider stream read failed: {error}"
                            ));
                            if state.semantic_delta_started {
                                self.finish_failed_turn(
                                    &session_id,
                                    &turn_id,
                                    &assistant_message_id,
                                    &state.content,
                                    &state.reasoning,
                                    mapped.clone(),
                                )
                                .await;
                            }
                            return StreamAttemptResult::Failed {
                                error: mapped,
                                semantic_delta_started: state.semantic_delta_started,
                            };
                        }
                    };
                    let Some(chunk) = chunk else {
                        break;
                    };
                    if let Some(result) = self
                        .clone()
                        .process_stream_chunk(
                            &mut state,
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &cancel,
                            chunk.as_ref(),
                            false,
                        )
                        .await
                    {
                        return result;
                    }
                }
            }
        }

        if !state.semantic_delta_started {
            return StreamAttemptResult::Failed {
                error: HamburError::ProviderUnavailable(format!(
                    "provider {} produced no semantic output",
                    route.provider_id
                )),
                semantic_delta_started: state.semantic_delta_started,
            };
        }

        if let Some(update) =
            self.append_stream_markdown(&session_id, &assistant_message_id, "", true)
        {
            let _ = self
                .emit_markdown_event_async(session_id.clone(), turn_id.clone(), update)
                .await;
        }

        let final_finish_reason = state.finish_reason.if_blank("stop".to_string());
        let final_native_finish_reason = state
            .native_finish_reason
            .if_blank(final_finish_reason.clone());
        if state.saw_tool_delta {
            state.complete_tool_calls = state.tool_accumulator.completed_calls();
        }
        if state.saw_tool_delta
            && (state.complete_tool_calls.is_empty()
                || state.tool_accumulator.has_incomplete_calls())
        {
            let error = HamburError::SseParse("incomplete tool call stream".to_string());
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            )
            .await;
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        if !state.complete_tool_calls.is_empty() {
            state.complete_tool_calls.sort_by_key(|call| call.index);
            let result = self
                .execute_tool_batch_and_continue(
                    &session_id,
                    &turn_id,
                    &assistant_message_id,
                    &route,
                    &route_candidates,
                    &cancel,
                    &state.content,
                    &state.reasoning,
                    final_finish_reason,
                    final_native_finish_reason,
                    state.complete_tool_calls,
                    tool_iteration,
                    current_request,
                    continuation_sse,
                    scripted_source,
                )
                .await;
            return match result {
                Ok(Some(continuation)) => StreamAttemptResult::Continue(continuation),
                Ok(None) => StreamAttemptResult::Cancelled,
                Err(error) => {
                    self.finish_failed_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
                        &state.content,
                        &state.reasoning,
                        error.clone(),
                    )
                    .await;
                    StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started: true,
                    }
                }
            };
        }

        if let Err(error) = self
            .database
            .update_message_stream_result(
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                "completed",
                &final_finish_reason,
                &final_native_finish_reason,
            )
            .await
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            )
            .await;
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        if let Err(error) = self
            .database
            .update_turn_status(&turn_id, "Finished", true)
            .await
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            )
            .await;
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .await
            .unwrap_or_default();

        self.clear_active_turn(&session_id, &turn_id);
        let _ = self.emit_session_event(
            RuntimeEventKind::AssistantMessageFinished,
            session_id.clone(),
            turn_id.clone(),
            snapshot.clone(),
            final_finish_reason,
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnFinished,
            session_id,
            turn_id,
            snapshot,
            final_native_finish_reason,
            None,
        );
        StreamAttemptResult::Completed
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_stream_chunk(
        self: Arc<Self>,
        state: &mut StreamAttemptState,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        cancel: &Arc<AtomicBool>,
        chunk: &[u8],
        delay_scripted_chunk: bool,
    ) -> Option<StreamAttemptResult> {
        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                &state.content,
                &state.reasoning,
            )
            .await;
            return Some(StreamAttemptResult::Cancelled);
        }

        if delay_scripted_chunk {
            sleep(Duration::from_millis(24)).await;
        }
        let payloads = match state.decoder.push(chunk) {
            Ok(payloads) => payloads,
            Err(error) => {
                if state.semantic_delta_started {
                    self.finish_failed_turn(
                        session_id,
                        turn_id,
                        assistant_message_id,
                        &state.content,
                        &state.reasoning,
                        error.clone(),
                    )
                    .await;
                }
                return Some(StreamAttemptResult::Failed {
                    error,
                    semantic_delta_started: state.semantic_delta_started,
                });
            }
        };

        for payload in payloads {
            let events = match OpenAiCompatibleAdapter::parse_stream_payload(&payload) {
                Ok(events) => events,
                Err(error) => {
                    if state.semantic_delta_started {
                        self.finish_failed_turn(
                            session_id,
                            turn_id,
                            assistant_message_id,
                            &state.content,
                            &state.reasoning,
                            error.clone(),
                        )
                        .await;
                    }
                    return Some(StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started: state.semantic_delta_started,
                    });
                }
            };

            for event in events {
                let newly_complete_tool_calls = state.tool_accumulator.apply(&event);
                if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                    self.finish_cancelled_turn(
                        session_id,
                        turn_id,
                        assistant_message_id,
                        &state.content,
                        &state.reasoning,
                    )
                    .await;
                    return Some(StreamAttemptResult::Cancelled);
                }

                match event {
                    ProviderStreamEvent::ContentDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.content.push_str(&delta);
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::AssistantContentDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot.clone(),
                            delta.clone(),
                            None,
                        );
                        if let Some(update) = self.append_stream_markdown(
                            session_id,
                            assistant_message_id,
                            &delta,
                            false,
                        ) {
                            let _ = self
                                .emit_markdown_event_async(
                                    session_id.to_string(),
                                    turn_id.to_string(),
                                    update,
                                )
                                .await;
                        }
                    }
                    ProviderStreamEvent::ReasoningDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.reasoning.push_str(&delta);
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::AssistantReasoningDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot,
                            delta,
                            None,
                        );
                    }
                    ProviderStreamEvent::ToolCallDelta { .. } => {
                        state.semantic_delta_started = true;
                        state.saw_tool_delta = true;
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::ToolCallDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot,
                            "Tool call delta".to_string(),
                            None,
                        );
                    }
                    ProviderStreamEvent::Finish {
                        finish_reason,
                        native_finish_reason,
                    } => {
                        state.finish_reason = finish_reason;
                        state.native_finish_reason = native_finish_reason;
                    }
                    ProviderStreamEvent::Error { code, message } => {
                        let error = stream_error_from_provider(code, message);
                        if state.semantic_delta_started {
                            self.finish_failed_turn(
                                session_id,
                                turn_id,
                                assistant_message_id,
                                &state.content,
                                &state.reasoning,
                                error.clone(),
                            )
                            .await;
                        }
                        return Some(StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: state.semantic_delta_started,
                        });
                    }
                }

                for complete in newly_complete_tool_calls {
                    if !state
                        .complete_tool_calls
                        .iter()
                        .any(|existing| existing.index == complete.index)
                    {
                        state.complete_tool_calls.push(complete);
                    }
                }
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_batch_and_continue(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        cancel: &Arc<AtomicBool>,
        content: &str,
        reasoning: &str,
        finish_reason: String,
        native_finish_reason: String,
        complete_tool_calls: Vec<CompleteToolCall>,
        tool_iteration: u32,
        current_request: ModelRequest,
        continuation_sse: Vec<String>,
        scripted_source: bool,
    ) -> HamburResult<Option<ToolContinuation>> {
        if tool_iteration >= MAX_TOOL_ITERATIONS_PER_TURN {
            return Err(HamburError::InvalidCommand(format!(
                "max tool iterations exceeded: {}",
                MAX_TOOL_ITERATIONS_PER_TURN
            )));
        }

        if let Some(update) =
            self.append_stream_markdown(session_id, assistant_message_id, "", true)
        {
            let _ = self
                .emit_markdown_event_async(session_id.to_string(), turn_id.to_string(), update)
                .await;
        }

        self.database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "requires_tool",
                &finish_reason.if_blank("tool_calls".to_string()),
                &native_finish_reason.if_blank("tool_calls".to_string()),
            )
            .await?;

        self.database
            .update_turn_status(turn_id, "ExecutingTools", false)
            .await?;
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStateChanged,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "Executing tools".to_string(),
            None,
        );

        let assistant_tool_calls = complete_tool_calls.clone();
        let mut invocations = Vec::new();
        let mut trace_ids = HashMap::<String, String>::new();
        for call in complete_tool_calls {
            let invocation = ToolInvocation::from_model_call(
                call.index,
                call.id,
                turn_id.to_string(),
                session_id.to_string(),
                call.name,
                call.arguments_json,
            )?;
            self.database
                .insert_tool_call(NewToolCall {
                    id: invocation.tool_call_id.clone(),
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    assistant_message_id: assistant_message_id.to_string(),
                    name: invocation.name.clone(),
                    arguments_json: invocation.arguments_json.clone(),
                    display_title: invocation.display_title.clone(),
                    status: "running".to_string(),
                    requires_approval: invocation.requires_approval,
                })
                .await?;
            let trace = self
                .database
                .insert_trace_span(NewTraceSpan {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    kind: "tool".to_string(),
                    title: invocation.display_title.clone(),
                    content: invocation.arguments_json.clone(),
                    status: "running".to_string(),
                    tool_call_id: invocation.tool_call_id.clone(),
                    payload_json: invocation.arguments_json.clone(),
                    visible: true,
                    ..Default::default()
                })
                .await?;
            trace_ids.insert(invocation.tool_call_id.clone(), trace.id);
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
                RuntimeEventKind::ToolCallStarted,
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                invocation.display_title.clone(),
                None,
            );
            invocations.push(invocation);
        }

        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                content,
                reasoning,
            )
            .await;
            return Ok(None);
        }

        let records = self
            .execute_runtime_tool_batch(
                session_id,
                turn_id,
                assistant_message_id,
                route,
                route_candidates,
                invocations,
            )
            .await?;
        let view_image_handoff = select_view_image_handoff_route(route, route_candidates, &records);
        let mut tool_result_messages = Vec::new();
        let mut context_stubs = Vec::new();
        for record in records {
            let tool_message = self
                .database
                .insert_tool_result_message(
                    session_id,
                    turn_id,
                    &record.invocation.tool_call_id,
                    &record.invocation.name,
                    &record.result.context_stub,
                    route,
                )
                .await?;
            let result = self
                .database
                .insert_tool_result(NewToolResult {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    tool_call_id: record.invocation.tool_call_id.clone(),
                    message_id: tool_message.id,
                    is_error: record.result.is_error,
                    content_json: record.result.content_json.clone(),
                    summary: record.result.summary.clone(),
                    artifacts_json: record.result.artifacts_json.clone(),
                    trust_level: record.result.trust_level.clone(),
                    truncated: record.result.truncated,
                    offloaded_file_id: record.result.offloaded_file_id.clone(),
                    offloaded_path: record.result.offloaded_path.clone(),
                    context_stub: record.result.context_stub.clone(),
                })
                .await?;
            let status = if record.result.is_error {
                "failed"
            } else {
                "completed"
            };
            self.database
                .update_tool_call_status(
                    &record.invocation.tool_call_id,
                    status,
                    &result.id,
                    if record.result.is_error {
                        "ToolError"
                    } else {
                        ""
                    },
                    if record.result.is_error {
                        &record.result.summary
                    } else {
                        ""
                    },
                    true,
                )
                .await?;
            self.update_latest_tool_trace(
                trace_ids
                    .get(&record.invocation.tool_call_id)
                    .map(String::as_str),
                &record.invocation.tool_call_id,
                status,
                &record.result.summary,
            )
            .await?;

            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
                if record.result.is_error {
                    RuntimeEventKind::ToolCallFailed
                } else {
                    RuntimeEventKind::ToolCallFinished
                },
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                record.result.summary.clone(),
                None,
            );
            context_stubs.push(record.result.context_stub.clone());
            tool_result_messages.push(ModelMessage {
                role: "tool".to_string(),
                content: record.result.context_stub,
                tool_calls_json: String::new(),
                tool_call_id: record.invocation.tool_call_id,
            });
        }

        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                content,
                reasoning,
            )
            .await;
            return Ok(None);
        }

        self.database
            .update_turn_status(turn_id, "ContinuingAfterTools", false)
            .await?;
        let continuation_route = view_image_handoff.unwrap_or_else(|| route.clone());
        if continuation_route.provider_id != route.provider_id
            || continuation_route.model_id != route.model_id
        {
            self.database
                .update_turn_route_snapshot(turn_id, &continuation_route)
                .await?;
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
                RuntimeEventKind::TurnStateChanged,
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                "RouteHandoff(ImageInspectionRequired)".to_string(),
                None,
            );
        }
        if context_stubs
            .iter()
            .any(|stub| stub.contains("ImagePart(fileId="))
        {
            let synthetic = self
                .database
                .insert_message_with_route(
                    session_id,
                    "user",
                    &format_synthetic_view_image_message(&context_stubs),
                    "",
                    "completed",
                    turn_id,
                    &continuation_route,
                )
                .await?;
            self.database
                .upsert_timeline_item(
                    session_id,
                    NewTimelineItem {
                        stable_key: synthetic.id.clone(),
                        content_type: "user_message".to_string(),
                        display_sequence: synthetic.created_at_ms,
                        payload_ref: synthetic.id.clone(),
                        small_summary: synthetic.content_text.chars().take(160).collect(),
                        kind: "SyntheticUserMessage".to_string(),
                    },
                )
                .await?;
        }

        let continuation_message = self
            .database
            .insert_message_with_route(
                session_id,
                "assistant",
                "",
                "",
                "streaming",
                turn_id,
                &continuation_route,
            )
            .await?;

        let continuation_source = tool_continuation_stream_source(
            current_request,
            &continuation_route,
            &self.tools.schemas().compile_openai_tools_json(),
            content,
            assistant_tool_calls,
            tool_result_messages,
            continuation_sse,
            scripted_source,
        )?;
        Ok(Some(ToolContinuation {
            assistant_message_id: continuation_message.id,
            route: continuation_route,
            stream_source: continuation_source,
            tool_iteration: tool_iteration.saturating_add(1),
        }))
    }

    async fn update_latest_tool_trace(
        &self,
        trace_id: Option<&str>,
        tool_call_id: &str,
        status: &str,
        summary: &str,
    ) -> HamburResult<()> {
        let Some(trace_id) = trace_id else {
            return Err(HamburError::Internal(format!(
                "tool trace missing for: {tool_call_id}"
            )));
        };
        self.database
            .update_trace_span_status(trace_id, status, summary, true)
            .await?;
        Ok(())
    }

    fn spawn_memory_review_if_session_changed(
        &self,
        previous_session_id: String,
        current_session_id: String,
        reason: &str,
    ) {
        if previous_session_id.trim().is_empty() || previous_session_id == current_session_id {
            return;
        }
        if let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) {
            engine.maybe_spawn_memory_review_for_session(previous_session_id, reason);
        }
    }

    fn schedule_startup_memory_review_check(self: &Arc<Self>) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            sleep(Duration::from_secs(30)).await;
            if engine.shutdown.load(Ordering::SeqCst) {
                return;
            }
            let sessions = engine
                .database
                .unreviewed_sessions(50)
                .await
                .unwrap_or_default();
            for session in sessions {
                engine
                    .clone()
                    .maybe_spawn_memory_review_for_session(session.id, "app_startup");
            }
        });
    }

    fn maybe_spawn_memory_review_for_session(self: Arc<Self>, session_id: String, reason: &str) {
        if session_id.trim().is_empty() || self.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let inserted = self
            .memory_review_sessions
            .lock()
            .map(|mut sessions| sessions.insert(session_id.clone()))
            .unwrap_or(false);
        if !inserted {
            return;
        }
        let reason = reason.to_string();
        let engine = self.clone();
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .clone()
                .review_memory_session_if_needed(session_id.clone(), reason)
                .await;
            if let Ok(mut sessions) = engine.memory_review_sessions.lock() {
                sessions.remove(&session_id);
            }
        });
    }

    async fn review_memory_session_if_needed(self: Arc<Self>, session_id: String, reason: String) {
        if self.active_turn_for_session(&session_id).is_some() {
            return;
        }
        let review = match self.database.session_review_record(&session_id).await {
            Ok(review) => review,
            Err(_) => return,
        };
        if self
            .delegate_sessions
            .lock()
            .map(|sessions| sessions.contains(&session_id))
            .unwrap_or(false)
            || review.title.starts_with("Delegate:")
        {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            return;
        }
        if review.memory_reviewed {
            return;
        }
        if review.messages.is_empty() && review.trace_spans.is_empty() {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            return;
        }
        if self
            .clone()
            .run_automatic_memory_review(review, &reason)
            .await
            .unwrap_or(false)
        {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            let snapshot = self
                .database
                .session_snapshot(&session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
                RuntimeEventKind::SettingsChanged,
                session_id,
                String::new(),
                snapshot,
                "Memory reviewed".to_string(),
                None,
            );
        }
    }

    async fn run_automatic_memory_review(
        self: Arc<Self>,
        review: SessionReviewRecord,
        reason: &str,
    ) -> HamburResult<bool> {
        let routes = self.database.memory_review_route().await?;
        let mut plan = route_plan_from_records(routes);
        plan.targets
            .retain(|target| target.model.capabilities.supports_tool_call);
        if plan.targets.is_empty() {
            return Ok(false);
        }
        let requirements = RouteRequirements {
            requires_tool_protocol: true,
            ..Default::default()
        };
        let route_plan = self
            .router
            .lock()
            .map_err(|_| HamburError::Internal("router registry poisoned".to_string()))?
            .resolve(plan, requirements)?;
        let memory_snapshot = self.memory_snapshot_async().await.unwrap_or_default();
        let initial_messages = build_memory_review_messages(&review, reason, &memory_snapshot);
        for target in route_plan.targets {
            let route = route_snapshot_from_target(&target);
            if !route.supports_tool_call {
                continue;
            }
            if self
                .clone()
                .run_memory_review_on_route(&review.id, &route, initial_messages.clone())
                .await
                .unwrap_or(false)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn run_memory_review_on_route(
        self: Arc<Self>,
        session_id: &str,
        route: &ModelRouteSnapshot,
        mut messages: Vec<ModelMessage>,
    ) -> HamburResult<bool> {
        let mut executed_memory_tool = false;
        let tools_json = compile_named_tools_json(
            &self.tools.schemas().compile_openai_tools_json(),
            &["memory"],
        )?;
        for _ in 0..MAX_MEMORY_REVIEW_TOOL_ITERATIONS {
            let request = ModelRequest {
                request_id: new_id("llm_req"),
                session_id: session_id.to_string(),
                turn_id: new_id("memory_review"),
                purpose: "memory_review".to_string(),
                stream: false,
                system_blocks: Vec::new(),
                messages: messages.clone(),
                reasoning_mode: ReasoningMode::Disabled,
                max_output_tokens: route.output_limit.min(2048),
                temperature: Some(0.0),
                tools_json: tools_json.clone(),
            };
            let assistant = self.memory_review_completion(&request, route).await?;
            if assistant.tool_calls.is_empty() {
                return Ok(true);
            }
            executed_memory_tool = true;
            messages.push(ModelMessage {
                role: "assistant".to_string(),
                content: assistant.content,
                tool_calls_json: complete_tool_calls_json(&assistant.tool_calls)?,
                tool_call_id: String::new(),
            });
            for call in assistant.tool_calls {
                let invocation = ToolInvocation::from_model_call(
                    call.index,
                    call.id,
                    request.turn_id.clone(),
                    session_id.to_string(),
                    call.name,
                    call.arguments_json,
                )?;
                let result = self.execute_memory_review_tool(invocation).await;
                messages.push(ModelMessage {
                    role: "tool".to_string(),
                    content: result.context_stub,
                    tool_calls_json: String::new(),
                    tool_call_id: result.tool_call_id,
                });
            }
        }
        Ok(executed_memory_tool)
    }

    async fn memory_review_completion(
        &self,
        request: &ModelRequest,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MemoryReviewAssistantMessage> {
        let cancel = Arc::new(AtomicBool::new(false));
        let api_key = self
            .resolve_provider_api_key(&request.session_id, &request.turn_id, route, &cancel)
            .await?;
        let target = provider_target_from_route(route.clone());
        let spec = openai_non_stream_request(request, &target, &api_key)?;
        let body = reqwest_json(spec).await?;
        parse_openai_non_stream_message(&body)
    }

    async fn execute_memory_review_tool(&self, invocation: ToolInvocation) -> ToolResult {
        if invocation.name != "memory" {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "Only the memory tool is enabled during automatic memory review.",
            );
        }
        let arguments = match invocation.arguments_value() {
            Ok(arguments) => arguments,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let old_text = arguments
            .get("old_text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if is_review_status_entry(content) || is_review_status_entry(old_text) {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "Review status JSON is not memory. Return it as final assistant content without calling tools.",
            );
        }
        self.execute_knowledge_tool(invocation).await.result
    }

    async fn execute_runtime_tool_batch(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocations: Vec<ToolInvocation>,
    ) -> HamburResult<Vec<ToolExecutionRecord>> {
        let mut regular = Vec::new();
        let mut records = Vec::new();
        let delegate_task_count = invocations
            .iter()
            .filter(|invocation| invocation.name == "delegate_task")
            .count();
        for invocation in invocations {
            match invocation.name.as_str() {
                "view_image" => {
                    records.push(
                        self.execute_view_image_tool(
                            session_id,
                            route,
                            route_candidates,
                            invocation,
                        )
                        .await,
                    );
                }
                "session_search" => {
                    records.push(self.execute_session_search_tool(invocation).await);
                }
                "terminal" | "process" => {
                    records.push(self.execute_sandbox_tool(invocation).await);
                }
                "read_file" | "write_file" | "patch" | "search_files" => {
                    records.push(self.execute_file_tool(invocation).await);
                }
                "memory" | "skill_list" | "skills_list" | "skill_view" => {
                    records.push(self.execute_knowledge_tool(invocation).await);
                }
                "browser_use" => {
                    records.push(
                        self.execute_browser_tool(session_id, turn_id, invocation)
                            .await,
                    );
                }
                "web_fetch" | "web_search" => {
                    records.push(self.execute_web_tool(invocation).await);
                }
                "delegate_task" | "submit_delegate_result" => {
                    if invocation.name == "delegate_task" && delegate_task_count > 3 {
                        let started_at_ms = now_ms();
                        records.push(ToolExecutionRecord {
                            result: ToolResult::failed(
                                &invocation.tool_call_id,
                                &invocation.name,
                                "delegate batch limit exceeded",
                            ),
                            invocation,
                            started_at_ms,
                            ended_at_ms: now_ms(),
                        });
                    } else {
                        records.push(
                            self.execute_delegate_tool(
                                session_id,
                                turn_id,
                                route,
                                route_candidates,
                                invocation,
                            )
                            .await,
                        );
                    }
                }
                _ => {
                    regular.push(invocation);
                }
            }
        }

        if !regular.is_empty() {
            let batch = ToolCallBatch::new(
                turn_id.to_string(),
                assistant_message_id.to_string(),
                regular,
            );
            records.extend(self.tools.execute_batch(batch).await?);
        }
        records.sort_by_key(|record| record.invocation.index);
        Ok(records)
    }

    async fn execute_session_search_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_session_search_result(&invocation, &arguments)
                    .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    async fn resolve_session_search_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(5)
            .clamp(1, 20);
        let sessions = self
            .database
            .search_sessions(query, limit)
            .await
            .unwrap_or_default();
        let matches = sessions
            .iter()
            .map(|session| {
                json!({
                    "sessionId": session.id,
                    "title": session.title,
                    "latestPreview": session.latest_preview,
                    "messageCount": session.message_count,
                    "updatedAtMs": session.updated_at_ms
                })
            })
            .collect::<Vec<_>>();
        let content = json!({
            "query": query,
            "limit": limit,
            "matches": matches
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: format!("Session search returned {} matches", sessions.len()),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    fn get_startup_tasks_and_enabled(&self) -> (Vec<hambur_sandbox::StartupTask>, bool) {
        let settings_snap = self
            .safe_block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let mut tasks = Vec::new();
        let mut enabled = true;
        for setting in &settings_snap.settings {
            if setting.key == "startupTasksEnabled" {
                enabled = setting.value == "true";
            } else if setting.key.starts_with("startup_task:") {
                if let Ok(task) =
                    serde_json::from_str::<hambur_sandbox::StartupTask>(&setting.value)
                {
                    tasks.push(task);
                }
            }
        }
        (tasks, enabled)
    }

    fn execute_sandbox_command(
        &self,
        session_id: &str,
        command: &str,
        cwd_sandbox: &str,
        timeout_ms: u64,
    ) -> HamburResult<hambur_sandbox::SandboxExecResult> {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .safe_block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        self.sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)?;
        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let status = self.sandbox.rootfs_status();
        if !status.available {
            return Err(HamburError::Internal(format!(
                "Sandbox rootfs not available: {}",
                status.reason
            )));
        }
        self.sandbox
            .execute(session_id, command, cwd_sandbox, timeout_ms)
    }

    async fn execute_sandbox_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_sandbox_tool_result(&invocation, &arguments),
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    fn resolve_sandbox_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        if invocation.name == "process" {
            return self.resolve_process_tool_result(invocation, arguments);
        }
        let command = arguments
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if command.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "terminal command must not be empty",
            );
        }
        let cwd = arguments
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or("/var/hambur/workspace");
        let cwd = match self
            .sandbox
            .resolve(&invocation.session_id, cwd, SandboxAccess::Read)
        {
            Ok(resolved) => resolved,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        if arguments
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return self.start_background_process(invocation, command, &cwd.sandbox_path);
        }

        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(30_000)
            .clamp(1_000, 300_000);
        let raw = match self.execute_sandbox_command(
            &invocation.session_id,
            command,
            &cwd.sandbox_path,
            timeout_ms,
        ) {
            Ok(exec_result) => {
                let is_error = exec_result.exit_code != 0 || exec_result.timed_out;
                let status = if exec_result.timed_out {
                    "timeout"
                } else if exec_result.exit_code != 0 {
                    "failed"
                } else {
                    "completed"
                };
                let summary = if exec_result.timed_out {
                    "terminal timed out".to_string()
                } else {
                    format!(
                        "terminal completed with exit code {}",
                        exec_result.exit_code
                    )
                };
                RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error,
                    content: serde_json::to_string(&exec_result).unwrap_or_default(),
                    summary,
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: status.to_string(),
                }
            }
            Err(error) => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: error.to_string(),
                summary: "sandbox execution error".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: "failed".to_string(),
            },
        };
        self.tools.normalize_raw(raw).unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
    }

    fn start_background_process(
        &self,
        invocation: &ToolInvocation,
        command: &str,
        sandbox_cwd: &str,
    ) -> ToolResult {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .safe_block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        if let Err(error) = self
            .sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("sandbox initialization failed: {error}"),
            );
        }
        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let process_session_id = new_id("proc");
        let backend = self.sandbox.rootfs_status().backend;
        let pid_file = if backend == "chroot" {
            match self.sandbox.chroot_process_pid_file(&process_session_id) {
                Ok(path) => Some(path),
                Err(error) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("build execution command failed: {error}"),
                    );
                }
            }
        } else {
            None
        };

        let (program, args, envs) = match self.sandbox.build_execution_command(
            &invocation.session_id,
            command,
            sandbox_cwd,
            &process_session_id,
        ) {
            Ok(res) => res,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("build execution command failed: {error}"),
                );
            }
        };

        let mut cmd = Command::new(&program);
        cmd.args(&args);
        for (k, v) in envs {
            cmd.env(k, v);
        }

        let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("terminal background spawn failed: {error}"),
                );
            }
        };
        let started_at_ms = now_ms();
        let pid = child.id();
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::default()));
        if let Some(stdout) = child.stdout.take() {
            spawn_process_pipe_reader(stdout, output.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_process_pipe_reader(stderr, output.clone(), false);
        }

        let state = BackgroundProcessSession {
            session_id: invocation.session_id.clone(),
            process_session_id: process_session_id.clone(),
            backend: self.sandbox.rootfs_status().backend.clone(),
            command: command.to_string(),
            cwd: sandbox_cwd.to_string(),
            started_at_ms,
            pid,
            pid_file,
            child,
            output,
            exit_code: None,
            finished_at_ms: 0,
        };
        if let Ok(mut sessions) = self.process_sessions.lock() {
            sessions.insert(process_session_id.clone(), state);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process registry unavailable",
            );
        }

        let content = json!({
            "processSessionId": process_session_id,
            "backend": backend,
            "command": command,
            "cwd": sandbox_cwd,
            "startedAt": started_at_ms,
            "pid": pid
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "background process started".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    fn resolve_process_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let process_session_id = arguments
            .get("process_session_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match action {
            "list" => self.list_process_sessions(invocation),
            "poll" | "log" => self.snapshot_process_session(invocation, process_session_id, false),
            "wait" => {
                let timeout_ms = arguments
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(30_000)
                    .clamp(1_000, 300_000);
                self.wait_process_session(invocation, process_session_id, timeout_ms)
            }
            "kill" | "close" => self.kill_process_session(invocation, process_session_id),
            "write" | "submit" => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process stdin is not attached for this backend",
            ),
            _ => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("unsupported process action: {action}"),
            ),
        }
    }

    async fn execute_file_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_file_tool_result(&invocation, &arguments),
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    fn resolve_file_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let result = match invocation.name.as_str() {
            "read_file" => self.read_sandbox_file(invocation, arguments),
            "write_file" => self.write_sandbox_file(invocation, arguments),
            "patch" => self.patch_sandbox_file(invocation, arguments),
            "search_files" => self.search_sandbox_files(invocation, arguments),
            _ => Err(HamburError::InvalidCommand(format!(
                "unknown file tool: {}",
                invocation.name
            ))),
        };
        match result {
            Ok(value) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                summary: value
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("file tool completed")
                    .to_string(),
                content_json: value.to_string(),
                artifacts_json: "[]".to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: value.to_string(),
            },
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        }
    }

    async fn execute_knowledge_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_knowledge_tool_result(&invocation, &arguments)
                    .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    async fn resolve_knowledge_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let tool_name = normalize_knowledge_tool_name(&invocation.name);
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(tool_name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let result = match tool_name {
            "skills_list" => {
                let category = arguments
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let disabled = self.disabled_skill_paths_async().await;
                let all_skills = self
                    .list_skills_with_disabled(&disabled)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|skill| skill.enabled)
                    .collect::<Vec<_>>();
                let mut categories = all_skills
                    .iter()
                    .filter_map(|skill| {
                        (!skill.category.is_empty()).then(|| skill.category.clone())
                    })
                    .collect::<Vec<_>>();
                categories.sort();
                categories.dedup();
                let skills = all_skills
                    .into_iter()
                    .filter(|skill| category.is_empty() || skill.category == category)
                    .map(skill_summary_json)
                    .collect::<Vec<_>>();
                let count = skills.len();
                Ok(json!({
                    "success": true,
                    "skills": skills,
                    "categories": categories,
                    "count": count,
                    "hint": "Use skill_view(name) to see full content, tags, and linked files.",
                    "summary": format!("{count} skills")
                }))
            }
            "skill_view" => {
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file_path = arguments
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let disabled = self.disabled_skill_paths_async().await;
                self.get_skill_detail_with_disabled(name, file_path, &disabled)
                    .map(|detail| skill_detail_json(detail, !file_path.trim().is_empty()))
            }
            "memory" => self.memory_tool_result(arguments),
            _ => Err(HamburError::InvalidCommand(format!(
                "unknown knowledge tool: {}",
                invocation.name
            ))),
        };
        match result {
            Ok(value) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !value
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                summary: value
                    .get("summary")
                    .or_else(|| value.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("knowledge tool completed")
                    .to_string(),
                content_json: value.to_string(),
                artifacts_json: "[]".to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: value.to_string(),
            },
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        }
    }

    fn list_process_sessions(&self, invocation: &ToolInvocation) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let mut values = Vec::new();
        for state in sessions
            .values_mut()
            .filter(|state| state.session_id == invocation.session_id)
        {
            refresh_process_exit(state);
            values.push(process_status_json(state, None));
        }
        let content = json!({ "processes": values });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: format!("{} process sessions", values.len()),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    fn snapshot_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        remove_finished: bool,
    ) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let Some(state) = sessions.get_mut(process_session_id) else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        refresh_process_exit(state);
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(state, Some(output));
        if remove_finished && state.exit_code.is_some() {
            sessions.remove(process_session_id);
        }
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session snapshot".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    fn wait_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        timeout_ms: u64,
    ) -> ToolResult {
        let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
        loop {
            {
                let mut sessions = match self.process_sessions.lock() {
                    Ok(sessions) => sessions,
                    Err(_) => {
                        return ToolResult::failed(
                            &invocation.tool_call_id,
                            &invocation.name,
                            "process registry unavailable",
                        );
                    }
                };
                let Some(state) = sessions.get_mut(process_session_id) else {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session not found",
                    );
                };
                if state.session_id != invocation.session_id {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session belongs to a different session",
                    );
                }
                refresh_process_exit(state);
                if state.exit_code.is_some() {
                    let output = state
                        .output
                        .lock()
                        .map(|buffer| buffer.snapshot())
                        .unwrap_or_default();
                    let content = process_status_json(state, Some(output));
                    sessions.remove(process_session_id);
                    return ToolResult {
                        tool_call_id: invocation.tool_call_id.clone(),
                        tool_name: invocation.name.clone(),
                        is_error: false,
                        content_json: content.to_string(),
                        summary: "process session finished".to_string(),
                        artifacts_json: "[]".to_string(),
                        trust_level: "untrusted".to_string(),
                        truncated: false,
                        offloaded_file_id: String::new(),
                        offloaded_path: String::new(),
                        context_stub: content.to_string(),
                    };
                }
            }
            if Instant::now() >= deadline {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process wait timed out",
                );
            }
            thread::sleep(StdDuration::from_millis(20));
        }
    }

    fn kill_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
    ) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let Some(mut state) = sessions.remove(process_session_id) else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            sessions.insert(process_session_id.to_string(), state);
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        terminate_background_process_wrapper(&state);
        let _ = state.child.kill();
        let _ = state.child.wait();
        state.exit_code = Some(-1);
        state.finished_at_ms = now_ms();
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(&state, Some(output));
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session closed".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    fn kill_processes_for_session(&self, session_id: &str) {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => return,
        };
        let ids = sessions
            .iter()
            .filter_map(|(id, state)| {
                if state.session_id == session_id {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(mut state) = sessions.remove(&id) {
                terminate_background_process_wrapper(&state);
                let _ = state.child.kill();
                let _ = state.child.wait();
            }
        }
    }

    async fn execute_browser_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_browser_tool_result(session_id, turn_id, &invocation, &arguments)
                    .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    async fn resolve_browser_tool_result(
        &self,
        session_id: &str,
        turn_id: &str,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let request_id = new_id("platform_req");
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(60_000)
            .clamp(1_000, 120_000);
        let request = PlatformRequest {
            request_id: request_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: "BrowserAction".to_string(),
            payload_json: json!({
                "toolCallId": invocation.tool_call_id,
                "action": arguments
            })
            .to_string(),
            timeout_ms,
            cancellable: true,
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut requests) = self.platform_requests.lock() {
            requests.insert(request_id.clone(), sender);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "platform request registry unavailable",
            );
        }
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        if let Err(error) = self.emit_platform_request_event(request, snapshot) {
            let _ = self
                .platform_requests
                .lock()
                .ok()
                .and_then(|mut requests| requests.remove(&request_id));
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => {
                let is_error = result.is_error;
                let payload_json = result.payload_json;
                let error_code = result.error_code;
                let message = result.message;
                let mut content = if payload_json.trim().is_empty() {
                    message
                } else {
                    payload_json
                };
                if !is_error {
                    match self
                        .materialize_browser_artifacts(
                            session_id,
                            &invocation.tool_call_id,
                            &content,
                        )
                        .await
                    {
                        Ok(materialized) => {
                            content = materialized;
                        }
                        Err(error) => {
                            return ToolResult::failed(
                                &invocation.tool_call_id,
                                &invocation.name,
                                error.to_string(),
                            );
                        }
                    }
                }
                let summary = if is_error {
                    error_code
                        .clone()
                        .if_blank("Browser action failed".to_string())
                } else {
                    "Browser action completed".to_string()
                };
                let status = if is_error {
                    error_code
                } else {
                    "ok".to_string()
                };
                let raw = RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error,
                    content,
                    summary,
                    trust_level: "untrusted".to_string(),
                    command_or_url: arguments.to_string(),
                    status,
                };
                self.tools.normalize_raw(raw).unwrap_or_else(|error| {
                    ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        error.to_string(),
                    )
                })
            }
            _ => {
                let _ = self
                    .platform_requests
                    .lock()
                    .ok()
                    .and_then(|mut requests| requests.remove(&request_id));
                let snapshot = self
                    .database
                    .session_snapshot(session_id)
                    .await
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::PlatformRequestTimedOut,
                    session_id.to_string(),
                    turn_id.to_string(),
                    snapshot,
                    request_id,
                    Some(&HamburError::InvalidCommand(
                        "PlatformRequestTimeout".to_string(),
                    )),
                );
                ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "PlatformRequestTimeout",
                )
            }
        }
    }

    async fn materialize_browser_artifacts(
        &self,
        session_id: &str,
        tool_call_id: &str,
        content: &str,
    ) -> HamburResult<String> {
        let Ok(mut value) = serde_json::from_str::<Value>(content) else {
            return Ok(content.to_string());
        };
        let Some(base64_value) = value.get("base64").and_then(Value::as_str) else {
            return Ok(content.to_string());
        };
        if base64_value.trim().is_empty() {
            return Ok(content.to_string());
        }
        let bytes = BASE64_STANDARD
            .decode(base64_value.as_bytes())
            .map_err(|error| {
                HamburError::InvalidCommand(format!("browser artifact base64: {error}"))
            })?;
        let mime_type = value
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("image/png")
            .to_string();
        let extension = match mime_type.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let reserved = self.filestore.reserve_cache_image(session_id, extension)?;
        fs::write(&reserved.host_path, &bytes)
            .map_err(|error| HamburError::Internal(format!("write browser artifact: {error}")))?;
        self.database
            .upsert_file_record(NewFileRecord {
                id: reserved.file_id.clone(),
                scope: "session".to_string(),
                session_id: session_id.to_string(),
                relative_path: reserved.relative_path.clone(),
                sandbox_path: reserved.sandbox_path.clone(),
                mime_type: mime_type.clone(),
                byte_size: bytes.len() as u64,
                sha256: String::new(),
                retention_policy: "delete_with_session".to_string(),
            })
            .await?;
        if let Some(object) = value.as_object_mut() {
            object.remove("base64");
            object.insert("fileId".to_string(), json!(reserved.file_id));
            object.insert("sandboxPath".to_string(), json!(reserved.sandbox_path));
            object.insert("relativePath".to_string(), json!(reserved.relative_path));
            object.insert("toolCallId".to_string(), json!(tool_call_id));
            object.insert("materialized".to_string(), json!(true));
        }
        Ok(value.to_string())
    }

    async fn execute_web_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_web_tool_result(&invocation, &arguments),
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    fn resolve_web_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let raw = match invocation.name.as_str() {
            "web_fetch" => run_web_fetch(invocation, arguments),
            "web_search" => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: "web_search requires a configured search provider".to_string(),
                summary: "Web search provider unavailable".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: arguments.to_string(),
                status: "ProviderUnavailable".to_string(),
            },
            _ => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: format!("unknown web tool: {}", invocation.name),
                summary: "Unknown web tool".to_string(),
                trust_level: "trusted".to_string(),
                command_or_url: arguments.to_string(),
                status: "InvalidCommand".to_string(),
            },
        };

        self.tools.normalize_raw(raw).unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
    }

    async fn execute_delegate_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_delegate_result(
                    session_id,
                    turn_id,
                    route,
                    route_candidates,
                    &invocation,
                    &arguments,
                )
                .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    async fn resolve_delegate_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        if invocation.name == "submit_delegate_result" {
            return self
                .resolve_submit_delegate_result(session_id, invocation, arguments)
                .await;
        }
        if self
            .delegate_sessions
            .lock()
            .map(|sessions| sessions.contains(session_id))
            .unwrap_or(false)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate_task is disabled inside delegate sessions",
            );
        }

        self.resolve_delegate_task_result(
            session_id,
            turn_id,
            route,
            route_candidates,
            invocation,
            arguments,
        )
        .await
    }

    async fn resolve_submit_delegate_result(
        &self,
        session_id: &str,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let mut child_paths = Vec::new();
        for path in arguments
            .get("artifact_paths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let child_path = match self.sandbox.resolve(session_id, path, SandboxAccess::Read) {
                Ok(resolved) => resolved,
                Err(error) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        error.to_string(),
                    );
                }
            };
            child_paths.push(child_path);
        }

        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|tasks| tasks.get(session_id).map(DelegateTaskState::snapshot))
        else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "submit_delegate_result is available only inside delegate sessions",
            );
        };
        let mut artifact_mappings = Vec::new();
        for child_path in child_paths {
            if child_path.host_path.exists() {
                let Ok(metadata) = fs::metadata(&child_path.host_path) else {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "delegate artifact metadata unavailable",
                    );
                };
                if metadata.len() > 10 * 1024 * 1024 {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "delegate artifact is too large",
                    );
                }
                let relative_name = child_path
                    .sandbox_path
                    .trim_start_matches("/var/hambur/workspace/")
                    .trim_start_matches('/')
                    .to_string()
                    .if_blank("artifact".to_string());
                let parent_sandbox_path = format!(
                    "/var/hambur/workspace/delegates/{}/{}",
                    state.child_session_id, relative_name
                );
                let parent_path = match self.sandbox.resolve(
                    &state.parent_session_id,
                    &parent_sandbox_path,
                    SandboxAccess::Write,
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        return ToolResult::failed(
                            &invocation.tool_call_id,
                            &invocation.name,
                            error.to_string(),
                        );
                    }
                };
                if parent_path.host_path.exists() {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!(
                            "delegate artifact target already exists: {}",
                            parent_path.sandbox_path
                        ),
                    );
                }
                if let Some(parent) = parent_path.host_path.parent()
                    && let Err(error) = fs::create_dir_all(parent)
                {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("create delegate artifact directory: {error}"),
                    );
                }
                if let Err(error) = fs::copy(&child_path.host_path, &parent_path.host_path) {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("copy delegate artifact: {error}"),
                    );
                }
                artifact_mappings.push(json!({
                    "childPath": child_path.sandbox_path,
                    "parentPath": parent_path.sandbox_path,
                    "bytes": metadata.len()
                }));
            }
        }

        let content = json!({
            "summary": arguments.get("summary").and_then(Value::as_str).unwrap_or_default(),
            "findings": arguments.get("findings").cloned().unwrap_or_else(|| json!([])),
            "changedFiles": arguments.get("changed_files").or_else(|| arguments.get("changedFiles")).cloned().unwrap_or_else(|| json!([])),
            "artifactPaths": arguments.get("artifact_paths").or_else(|| arguments.get("artifactPaths")).cloned().unwrap_or_else(|| json!([])),
            "artifactMappings": artifact_mappings,
            "risks": arguments.get("risks").cloned().unwrap_or_else(|| json!([])),
            "nextSteps": arguments.get("next_steps").or_else(|| arguments.get("nextSteps")).cloned().unwrap_or_else(|| json!([]))
        });
        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(session_id))
        else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "submit_delegate_result is available only inside delegate sessions",
            );
        };
        let summary = arguments
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("Delegate result submitted")
            .to_string();
        let _ = self
            .database
            .update_trace_span_status(&state.trace_id, "completed", &summary, true)
            .await;
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: false,
            content: content.clone(),
            summary: summary.clone(),
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Delegate result submitted".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    async fn resolve_delegate_task_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let pending_count = self
            .delegate_tasks
            .lock()
            .map(|tasks| {
                tasks
                    .values()
                    .filter(|task| task.parent_turn_id == turn_id)
                    .count()
            })
            .unwrap_or(usize::MAX);
        if pending_count >= 3 {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate batch limit exceeded",
            );
        }

        let task = arguments
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if task.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate task must not be empty",
            );
        }
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(600_000)
            .clamp(1_000, 600_000);
        let payload_json = arguments
            .get("payload_json")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let child_snapshot = match self
            .database
            .create_session(&format!(
                "Delegate: {}",
                task.chars().take(72).collect::<String>()
            ))
            .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        let delegate_session_id = child_snapshot.selected_session_id;
        if let Err(error) = self.sandbox.prepare_session(&delegate_session_id) {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        if let Err(error) = self.database.open_session(session_id).await {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let trace = match self
            .database
            .insert_trace_span(NewTraceSpan {
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                kind: "tool".to_string(),
                title: "Delegate session".to_string(),
                content: format!("delegateSessionId={delegate_session_id}\ntask={task}"),
                status: "running".to_string(),
                tool_call_id: invocation.tool_call_id.clone(),
                payload_json: json!({
                    "delegateSessionId": delegate_session_id,
                    "task": task,
                    "toolsets": arguments.get("toolsets").cloned().unwrap_or_else(|| json!([]))
                })
                .to_string(),
                visible: true,
                ..Default::default()
            })
            .await
        {
            Ok(trace) => trace,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };

        let prepared = match self
            .prepare_delegate_child_turn(
                session_id,
                turn_id,
                &delegate_session_id,
                task,
                payload_json,
                route.clone(),
                route_candidates.to_vec(),
            )
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = self
                    .database
                    .update_trace_span_status(&trace.id, "failed", &error.to_string(), true)
                    .await;
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        let (sender, receiver) = oneshot::channel();
        let state = DelegateTaskState {
            parent_session_id: session_id.to_string(),
            parent_turn_id: turn_id.to_string(),
            child_session_id: delegate_session_id.clone(),
            trace_id: trace.id.clone(),
            sender,
        };
        if let Ok(mut tasks) = self.delegate_tasks.lock() {
            tasks.insert(delegate_session_id.clone(), state);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate registry unavailable",
            );
        }
        if let Ok(mut sessions) = self.delegate_sessions.lock() {
            sessions.insert(delegate_session_id.clone());
        }
        self.spawn_prepared_delegate_turn(delegate_session_id.clone(), prepared);

        match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(completion)) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: completion.is_error,
                content_json: completion.content.to_string(),
                summary: completion.summary.clone(),
                artifacts_json: completion
                    .content
                    .get("artifactMappings")
                    .cloned()
                    .unwrap_or_else(|| json!([]))
                    .to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: completion.content.to_string(),
            },
            _ => {
                let _ = self
                    .delegate_tasks
                    .lock()
                    .ok()
                    .and_then(|mut tasks| tasks.remove(&delegate_session_id));
                let message = "delegate task timed out before submit_delegate_result";
                let _ = self
                    .database
                    .update_trace_span_status(&trace.id, "failed", message, true)
                    .await;
                ToolResult::failed(&invocation.tool_call_id, &invocation.name, message)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn prepare_delegate_child_turn(
        &self,
        parent_session_id: &str,
        parent_turn_id: &str,
        delegate_session_id: &str,
        task: &str,
        payload_json: String,
        route: ModelRouteSnapshot,
        route_candidates: Vec<ModelRouteSnapshot>,
    ) -> HamburResult<PreparedDelegateTurn> {
        let child_content = format!(
            "Delegate task from parent session {parent_session_id}, turn {parent_turn_id}.\n\n{task}\n\nFinish by calling submit_delegate_result."
        );
        let turn = self
            .database
            .create_turn_with_route(delegate_session_id, "StreamingAssistant", &route)
            .await?;
        let user_message = self
            .database
            .insert_message_with_route(
                delegate_session_id,
                "user",
                &child_content,
                "",
                "completed",
                &turn.id,
                &route,
            )
            .await?;
        self.database
            .upsert_timeline_item(
                delegate_session_id,
                NewTimelineItem {
                    stable_key: user_message.id.clone(),
                    content_type: "user_message".to_string(),
                    display_sequence: user_message.created_at_ms,
                    payload_ref: user_message.id.clone(),
                    small_summary: child_content.chars().take(160).collect(),
                    kind: "UserMessage".to_string(),
                },
            )
            .await?;
        let assistant_message = self
            .database
            .insert_message_with_route(
                delegate_session_id,
                "assistant",
                "",
                "",
                "streaming",
                &turn.id,
                &route,
            )
            .await?;
        let snapshot = self.database.session_snapshot(delegate_session_id).await?;
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut active_turns) = self.active_turns.lock() {
            active_turns.insert(
                delegate_session_id.to_string(),
                ActiveTurn {
                    turn_id: turn.id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStarted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            "DelegateTask".to_string(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::MessageUpserted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            child_content.clone(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::AssistantMessageStarted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot,
            route.model_display_name.clone(),
            None,
        );

        let route_candidates = if route_candidates.is_empty() {
            vec![route.clone()]
        } else {
            route_candidates
        };
        let fallback_policy = FallbackPolicy::parse(&route.fallback_policy);
        let stream_command = RuntimeCommand {
            session_id: delegate_session_id.to_string(),
            payload_json,
            ..RuntimeCommand::default()
        };
        let tools_json = self.tools.schemas().compile_openai_tools_json();
        let skills_index_prompt = self.build_skills_index_prompt_async().await;
        let memory_system_prompt = self.build_memory_system_prompt_async().await;
        let stream_sources_by_route = route_candidates
            .iter()
            .map(|candidate| {
                stream_source_for_command(
                    &stream_command,
                    &child_content,
                    candidate,
                    &tools_json,
                    &skills_index_prompt,
                    &memory_system_prompt,
                    false,
                    false,
                )
            })
            .collect::<Vec<_>>();
        Ok(PreparedDelegateTurn {
            turn_id: turn.id,
            assistant_message_id: assistant_message.id,
            route_candidates,
            fallback_policy,
            cancel,
            stream_sources_by_route,
        })
    }

    fn spawn_prepared_delegate_turn(
        &self,
        delegate_session_id: String,
        prepared: PreparedDelegateTurn,
    ) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .run_chat_turn(
                    delegate_session_id,
                    prepared.turn_id,
                    prepared.assistant_message_id,
                    prepared.route_candidates,
                    prepared.fallback_policy,
                    prepared.cancel,
                    prepared.stream_sources_by_route,
                    0,
                )
                .await;
        });
    }

    async fn execute_view_image_tool(
        &self,
        session_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_view_image_result(
                    session_id,
                    route,
                    route_candidates,
                    &invocation,
                    &arguments,
                )
                .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    async fn resolve_view_image_result(
        &self,
        session_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let detail = arguments
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("auto")
            .trim();
        if path.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image path must not be empty",
            );
        }
        if !route.supports_image_input && vision_handoff_target(route_candidates, route).is_none() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image unavailable: no vision-capable handoff target",
            );
        }

        let file = match self
            .database
            .resolve_file_by_sandbox_path(session_id, path)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        if !file.mime_type.starts_with("image/") {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image requires an image file",
            );
        }
        let image_attached_to_next_request =
            route.supports_image_input || vision_handoff_target(route_candidates, route).is_some();
        let content = json!({
            "path": path,
            "resolvedPath": file.sandbox_path,
            "detail": normalize_image_detail(detail),
            "width": 0,
            "height": 0,
            "mimeType": file.mime_type,
            "fileId": file.id,
            "imageAttachedToNextRequest": image_attached_to_next_request
        });
        let context_stub = format!(
            "Image returned by view_image for tool_call_id={}: ImagePart(fileId={}, path={}, mimeType={}, detail={})",
            invocation.tool_call_id,
            content
                .get("fileId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("resolvedPath")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Image prepared for vision continuation".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub,
        }
    }

    async fn finish_cancelled_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        content: &str,
        reasoning: &str,
    ) {
        let _ = self
            .database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "cancelled",
                "cancelled",
                "cancelled",
            )
            .await;
        let _ = self
            .database
            .fail_turn(turn_id, "Cancelled", "Cancelled", "turn cancelled")
            .await;
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnCancelled,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "turn cancelled".to_string(),
            Some(&HamburError::Cancelled),
        );
    }

    async fn finish_failed_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        content: &str,
        reasoning: &str,
        error: HamburError,
    ) {
        let _ = self
            .database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "failed_partial",
                "error",
                error.code().as_str(),
            )
            .await;
        let _ = self
            .database
            .fail_turn(turn_id, "Failed", error.code().as_str(), &error.to_string())
            .await;
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnFailed,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            error.to_string(),
            Some(&error),
        );
    }

    fn append_stream_markdown(
        &self,
        session_id: &str,
        message_id: &str,
        chunk: &str,
        finalize: bool,
    ) -> Option<MarkdownRenderUpdate> {
        let stream_key = format!("{session_id}:{message_id}");
        let mut streams = self.markdown_streams.lock().ok()?;
        let pipeline = streams
            .entry(stream_key.clone())
            .or_insert_with(|| MarkdownPipeline::new(message_id.to_string()));
        let update = if finalize {
            pipeline.finalize()
        } else {
            pipeline.append(chunk)
        };
        if finalize {
            streams.remove(&stream_key);
        }
        if update.committed_nodes.is_empty()
            && update.pending_node.is_none()
            && !update.reset
            && update.invalidated_block_ids.is_empty()
        {
            None
        } else {
            Some(update)
        }
    }

    async fn persist_markdown_update_for_timeline(
        &self,
        session_id: &str,
        turn_id: &str,
        update: &MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if update.message_id.trim().is_empty() {
            return Ok(());
        }
        for node in &update.committed_nodes {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize markdown block payload: {error}"))
            })?;
            self.database
                .upsert_markdown_block_payload(
                    session_id,
                    turn_id,
                    NewMarkdownBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        stable_key: node.stable_key.clone(),
                        committed: true,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )
                .await?;
        }
        if let Some(node) = &update.pending_node {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize pending markdown block payload: {error}"))
            })?;
            self.database
                .upsert_markdown_block_payload(
                    session_id,
                    turn_id,
                    NewMarkdownBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        stable_key: hambur_db::pending_markdown_stable_key(&update.message_id),
                        committed: false,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )
                .await?;
        } else if !update.committed_nodes.is_empty() || update.reset {
            self.database
                .remove_pending_markdown_block(session_id, &update.message_id)
                .await?;
        }
        Ok(())
    }

    pub fn get_session_list_snapshot(&self, limit: u32, offset: u32) -> RuntimeSessionListSnapshot {
        let sessions = self
            .tokio
            .block_on(self.database.session_list(limit, offset))
            .unwrap_or_default();
        let selected_session_id = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        RuntimeSessionListSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            sessions,
            selected_session_id,
        }
    }

    pub fn get_session_snapshot(&self, session_id: String) -> RuntimeSessionSnapshot {
        let session = self
            .tokio
            .block_on(self.database.session_summary(&session_id))
            .ok();
        let snapshot = self
            .tokio
            .block_on(self.database.session_snapshot(&session_id))
            .unwrap_or_default();
        RuntimeSessionSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            session,
            timeline_items: snapshot.timeline_items,
            markdown_block_payloads: snapshot.markdown_block_payloads,
        }
    }

    pub fn get_timeline_page(
        &self,
        session_id: String,
        before_cursor: u64,
        limit: u32,
    ) -> RuntimeTimelinePage {
        let page = self
            .tokio
            .block_on(
                self.database
                    .timeline_page(&session_id, before_cursor, limit),
            )
            .unwrap_or_default();
        RuntimeTimelinePage {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            session_id,
            items: page.items,
            markdown_block_payloads: page.markdown_block_payloads,
            next_before_cursor: page.next_before_cursor,
            has_more: page.has_more,
        }
    }

    pub fn get_message_snapshot(&self, message_id: String) -> RuntimeMessageSnapshot {
        RuntimeMessageSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            message: self
                .tokio
                .block_on(self.database.message_snapshot(&message_id))
                .unwrap_or_default(),
        }
    }

    pub fn search_sessions(&self, query: String, limit: u32) -> RuntimeSearchSnapshot {
        RuntimeSearchSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            sessions: self
                .tokio
                .block_on(self.database.search_sessions(&query, limit))
                .unwrap_or_default(),
            query,
        }
    }

    pub fn get_settings_snapshot(&self) -> RuntimeSettingsSnapshot {
        RuntimeSettingsSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            settings: self
                .tokio
                .block_on(self.database.settings_snapshot())
                .unwrap_or_default(),
        }
    }

    pub fn resolve_sandbox_file(
        &self,
        session_id: String,
        sandbox_path: String,
    ) -> RuntimeFileResolution {
        let resolved = match self
            .sandbox
            .resolve(&session_id, &sandbox_path, SandboxAccess::Read)
        {
            Ok(resolved) => resolved,
            Err(_) => return RuntimeFileResolution::default(),
        };
        let metadata = fs::metadata(&resolved.host_path).ok();
        let db_file = self
            .tokio
            .block_on(
                self.database
                    .resolve_file_by_sandbox_path(&session_id, &resolved.sandbox_path),
            )
            .ok();
        RuntimeFileResolution {
            sandbox_path: resolved.sandbox_path,
            host_path: resolved.host_path.to_string_lossy().to_string(),
            relative_path: resolved.relative_path,
            root: resolved.root,
            writable: resolved.writable,
            exists: metadata.is_some(),
            is_file: metadata.as_ref().is_some_and(|metadata| metadata.is_file()),
            mime_type: db_file
                .as_ref()
                .map(|file| file.mime_type.clone())
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            byte_size: metadata
                .as_ref()
                .map(|metadata| metadata.len())
                .or_else(|| db_file.as_ref().map(|file| file.byte_size))
                .unwrap_or_default(),
            file_id: db_file.map(|file| file.id).unwrap_or_default(),
        }
    }

    pub fn get_rootfs_status(&self) -> RuntimeRootfsStatus {
        let settings_snap = self
            .tokio
            .block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);

        // Dynamically probe status
        let probed = self.sandbox.probe_rootfs_status(requested_backend);

        let rootfs_installed = self.sandbox.is_rootfs_installed();

        let root_available = self.sandbox.probe_root_available();
        let chroot_available = self.sandbox.probe_chroot_available();
        let proot_available = self.sandbox.probe_proot_available();

        let version = if rootfs_installed {
            let version_file = self.sandbox.rootfs_dir().join(".hambur-rootfs.version");
            fs::read_to_string(&version_file)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let size_bytes = dir_size(self.sandbox.rootfs_dir());

        RuntimeRootfsStatus {
            rootfs_installed,
            proot_available,
            root_available,
            chroot_available,
            backend: probed.backend,
            version,
            rootfs_size_bytes: size_bytes,
            rootfs_path: self.sandbox.rootfs_dir().to_string_lossy().to_string(),
        }
    }

    pub fn list_skills(&self) -> Vec<RuntimeSkillSummary> {
        self.list_skills_internal().unwrap_or_default()
    }

    pub fn get_skill_detail(&self, identifier: String, file_path: String) -> RuntimeSkillDetail {
        self.get_skill_detail_internal(&identifier, &file_path)
            .unwrap_or_default()
    }

    pub fn delete_skill(&self, identifier: String) -> RuntimeCommandAck {
        let command = RuntimeCommand {
            kind: "DeleteSkill".to_string(),
            message_id: identifier.clone(),
            idempotency_key: format!("skill:{identifier}:delete:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        };
        self.dispatch(command)
    }

    pub fn list_memory_files(&self) -> Vec<RuntimeMemoryFileSummary> {
        self.list_memory_files_internal().unwrap_or_default()
    }

    pub fn get_memory_file_detail(&self, name: String) -> RuntimeMemoryFileDetail {
        self.get_memory_file_detail_internal(&name)
            .unwrap_or_default()
    }

    pub fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Ok(active_turns) = self.active_turns.lock() {
            for active in active_turns.values() {
                active.cancel.store(true, Ordering::SeqCst);
            }
        }

        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        let _ = self.emit(RuntimeEventKind::RuntimeClosed, snapshot, None);
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn app_files_dir(&self) -> &str {
        &self.bootstrap.app_files_dir
    }

    fn snapshot_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    fn active_turn_for_session(&self, session_id: &str) -> Option<ActiveTurn> {
        self.active_turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(session_id).cloned())
    }

    fn clear_active_turn(&self, session_id: &str, turn_id: &str) {
        if let Ok(mut turns) = self.active_turns.lock()
            && turns
                .get(session_id)
                .is_some_and(|active| active.turn_id == turn_id)
        {
            turns.remove(session_id);
        }
    }

    fn emit(
        &self,
        kind: RuntimeEventKind,
        snapshot: AppSnapshot,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) && !matches!(kind, RuntimeEventKind::RuntimeClosed)
        {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let (error_code, message) = match error {
            Some(error) => (error.code().as_str().to_string(), error.to_string()),
            None => (String::new(), String::new()),
        };
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id: snapshot.selected_session_id.clone(),
            turn_id: String::new(),
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: PlatformRequest::default(),
            error_code,
            message,
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }

    fn emit_session_event(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        snapshot: AppSnapshot,
        message: String,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) && !matches!(kind, RuntimeEventKind::RuntimeClosed)
        {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let (error_code, error_message) = match error {
            Some(error) => (error.code().as_str().to_string(), error.to_string()),
            None => (String::new(), message),
        };
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id,
            turn_id,
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: PlatformRequest::default(),
            error_code,
            message: error_message,
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }

    fn emit_error(&self, error: HamburError) -> HamburResult<()> {
        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        self.emit(RuntimeEventKind::RuntimeError, snapshot, Some(&error))
    }

    fn emit_markdown(
        &self,
        session_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        self.emit_markdown_for_turn(session_id, String::new(), markdown_render_update)
    }

    fn emit_markdown_for_turn(
        &self,
        session_id: String,
        turn_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let snapshot = self
            .tokio
            .block_on(async {
                if session_id.is_empty() {
                    self.database.bootstrap_snapshot().await
                } else {
                    self.database.session_snapshot(&session_id).await
                }
            })
            .unwrap_or_default();
        self.emit_markdown_event_with_snapshot(
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
        )
    }

    async fn emit_markdown_event_async(
        &self,
        session_id: String,
        turn_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }
        self.persist_markdown_update_for_timeline(&session_id, &turn_id, &markdown_render_update)
            .await?;
        let snapshot = if session_id.is_empty() {
            self.database.bootstrap_snapshot().await?
        } else {
            self.database.session_snapshot(&session_id).await?
        };
        self.emit_markdown_event_with_snapshot(
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
        )
    }

    fn emit_markdown_event_with_snapshot(
        &self,
        session_id: String,
        turn_id: String,
        snapshot: AppSnapshot,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind: RuntimeEventKind::MarkdownRenderUpdate,
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
            platform_request: PlatformRequest::default(),
            error_code: String::new(),
            message: String::new(),
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }

    fn emit_platform_request_event(
        &self,
        request: PlatformRequest,
        snapshot: AppSnapshot,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind: RuntimeEventKind::PlatformRequest,
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: request,
            error_code: String::new(),
            message: String::new(),
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
}

fn safe_join(root: &Path, relative: &str) -> HamburResult<PathBuf> {
    if relative.contains('\0')
        || relative.contains("..")
        || relative.contains('\\')
        || Path::new(relative).is_absolute()
    {
        return Err(HamburError::InvalidCommand(
            "path must not escape root".to_string(),
        ));
    }
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)?;
            root.canonicalize()
        })
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let candidate = root.join(relative);
    let check_path = if candidate.exists() {
        candidate.canonicalize().map_err(|error| {
            HamburError::Internal(format!(
                "canonicalize path {}: {error}",
                candidate.display()
            ))
        })?
    } else {
        candidate
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .unwrap_or_else(|| root.clone())
    };
    if !check_path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(candidate)
}

fn relative_path(root: &Path, path: &Path) -> HamburResult<String> {
    let root = root
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let path = path
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize path: {error}")))?;
    if !path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn collect_named_files(root: &Path, name: &str, output: &mut Vec<PathBuf>) -> HamburResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, name, output)?;
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
    Ok(())
}

fn list_relative_files(root: &Path) -> HamburResult<Vec<String>> {
    let mut output = Vec::new();
    collect_relative_files(root, root, &mut output)?;
    output.sort();
    Ok(output)
}

fn collect_relative_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> HamburResult<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else if path.is_file() {
            output.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn parse_frontmatter(raw: &str) -> HashMap<String, Vec<String>> {
    if !raw.starts_with("---\n") {
        return HashMap::new();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return HashMap::new();
    };
    let yaml = &raw[4..4 + end];
    yaml.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains(':') {
                return None;
            }
            let key = trimmed.split_once(':')?.0.trim().to_string();
            let value = trimmed.split_once(':')?.1.trim();
            Some((key, parse_frontmatter_value(value)))
        })
        .collect()
}

fn parse_frontmatter_value(value: &str) -> Vec<String> {
    let unquoted = value.trim().trim_matches('"').trim_matches('\'');
    if unquoted.starts_with('[') && unquoted.ends_with(']') {
        return unquoted
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|item| !item.is_empty())
            .collect();
    }
    if unquoted.is_empty() {
        Vec::new()
    } else {
        vec![unquoted.to_string()]
    }
}

fn strip_frontmatter(raw: &str) -> String {
    if !raw.starts_with("---\n") {
        return raw.to_string();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return raw.to_string();
    };
    raw[4 + end + 4..].trim_start_matches('\n').to_string()
}

fn seed_bundled_skills(root: &Path) -> HamburResult<()> {
    fs::create_dir_all(root)
        .map_err(|error| HamburError::Internal(format!("create skills root: {error}")))?;
    for bundled in BUNDLED_SKILLS {
        let destination = safe_join(root, bundled.relative_path)?;
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| HamburError::Internal(format!("create skill dir: {error}")))?;
        }
        fs::write(&destination, bundled.content)
            .map_err(|error| HamburError::Internal(format!("seed skill: {error}")))?;
    }
    Ok(())
}

fn is_bundled_skill_path(path: &str) -> bool {
    BUNDLED_SKILLS
        .iter()
        .any(|bundled| bundled.relative_path == path)
}

fn linked_skill_files_json(skill_dir: &Path) -> String {
    let mut groups = serde_json::Map::new();
    for child in ["references", "templates", "scripts", "assets"] {
        let dir = skill_dir.join(child);
        if !dir.is_dir() {
            continue;
        }
        let files = list_relative_files(&dir)
            .unwrap_or_default()
            .into_iter()
            .map(|path| Value::String(format!("{child}/{path}")))
            .collect::<Vec<_>>();
        if !files.is_empty() {
            groups.insert(child.to_string(), Value::Array(files));
        }
    }
    Value::Object(groups).to_string()
}

fn system_time_to_ms(value: std::time::SystemTime) -> Option<u64> {
    value
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn read_memory_file_content(path: &Path) -> HamburResult<String> {
    let raw = fs::read_to_string(path).unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if is_managed_memory_file(name) {
        let entries = parse_memory_entries(&raw)
            .into_iter()
            .filter(|entry| !is_review_status_entry(entry))
            .collect::<Vec<_>>();
        Ok(entries.join(MEMORY_ENTRY_DELIMITER))
    } else {
        Ok(raw)
    }
}

fn memory_snapshot_from_root(root: &Path) -> HamburResult<MemorySnapshot> {
    fs::create_dir_all(root)
        .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
    let memory_entries =
        parse_memory_entries(&read_memory_file_content(&root.join(MEMORY_FILE_NAME))?);
    let user_entries = parse_memory_entries(&read_memory_file_content(&root.join(USER_FILE_NAME))?);
    Ok(MemorySnapshot {
        memory_block: render_memory_block("memory", &memory_entries, MEMORY_CHAR_LIMIT),
        user_block: render_memory_block("user", &user_entries, USER_MEMORY_CHAR_LIMIT),
    })
}

fn render_memory_block(target: &str, entries: &[String], limit: usize) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let content = entries.join(MEMORY_ENTRY_DELIMITER);
    let pct = if limit == 0 {
        0
    } else {
        ((content.len() as f32 / limit as f32) * 100.0) as u32
    }
    .min(100);
    let header = if target == "user" {
        format!(
            "USER PROFILE (who the user is) [{pct}% - {}/{} chars]",
            content.len(),
            limit
        )
    } else {
        format!(
            "MEMORY (your personal notes) [{pct}% - {}/{} chars]",
            content.len(),
            limit
        )
    };
    let separator = "=".repeat(46);
    format!("{separator}\n{header}\n{separator}\n{content}")
}

fn format_memory_system_prompt(snapshot: &MemorySnapshot) -> String {
    if snapshot.is_empty() {
        return String::new();
    }
    let mut prompt = String::new();
    prompt.push_str("You have persistent memory across chats. Use it as durable background context, not as a new user message.\n");
    prompt.push_str("Save stable preferences, corrections, environment facts, and recurring conventions with the memory tool. Do not save temporary task progress or short-lived todos.\n");
    if !snapshot.memory_block.trim().is_empty() {
        prompt.push('\n');
        prompt.push_str(&snapshot.memory_block);
        prompt.push('\n');
    }
    if !snapshot.user_block.trim().is_empty() {
        prompt.push('\n');
        prompt.push_str(&snapshot.user_block);
        prompt.push('\n');
    }
    prompt.trim().to_string()
}

fn build_memory_review_messages(
    review: &SessionReviewRecord,
    reason: &str,
    memory_snapshot: &MemorySnapshot,
) -> Vec<ModelMessage> {
    vec![
        ModelMessage {
            role: "system".to_string(),
            content: build_memory_review_system_prompt(),
            ..Default::default()
        },
        ModelMessage {
            role: "user".to_string(),
            content: build_memory_review_user_prompt(review, reason, memory_snapshot),
            ..Default::default()
        },
    ]
}

fn build_memory_review_system_prompt() -> String {
    r#"You are Hambur's background memory curator. The assistant's answer has already been shown to the user, so never answer the user's task, never ask follow-up questions, and never mention that you are reviewing memory.

Your only side effect is the memory tool. Use it to keep durable, future-useful memory accurate and compact.

Save these when they are stable and likely useful later:
- User identity, preferences, standing instructions, corrections, communication style, accessibility needs, and long-term goals.
- Stable project, app, repository, workspace, device, model, or tool conventions that will matter across chats.
- Recurring constraints the user expects the assistant to remember.

Do not save these:
- Temporary task progress, plans, one-off debugging details, transient todos, ephemeral files, branch names, commit hashes, or facts likely to expire soon.
- Secrets, API keys, tokens, passwords, private credentials, or sensitive data that the user did not explicitly ask to remember.
- Inferences about the user that are not directly supported by the transcript.
- Anything already represented well in current memory, even if the wording is not identical.

Target selection:
- Use target="user" for facts about the user as a person or their stable preferences.
- Use target="memory" for durable assistant/workspace/project/app operating notes.

Editing policy:
- Compare the transcript with the provided current memory before writing.
- Do not store duplicate memories. If a fact is already present, make no change for that fact.
- Prefer replace/remove when a current entry is stale, duplicated, or contradicted.
- Prefer add only for new atomic facts. Keep each entry short, declarative, and specific.
- It is allowed and often correct to make no modifications. If nothing durable should change, call no tools and return only: no_changes.
- Never pass "no_changes", review summaries, or JSON containing "memory_review", "changed_targets", or "action_counts" as memory tool content or old_text.
- After tool calls, return a short plain-text summary of changed targets and action counts."#
        .to_string()
}

fn build_memory_review_user_prompt(
    review: &SessionReviewRecord,
    reason: &str,
    memory_snapshot: &MemorySnapshot,
) -> String {
    let memory_block = if memory_snapshot.memory_block.trim().is_empty() {
        "MEMORY (your personal notes): empty".to_string()
    } else {
        memory_snapshot.memory_block.clone()
    };
    let user_block = if memory_snapshot.user_block.trim().is_empty() {
        "USER PROFILE (who the user is): empty".to_string()
    } else {
        memory_snapshot.user_block.clone()
    };
    format!(
        "Review trigger: {reason}.\nSession id: {}\n\nCurrent persistent memory snapshot:\n{memory_block}\n\n{user_block}\n\nRecent conversation transcript and tool traces:\n{}\n\nReview the transcript deeply but write conservatively. Use the memory tool only if the update is durable, clearly supported, and not already present in current memory. Making no changes is acceptable.",
        review.id,
        build_memory_review_transcript(review)
    )
}

fn build_memory_review_transcript(review: &SessionReviewRecord) -> String {
    let mut output = String::new();
    let start = review
        .messages
        .len()
        .saturating_sub(MAX_MEMORY_REVIEW_TRANSCRIPT_MESSAGES);
    for (index, message) in review.messages.iter().skip(start).enumerate() {
        output.push_str(&format!(
            "[{index}] role={} id={} turn={}\n",
            message.role, message.id, message.turn_id
        ));
        if !message.provider_name_snapshot.trim().is_empty()
            || !message.provider_id_snapshot.trim().is_empty()
            || !message.model_id_snapshot.trim().is_empty()
        {
            output.push_str(&format!(
                "model={}/{}\n",
                message
                    .clone()
                    .provider_name_snapshot
                    .if_blank(message.provider_id_snapshot.clone()),
                message
                    .clone()
                    .model_id_snapshot
                    .if_blank(message.model_name_snapshot.clone())
            ));
        }
        if !message.attachments.is_empty() {
            let attachments = message
                .attachments
                .iter()
                .map(|attachment| {
                    format!("{}:{}", attachment.kind, attachment.display_name)
                        .chars()
                        .take(MAX_MEMORY_REVIEW_ATTACHMENT_CHARS)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str("attachments=");
            output.push_str(&attachments);
            output.push('\n');
        }
        if !message.tool_name.trim().is_empty() {
            output.push_str(&format!(
                "tool_result={} title={}\n",
                message.tool_name,
                message.tool_title.clone().if_blank("(none)".to_string())
            ));
        }
        let content = message.content_text.trim();
        output.push_str("content:\n");
        if content.is_empty() {
            output.push_str("(empty)\n");
        } else {
            output.push_str(
                &content
                    .chars()
                    .take(MAX_MEMORY_REVIEW_MESSAGE_CHARS)
                    .collect::<String>(),
            );
            output.push('\n');
            if content.chars().count() > MAX_MEMORY_REVIEW_MESSAGE_CHARS {
                output.push_str("[message truncated]\n");
            }
        }
        output.push('\n');
    }

    let trace_start = review
        .trace_spans
        .len()
        .saturating_sub(MAX_MEMORY_REVIEW_TRACE_EVENTS);
    let traces = review
        .trace_spans
        .iter()
        .skip(trace_start)
        .collect::<Vec<_>>();
    if !traces.is_empty() {
        output.push_str("Latest turn trace events:\n");
        for (index, event) in traces.iter().enumerate() {
            output.push_str(&format!(
                "[{index}] kind={} status={} title={} tool={}\n",
                event.kind,
                event.status,
                event.title,
                if event.tool_call_id.trim().is_empty() {
                    "(none)"
                } else {
                    event.tool_call_id.as_str()
                }
            ));
            if !event.content.trim().is_empty() {
                output.push_str(
                    &event
                        .content
                        .trim()
                        .chars()
                        .take(MAX_MEMORY_REVIEW_TRACE_CHARS)
                        .collect::<String>(),
                );
                output.push('\n');
            }
        }
    }

    let transcript = output.trim().to_string();
    if transcript.chars().count() <= MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS {
        return transcript;
    }
    let tail = transcript
        .chars()
        .rev()
        .take(MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("[older transcript omitted]\n{tail}")
}

fn parse_memory_entries(raw: &str) -> Vec<String> {
    raw.split(MEMORY_ENTRY_DELIMITER)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn count_memory_entries(raw: &str) -> u32 {
    u32::try_from(parse_memory_entries(raw).len()).unwrap_or(u32::MAX)
}

fn memory_preview(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "§")
        .unwrap_or_default()
        .chars()
        .take(240)
        .collect()
}

fn memory_file_sort_priority(name: &str) -> u8 {
    if name.eq_ignore_ascii_case(MEMORY_FILE_NAME) {
        0
    } else if name.eq_ignore_ascii_case(USER_FILE_NAME) {
        1
    } else {
        2
    }
}

fn is_managed_memory_file(name: &str) -> bool {
    name.eq_ignore_ascii_case(MEMORY_FILE_NAME) || name.eq_ignore_ascii_case(USER_FILE_NAME)
}

fn is_review_status_entry(content: &str) -> bool {
    let compact = content
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return false;
    }
    if compact == "no_changes" || compact == "\"no_changes\"" {
        return true;
    }
    compact.starts_with('{')
        && (compact.contains("\"memory_review\"")
            || compact.contains("\"changed_targets\"")
            || compact.contains("\"action_counts\""))
}

fn normalize_tool_sandbox_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/var/hambur/workspace".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/var/hambur/workspace/{trimmed}")
    }
}

fn collect_all_files(root: &Path, output: &mut Vec<PathBuf>) -> HamburResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_all_files(&path, output)?;
        } else if path.is_file() {
            output.push(path);
        }
    }
    Ok(())
}

fn path_for_search_result(resolved: &hambur_sandbox::SandboxPathResolution, file: &Path) -> String {
    if resolved.host_path.is_file() {
        return resolved.sandbox_path.clone();
    }
    let relative = file
        .strip_prefix(&resolved.host_path)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    if relative.is_empty() {
        resolved.sandbox_path.clone()
    } else {
        format!(
            "{}/{}",
            resolved.sandbox_path.trim_end_matches('/'),
            relative.trim_start_matches('/')
        )
    }
}

fn write_memory_entries(path: &Path, entries: &[String]) -> HamburResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| HamburError::Internal(format!("create memory parent: {error}")))?;
    }
    fs::write(path, entries.join(MEMORY_ENTRY_DELIMITER)).map_err(|error| {
        HamburError::Internal(format!("write memory file {}: {error}", path.display()))
    })
}

fn memory_response(
    success: bool,
    target: &str,
    entries: &[String],
    message: &str,
    error: &str,
) -> Value {
    let limit = if target == "user" { 1375 } else { 2200 };
    let current = entries.join(MEMORY_ENTRY_DELIMITER).len();
    let pct = if limit == 0 {
        0
    } else {
        ((current as f32 / limit as f32) * 100.0).round() as u32
    }
    .min(100);
    json!({
        "success": success,
        "target": target,
        "entries": entries,
        "usage": format!("{pct}% - {current}/{limit} chars"),
        "entry_count": entries.len(),
        "message": message,
        "error": error,
        "summary": if success { message } else { error }
    })
}

fn skill_summary_json(skill: RuntimeSkillSummary) -> Value {
    json!({
        "name": skill.name,
        "description": skill.description,
        "category": skill.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", skill.path),
        "tags": skill.tags
    })
}

fn skill_detail_json(detail: RuntimeSkillDetail, selected_file: bool) -> Value {
    if selected_file {
        return json!({
            "success": true,
            "name": detail.summary.name,
            "file_path": detail.selected_file_path,
            "path": format!("{SANDBOX_SKILLS_PATH}/{}/{}", detail.skill_dir_path, detail.selected_file_path),
            "content": detail.selected_file_content,
            "summary": "skill file loaded"
        });
    }
    let linked_files =
        serde_json::from_str::<Value>(&detail.linked_files_json).unwrap_or_else(|_| json!({}));
    json!({
        "success": true,
        "name": detail.summary.name,
        "description": detail.summary.description,
        "category": detail.summary.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", detail.summary.path),
        "skill_dir": format!("{SANDBOX_SKILLS_PATH}/{}", detail.skill_dir_path),
        "linked_files": linked_files,
        "content": detail.content,
        "hint": "To view linked files, call skill_view(name, file_path) where file_path is e.g. references/api.md, templates/config.yaml, or scripts/setup.sh."
    })
}

fn normalize_knowledge_tool_name(name: &str) -> &str {
    match name {
        "skill_list" => "skills_list",
        other => other,
    }
}

fn disabled_skill_paths_from_snapshot(snapshot: SettingsSnapshot) -> HashSet<String> {
    snapshot
        .settings
        .into_iter()
        .filter_map(|setting| {
            setting
                .key
                .strip_prefix("skill_enabled:")
                .filter(|_| setting.value == "false")
                .map(ToString::to_string)
        })
        .collect()
}

fn format_skills_index_prompt(skills: Vec<RuntimeSkillSummary>) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut prompt = String::new();
    prompt.push_str("Hambur has a local Skills system at /var/hambur/skills. This directory is shared by all chat sessions and visible inside the Linux sandbox.\n");
    prompt.push_str("Skills are reusable task instructions, references, scripts, and templates. They are loaded through progressive disclosure: use `skills_list` for compact metadata, then `skill_view` to inspect a skill before following its detailed workflow.\n");
    prompt.push_str("Available skills:\n");
    for skill in skills {
        prompt.push_str("- ");
        prompt.push_str(&skill.name);
        if !skill.category.is_empty() {
            prompt.push_str(" [");
            prompt.push_str(&skill.category);
            prompt.push(']');
        }
        prompt.push_str(": ");
        prompt.push_str(
            &skill
                .description
                .chars()
                .take(SKILL_MAX_DESCRIPTION_CHARS)
                .collect::<String>(),
        );
        prompt.push('\n');
    }
    prompt.trim().to_string()
}

fn database_path(bootstrap: &AppBootstrap) -> PathBuf {
    PathBuf::from(&bootstrap.app_files_dir).join("hambur.db")
}

const SANDBOX_SKILLS_PATH: &str = "/var/hambur/skills";
const SKILL_MAX_LINKED_FILE_BYTES: u64 = 512_000;
const SKILL_MAX_DESCRIPTION_CHARS: usize = 320;
const MEMORY_FILE_NAME: &str = "MEMORY.md";
const USER_FILE_NAME: &str = "USER.md";
const MEMORY_ENTRY_DELIMITER: &str = "\n§\n";
const MEMORY_CHAR_LIMIT: usize = 2200;
const USER_MEMORY_CHAR_LIMIT: usize = 1375;
const MAX_MEMORY_REVIEW_TOOL_ITERATIONS: u32 = 8;
const MAX_MEMORY_REVIEW_TRANSCRIPT_MESSAGES: usize = 40;
const MAX_MEMORY_REVIEW_TRACE_EVENTS: usize = 24;
const MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS: usize = 24_000;
const MAX_MEMORY_REVIEW_MESSAGE_CHARS: usize = 3_000;
const MAX_MEMORY_REVIEW_TRACE_CHARS: usize = 1_000;
const MAX_MEMORY_REVIEW_ATTACHMENT_CHARS: usize = 200;
const BUNDLED_SKILLS: &[BundledSkillFile] = &[BundledSkillFile {
    relative_path: "system/skill-creator/SKILL.md",
    content: include_str!("../assets/skills/system/skill-creator/SKILL.md"),
}];

struct BundledSkillFile {
    relative_path: &'static str,
    content: &'static str,
}

impl RuntimeEngine {
    fn skills_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("skills")
    }

    fn memory_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("memory")
    }

    fn ensure_seeded_skills(&self) -> HamburResult<()> {
        seed_bundled_skills(&self.skills_root())
    }

    fn list_skills_internal(&self) -> HamburResult<Vec<RuntimeSkillSummary>> {
        let disabled = self.disabled_skill_paths();
        self.list_skills_with_disabled(&disabled)
    }

    fn list_skills_with_disabled(
        &self,
        disabled: &HashSet<String>,
    ) -> HamburResult<Vec<RuntimeSkillSummary>> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let mut skill_files = Vec::new();
        collect_named_files(&root, "SKILL.md", &mut skill_files)?;
        let mut skills = skill_files
            .into_iter()
            .filter_map(|path| self.load_skill_from_file(&root, &path, disabled).ok())
            .map(|detail| detail.summary)
            .collect::<Vec<_>>();
        skills.sort_by(|a, b| {
            a.category
                .cmp(&b.category)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(skills)
    }

    fn get_skill_detail_internal(
        &self,
        identifier: &str,
        selected_file_path: &str,
    ) -> HamburResult<RuntimeSkillDetail> {
        let disabled = self.disabled_skill_paths();
        self.get_skill_detail_with_disabled(identifier, selected_file_path, &disabled)
    }

    fn get_skill_detail_with_disabled(
        &self,
        identifier: &str,
        selected_file_path: &str,
        disabled: &HashSet<String>,
    ) -> HamburResult<RuntimeSkillDetail> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let skill_file = self.resolve_skill_file(&root, identifier, disabled)?;
        let mut detail = self.load_skill_from_file(&root, &skill_file, disabled)?;
        let file_path = selected_file_path.trim().trim_start_matches('/');
        if !file_path.is_empty() {
            let skill_dir = root.join(&detail.skill_dir_path);
            let selected = safe_join(&skill_dir, file_path)?;
            if !selected.is_file() {
                return Err(HamburError::InvalidCommand(format!(
                    "skill file not found: {file_path}"
                )));
            }
            let metadata = fs::metadata(&selected).map_err(|error| {
                HamburError::Internal(format!("read skill file metadata: {error}"))
            })?;
            if metadata.len() > SKILL_MAX_LINKED_FILE_BYTES {
                return Err(HamburError::InvalidCommand(
                    "skill file is too large to load".to_string(),
                ));
            }
            detail.selected_file_path = file_path.to_string();
            detail.selected_file_content = fs::read_to_string(&selected).map_err(|error| {
                HamburError::Internal(format!("read skill file {}: {error}", selected.display()))
            })?;
        }
        Ok(detail)
    }

    fn delete_skill_internal(&self, identifier: &str) -> HamburResult<String> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let disabled = self.disabled_skill_paths();
        let skill_file = self.resolve_skill_file(&root, identifier, &disabled)?;
        let skill_dir = skill_file.parent().ok_or_else(|| {
            HamburError::InvalidCommand(format!("skill directory not found: {identifier}"))
        })?;
        if !skill_dir.join("SKILL.md").is_file() {
            return Err(HamburError::InvalidCommand(format!(
                "skill not found: {identifier}"
            )));
        }
        let relative = relative_path(&root, &skill_file)?.replace('\\', "/");
        fs::remove_dir_all(skill_dir)
            .map_err(|error| HamburError::Internal(format!("delete skill directory: {error}")))?;
        Ok(relative)
    }

    fn resolve_skill_file(
        &self,
        root: &PathBuf,
        identifier: &str,
        disabled: &HashSet<String>,
    ) -> HamburResult<PathBuf> {
        let raw = identifier.trim();
        let normalized = raw
            .strip_prefix(SANDBOX_SKILLS_PATH)
            .unwrap_or(raw)
            .trim_start_matches('/')
            .strip_prefix("skills/")
            .unwrap_or_else(|| {
                raw.strip_prefix(SANDBOX_SKILLS_PATH)
                    .unwrap_or(raw)
                    .trim_start_matches('/')
            });
        if normalized.is_empty()
            || normalized.contains("..")
            || normalized.contains('\\')
            || normalized.starts_with('/')
        {
            return Err(HamburError::InvalidCommand(
                "invalid skill identifier".to_string(),
            ));
        }
        let candidates = [
            normalized.to_string(),
            format!("{normalized}/SKILL.md"),
            format!("{normalized}.md"),
        ];
        for candidate in candidates {
            let path = safe_join(root, &candidate)?;
            if path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            {
                return Ok(path);
            }
        }
        let lowered = normalized.to_ascii_lowercase();
        for skill in self.list_skills_with_disabled(disabled)? {
            let path_without_file = skill.path.trim_end_matches("/SKILL.md");
            if skill.name.eq_ignore_ascii_case(&lowered)
                || skill.name.eq_ignore_ascii_case(normalized)
                || path_without_file.eq_ignore_ascii_case(normalized)
                || path_without_file
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(normalized))
            {
                return safe_join(root, &skill.path);
            }
        }
        Err(HamburError::InvalidCommand(format!(
            "skill not found: {identifier}"
        )))
    }

    fn load_skill_from_file(
        &self,
        root: &PathBuf,
        skill_file: &PathBuf,
        disabled: &HashSet<String>,
    ) -> HamburResult<RuntimeSkillDetail> {
        let raw = fs::read_to_string(skill_file).map_err(|error| {
            HamburError::Internal(format!("read skill {}: {error}", skill_file.display()))
        })?;
        let path = relative_path(root, skill_file)?.replace('\\', "/");
        let skill_dir_path = path.trim_end_matches("/SKILL.md").to_string();
        let skill_dir = root.join(&skill_dir_path);
        let frontmatter = parse_frontmatter(&raw);
        let body = strip_frontmatter(&raw);
        let name = frontmatter
            .get("name")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_else(|| {
                skill_dir_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("skill")
                    .to_string()
            });
        let description = frontmatter
            .get("description")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default();
        let tags = frontmatter.get("tags").cloned().unwrap_or_default();
        let category = skill_dir_path
            .rsplit_once('/')
            .map(|(category, _)| category.to_string())
            .unwrap_or_default();
        let files = list_relative_files(&skill_dir)?;
        let modified_at_ms = files
            .iter()
            .filter_map(|file| fs::metadata(skill_dir.join(file)).ok())
            .filter_map(|metadata| metadata.modified().ok())
            .filter_map(system_time_to_ms)
            .max()
            .unwrap_or_default();
        let created_at_ms = fs::metadata(&skill_dir)
            .ok()
            .and_then(|metadata| metadata.created().ok())
            .and_then(system_time_to_ms)
            .unwrap_or(modified_at_ms);
        let linked_files_json = linked_skill_files_json(&skill_dir);
        Ok(RuntimeSkillDetail {
            summary: RuntimeSkillSummary {
                name,
                description: description
                    .chars()
                    .take(SKILL_MAX_DESCRIPTION_CHARS)
                    .collect(),
                path: path.clone(),
                category,
                tags,
                built_in: is_bundled_skill_path(&path),
                enabled: !disabled.contains(&path),
                created_at_ms,
                modified_at_ms,
                files,
            },
            content: body,
            raw_content: raw,
            skill_dir_path,
            linked_files_json,
            selected_file_path: String::new(),
            selected_file_content: String::new(),
        })
    }

    fn disabled_skill_paths(&self) -> HashSet<String> {
        self.tokio
            .block_on(self.database.settings_snapshot())
            .map(disabled_skill_paths_from_snapshot)
            .unwrap_or_default()
    }

    async fn disabled_skill_paths_async(&self) -> HashSet<String> {
        self.database
            .settings_snapshot()
            .await
            .map(disabled_skill_paths_from_snapshot)
            .unwrap_or_default()
    }

    fn build_skills_index_prompt(&self) -> String {
        let skills = self
            .list_skills_internal()
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();
        format_skills_index_prompt(skills)
    }

    async fn build_skills_index_prompt_async(&self) -> String {
        let disabled = self.disabled_skill_paths_async().await;
        let skills = self
            .list_skills_with_disabled(&disabled)
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();
        format_skills_index_prompt(skills)
    }

    fn build_memory_system_prompt(&self) -> String {
        self.memory_snapshot()
            .map(|snapshot| format_memory_system_prompt(&snapshot))
            .unwrap_or_default()
    }

    async fn build_memory_system_prompt_async(&self) -> String {
        self.memory_snapshot_async()
            .await
            .map(|snapshot| format_memory_system_prompt(&snapshot))
            .unwrap_or_default()
    }

    fn memory_snapshot(&self) -> HamburResult<MemorySnapshot> {
        let root = self.memory_root();
        memory_snapshot_from_root(&root)
    }

    async fn memory_snapshot_async(&self) -> HamburResult<MemorySnapshot> {
        let root = self.memory_root();
        tokio::task::spawn_blocking(move || memory_snapshot_from_root(&root))
            .await
            .map_err(|error| HamburError::Internal(format!("memory snapshot task: {error}")))?
    }

    fn list_memory_files_internal(&self) -> HamburResult<Vec<RuntimeMemoryFileSummary>> {
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let mut files = Vec::new();
        for entry in fs::read_dir(&root)
            .map_err(|error| HamburError::Internal(format!("read memory root: {error}")))?
        {
            let entry = entry
                .map_err(|error| HamburError::Internal(format!("read memory entry: {error}")))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".md") {
                continue;
            }
            let raw = read_memory_file_content(&path)?;
            let metadata = fs::metadata(&path)
                .map_err(|error| HamburError::Internal(format!("memory metadata: {error}")))?;
            files.push(RuntimeMemoryFileSummary {
                name: name.to_string(),
                size_bytes: metadata.len(),
                modified_at_ms: metadata
                    .modified()
                    .ok()
                    .and_then(system_time_to_ms)
                    .unwrap_or_default(),
                entry_count: count_memory_entries(&raw),
                preview: memory_preview(&raw),
            });
        }
        files.sort_by(|a, b| {
            memory_file_sort_priority(&a.name)
                .cmp(&memory_file_sort_priority(&b.name))
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
        });
        Ok(files)
    }

    fn get_memory_file_detail_internal(&self, name: &str) -> HamburResult<RuntimeMemoryFileDetail> {
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let clean = name.trim();
        if clean.is_empty()
            || clean.contains('/')
            || clean.contains('\\')
            || !clean.to_ascii_lowercase().ends_with(".md")
        {
            return Err(HamburError::InvalidCommand(format!(
                "invalid memory file name: {name}"
            )));
        }
        let path = safe_join(&root, clean)?;
        if !path.is_file() {
            return Err(HamburError::InvalidCommand(format!(
                "memory file not found: {clean}"
            )));
        }
        let raw = read_memory_file_content(&path)?;
        let metadata = fs::metadata(&path)
            .map_err(|error| HamburError::Internal(format!("memory metadata: {error}")))?;
        Ok(RuntimeMemoryFileDetail {
            name: clean.to_string(),
            size_bytes: metadata.len(),
            modified_at_ms: metadata
                .modified()
                .ok()
                .and_then(system_time_to_ms)
                .unwrap_or_default(),
            entry_count: count_memory_entries(&raw),
            content: raw,
        })
    }
    fn resolve_tool_sandbox_path(
        &self,
        session_id: &str,
        raw_path: &str,
        access: SandboxAccess,
    ) -> HamburResult<hambur_sandbox::SandboxPathResolution> {
        let path = normalize_tool_sandbox_path(raw_path);
        self.sandbox.resolve(session_id, &path, access)
    }

    fn read_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let offset = arguments
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(500)
            .clamp(1, 2000);
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Read)?;
        let content = fs::read_to_string(&resolved.host_path).map_err(|error| {
            HamburError::InvalidCommand(format!("read file {}: {error}", resolved.sandbox_path))
        })?;
        let lines = content.lines().collect::<Vec<_>>();
        let start = usize::try_from(offset.saturating_sub(1)).unwrap_or(usize::MAX);
        let limit = usize::try_from(limit).unwrap_or(2000);
        let rendered = lines
            .iter()
            .enumerate()
            .skip(start)
            .take(limit)
            .map(|(index, line)| format!("{}|{}", index + 1, line))
            .collect::<Vec<_>>();
        Ok(json!({
            "path": resolved.sandbox_path,
            "offset": offset,
            "limit": limit,
            "totalLines": lines.len(),
            "content": rendered.join("\n"),
            "summary": format!("read {} lines", rendered.len())
        }))
    }

    fn write_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Write)?;
        if let Some(parent) = resolved.host_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| HamburError::Internal(format!("create parent: {error}")))?;
        }
        fs::write(&resolved.host_path, content.as_bytes())
            .map_err(|error| HamburError::Internal(format!("write file: {error}")))?;
        Ok(json!({
            "path": resolved.sandbox_path,
            "bytes": content.len(),
            "summary": "file written"
        }))
    }

    fn patch_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("replace");
        if mode == "patch" {
            return Err(HamburError::InvalidCommand(
                "patch mode='patch' is not implemented; use mode='replace'".to_string(),
            ));
        }
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let old = arguments
            .get("old_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let new = arguments
            .get("new_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if old.is_empty() {
            return Err(HamburError::InvalidCommand(
                "old_string must not be empty".to_string(),
            ));
        }
        let replace_all = arguments
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Write)?;
        let content = fs::read_to_string(&resolved.host_path)
            .map_err(|error| HamburError::InvalidCommand(format!("read file: {error}")))?;
        let count = content.matches(old).count();
        if count == 0 {
            return Err(HamburError::InvalidCommand(
                "old_string was not found".to_string(),
            ));
        }
        if !replace_all && count > 1 {
            return Err(HamburError::InvalidCommand(format!(
                "old_string matched {count} times; pass replace_all=true or add context"
            )));
        }
        let updated = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        fs::write(&resolved.host_path, updated.as_bytes())
            .map_err(|error| HamburError::Internal(format!("write patched file: {error}")))?;
        Ok(json!({
            "path": resolved.sandbox_path,
            "replacements": if replace_all { count } else { 1 },
            "summary": "file patched"
        }))
    }

    fn search_sandbox_files(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let pattern = arguments
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if pattern.is_empty() {
            return Err(HamburError::InvalidCommand(
                "pattern must not be empty".to_string(),
            ));
        }
        let target = arguments
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("content");
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("/var/hambur/workspace");
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Read)?;
        let mut files = Vec::new();
        if resolved.host_path.is_file() {
            files.push(resolved.host_path.clone());
        } else {
            collect_all_files(&resolved.host_path, &mut files)?;
        }
        let mut results = Vec::new();
        if target == "files" {
            for file in files {
                let relative = file
                    .strip_prefix(&resolved.host_path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .replace('\\', "/");
                if file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .contains(pattern)
                    || relative.contains(pattern)
                {
                    results.push(json!({"path": path_for_search_result(&resolved, &file)}));
                }
            }
        } else {
            for file in files {
                let Ok(content) = fs::read_to_string(&file) else {
                    continue;
                };
                for (index, line) in content.lines().enumerate() {
                    if line.contains(pattern) {
                        results.push(json!({
                            "path": path_for_search_result(&resolved, &file),
                            "line": index + 1,
                            "content": line
                        }));
                    }
                }
            }
        }
        let total = results.len();
        let page = results
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(json!({
            "pattern": pattern,
            "target": target,
            "results": page,
            "total": total,
            "summary": format!("{} matches", total)
        }))
    }

    fn memory_tool_result(&self, arguments: &Value) -> HamburResult<Value> {
        let target = arguments
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("memory");
        let target = if target.eq_ignore_ascii_case("user") {
            "user"
        } else {
            "memory"
        };
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let path = root.join(if target == "user" {
            USER_FILE_NAME
        } else {
            MEMORY_FILE_NAME
        });
        let mut entries = parse_memory_entries(&read_memory_file_content(&path)?);
        let result = match action {
            "read" => memory_response(true, target, &entries, "Entries loaded.", ""),
            "add" => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    memory_response(false, target, &entries, "", "Content cannot be empty.")
                } else if is_review_status_entry(&content) {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "Memory review status is not durable memory and must not be stored.",
                    )
                } else if entries.contains(&content) {
                    memory_response(
                        true,
                        target,
                        &entries,
                        "Entry already exists (no duplicate added).",
                        "",
                    )
                } else {
                    entries.push(content);
                    write_memory_entries(&path, &entries)?;
                    memory_response(true, target, &entries, "Entry added.", "")
                }
            }
            "replace" => {
                let old = arguments
                    .get("old_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if old.is_empty() || content.is_empty() {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "old_text and content are required.",
                    )
                } else {
                    let matches = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| entry.contains(old))
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    if matches.len() != 1 {
                        memory_response(
                            false,
                            target,
                            &entries,
                            "",
                            "Expected exactly one matching entry.",
                        )
                    } else {
                        entries[matches[0]] = content.to_string();
                        write_memory_entries(&path, &entries)?;
                        memory_response(true, target, &entries, "Entry replaced.", "")
                    }
                }
            }
            "remove" => {
                let old = arguments
                    .get("old_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let matches = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| entry.contains(old))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if old.is_empty() || matches.len() != 1 {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "Expected exactly one matching entry.",
                    )
                } else {
                    entries.remove(matches[0]);
                    write_memory_entries(&path, &entries)?;
                    memory_response(true, target, &entries, "Entry removed.", "")
                }
            }
            _ => memory_response(false, target, &entries, "", "Unknown memory action."),
        };
        Ok(result)
    }
}

fn normalize_command(mut command: RuntimeCommand) -> RuntimeCommand {
    if command.command_id.trim().is_empty() {
        command.command_id = new_id("cmd");
    }
    if command.created_at_ms == 0 {
        command.created_at_ms = now_ms();
    }
    command.kind = command.kind.trim().to_string();
    command.idempotency_key = command.idempotency_key.trim().to_string();
    command.session_id = command.session_id.trim().to_string();
    command.turn_id = command.turn_id.trim().to_string();
    command.message_id = command.message_id.trim().to_string();
    command.provider_id = command.provider_id.trim().to_string();
    command.model_id = command.model_id.trim().to_string();
    command.source_message_id = command.source_message_id.trim().to_string();
    command
}

fn validate_command(command: &RuntimeCommand) -> HamburResult<()> {
    if command.kind.is_empty() {
        return Err(HamburError::InvalidCommand(
            "command kind must not be empty".to_string(),
        ));
    }
    if command.idempotency_key.is_empty() {
        return Err(HamburError::InvalidCommand(
            "idempotency_key must not be empty".to_string(),
        ));
    }

    match command.kind.as_str() {
        "Initialize" | "Shutdown" | "CreateSession" => Ok(()),
        "OpenSession" | "DeleteSession" | "SoftDeleteSession" | "HardPurgeSession"
        | "SetSessionPinned" | "PinSession" | "UnpinSession" => require_session_id(command),
        "RenameSession" | "UpdateSessionTitle" => {
            require_session_id(command)?;
            if command.title.trim().is_empty()
                && command.content.trim().is_empty()
                && command.chunk.trim().is_empty()
                && config_payload_string(&command.payload_json, "title").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "session title must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateProvider" => {
            if command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider base_url must not be empty".to_string(),
                ));
            }
            if command.payload_json.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider secret_ref must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteProvider" => {
            if command.provider_id.is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RefreshProviderModels" => {
            if command.provider_id.is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateModelOverride" | "UpdateModelDetail" => {
            if command.provider_id.is_empty()
                && command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "providerId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            if command.model_id.is_empty()
                && config_payload_string(&command.payload_json, "modelId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "model_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateModelGroup" => Ok(()),
        "UpdateModelGroupMember" => Ok(()),
        "SetDefaultModelGroup" => Ok(()),
        "UpdateDefaultModelGroups" => {
            if command.payload_json.trim().is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "default model group payload must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteModelGroup" => {
            if command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "groupId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "group_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteModelGroupMember" => {
            if command.message_id.is_empty()
                || command.provider_id.is_empty()
                || command.model_id.is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "group_id (message_id), provider_id, and model_id must not be empty"
                        .to_string(),
                ));
            }
            Ok(())
        }
        "UpdateToolSettings"
        | "UpdateSkills"
        | "UpdateMemoryProjections"
        | "UpdateStartupTasks"
        | "UpdateRootfsSettings"
        | "UpdateAppearance"
        | "UpdateLogs"
        | "UpdateTokenUsage"
        | "UpdatePersona"
        | "UpdateEnvironmentVariables"
        | "UpdateAppSetting"
        | "UpdateBrowserToolSettings"
        | "UpdateSkillEnabled"
        | "UpdateStartupTask"
        | "DeleteStartupTask"
        | "UpdateRootfsSetting" => {
            if command.payload_json.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "setting payload_json must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteSkill" => {
            if command.message_id.trim().is_empty()
                && config_payload_string(&command.payload_json, "skillId").is_empty()
                && config_payload_string(&command.payload_json, "skillPath").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "skill id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "ImportAttachmentFromUri" => {
            require_session_id(command)?;
            Ok(())
        }
        "RemovePendingAttachment" => {
            require_session_id(command)?;
            if command.message_id.is_empty() && command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "attachment_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "ClearPendingAttachments" => {
            require_session_id(command)?;
            Ok(())
        }
        "SendMessage" | "EditMessage" => {
            require_session_id(command)?;
            if command.content.trim().is_empty() && command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "message content must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RetryTurn" | "RegenerateMessage" => {
            require_session_id(command)?;
            if command.content.trim().is_empty()
                && command.chunk.trim().is_empty()
                && command.source_message_id.is_empty()
                && command.message_id.is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "source_message_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "CancelTurn" => {
            if command.session_id.is_empty() && command.turn_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "CancelTurn requires session_id or turn_id".to_string(),
                ));
            }
            Ok(())
        }
        "SubmitPlatformResult" => {
            if command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "requestId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "request_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RunRootfsWarmup" => Ok(()),
        "ResetRootfs" => require_approval(command, "rootfs_reset"),
        "AppendMarkdownDelta" | "MarkdownRenderUpdate" => {
            require_session_id(command)?;
            if command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "message_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        _ => Err(HamburError::InvalidCommand(format!(
            "unsupported command kind: {}",
            command.kind
        ))),
    }
}

fn require_session_id(command: &RuntimeCommand) -> HamburResult<()> {
    if command.session_id.is_empty() {
        Err(HamburError::InvalidCommand(
            "session_id must not be empty".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn route_plan_from_records(records: Vec<ModelRouteSnapshot>) -> RoutePlan {
    let first = records.first().cloned().unwrap_or_default();
    RoutePlan {
        group_id: first.model_group_id,
        routing_strategy: RoutingStrategy::parse(&first.routing_strategy),
        fallback_policy: FallbackPolicy::parse(&first.fallback_policy),
        targets: records
            .into_iter()
            .map(provider_target_from_route)
            .collect(),
    }
}

fn provider_target_from_route(route: ModelRouteSnapshot) -> ProviderTarget {
    ProviderTarget {
        provider: ProviderConfig {
            id: route.provider_id.clone(),
            name: route.provider_name.clone(),
            protocol: route.provider_protocol.clone(),
            base_url: route.base_url.clone(),
            secret_ref: route.secret_ref.clone(),
            enabled: true,
        },
        model: ProviderModel {
            provider_id: route.provider_id.clone(),
            model_id: route.model_id.clone(),
            display_name: route.model_display_name.clone(),
            capabilities: ModelCapabilities {
                supports_tool_call: route.supports_tool_call,
                supports_reasoning: route.supports_reasoning,
                supports_image_input: route.supports_image_input,
                supports_structured_output: route.supports_structured_output,
                supports_temperature: route.supports_temperature,
                context_limit: route.context_limit,
                output_limit: route.output_limit,
                reasoning_field: route.reasoning_field.clone(),
            },
            metadata_json: "{}".to_string(),
        },
        model_group_id: route.model_group_id,
        model_group_name: route.model_group_name,
        position: route.position,
    }
}

fn route_snapshot_from_target(target: &ProviderTarget) -> ModelRouteSnapshot {
    ModelRouteSnapshot {
        provider_id: target.provider.id.clone(),
        provider_name: target.provider.name.clone(),
        provider_protocol: target.provider.protocol.clone(),
        base_url: target.provider.base_url.clone(),
        secret_ref: target.provider.secret_ref.clone(),
        model_id: target.model.model_id.clone(),
        model_display_name: target.model.display_name.clone(),
        model_group_id: target.model_group_id.clone(),
        model_group_name: target.model_group_name.clone(),
        routing_strategy: RoutingStrategy::Fallback.as_str().to_string(),
        fallback_policy: FallbackPolicy::Default.as_str().to_string(),
        position: target.position,
        supports_tool_call: target.model.capabilities.supports_tool_call,
        supports_reasoning: target.model.capabilities.supports_reasoning,
        supports_image_input: target.model.capabilities.supports_image_input,
        supports_structured_output: target.model.capabilities.supports_structured_output,
        supports_temperature: target.model.capabilities.supports_temperature,
        context_limit: target.model.capabilities.context_limit,
        output_limit: target.model.capabilities.output_limit,
        reasoning_field: target.model.capabilities.reasoning_field.clone(),
    }
}

fn stream_source_for_command(
    command: &RuntimeCommand,
    content: &str,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let provider_source = provider_stream_source(
        &command.session_id,
        &command.turn_id,
        content,
        route,
        tools_json,
        skills_index_prompt,
        memory_system_prompt,
        deep_thinking_enabled,
        search_enabled,
    );
    let request = match &provider_source {
        RouteStreamSource::Provider(request) => request.clone(),
        RouteStreamSource::Scripted { request, .. } => request.clone(),
    };
    let payload = command.payload_json.trim();
    if payload.starts_with("data:") {
        return scripted_stream_source(request, payload.to_string(), Vec::new());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Some(route_value) = scripted_route_value(&value, route) {
            let continuation_sse = scripted_continuation_sse(route_value);
            if let Some(sse) = route_value.get("sse").and_then(serde_json::Value::as_str) {
                return scripted_stream_source(request, sse.to_string(), continuation_sse);
            }
            let response = route_value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = route_value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
        let continuation_sse = scripted_continuation_sse(&value);
        if let Some(sse) = value.get("sse").and_then(serde_json::Value::as_str) {
            return scripted_stream_source(request, sse.to_string(), continuation_sse);
        }
        if value.get("content").is_some() || value.get("reasoning").is_some() {
            let response = value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
    }

    provider_source
}

fn scripted_stream_source(
    request: ModelRequest,
    sse: String,
    continuation_sse: Vec<String>,
) -> RouteStreamSource {
    RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&sse),
        continuation_sse,
    }
}

fn scripted_continuation_sse(value: &Value) -> Vec<String> {
    if let Some(items) = value
        .get("sse_sequence")
        .or_else(|| value.get("continuation_sse"))
        .and_then(Value::as_array)
    {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    value
        .get("continuationSse")
        .and_then(Value::as_str)
        .map(|sse| vec![sse.to_string()])
        .unwrap_or_default()
}

fn provider_stream_source(
    session_id: &str,
    turn_id: &str,
    content: &str,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let mut system_blocks = vec!["You are Hambur, a concise assistant.".to_string()];
    if !skills_index_prompt.trim().is_empty() {
        system_blocks.push(skills_index_prompt.to_string());
    }
    if !memory_system_prompt.trim().is_empty() {
        system_blocks.push(memory_system_prompt.to_string());
    }
    if search_enabled {
        system_blocks.push(
            "Web/search assistance is enabled for this turn. Use available search or fetch tools when current external information is needed."
                .to_string(),
        );
    }
    RouteStreamSource::Provider(ModelRequest {
        request_id: new_id("llm_req"),
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        purpose: "chat".to_string(),
        stream: true,
        system_blocks,
        messages: vec![ModelMessage {
            role: "user".to_string(),
            content: content.to_string(),
            ..Default::default()
        }],
        reasoning_mode: if route.supports_reasoning && deep_thinking_enabled {
            ReasoningMode::Enabled
        } else {
            ReasoningMode::Disabled
        },
        max_output_tokens: route.output_limit,
        temperature: Some(0.7),
        tools_json: if route.supports_tool_call {
            tools_json.to_string()
        } else {
            String::new()
        },
    })
}

fn tool_continuation_stream_source(
    mut request: ModelRequest,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    assistant_content: &str,
    tool_calls: Vec<CompleteToolCall>,
    tool_result_messages: Vec<ModelMessage>,
    mut continuation_sse: Vec<String>,
    scripted_source: bool,
) -> HamburResult<RouteStreamSource> {
    request.request_id = new_id("llm_req");
    request.max_output_tokens = route.output_limit;
    request.tools_json = if route.supports_tool_call {
        tools_json.to_string()
    } else {
        String::new()
    };
    request.messages.push(ModelMessage {
        role: "assistant".to_string(),
        content: assistant_content.to_string(),
        tool_calls_json: complete_tool_calls_json(&tool_calls)?,
        tool_call_id: String::new(),
    });
    request.messages.extend(tool_result_messages);

    if continuation_sse.is_empty() {
        if scripted_source {
            return Err(HamburError::InvalidCommand(
                "scripted tool loop requires a continuation SSE".to_string(),
            ));
        }
        return Ok(RouteStreamSource::Provider(request));
    }
    let next_sse = continuation_sse.remove(0);
    Ok(RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&next_sse),
        continuation_sse,
    })
}

fn complete_tool_calls_json(calls: &[CompleteToolCall]) -> HamburResult<String> {
    let values = calls
        .iter()
        .map(|call| {
            let arguments_value: Value =
                serde_json::from_str(&call.arguments_json).unwrap_or_else(|_| json!({}));
            let arguments_json = serde_json::to_string(&arguments_value).map_err(|error| {
                HamburError::Internal(format!("serialize tool call arguments: {error}"))
            })?;
            Ok(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments_json
                }
            }))
        })
        .collect::<HamburResult<Vec<_>>>()?;
    serde_json::to_string(&values)
        .map_err(|error| HamburError::Internal(format!("serialize tool calls: {error}")))
}

fn openai_non_stream_request(
    request: &ModelRequest,
    target: &ProviderTarget,
    api_key: &str,
) -> HamburResult<hambur_llm::HttpRequestSpec> {
    let mut spec = OpenAiCompatibleAdapter::build_stream_request(request, target, api_key)?;
    let mut body: Value = serde_json::from_str(&spec.body_json)
        .map_err(|error| HamburError::InvalidCommand(format!("invalid request body: {error}")))?;
    body["stream"] = json!(false);
    if request.reasoning_mode == ReasoningMode::Disabled {
        body["thinking"] = json!({"type": "disabled"});
    }
    spec.body_json = body.to_string();
    Ok(spec)
}

async fn reqwest_json(spec: hambur_llm::HttpRequestSpec) -> HamburResult<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_provider_http_status(status));
    }
    response
        .text()
        .await
        .map_err(|error| HamburError::ProviderUnavailable(format!("NetworkError: {error}")))
}

fn parse_openai_non_stream_message(body: &str) -> HamburResult<MemoryReviewAssistantMessage> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| HamburError::SseParse(format!("parse chat completion JSON: {error}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("provider error");
        return Err(HamburError::ProviderUnavailable(message.to_string()));
    }
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| HamburError::SseParse("chat completion missing message".to_string()))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(items) = message.get("tool_calls").and_then(Value::as_array) {
        for (fallback_index, item) in items.iter().enumerate() {
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(fallback_index as u32);
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let function = item.get("function").unwrap_or(&Value::Null);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments_json = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            if !id.is_empty() && !name.is_empty() {
                tool_calls.push(CompleteToolCall {
                    index,
                    id,
                    name,
                    arguments_json,
                });
            }
        }
    }
    Ok(MemoryReviewAssistantMessage {
        content,
        tool_calls,
    })
}

fn compile_named_tools_json(tools_json: &str, names: &[&str]) -> HamburResult<String> {
    let allowed = names.iter().copied().collect::<HashSet<_>>();
    let value: Value = serde_json::from_str(tools_json.trim()).map_err(|error| {
        HamburError::InvalidCommand(format!("invalid OpenAI tools JSON: {error}"))
    })?;
    let Value::Array(items) = value else {
        return Err(HamburError::InvalidCommand(
            "OpenAI tools JSON must be an array".to_string(),
        ));
    };
    let filtered = items
        .into_iter()
        .filter(|item| {
            item.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| allowed.contains(name))
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(filtered).to_string())
}

async fn reqwest_stream(spec: hambur_llm::HttpRequestSpec) -> HamburResult<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        Err(map_provider_http_status(status))
    }
}

fn map_provider_http_status(status: StatusCode) -> HamburError {
    if status == StatusCode::TOO_MANY_REQUESTS {
        HamburError::ProviderUnavailable(format!("Http429: HTTP {}", status.as_u16()))
    } else if status.is_server_error() {
        HamburError::ProviderUnavailable(format!("Http5xx: HTTP {}", status.as_u16()))
    } else {
        HamburError::ProviderUnavailable(format!(
            "Http{}: HTTP {}",
            status.as_u16(),
            status.as_u16()
        ))
    }
}

#[derive(Debug, Clone, Default)]
struct AttachmentImportPayload {
    display_name: String,
    mime_type: String,
    byte_size: u64,
    origin_type: String,
    original_uri: String,
    source_path: String,
    bytes_base64: String,
    kind: String,
    width: u32,
    height: u32,
    sha256: String,
}

impl AttachmentImportPayload {
    fn parse(payload_json: &str) -> HamburResult<Self> {
        let value = if payload_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str::<Value>(payload_json).map_err(|error| {
                HamburError::InvalidCommand(format!(
                    "attachment import payload must be JSON: {error}"
                ))
            })?
        };
        let get_string = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| value.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .to_string()
        };
        let mime_type = get_string(&["mimeType", "mime_type"]);
        let kind = get_string(&["kind"]);
        Ok(Self {
            display_name: get_string(&["displayName", "display_name", "name"]),
            mime_type: if mime_type.trim().is_empty() {
                "application/octet-stream".to_string()
            } else {
                mime_type
            },
            byte_size: value
                .get("byteSize")
                .or_else(|| value.get("byte_size"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            origin_type: get_string(&["originType", "origin_type"])
                .if_blank("content_uri".to_string()),
            original_uri: get_string(&["originalUri", "original_uri", "uri"]),
            source_path: get_string(&["sourcePath", "source_path", "path"]),
            bytes_base64: get_string(&["bytesBase64", "bytes_base64", "base64"]),
            kind,
            width: value
                .get("width")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default(),
            height: value
                .get("height")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default(),
            sha256: get_string(&["sha256"]),
        })
    }
}

#[derive(Debug, Clone, Default)]
struct SendOptions {
    attachment_ids: Vec<String>,
    deep_thinking_enabled: bool,
    search_enabled: bool,
}

impl SendOptions {
    fn parse(payload_json: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload_json) else {
            return Self::default();
        };
        let attachment_ids = value
            .get("attachmentIds")
            .or_else(|| value.get("attachment_ids"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        Self {
            attachment_ids,
            deep_thinking_enabled: value
                .get("deepThinkingEnabled")
                .or_else(|| value.get("deep_thinking_enabled"))
                .or_else(|| value.get("deepThinking"))
                .or_else(|| value.get("deep_thinking"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            search_enabled: value
                .get("searchEnabled")
                .or_else(|| value.get("search_enabled"))
                .or_else(|| value.get("search"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

fn format_user_content_with_attachments(content: &str, attachments: &[AttachmentRecord]) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }
    let mut formatted = content.trim().to_string();
    formatted.push_str("\n\nAttachments:");
    for attachment in attachments {
        if attachment.kind == "image" {
            formatted.push_str(&format!(
                "\n- ImagePart(fileId={}, sandboxPath={}, mimeType={}, detail=auto)",
                attachment.file_id, attachment.sandbox_path, attachment.mime_type
            ));
        } else {
            formatted.push_str(&format!(
                "\n- FileReferencePart(fileId={}, sandboxPath={}, name={}, size={}, mimeType={})",
                attachment.file_id,
                attachment.sandbox_path,
                attachment.display_name,
                attachment.byte_size,
                attachment.mime_type
            ));
        }
    }
    formatted
}

fn format_synthetic_view_image_message(context_stubs: &[String]) -> String {
    let image_parts = context_stubs
        .iter()
        .filter(|stub| stub.contains("ImagePart(fileId="))
        .map(|stub| stub.trim())
        .collect::<Vec<_>>();
    if image_parts.is_empty() {
        "Image returned by view_image.".to_string()
    } else {
        format!(
            "Synthetic multimodal continuation for view_image.\n{}",
            image_parts.join("\n")
        )
    }
}

#[allow(dead_code)]
fn run_terminal_command(
    invocation: &ToolInvocation,
    command: &str,
    cwd: &std::path::Path,
    timeout_ms: u64,
) -> RawToolOutput {
    let shell = platform_shell();
    let mut child = match Command::new(shell)
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: error.to_string(),
                summary: "terminal execution failed".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: "spawn_failed".to_string(),
            };
        }
    };
    let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(StdDuration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: json!({
                        "command": command,
                        "cwd": cwd.to_string_lossy(),
                        "timeoutMs": timeout_ms,
                        "timedOut": true
                    })
                    .to_string(),
                    summary: "terminal timed out".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "timeout".to_string(),
                };
            }
            Err(error) => {
                let _ = child.kill();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: error.to_string(),
                    summary: "terminal wait failed".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "wait_failed".to_string(),
                };
            }
        }
    }

    match child.wait_with_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            let content = json!({
                "command": command,
                "cwd": cwd.to_string_lossy(),
                "timeoutMs": timeout_ms,
                "exitCode": exit_code,
                "stdout": stdout,
                "stderr": stderr
            })
            .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !output.status.success(),
                summary: if output.status.success() {
                    format!(
                        "terminal exited 0 (stdout {} bytes, stderr {} bytes)",
                        output.stdout.len(),
                        output.stderr.len()
                    )
                } else {
                    format!("terminal exited {exit_code}")
                },
                content,
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: exit_code.to_string(),
            }
        }
        Err(error) => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: error.to_string(),
            summary: "terminal execution failed".to_string(),
            trust_level: "untrusted".to_string(),
            command_or_url: command.to_string(),
            status: "spawn_failed".to_string(),
        },
    }
}

#[allow(dead_code)]
fn platform_shell() -> &'static str {
    if cfg!(target_os = "android") {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    }
}

fn spawn_process_pipe_reader(
    mut reader: impl Read + Send + 'static,
    output: Arc<Mutex<ProcessOutputBuffer>>,
    stdout: bool,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if let Ok(mut output) = output.lock() {
                        if stdout {
                            output.push_stdout(&buffer[..size]);
                        } else {
                            output.push_stderr(&buffer[..size]);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn push_ring(buffer: &mut VecDeque<u8>, bytes: &[u8]) {
    const PROCESS_OUTPUT_RING_BYTES: usize = 64 * 1024;
    for byte in bytes {
        if buffer.len() >= PROCESS_OUTPUT_RING_BYTES {
            buffer.pop_front();
        }
        buffer.push_back(*byte);
    }
}

fn refresh_process_exit(state: &mut BackgroundProcessSession) {
    if state.exit_code.is_some() {
        return;
    }
    if let Ok(Some(status)) = state.child.try_wait() {
        state.exit_code = Some(status.code().unwrap_or(-1));
        state.finished_at_ms = now_ms();
    }
}

fn terminate_background_process_wrapper(state: &BackgroundProcessSession) {
    let Some(pid_file) = &state.pid_file else {
        return;
    };
    let Ok(pid) = fs::read_to_string(pid_file) else {
        return;
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return;
    };
    let _ = Command::new("su")
        .arg("-c")
        .arg(format!("kill -TERM {pid} 2>/dev/null || true"))
        .output();
}

fn process_status_json(
    state: &BackgroundProcessSession,
    output: Option<ProcessOutputSnapshot>,
) -> Value {
    let mut value = json!({
        "processSessionId": state.process_session_id,
        "backend": state.backend,
        "command": state.command,
        "cwd": state.cwd,
        "startedAt": state.started_at_ms,
        "pid": state.pid,
        "running": state.exit_code.is_none(),
        "exitCode": state.exit_code,
        "finishedAt": state.finished_at_ms
    });
    if let Some(output) = output
        && let Some(object) = value.as_object_mut()
    {
        object.insert("stdout".to_string(), json!(output.stdout));
        object.insert("stderr".to_string(), json!(output.stderr));
        object.insert(
            "stdoutTotalBytes".to_string(),
            json!(output.stdout_total_bytes),
        );
        object.insert(
            "stderrTotalBytes".to_string(),
            json!(output.stderr_total_bytes),
        );
    }
    value
}

fn markdown_block_summary(node: &hambur_markdown::MarkdownBlockNode) -> String {
    node.text
        .trim()
        .to_string()
        .if_blank(node.raw.trim().to_string())
        .chars()
        .take(160)
        .collect()
}

fn run_web_fetch(invocation: &ToolInvocation, arguments: &Value) -> RawToolOutput {
    let urls = arguments
        .get("urls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    if urls.is_empty() {
        return RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "web_fetch urls must not be empty".to_string(),
            summary: "No URLs to fetch".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: arguments.to_string(),
            status: "InvalidCommand".to_string(),
        };
    }
    if urls.len() > 5 {
        return RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "web_fetch supports at most 5 URLs per call".to_string(),
            summary: "Too many URLs".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: arguments.to_string(),
            status: "InvalidCommand".to_string(),
        };
    }
    let max_bytes = arguments
        .get("max_bytes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(1_000_000)
        .clamp(1_024, 10_000_000);
    let mut fetched = Vec::new();
    let mut errors = Vec::new();
    for url in urls {
        let fetch_url = url.clone();
        let result = thread::spawn(move || fetch_http_url(&fetch_url, max_bytes))
            .join()
            .unwrap_or_else(|_| Err("web_fetch worker panicked".to_string()));
        match result {
            Ok(value) => fetched.push(value),
            Err(error) => errors.push(json!({
                "url": url,
                "error": error
            })),
        }
    }
    let fetched_count = fetched.len();
    let error_count = errors.len();
    let content = json!({
        "fetched": fetched,
        "errors": errors
    });
    RawToolOutput {
        tool_call_id: invocation.tool_call_id.clone(),
        tool_name: invocation.name.clone(),
        is_error: error_count > 0 && fetched_count == 0,
        content: content.to_string(),
        summary: format!("Fetched {fetched_count} URLs, {error_count} failed"),
        trust_level: "untrusted".to_string(),
        command_or_url: arguments.to_string(),
        status: if error_count == 0 { "ok" } else { "partial" }.to_string(),
    }
}

fn fetch_http_url(url: &str, max_bytes: usize) -> Result<Value, String> {
    validate_web_fetch_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| format!("build web client failed: {error}"))?;
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/*, application/json;q=0.9, */*;q=0.1",
        )
        .header(reqwest::header::USER_AGENT, "Hambur/0.1")
        .send()
        .map_err(|error| format!("fetch failed: {error}"))?;
    let status = response.status().as_u16();
    let headers = response_headers_json(response.headers());
    let mut bytes = Vec::new();
    response
        .copy_to(&mut LimitedWrite::new(&mut bytes, max_bytes))
        .map_err(|error| format!("read response failed: {error}"))?;
    let truncated = bytes.len() >= max_bytes;
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(json!({
        "url": url,
        "status": status,
        "headers": headers,
        "text": body,
        "truncated": truncated
    }))
}

fn validate_web_fetch_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("web_fetch URL must start with http:// or https://".to_string());
    }
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    if parsed.host_str().unwrap_or_default().trim().is_empty() {
        return Err("web_fetch host must not be empty".to_string());
    }
    Ok(())
}

fn response_headers_json(headers: &reqwest::header::HeaderMap) -> Value {
    let values = headers
        .iter()
        .map(|(name, value)| {
            json!({
                "name": name.as_str(),
                "value": value.to_str().unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    Value::Array(values)
}

struct LimitedWrite<'a> {
    target: &'a mut Vec<u8>,
    limit: usize,
}

impl<'a> LimitedWrite<'a> {
    fn new(target: &'a mut Vec<u8>, limit: usize) -> Self {
        Self { target, limit }
    }
}

impl Write for LimitedWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.target.len() >= self.limit {
            return Ok(buf.len());
        }
        let remaining = self.limit - self.target.len();
        let take = remaining.min(buf.len());
        self.target.extend_from_slice(&buf[..take]);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn normalize_image_detail(detail: &str) -> &'static str {
    match detail {
        "low" => "low",
        "high" => "high",
        _ => "auto",
    }
}

fn vision_handoff_target(
    route_candidates: &[ModelRouteSnapshot],
    active_route: &ModelRouteSnapshot,
) -> Option<ModelRouteSnapshot> {
    route_candidates
        .iter()
        .find(|candidate| {
            candidate.supports_image_input
                && (candidate.provider_id != active_route.provider_id
                    || candidate.model_id != active_route.model_id)
        })
        .cloned()
}

fn select_view_image_handoff_route(
    active_route: &ModelRouteSnapshot,
    route_candidates: &[ModelRouteSnapshot],
    records: &[ToolExecutionRecord],
) -> Option<ModelRouteSnapshot> {
    if active_route.supports_image_input {
        return None;
    }
    let needs_handoff = records.iter().any(|record| {
        record.invocation.name == "view_image"
            && !record.result.is_error
            && serde_json::from_str::<Value>(&record.result.content_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("imageAttachedToNextRequest")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false)
    });
    if needs_handoff {
        vision_handoff_target(route_candidates, active_route)
    } else {
        None
    }
}

fn scripted_route_value<'a>(
    value: &'a serde_json::Value,
    route: &ModelRouteSnapshot,
) -> Option<&'a serde_json::Value> {
    let routes = value.get("routes")?.as_object()?;
    routes
        .get(&route.provider_id)
        .or_else(|| routes.get(&route.model_id))
        .or_else(|| routes.get(&route.position.to_string()))
}

fn split_scripted_sse(sse: &str) -> Vec<Vec<u8>> {
    sse.as_bytes()
        .chunks(13)
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn stream_error_from_provider(code: String, message: String) -> HamburError {
    match code.as_str() {
        "Http429" | "Http5xx" | "NetworkTimeout" | "NetworkError" => {
            HamburError::ProviderUnavailable(format!("{code}: {message}"))
        }
        "CapabilityMismatch" => HamburError::CapabilityMismatch(message),
        "ModelUnavailable" => HamburError::ModelUnavailable(message),
        "SseParseError" => HamburError::SseParse(message),
        _ => HamburError::Internal(format!("{code}: {message}")),
    }
}

fn fallback_error_code(error: &HamburError) -> &'static str {
    let message = error.to_string();
    if message.contains("Http429:") {
        "Http429"
    } else if message.contains("Http5xx:") {
        "Http5xx"
    } else if message.contains("NetworkTimeout:") {
        "NetworkTimeout"
    } else if message.contains("NetworkError:") {
        "NetworkError"
    } else {
        error.code().as_str()
    }
}

fn default_models_response(model_id: &str) -> String {
    let model_id = if model_id.trim().is_empty() {
        "hambur-openai-compatible-text"
    } else {
        model_id.trim()
    };
    let supports_image_input = model_id.to_ascii_lowercase().contains("vision");
    format!(
        r#"{{"data":[{{"id":"{model_id}","display_name":"{model_id}","supports_reasoning":true,"supports_tool_call":true,"supports_image_input":{supports_image_input},"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}}]}}"#
    )
}

fn config_payload_value(payload_json: &str) -> Value {
    serde_json::from_str::<Value>(payload_json)
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

fn config_payload_string(payload_json: &str, key: &str) -> String {
    let value = config_payload_value(payload_json);
    config_string(&value, key)
}

fn config_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn config_value_string(value: &Value, key: &str) -> String {
    let Some(child) = value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
    else {
        return String::new();
    };
    match child {
        Value::String(text) => text.trim().to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => child.to_string(),
    }
}

fn config_bool(value: &Value, key: &str, fallback: bool) -> bool {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_bool)
        .unwrap_or(fallback)
}

fn config_payload_bool(payload_json: &str, key: &str, fallback: bool) -> bool {
    let value = config_payload_value(payload_json);
    config_bool(&value, key, fallback)
}

fn config_u32(value: &Value, key: &str, fallback: u32) -> u32 {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback)
}

fn config_object_string(value: &Value, key: &str) -> String {
    let Some(child) = value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
    else {
        return "{}".to_string();
    };
    if let Some(text) = child.as_str() {
        if text.trim().is_empty() {
            "{}".to_string()
        } else {
            text.to_string()
        }
    } else {
        child.to_string()
    }
}

fn to_snake_key(key: &str) -> String {
    let mut output = String::new();
    for (index, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn provider_secret_ref_from_payload(payload_json: &str) -> String {
    let parsed = config_payload_value(payload_json);
    config_string(&parsed, "secretRef")
        .if_blank(config_string(&parsed, "secret_ref"))
        .if_blank(payload_json.trim().to_string())
}

fn redacted_secret_label(secret_ref: &str) -> &'static str {
    if secret_ref.starts_with("android-secret://") {
        "android-secret"
    } else if secret_ref.starts_with("env://") {
        "env"
    } else {
        "secret-ref"
    }
}

fn approval_token_from_payload(payload_json: &str) -> String {
    config_payload_string(payload_json, "approvalToken")
}

fn require_approval(command: &RuntimeCommand, scope: &str) -> HamburResult<()> {
    let token = approval_token_from_payload(&command.payload_json);
    let expected = approval_tokens_for_scope(scope);
    if expected.contains(&token) {
        Ok(())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "{scope} requires approval token: {}",
            expected.join(" or ")
        )))
    }
}

fn approval_tokens_for_scope(scope: &str) -> Vec<String> {
    let mut tokens = vec![format!("approve:{scope}")];
    if scope.starts_with("rootfs_setting:") {
        tokens.push("approve:rootfs_settings".to_string());
    }
    if scope.starts_with("startup_task:") {
        tokens.push("approve:startup_tasks".to_string());
    }
    tokens
}

fn setting_key_for_command(command: &RuntimeCommand) -> HamburResult<String> {
    let payload = config_payload_value(&command.payload_json);
    let key = match command.kind.as_str() {
        "UpdateToolSettings" => "tool_settings".to_string(),
        "UpdateSkills" => "skills".to_string(),
        "UpdateMemoryProjections" => "memory_projections".to_string(),
        "UpdateStartupTasks" => "startup_tasks".to_string(),
        "UpdateRootfsSettings" => "rootfs_settings".to_string(),
        "UpdateAppearance" => "appearance".to_string(),
        "UpdateLogs" => "logs".to_string(),
        "UpdateTokenUsage" => "token_usage".to_string(),
        "UpdatePersona" => "persona".to_string(),
        "UpdateEnvironmentVariables" => "environment_variables".to_string(),
        "UpdateBrowserToolSettings" => "browser_tool_settings".to_string(),
        "UpdateAppSetting" => command
            .chunk
            .clone()
            .if_blank(config_string(&payload, "settingKey"))
            .if_blank(config_string(&payload, "key")),
        "UpdateSkillEnabled" => {
            let skill_id = command
                .message_id
                .clone()
                .if_blank(config_string(&payload, "skillId"))
                .if_blank(config_string(&payload, "skillPath"));
            format!("skill_enabled:{skill_id}")
        }
        "UpdateStartupTask" | "DeleteStartupTask" => {
            let task_id = command
                .message_id
                .clone()
                .if_blank(config_string(&payload, "startupTaskId"))
                .if_blank(config_string(&payload, "taskId"))
                .if_blank(config_string(&payload, "id"));
            format!("startup_task:{task_id}")
        }
        "UpdateRootfsSetting" => {
            let rootfs_key = command
                .chunk
                .clone()
                .if_blank(config_string(&payload, "settingKey"))
                .if_blank(config_string(&payload, "key"));
            format!("rootfs_setting:{rootfs_key}")
        }
        _ => {
            return Err(HamburError::InvalidCommand(format!(
                "unsupported setting command kind: {}",
                command.kind
            )));
        }
    };
    if key.trim().is_empty()
        || key.ends_with(':')
        || matches!(
            key.as_str(),
            "skill_enabled:" | "startup_task:" | "rootfs_setting:"
        )
    {
        return Err(HamburError::InvalidCommand(
            "setting key must not be empty".to_string(),
        ));
    }
    Ok(key)
}

fn setting_value_for_command(command: &RuntimeCommand) -> String {
    let payload = config_payload_value(&command.payload_json);
    match command.kind.as_str() {
        "UpdateAppSetting" => command
            .content
            .clone()
            .if_blank(config_value_string(&payload, "value"))
            .if_blank(command.payload_json.clone()),
        "UpdateSkillEnabled" => config_bool(&payload, "enabled", true).to_string(),
        "DeleteStartupTask" => String::new(),
        "UpdateRootfsSetting" => command
            .content
            .clone()
            .if_blank(config_value_string(&payload, "value"))
            .if_blank(command.payload_json.clone()),
        _ => command.payload_json.clone(),
    }
}

fn setting_audit_summary(command_kind: &str, setting_key: &str) -> String {
    match command_kind {
        "DeleteStartupTask" => format!("Setting '{setting_key}' deleted"),
        _ => format!("Setting '{setting_key}' updated"),
    }
}

fn setting_requires_approval(setting_key: &str) -> bool {
    setting_key == "rootfs_settings"
        || setting_key == "startup_tasks"
        || setting_key.starts_with("startup_task:")
        || setting_key.starts_with("rootfs_setting:")
}

trait IfBlank {
    fn if_blank(self, fallback: String) -> String;
}

impl IfBlank for String {
    fn if_blank(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

fn accepted_ack(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
) -> RuntimeCommandAck {
    RuntimeCommandAck {
        command_id: command_id.into(),
        idempotency_key: idempotency_key.into(),
        accepted: true,
        duplicate: false,
        rejection_code: String::new(),
        message: String::new(),
    }
}

fn rejected_ack(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    error: HamburError,
) -> RuntimeCommandAck {
    RuntimeCommandAck {
        command_id: command_id.into(),
        idempotency_key: idempotency_key.into(),
        accepted: false,
        duplicate: false,
        rejection_code: error.code().as_str().to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        process::{Command, Stdio},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use base64::Engine;
    use hambur_core::{new_id, now_ms};
    use hambur_sandbox::SandboxAccess;
    use hambur_tools::ToolInvocation;
    use serde_json::Value;
    use tokio::sync::oneshot;

    use super::{
        AppBootstrap, BackgroundProcessSession, DelegateTaskState, ModelRouteSnapshot,
        NewTraceSpan, ProcessOutputBuffer, RouteStreamSource, RuntimeCommand, RuntimeEngine,
        RuntimeEvent, platform_shell, provider_stream_source, spawn_process_pipe_reader,
        validate_web_fetch_url,
    };

    #[test]
    fn bundled_skills_are_seeded_and_exposed_by_tools() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let skills = runtime.list_skills();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "skill-creator")
            .expect("seeded skill");
        assert_eq!(skill.path, "system/skill-creator/SKILL.md");
        assert!(skill.built_in);
        assert!(skill.enabled);

        let list_invocation = ToolInvocation::from_model_call(
            0,
            "call_skills".to_string(),
            "turn".to_string(),
            "session".to_string(),
            "skills_list".to_string(),
            "{}".to_string(),
        )
        .expect("invocation");
        let list = runtime
            .tokio
            .block_on(
                runtime.resolve_knowledge_tool_result(&list_invocation, &serde_json::json!({})),
            )
            .content_json;
        let list_json: Value = serde_json::from_str(&list).expect("skills list json");
        assert_eq!(list_json["success"], true);
        assert_eq!(list_json["count"], 1);
        assert_eq!(list_json["categories"][0], "system");
        assert_eq!(
            list_json["hint"],
            "Use skill_view(name) to see full content, tags, and linked files."
        );

        let alias_invocation = ToolInvocation::from_model_call(
            0,
            "call_skill_list_alias".to_string(),
            "turn".to_string(),
            "session".to_string(),
            "skill_list".to_string(),
            "{}".to_string(),
        )
        .expect("alias invocation");
        let alias = runtime.tokio.block_on(
            runtime.resolve_knowledge_tool_result(&alias_invocation, &serde_json::json!({})),
        );
        assert!(!alias.is_error, "alias failed: {}", alias.summary);

        let detail = runtime.get_skill_detail("skill-creator".to_string(), String::new());
        assert!(detail.content.contains("# Skill Creator"));
        assert_eq!(detail.skill_dir_path, "system/skill-creator");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn skills_index_prompt_is_injected_into_initial_request() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let prompt = runtime.build_skills_index_prompt();
        assert!(prompt.contains("Hambur has a local Skills system at /var/hambur/skills."));
        assert!(prompt.contains("- skill-creator [system]: Create or update Hambur skills"));

        let route = ModelRouteSnapshot {
            supports_tool_call: true,
            supports_reasoning: true,
            output_limit: 1024,
            ..Default::default()
        };
        let source = provider_stream_source(
            "session", "turn", "hello", &route, "[]", &prompt, "", false, false,
        );
        let RouteStreamSource::Provider(request) = source else {
            panic!("expected provider source");
        };
        assert!(request.system_blocks.iter().any(|block| {
            block.contains("Skills are reusable task instructions")
                && block.contains("skill_view")
                && block.contains("skill-creator")
        }));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn memory_prompt_is_injected_into_initial_request() {
        let app_files_dir = temp_app_dir();
        let memory_dir = app_files_dir.join("sandbox").join("global").join("memory");
        fs::create_dir_all(&memory_dir).expect("create memory dir");
        fs::write(
            memory_dir.join("MEMORY.md"),
            "Project uses Rust backend and Compose frontend.",
        )
        .expect("write memory");
        fs::write(memory_dir.join("USER.md"), "User prefers concise Chinese.")
            .expect("write user memory");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let memory_prompt = runtime.build_memory_system_prompt();
        assert!(memory_prompt.contains("You have persistent memory across chats."));
        assert!(memory_prompt.contains("Project uses Rust backend and Compose frontend."));
        assert!(memory_prompt.contains("User prefers concise Chinese."));

        let route = ModelRouteSnapshot {
            supports_tool_call: true,
            output_limit: 1024,
            ..Default::default()
        };
        let source = provider_stream_source(
            "session",
            "turn",
            "hello",
            &route,
            "[]",
            "",
            &memory_prompt,
            false,
            false,
        );
        let RouteStreamSource::Provider(request) = source else {
            panic!("expected provider source");
        };
        assert!(request
            .system_blocks
            .iter()
            .any(|block| block.contains("MEMORY (your personal notes)") && block.contains("USER PROFILE")));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn bootstrap_snapshot_survives_restart_without_replayed_session_event() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let first = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = first.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");
        assert!(ready.snapshot.sessions.is_empty());

        let ack = first.create_session("Persisted".to_string());
        assert!(ack.accepted, "create session rejected: {}", ack.message);
        let created = first.next_event().expect("created event");
        assert_eq!(created.kind.as_str(), "SessionCreated");
        assert_eq!(created.snapshot.sessions.len(), 1);
        let created_session_id = created.snapshot.selected_session_id.clone();
        let workspace = first
            .sandbox
            .resolve(
                &created_session_id,
                "/var/hambur/workspace",
                SandboxAccess::Read,
            )
            .expect("created session workspace");
        assert!(workspace.host_path.is_dir());
        first.shutdown();
        drop(first);

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let restarted_ready = restarted.next_event().expect("restart ready event");
        assert_eq!(restarted_ready.kind.as_str(), "RuntimeReady");
        assert_eq!(restarted_ready.snapshot.sessions.len(), 1);
        assert_eq!(restarted_ready.snapshot.sessions[0].title, "Persisted");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn markdown_delta_emits_render_update_event() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = runtime.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");

        let ack = runtime.append_markdown_delta(
            "session".to_string(),
            "message".to_string(),
            "# Heading\n\n".to_string(),
            false,
        );
        assert!(ack.accepted, "markdown command rejected: {}", ack.message);

        let event = runtime.next_event().expect("markdown event");
        assert_eq!(event.kind.as_str(), "MarkdownRenderUpdate");
        assert_eq!(event.session_id, "session");
        assert_eq!(event.markdown_render_update.message_id, "message");
        assert_eq!(event.markdown_render_update.committed_nodes.len(), 1);
        assert_eq!(
            event.markdown_render_update.committed_nodes[0].node_kind,
            "Heading"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn duplicate_idempotency_key_does_not_repeat_session_creation() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = runtime.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");

        let command = RuntimeCommand {
            command_id: "cmd_create_once".to_string(),
            idempotency_key: "session:create:once".to_string(),
            kind: "CreateSession".to_string(),
            title: "Once".to_string(),
            ..RuntimeCommand::default()
        };
        let first_ack = runtime.dispatch(command.clone());
        assert!(first_ack.accepted, "first command rejected");
        let created = runtime.next_event().expect("created event");
        assert_eq!(created.kind.as_str(), "SessionCreated");

        let duplicate_ack = runtime.dispatch(command);
        assert!(duplicate_ack.accepted);
        assert!(duplicate_ack.duplicate);

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].title, "Once");
        assert_eq!(snapshot.snapshot_sequence, created.sequence);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn provider_config_rejects_plain_api_key_secret() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_provider".to_string(),
            idempotency_key: "provider:update:bad-secret".to_string(),
            kind: "UpdateProvider".to_string(),
            provider_id: "provider_bad".to_string(),
            title: "Bad".to_string(),
            chunk: "https://api.test/v1".to_string(),
            payload_json: "sk-raw-secret".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!ack.accepted);
        assert_eq!(ack.rejection_code, "InvalidCommand");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_persists_reasoning_content_and_model_snapshot() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send".to_string(),
            idempotency_key: "message:client-1".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "hello".to_string(),
            payload_json: r#"{"content":"hello world","reasoning":"thinking separately"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_content = false;
        let mut saw_reasoning = false;
        let mut saw_markdown = false;
        let mut finished = None;
        for _ in 0..32 {
            let event = runtime.next_event().expect("stream event");
            match event.kind.as_str() {
                "AssistantContentDelta" => {
                    saw_content = true;
                    assert!(event.message.contains("hello") || event.message.contains("world"));
                }
                "AssistantReasoningDelta" => {
                    saw_reasoning = true;
                    assert_eq!(event.message, "thinking separately");
                }
                "MarkdownRenderUpdate" => {
                    saw_markdown = true;
                    assert!(!event.markdown_render_update.message_id.is_empty());
                }
                "TurnFinished" => {
                    finished = Some(event);
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_content, "missing content delta");
        assert!(saw_reasoning, "missing reasoning delta");
        assert!(saw_markdown, "missing markdown update");
        let finished = finished.expect("turn finished");

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("hello world"));
        assert_eq!(finished.session_id, session_id);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn local_http_provider_streams_openai_compatible_sse() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).expect("read provider request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if String::from_utf8_lossy(&request).contains("\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request).to_string();
                *captured.lock().expect("capture request") = text;
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"real reasoning\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"real provider answer\"}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write provider response");
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_http_test_provider(
            &runtime,
            "provider_http",
            "gpt-real",
            &format!("http://{addr}/v1"),
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_real_provider".to_string(),
            idempotency_key: "message:real-provider:http".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "use real HTTP".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer test-api-key"));
        assert!(request.contains("content-type: application/json"));

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("real provider answer"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_streams_text() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_real",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_provider".to_string(),
            idempotency_key: "message:e2e-real-provider".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with a short sentence containing hambur-e2e-ok.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_content_delta = false;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real provider event");
            match event.kind.as_str() {
                "AssistantContentDelta" => saw_content_delta = true,
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real provider turn did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_content_delta, "real provider emitted no content delta");

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item");
        let message = runtime
            .get_message_snapshot(assistant.payload_ref.clone())
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_real");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert!(
            !message.content_text.trim().is_empty(),
            "assistant content should not be empty"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_streams_markdown_updates() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_markdown",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_markdown".to_string(),
            idempotency_key: "message:e2e-real-markdown".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply in Markdown with one heading, one bullet list, and one fenced code block. Keep it short.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut markdown_updates = 0;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real markdown provider event");
            match event.kind.as_str() {
                "MarkdownRenderUpdate" => {
                    let update = event.markdown_render_update;
                    if !update.committed_nodes.is_empty() || update.pending_node.is_some() {
                        markdown_updates += 1;
                    }
                }
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real markdown provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(
            markdown_updates > 0,
            "real provider emitted no markdown updates"
        );

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item");
        let message = runtime
            .get_message_snapshot(assistant.payload_ref.clone())
            .message
            .expect("assistant message snapshot");
        assert!(!message.content_text.trim().is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_can_be_cancelled_after_stream_start() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_cancel",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_cancel".to_string(),
            idempotency_key: "message:e2e-real-cancel".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Count from 1 to 5000, one number per line. Start immediately.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut turn_id = String::new();
        let mut saw_content_delta = false;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real cancel provider event");
            match event.kind.as_str() {
                "TurnStarted" => turn_id = event.turn_id,
                "AssistantContentDelta" => {
                    saw_content_delta = true;
                    break;
                }
                "TurnFailed" | "TurnCancelled" | "TurnFinished" => {
                    panic!(
                        "turn ended before cancellation could be issued: {} {}",
                        event.kind.as_str(),
                        event.message
                    );
                }
                _ => {}
            }
        }
        assert!(
            saw_content_delta,
            "real provider emitted no content before cancellation"
        );
        assert!(!turn_id.is_empty(), "missing turn id before cancellation");

        let cancel = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_cancel_turn".to_string(),
            idempotency_key: format!("{turn_id}:cancel"),
            kind: "CancelTurn".to_string(),
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(cancel.accepted, "cancel rejected: {}", cancel.message);

        let mut cancelled = None;
        for _ in 0..128 {
            let event = runtime.next_event().expect("real cancel terminal event");
            match event.kind.as_str() {
                "TurnCancelled" => {
                    cancelled = Some(event);
                    break;
                }
                "TurnFinished" | "TurnFailed" => {
                    panic!(
                        "expected cancellation but got {}: {}",
                        event.kind.as_str(),
                        event.message
                    );
                }
                _ => {}
            }
        }
        let cancelled = cancelled.expect("real provider turn did not cancel");
        assert_eq!(cancelled.turn_id, turn_id);
        assert_eq!(cancelled.error_code, "Cancelled");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_is_used_after_failed_primary_fallback() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_a_e2e_bad",
            "bad-e2e-model",
            "http://127.0.0.1:9/v1",
            &config.secret_env,
        );
        configure_env_secret_provider(
            &runtime,
            "provider_z_e2e_real",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_fallback".to_string(),
            idempotency_key: "message:e2e-real-fallback".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with one short sentence containing hambur-fallback-ok.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        let mut terminal = None;
        for _ in 0..512 {
            let event = runtime.next_event().expect("real fallback provider event");
            if event.kind.as_str() == "TurnStateChanged" && event.message.contains("Fallback to") {
                saw_fallback = true;
            }
            if matches!(
                event.kind.as_str(),
                "TurnFinished" | "TurnFailed" | "TurnCancelled"
            ) {
                terminal = Some(event);
                break;
            }
        }
        let terminal = terminal.expect("real fallback provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "fallback did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_fallback, "missing fallback state event");

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item");
        let message = runtime
            .get_message_snapshot(assistant.payload_ref.clone())
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_z_e2e_real");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert_eq!(message.status, "completed");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_regenerate_uses_real_provider() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_regen",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_regen_seed".to_string(),
            idempotency_key: "message:e2e-regenerate:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with a short sentence containing hambur-regenerate-seed.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "seed rejected: {}", first.message);
        wait_for_session_finished(&runtime, &session_id, "seed turn");

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let first_assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item")
            .clone();
        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_regenerate".to_string(),
            idempotency_key: format!("{}:regenerate:e2e", first_assistant.payload_ref),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: first_assistant.payload_ref,
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_finished(&runtime, &session_id, "regenerate turn");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let assistant_messages = timeline
            .items
            .iter()
            .filter(|item| item.kind == "AssistantMessage")
            .collect::<Vec<_>>();
        assert_eq!(assistant_messages.len(), 2);
        let latest = assistant_messages.last().expect("latest assistant");
        let message = runtime
            .get_message_snapshot(latest.payload_ref.clone())
            .message
            .expect("latest assistant message");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_regen");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert_eq!(message.status, "completed");
        assert!(!message.content_text.trim().is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider with tool calling"]
    fn e2e_real_openai_compatible_provider_executes_tool_call() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider_with_capabilities(
            &runtime,
            "provider_e2e_tools",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
            true,
            false,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_tools".to_string(),
            idempotency_key: "message:e2e-real-tools".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: concat!(
                "Call the echo tool exactly once with JSON arguments ",
                "{\"text\":\"hambur-real-tool-ok\"}. ",
                "Do not answer directly before calling the tool."
            )
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_tool_delta = false;
        let mut finished_tools = 0;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real tool provider event");
            if event.session_id != session_id {
                continue;
            }
            match event.kind.as_str() {
                "ToolCallDelta" => saw_tool_delta = true,
                "ToolCallFinished" => {
                    finished_tools += 1;
                    assert!(
                        event.message.contains("hambur-real-tool-ok"),
                        "unexpected tool summary: {}",
                        event.message
                    );
                }
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real tool provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider tool turn did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_tool_delta, "real provider emitted no tool call delta");
        assert!(finished_tools > 0, "real provider executed no tools");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        assert!(
            timeline.items.iter().any(|item| item.kind == "ToolTrace"),
            "missing tool trace"
        );
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("continuation message");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_tools");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert!(message.content_text.contains("hambur-real-tool-ok"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn duplicate_send_message_is_rejected_while_turn_is_active() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_first".to_string(),
            idempotency_key: "message:busy:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "first".to_string(),
            payload_json: r#"{"content":"one two three four five six","reasoning":"busy"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted);

        let second = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_second".to_string(),
            idempotency_key: "message:busy:second".to_string(),
            kind: "SendMessage".to_string(),
            session_id,
            content: "second".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!second.accepted);
        assert_eq!(second.rejection_code, "SessionBusy");

        let turn_started = loop {
            let event = runtime.next_event().expect("turn started");
            if event.kind.as_str() == "TurnStarted" {
                break event;
            }
        };
        let _ = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_busy_cancel".to_string(),
            idempotency_key: format!("{}:cancel", turn_started.turn_id),
            kind: "CancelTurn".to_string(),
            turn_id: turn_started.turn_id,
            ..RuntimeCommand::default()
        });
        for _ in 0..32 {
            let event = runtime.next_event().expect("terminal busy event");
            if matches!(
                event.kind.as_str(),
                "TurnCancelled" | "TurnFinished" | "TurnFailed"
            ) {
                break;
            }
        }

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_cancel_turn_stops_streaming() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_cancel_send".to_string(),
            idempotency_key: "message:cancel:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "cancel me".to_string(),
            payload_json:
                r#"{"content":"one two three four five six seven eight","reasoning":"cancel path"}"#
                    .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted);

        let turn_started = loop {
            let event = runtime.next_event().expect("turn started");
            if event.kind.as_str() == "TurnStarted" {
                break event;
            }
        };
        let cancel = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_cancel".to_string(),
            idempotency_key: format!("{}:cancel", turn_started.turn_id),
            kind: "CancelTurn".to_string(),
            session_id,
            turn_id: turn_started.turn_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(cancel.accepted);

        let mut cancelled = None;
        for _ in 0..32 {
            let event = runtime.next_event().expect("cancel event");
            if event.kind.as_str() == "TurnCancelled" {
                cancelled = Some(event);
                break;
            }
        }
        let cancelled = cancelled.expect("turn cancelled");
        assert_eq!(cancelled.turn_id, turn_started.turn_id);
        assert_eq!(cancelled.error_code, "Cancelled");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_regenerate_message_uses_source_user_content() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_seed".to_string(),
            idempotency_key: "message:regenerate:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "seed prompt".to_string(),
            payload_json: r#"{"content":"first answer","reasoning":"seed reasoning"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "seed rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item")
            .clone();

        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate".to_string(),
            idempotency_key: format!("{}:regenerate:test", assistant.payload_ref),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: assistant.payload_ref,
            payload_json: r#"{"content":"regenerated answer","reasoning":"regen reasoning"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "AssistantMessage")
            .count();
        let user_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "UserMessage")
            .count();
        assert_eq!(assistant_count, 2);
        assert_eq!(user_count, 2);

        let latest_assistant = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("latest assistant");
        let message = runtime
            .get_message_snapshot(latest_assistant.payload_ref.clone())
            .message
            .expect("latest assistant message");
        assert_eq!(message.content_text, "regenerated answer");
        assert_eq!(message.reasoning_content, "regen reasoning");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_routes_fallback_switches_target_before_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_a", "model-a");
        configure_named_test_provider(&runtime, "provider_b", "model-b");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_fallback_before_output".to_string(),
            idempotency_key: "message:fallback:before-output".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "fallback please".to_string(),
            payload_json: serde_json::json!({
                "routes": {
                    "provider_a": {
                        "sse": "data: {\"error\":{\"code\":\"Http5xx\",\"message\":\"first target down\"}}\n\n"
                    },
                    "provider_b": {
                        "content": "fallback answer",
                        "reasoning": "second target selected"
                    }
                }
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        for _ in 0..64 {
            let event = runtime.next_event().expect("fallback event");
            if event.kind.as_str() == "TurnStateChanged" {
                saw_fallback = true;
            }
            if event.kind.as_str() == "TurnFinished" {
                break;
            }
        }
        assert!(saw_fallback, "missing fallback state event");

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item");
        let message = runtime
            .get_message_snapshot(assistant.payload_ref.clone())
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_b");
        assert_eq!(message.model_id_snapshot, "model-b");
        assert_eq!(message.status, "completed");
        assert_eq!(message.content_text, "fallback answer");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_routes_fallback_does_not_switch_after_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_a", "model-a");
        configure_named_test_provider(&runtime, "provider_b", "model-b");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_fallback_after_output".to_string(),
            idempotency_key: "message:fallback:after-output".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "partial then fail".to_string(),
            payload_json: serde_json::json!({
                "routes": {
                    "provider_a": {
                        "sse": "data: {\"choices\":[{\"delta\":{\"content\":\"partial \"}}]}\n\ndata: {\"error\":{\"code\":\"Http5xx\",\"message\":\"late failure\"}}\n\n"
                    },
                    "provider_b": {
                        "content": "should not be used"
                    }
                }
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        for _ in 0..64 {
            let event = runtime.next_event().expect("partial failure event");
            if event.kind.as_str() == "TurnStateChanged" {
                saw_fallback = true;
            }
            if event.kind.as_str() == "TurnFailed" {
                break;
            }
        }
        assert!(!saw_fallback, "fallback happened after semantic output");

        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item");
        let message = runtime
            .get_message_snapshot(assistant.payload_ref.clone())
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_a");
        assert_eq!(message.model_id_snapshot, "model-a");
        assert_eq!(message.status, "failed_partial");
        assert_eq!(message.content_text, "partial ");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_tool_calls_execute_as_one_batch_and_render_traces() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-tools");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_time\",\"function\":{\"name\":\"get_current_time\",\"arguments\":\"{}\"}},",
            "{\"index\":1,\"id\":\"call_echo\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"hello tools\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Tool results received: hello tools\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_tools".to_string(),
            idempotency_key: "message:tools:batch".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "use tools".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut finished_tools = 0;
        let mut turn_finished = false;
        let mut seen_events = Vec::new();
        for _ in 0..96 {
            let event = next_event_with_timeout(&runtime, "tool event");
            seen_events.push(event.kind.clone());
            match event.kind.as_str() {
                "ToolCallFinished" => finished_tools += 1,
                "TurnFinished" => {
                    turn_finished = true;
                    break;
                }
                "TurnFailed" => panic!("turn failed: {}", event.message),
                _ => {}
            }
        }
        assert!(turn_finished, "missing turn finish; seen={seen_events:?}");
        assert_eq!(finished_tools, 2);

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let trace_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "ToolTrace")
            .count();
        assert!(trace_count >= 2, "missing tool traces: {trace_count}");
        let assistant_block = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.content_type == "assistant_markdown_block")
            .expect("assistant markdown block");
        let payload = timeline
            .markdown_block_payloads
            .iter()
            .find(|payload| payload.id == assistant_block.payload_ref)
            .expect("assistant markdown payload");
        assert!(payload.raw.contains("hello tools"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn attachment_import_remove_and_startup_cleanup() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_attachment".to_string(),
            idempotency_key: "content://image:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png",
                "byteSize": 12,
                "originalUri": "content://images/photo"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted, "import rejected: {}", import.message);
        let imported = runtime.next_event().expect("attachment imported");
        assert_eq!(imported.kind.as_str(), "AttachmentImported");
        assert_eq!(imported.snapshot.pending_attachments.len(), 1);
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let remove = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_remove_attachment".to_string(),
            idempotency_key: format!("{attachment_id}:remove"),
            kind: "RemovePendingAttachment".to_string(),
            session_id: session_id.clone(),
            message_id: attachment_id,
            ..RuntimeCommand::default()
        });
        assert!(remove.accepted, "remove rejected: {}", remove.message);
        let removed = runtime.next_event().expect("attachment removed");
        assert_eq!(removed.kind.as_str(), "PendingAttachmentRemoved");
        assert!(removed.snapshot.pending_attachments.is_empty());

        let import_stale = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_stale".to_string(),
            idempotency_key: "content://stale:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "stale.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import_stale.accepted);
        let stale_event = runtime.next_event().expect("stale imported");
        assert_eq!(stale_event.snapshot.pending_attachments.len(), 1);
        runtime.shutdown();
        drop(runtime);

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let ready = restarted.next_event().expect("ready after cleanup");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");
        assert!(ready.snapshot.pending_attachments.is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn send_message_consumes_image_attachment_and_requires_vision_route() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_models(
            &runtime,
            "provider_vision",
            &[("model-text", false), ("model-vision", true)],
        );

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_for_send".to_string(),
            idempotency_key: "content://image:send:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_with_image".to_string(),
            idempotency_key: "message:image:send".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "describe this".to_string(),
            payload_json: serde_json::json!({
                "attachmentIds": [attachment_id],
                "content": "image accepted",
                "reasoning": "vision"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");
        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let user = timeline
            .items
            .iter()
            .find(|item| item.kind == "UserMessage")
            .expect("user message");
        let user_message = runtime
            .get_message_snapshot(user.payload_ref.clone())
            .message
            .expect("user message snapshot");
        assert!(user_message.content_text.contains("ImagePart"));
        assert!(
            runtime
                .get_session_snapshot(user_message.session_id)
                .timeline_items
                .len()
                >= 2
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn text_only_route_rejects_image_attachment_without_vision_model() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "model-text");

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_image_text_only".to_string(),
            idempotency_key: "content://image:text-only:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_image_text_only".to_string(),
            idempotency_key: "message:image:text-only".to_string(),
            kind: "SendMessage".to_string(),
            session_id,
            content: "describe this".to_string(),
            payload_json: serde_json::json!({"attachmentIds": [attachment_id]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!send.accepted);
        assert_eq!(send.rejection_code, "CapabilityMismatch");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn view_image_text_route_hands_off_to_vision_continuation() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_models(
            &runtime,
            "provider_combo",
            &[("model-text", false), ("model-vision", true)],
        );

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_view_image".to_string(),
            idempotency_key: "content://view-image:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "view.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let path = imported.snapshot.pending_attachments[0]
            .sandbox_path
            .clone();

        let sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_view\",\"function\":{{\"name\":\"view_image\",\"arguments\":\"{{\\\"path\\\":\\\"{}\\\"}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
            path
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_view_image_turn".to_string(),
            idempotency_key: "message:view-image:handoff".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "inspect image by tool".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);

        let mut saw_handoff = false;
        for _ in 0..96 {
            let event = runtime.next_event().expect("view image event");
            if event.kind.as_str() == "TurnStateChanged"
                && event.message.contains("ImageInspectionRequired")
            {
                saw_handoff = true;
            }
            if event.kind.as_str() == "TurnFinished" {
                break;
            }
        }
        assert!(saw_handoff, "missing vision handoff event");
        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("continuation message");
        assert_eq!(message.model_id_snapshot, "model-vision");
        assert!(message.content_text.contains("ImagePart"));
        assert!(!message.content_text.contains("base64"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn settings_snapshot_redacts_secrets_and_audits_config_mutations() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let provider = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_settings_provider".to_string(),
            idempotency_key: "settings:provider:update".to_string(),
            kind: "UpdateProvider".to_string(),
            provider_id: "provider_settings".to_string(),
            title: "Settings Provider".to_string(),
            chunk: "https://api.settings.test/v1".to_string(),
            payload_json: serde_json::json!({
                "secretRef": "android-secret://providers/settings",
                "enabled": true
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let event = runtime.next_event().expect("settings event");
        assert_eq!(event.kind.as_str(), "SettingsChanged");

        let snapshot = runtime.get_settings_snapshot();
        assert_eq!(snapshot.settings.providers.len(), 1);
        let provider = &snapshot.settings.providers[0];
        assert_eq!(provider.id, "provider_settings");
        assert_eq!(provider.secret_label, "Android Secret Store");
        assert!(!provider.secret_label.contains("settings"));
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "UpdateProvider"
                    && audit.redacted_summary.contains("redacted secret"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn destructive_config_mutations_require_approval() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_named_test_provider(&runtime, "provider_delete", "model-delete");

        let rejected_delete = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_delete_without_approval".to_string(),
            idempotency_key: "settings:delete:without-approval".to_string(),
            kind: "DeleteProvider".to_string(),
            provider_id: "provider_delete".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected_delete.accepted);
        assert_eq!(rejected_delete.rejection_code, "InvalidCommand");

        let rejected_rootfs = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_rootfs_without_approval".to_string(),
            idempotency_key: "settings:rootfs:without-approval".to_string(),
            kind: "UpdateRootfsSettings".to_string(),
            payload_json: serde_json::json!({"enabled": true}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected_rootfs.accepted);
        assert_eq!(rejected_rootfs.rejection_code, "InvalidCommand");

        let approved_rootfs = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_rootfs_with_approval".to_string(),
            idempotency_key: "settings:rootfs:with-approval".to_string(),
            kind: "UpdateRootfsSettings".to_string(),
            payload_json: serde_json::json!({
                "enabled": true,
                "approvalToken": "approve:rootfs_settings"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            approved_rootfs.accepted,
            "rootfs rejected: {}",
            approved_rootfs.message
        );
        let _ = runtime.next_event().expect("rootfs settings event");

        let approved_delete = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_delete_with_approval".to_string(),
            idempotency_key: "settings:delete:with-approval".to_string(),
            kind: "DeleteProvider".to_string(),
            provider_id: "provider_delete".to_string(),
            payload_json: serde_json::json!({
                "approvalToken": "approve:delete-provider"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            approved_delete.accepted,
            "delete rejected: {}",
            approved_delete.message
        );
        let _ = runtime.next_event().expect("provider deleted event");

        let snapshot = runtime.get_settings_snapshot();
        assert!(snapshot.settings.providers.is_empty());
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "DeleteProvider" && audit.approval_required)
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn reset_rootfs_requires_approval_and_preserves_session_dirs() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        let workspace = runtime
            .sandbox
            .resolve(
                &session_id,
                "/var/hambur/workspace/rootfs-marker.txt",
                SandboxAccess::Write,
            )
            .expect("workspace path");
        if let Some(parent) = workspace.host_path.parent() {
            fs::create_dir_all(parent).expect("workspace parent");
        }
        fs::write(&workspace.host_path, "stale").expect("write marker");
        assert!(workspace.host_path.exists());

        let rejected = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_reset_rootfs_rejected".to_string(),
            idempotency_key: "rootfs:reset:rejected".to_string(),
            kind: "ResetRootfs".to_string(),
            session_id: session_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected.accepted);
        assert_eq!(rejected.rejection_code, "InvalidCommand");

        let accepted = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_reset_rootfs_approved".to_string(),
            idempotency_key: "rootfs:reset:approved".to_string(),
            kind: "ResetRootfs".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "approvalToken": "approve:rootfs_reset"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(accepted.accepted, "reset rejected: {}", accepted.message);
        let event = wait_for_session_event_result(&runtime, "TurnStateChanged", &session_id);
        assert!(event.message.contains("\"action\":\"ResetRootfs\""));
        let prepared = runtime
            .sandbox
            .resolve(&session_id, "/var/hambur/workspace", SandboxAccess::Read)
            .expect("prepared workspace");
        assert!(prepared.host_path.exists());
        assert!(workspace.host_path.exists());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn model_groups_and_app_settings_persist_through_restart() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        {
            let runtime = RuntimeEngine::create(AppBootstrap {
                app_files_dir: app_files_dir.to_string_lossy().to_string(),
                native_library_dir: String::new(),
            })
            .expect("create runtime");
            let _ = runtime.next_event().expect("ready event");
            configure_named_test_provider(&runtime, "provider_persist", "model-persist");

            let group = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_group_persist".to_string(),
                idempotency_key: "settings:group:persist".to_string(),
                kind: "UpdateModelGroup".to_string(),
                message_id: "grp_persist".to_string(),
                title: "Persistent Group".to_string(),
                payload_json: serde_json::json!({
                    "groupId": "grp_persist",
                    "name": "Persistent Group",
                    "routingStrategy": "fallback",
                    "fallbackPolicy": "default"
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(group.accepted, "group rejected: {}", group.message);
            let _ = runtime.next_event().expect("group event");

            let member = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_group_member_persist".to_string(),
                idempotency_key: "settings:group-member:persist".to_string(),
                kind: "UpdateModelGroupMember".to_string(),
                message_id: "grp_persist".to_string(),
                provider_id: "provider_persist".to_string(),
                model_id: "model-persist".to_string(),
                payload_json: serde_json::json!({
                    "groupId": "grp_persist",
                    "providerId": "provider_persist",
                    "modelId": "model-persist",
                    "position": 0,
                    "enabled": true
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(member.accepted, "member rejected: {}", member.message);
            let _ = runtime.next_event().expect("member event");

            let default = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_default_group_persist".to_string(),
                idempotency_key: "settings:default-group:persist".to_string(),
                kind: "SetDefaultModelGroup".to_string(),
                chunk: "primary".to_string(),
                message_id: "grp_persist".to_string(),
                payload_json: serde_json::json!({
                    "key": "primary",
                    "groupId": "grp_persist"
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(default.accepted, "default rejected: {}", default.message);
            let _ = runtime.next_event().expect("default event");

            let tool_settings = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_tool_settings_persist".to_string(),
                idempotency_key: "settings:tool:persist".to_string(),
                kind: "UpdateToolSettings".to_string(),
                payload_json: serde_json::json!({
                    "terminal": false,
                    "browser": true
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(
                tool_settings.accepted,
                "tool setting rejected: {}",
                tool_settings.message
            );
            let _ = runtime.next_event().expect("tool settings event");
            runtime.shutdown();
        }

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let _ = restarted.next_event().expect("restart ready");
        let snapshot = restarted.get_settings_snapshot();
        assert!(
            snapshot
                .settings
                .model_groups
                .iter()
                .any(|group| group.id == "grp_persist")
        );
        assert!(
            snapshot
                .settings
                .model_group_members
                .iter()
                .any(|member| member.group_id == "grp_persist"
                    && member.provider_id == "provider_persist"
                    && member.model_id == "model-persist")
        );
        assert!(
            snapshot
                .settings
                .default_model_groups
                .iter()
                .any(|default| default.key == "primary" && default.group_id == "grp_persist")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "tool_settings" && setting.value.contains("browser"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone6_global_settings_commands_match_architecture_contract() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_named_test_provider(&runtime, "provider_m6", "model-m6");

        let group = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_group".to_string(),
            idempotency_key: "settings:m6:group".to_string(),
            kind: "UpdateModelGroup".to_string(),
            message_id: "grp_m6".to_string(),
            title: "Milestone 6".to_string(),
            payload_json: serde_json::json!({
                "groupId": "grp_m6",
                "name": "Milestone 6",
                "routingStrategy": "load_balance",
                "fallbackPolicy": "always"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(group.accepted, "group rejected: {}", group.message);
        let _ = runtime.next_event().expect("group event");

        let default_groups = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_defaults".to_string(),
            idempotency_key: "settings:m6:defaults".to_string(),
            kind: "UpdateDefaultModelGroups".to_string(),
            payload_json: serde_json::json!({
                "primaryGroupId": "grp_m6",
                "secondaryGroupId": "grp_m6"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            default_groups.accepted,
            "defaults rejected: {}",
            default_groups.message
        );
        let _ = runtime.next_event().expect("defaults event");

        let theme = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_theme".to_string(),
            idempotency_key: "settings:m6:theme".to_string(),
            kind: "UpdateAppSetting".to_string(),
            chunk: "themeMode".to_string(),
            payload_json: serde_json::json!({"settingKey": "themeMode", "value": "dark"})
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(theme.accepted, "theme rejected: {}", theme.message);
        let _ = runtime.next_event().expect("theme event");

        let bad_theme = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_bad_theme".to_string(),
            idempotency_key: "settings:m6:bad-theme".to_string(),
            kind: "UpdateAppSetting".to_string(),
            chunk: "themeMode".to_string(),
            payload_json: serde_json::json!({"value": "purple"}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!bad_theme.accepted);
        assert_eq!(bad_theme.rejection_code, "InvalidCommand");

        let browser = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_browser".to_string(),
            idempotency_key: "settings:m6:browser".to_string(),
            kind: "UpdateBrowserToolSettings".to_string(),
            payload_json: serde_json::json!({
                "acceptCookies": false,
                "acceptThirdPartyCookies": false,
                "maxFetchBytes": 250000,
                "autoCloseMinutes": 30
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(browser.accepted, "browser rejected: {}", browser.message);
        let _ = runtime.next_event().expect("browser event");

        let bad_browser = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_bad_browser".to_string(),
            idempotency_key: "settings:m6:bad-browser".to_string(),
            kind: "UpdateBrowserToolSettings".to_string(),
            payload_json: serde_json::json!({
                "acceptCookies": false,
                "acceptThirdPartyCookies": true,
                "maxFetchBytes": 250000
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!bad_browser.accepted);
        assert_eq!(bad_browser.rejection_code, "InvalidCommand");

        let skill = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_skill".to_string(),
            idempotency_key: "settings:m6:skill".to_string(),
            kind: "UpdateSkillEnabled".to_string(),
            message_id: "skills/test".to_string(),
            payload_json: serde_json::json!({"enabled": false}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(skill.accepted, "skill rejected: {}", skill.message);
        let _ = runtime.next_event().expect("skill event");

        let rootfs_without_approval = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_rootfs_no_approval".to_string(),
            idempotency_key: "settings:m6:rootfs:no-approval".to_string(),
            kind: "UpdateRootfsSetting".to_string(),
            chunk: "backend".to_string(),
            payload_json: serde_json::json!({"value": "proot"}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rootfs_without_approval.accepted);
        assert_eq!(rootfs_without_approval.rejection_code, "InvalidCommand");

        let startup = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_startup".to_string(),
            idempotency_key: "settings:m6:startup".to_string(),
            kind: "UpdateStartupTask".to_string(),
            message_id: "task_m6".to_string(),
            payload_json: serde_json::json!({
                "id": "task_m6",
                "name": "Warmup",
                "script": "echo ready",
                "enabled": true,
                "approvalToken": "approve:startup_tasks"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(startup.accepted, "startup rejected: {}", startup.message);
        let _ = runtime.next_event().expect("startup event");

        let snapshot = runtime.get_settings_snapshot();
        assert!(
            snapshot
                .settings
                .default_model_groups
                .iter()
                .any(|default| default.key == "secondary" && default.group_id == "grp_m6")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "themeMode" && setting.value == "dark")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "skill_enabled:skills-test"
                    && setting.value == "false")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "startup_task:task_m6")
        );
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "UpdateStartupTask" && audit.approval_required)
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_session_search_tool_uses_database_matches() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let alpha = runtime.create_session("Alpha Project".to_string());
        assert!(alpha.accepted);
        let _ = runtime.next_event().expect("alpha event");
        let beta = runtime.create_session("Beta Research".to_string());
        assert!(beta.accepted);
        let _ = runtime.next_event().expect("beta event");
        configure_test_provider(&runtime, "gpt-tools");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_search\",\"function\":{\"name\":\"session_search\",\"arguments\":\"{\\\"query\\\":\\\"Alpha\\\",\\\"limit\\\":5}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Found Alpha Project in prior sessions.\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_session_search".to_string(),
            idempotency_key: "message:m7:session-search".to_string(),
            kind: "SendMessage".to_string(),
            session_id: runtime.get_session_list_snapshot(10, 0).selected_session_id,
            content: "search old sessions".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        let selected = snapshot.selected_session_id;
        let assistant_text = assistant_markdown_text(&runtime, &selected);
        assert!(assistant_text.contains("Alpha Project"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_browser_use_waits_for_submit_platform_result() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-browser");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_browser\",\"function\":{\"name\":\"browser_use\",\"arguments\":\"{\\\"action\\\":\\\"get_text\\\",\\\"url\\\":\\\"https://example.test\\\",\\\"timeout_ms\\\":5000}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Example Domain <untrusted_tool_result\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_browser".to_string(),
            idempotency_key: "message:m7:browser".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "read page".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);

        let platform_request = loop {
            let event = runtime.next_event().expect("event");
            if event.kind.as_str() == "PlatformRequest" {
                break event.platform_request;
            }
        };
        assert_eq!(platform_request.kind, "BrowserAction");
        assert_eq!(platform_request.session_id, session_id);
        assert!(platform_request.payload_json.contains("call_browser"));

        let submit = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_submit_browser".to_string(),
            idempotency_key: "platform:m7:browser:result".to_string(),
            kind: "SubmitPlatformResult".to_string(),
            message_id: platform_request.request_id,
            payload_json: serde_json::json!({
                "payloadJson": {"text": "Example Domain", "url": "https://example.test"},
                "isError": false
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(submit.accepted);
        wait_for_event(&runtime, "TurnFinished");

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("Example Domain"));
        assert!(assistant_text.contains("<untrusted_tool_result"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_browser_screenshot_payload_materializes_to_filestore() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        let content = serde_json::json!({
            "url": "https://example.test",
            "mimeType": "image/png",
            "width": 1,
            "height": 1,
            "byteSize": 3,
            "base64": "AQID"
        })
        .to_string();
        let materialized = runtime
            .tokio
            .block_on(runtime.materialize_browser_artifacts(
                &session_id,
                "call_browser_screenshot",
                &content,
            ))
            .expect("materialize screenshot");
        assert!(!materialized.contains("AQID"));
        assert!(materialized.contains("sandboxPath"));
        let value = serde_json::from_str::<Value>(&materialized).expect("json");
        let file_id = value
            .get("fileId")
            .and_then(Value::as_str)
            .expect("file id");
        let file = runtime
            .tokio
            .block_on(runtime.database.file_by_id(file_id))
            .expect("file record");
        assert_eq!(file.session_id, session_id);
        assert_eq!(file.mime_type, "image/png");
        assert_eq!(file.byte_size, 3);
        let host_path = runtime
            .filestore
            .host_path_for_relative(&file.relative_path)
            .expect("host path");
        assert_eq!(fs::read(host_path).expect("read artifact"), vec![1, 2, 3]);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_delegate_submit_rejects_artifact_path_escape() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-delegate");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_delegate_submit\",\"function\":{\"name\":\"submit_delegate_result\",\"arguments\":\"{\\\"summary\\\":\\\"done\\\",\\\"artifact_paths\\\":[\\\"/var/hambur/workspace/../memory/MEMORY.md\\\"]}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_delegate_escape".to_string(),
            idempotency_key: "message:m7:delegate-escape".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "delegate submit".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("message");
        assert!(
            message
                .content_text
                .contains("sandbox path must not escape")
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_delegate_task_waits_for_child_submit_result() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-delegate");

        let child_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_child_submit\",\"function\":{\"name\":\"submit_delegate_result\",\"arguments\":\"{\\\"summary\\\":\\\"child done\\\",\\\"findings\\\":[\\\"verified\\\"],\\\"changed_files\\\":[\\\"src/lib.rs\\\"],\\\"risks\\\":[],\\\"next_steps\\\":[]}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let child_payload_json = serde_json::json!({"sse": child_sse}).to_string();
        let delegate_args = serde_json::json!({
            "task": "inspect a narrow implementation detail",
            "timeout_ms": 30_000,
            "payload_json": child_payload_json
        })
        .to_string();
        let escaped_delegate_args = delegate_args.replace('\\', "\\\\").replace('"', "\\\"");
        let parent_sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_delegate_task\",\"function\":{{\"name\":\"delegate_task\",\"arguments\":\"{escaped_delegate_args}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
             data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_delegate_task".to_string(),
            idempotency_key: "message:m7:delegate-task".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "delegate work".to_string(),
            payload_json: serde_json::json!({"sse": parent_sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 50);
        assert!(
            timeline
                .items
                .iter()
                .any(|item| item.kind == "ToolTrace" && item.trace_title == "Delegate session")
        );
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("message");
        assert!(message.content_text.contains("child done"));
        assert!(message.content_text.contains("verified"));
        assert!(message.content_text.contains("changedFiles"));

        let sessions = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(sessions.selected_session_id, session_id);
        assert!(
            sessions
                .sessions
                .iter()
                .any(|session| session.title.starts_with("Delegate: inspect"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_submit_delegate_result_copies_child_artifacts_to_parent_workspace() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let child_snapshot = runtime
            .tokio
            .block_on(runtime.database.create_session("Delegate: artifact"))
            .expect("create child");
        let delegate_session_id = child_snapshot.selected_session_id;
        runtime
            .tokio
            .block_on(runtime.database.open_session(&session_id))
            .expect("restore parent");
        runtime
            .sandbox
            .prepare_session(&delegate_session_id)
            .expect("prepare child sandbox");
        let child_report = runtime
            .sandbox
            .resolve(
                &delegate_session_id,
                "/var/hambur/workspace/report.txt",
                SandboxAccess::Write,
            )
            .expect("child report path");
        fs::write(&child_report.host_path, "delegate report").expect("write child report");
        let (sender, receiver) = oneshot::channel();
        let trace = runtime
            .tokio
            .block_on(runtime.database.insert_trace_span(NewTraceSpan {
                session_id: session_id.clone(),
                kind: "tool".to_string(),
                title: "Delegate session".to_string(),
                content: format!("delegateSessionId={delegate_session_id}"),
                status: "running".to_string(),
                tool_call_id: "call_delegate_artifact".to_string(),
                visible: true,
                ..Default::default()
            }))
            .expect("delegate trace");
        runtime
            .delegate_tasks
            .lock()
            .expect("delegate registry")
            .insert(
                delegate_session_id.clone(),
                DelegateTaskState {
                    parent_session_id: session_id.clone(),
                    parent_turn_id: "turn_parent".to_string(),
                    child_session_id: delegate_session_id.clone(),
                    trace_id: trace.id,
                    sender,
                },
            );

        let invocation = ToolInvocation::from_model_call(
            0,
            "call_child_submit_artifact".to_string(),
            "turn_child".to_string(),
            delegate_session_id.clone(),
            "submit_delegate_result".to_string(),
            serde_json::json!({
                "summary": "artifact ready",
                "artifact_paths": ["/var/hambur/workspace/report.txt"]
            })
            .to_string(),
        )
        .expect("invocation");
        let result = runtime
            .tokio
            .block_on(runtime.resolve_submit_delegate_result(
                &delegate_session_id,
                &invocation,
                &invocation.arguments_value().expect("arguments"),
            ));
        assert!(!result.is_error, "submit failed: {}", result.summary);
        let completion = receiver
            .blocking_recv()
            .expect("delegate completion payload");
        assert_eq!(completion.summary, "artifact ready");

        let parent_path =
            format!("/var/hambur/workspace/delegates/{delegate_session_id}/report.txt");
        assert!(result.content_json.contains(&parent_path));
        let parent_report = runtime
            .sandbox
            .resolve(&session_id, &parent_path, SandboxAccess::Read)
            .expect("parent report path");
        assert_eq!(
            fs::read_to_string(parent_report.host_path).expect("read copied report"),
            "delegate report"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_terminal_uses_sandbox_path_policy_before_execution() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-terminal");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_terminal\",\"function\":{\"name\":\"terminal\",\"arguments\":\"{\\\"command\\\":\\\"echo no\\\",\\\"cwd\\\":\\\"/var/hambur/workspace/../memory\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_terminal_escape".to_string(),
            idempotency_key: "message:m7:terminal-escape".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "run terminal".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("message");
        assert!(
            message
                .content_text
                .contains("sandbox path must not escape")
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_process_wait_returns_background_output_and_removes_finished_session() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let mut child = Command::new(platform_shell())
            .arg("-lc")
            .arg("printf process-ready")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn background process");
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::default()));
        if let Some(stdout) = child.stdout.take() {
            spawn_process_pipe_reader(stdout, output.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_process_pipe_reader(stderr, output.clone(), false);
        }
        let process_session_id = new_id("proc");
        runtime
            .process_sessions
            .lock()
            .expect("process registry")
            .insert(
                process_session_id.clone(),
                BackgroundProcessSession {
                    session_id: session_id.clone(),
                    process_session_id: process_session_id.clone(),
                    backend: "host-test".to_string(),
                    command: "printf process-ready".to_string(),
                    cwd: "/var/hambur/workspace".to_string(),
                    started_at_ms: now_ms(),
                    pid: child.id(),
                    pid_file: None,
                    child,
                    output,
                    exit_code: None,
                    finished_at_ms: 0,
                },
            );

        let invocation = ToolInvocation::from_model_call(
            0,
            "call_process_wait".to_string(),
            "turn_process".to_string(),
            session_id.clone(),
            "process".to_string(),
            serde_json::json!({
                "action": "wait",
                "process_session_id": process_session_id,
                "timeout_ms": 30_000
            })
            .to_string(),
        )
        .expect("process invocation");
        let result = runtime
            .resolve_process_tool_result(&invocation, &invocation.arguments_value().unwrap());
        assert!(!result.is_error, "process wait failed: {}", result.summary);
        assert!(result.context_stub.contains("process-ready"));
        assert!(result.context_stub.contains("processSessionId"));
        assert!(
            runtime
                .process_sessions
                .lock()
                .expect("process registry")
                .is_empty()
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_web_fetch_returns_untrusted_http_content() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let body = "hello from local web";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-web");

        let url = format!("http://{addr}/page");
        let sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_web_fetch\",\"function\":{{\"name\":\"web_fetch\",\"arguments\":\"{{\\\"urls\\\":[\\\"{}\\\"]}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
            url
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_web_fetch".to_string(),
            idempotency_key: "message:m7:web-fetch".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "fetch local web".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");
        server.join().expect("server thread");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("message");
        assert!(message.content_text.contains("hello from local web"));
        assert!(message.content_text.contains("<untrusted_tool_result"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_web_fetch_accepts_https_urls_for_tls_provider_client() {
        assert!(validate_web_fetch_url("https://example.test/path").is_ok());
        assert!(validate_web_fetch_url("http://example.test/path").is_ok());
        assert!(validate_web_fetch_url("file:///tmp/nope").is_err());
    }

    #[test]
    fn session_rename_and_pin_are_persisted_and_sorted_first() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let first = create_test_session(&runtime);
        let second = create_test_session(&runtime);

        let rename = runtime.dispatch(RuntimeCommand {
            idempotency_key: "rename:first".to_string(),
            kind: "RenameSession".to_string(),
            session_id: first.clone(),
            title: "renamed first".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(rename.accepted, "rename rejected: {}", rename.message);
        let _ = runtime.next_event().expect("rename event");

        let pin = runtime.dispatch(RuntimeCommand {
            idempotency_key: "pin:first".to_string(),
            kind: "SetSessionPinned".to_string(),
            session_id: first.clone(),
            payload_json: serde_json::json!({"pinned": true}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(pin.accepted, "pin rejected: {}", pin.message);
        let _ = runtime.next_event().expect("pin event");

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(
            snapshot.sessions.first().map(|session| session.id.as_str()),
            Some(first.as_str())
        );
        let first_summary = snapshot
            .sessions
            .iter()
            .find(|session| session.id == first)
            .expect("first session");
        assert_eq!(first_summary.title, "renamed first");
        assert!(first_summary.pinned_at_ms > 0);
        assert!(snapshot.sessions.iter().any(|session| session.id == second));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn message_snapshot_contains_attached_attachment_records() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_attach_dto", "model-attach-dto");

        let import = runtime.dispatch(RuntimeCommand {
            idempotency_key: "attachment:dto:import".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "note.txt",
                "mimeType": "text/plain",
                "base64": base64::engine::general_purpose::STANDARD.encode("hello attachment")
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted, "import rejected: {}", import.message);
        let imported = runtime.next_event().expect("import event");
        let attachment_id = imported
            .snapshot
            .pending_attachments
            .first()
            .expect("pending attachment")
            .id
            .clone();

        let send = runtime.dispatch(RuntimeCommand {
            idempotency_key: "attachment:dto:send".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "see attached".to_string(),
            payload_json: serde_json::json!({
                "attachmentIds": [attachment_id],
                "content": "ok"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_session_finished(&runtime, &session_id, "attachment dto");

        let user_item = runtime
            .get_session_snapshot(session_id)
            .timeline_items
            .into_iter()
            .find(|item| item.kind == "UserMessage")
            .expect("user message item");
        let message = runtime
            .get_message_snapshot(user_item.payload_ref)
            .message
            .expect("message snapshot");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].display_name, "note.txt");
        assert_eq!(message.attachments[0].status, "attached");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    fn temp_app_dir() -> PathBuf {
        unsafe {
            std::env::set_var("HAMBUR_TEST_MOCK_ROOTFS", "1");
        }
        std::env::temp_dir().join(new_id("hambur_runtime_test"))
    }

    fn create_test_session(runtime: &RuntimeEngine) -> String {
        let ack = runtime.create_session("Chat".to_string());
        assert!(ack.accepted, "create rejected: {}", ack.message);
        let event = runtime.next_event().expect("session created");
        assert_eq!(event.kind.as_str(), "SessionCreated");
        event.snapshot.selected_session_id
    }

    fn configure_test_provider(runtime: &RuntimeEngine, model_id: &str) {
        configure_named_test_provider(runtime, "provider_test", model_id);
    }

    fn configure_named_test_provider(runtime: &RuntimeEngine, provider_id: &str, model_id: &str) {
        configure_named_models(runtime, provider_id, &[(model_id, false)]);
    }

    fn configure_named_models(runtime: &RuntimeEngine, provider_id: &str, models: &[(&str, bool)]) {
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_good_{provider_id}"),
            idempotency_key: format!("{provider_id}:update:test"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("Test Provider {provider_id}"),
            chunk: "https://api.test/v1".to_string(),
            payload_json: format!("android-secret://provider/{provider_id}"),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let data = models
            .iter()
            .map(|(model_id, supports_image_input)| {
                serde_json::json!({
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": true,
                    "supports_tool_call": true,
                    "supports_image_input": supports_image_input,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 4096
                })
            })
            .collect::<Vec<_>>();
        let models_json = serde_json::json!({"data": data}).to_string();
        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_{provider_id}"),
            idempotency_key: format!("{provider_id}:models:test"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: models_json,
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    fn configure_http_test_provider(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
    ) {
        // Tests use env:// so the database still stores only a secret reference.
        unsafe {
            std::env::set_var("HAMBUR_TEST_OPENAI_KEY", "test-api-key");
        }
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_http_{provider_id}"),
            idempotency_key: format!("{provider_id}:http-provider:test"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("HTTP Provider {provider_id}"),
            chunk: base_url.to_string(),
            payload_json: "env://HAMBUR_TEST_OPENAI_KEY".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_http_{provider_id}"),
            idempotency_key: format!("{provider_id}:http-models:test"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: serde_json::json!({
                "data": [{
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": true,
                    "supports_tool_call": true,
                    "supports_image_input": false,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 4096
                }]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    struct RealProviderTestConfig {
        secret_env: String,
        base_url: String,
        model_id: String,
    }

    impl RealProviderTestConfig {
        fn from_env() -> Self {
            let secret_env = "HAMBUR_E2E_OPENAI_API_KEY".to_string();
            let api_key = std::env::var(&secret_env)
                .expect("set HAMBUR_E2E_OPENAI_API_KEY to run this ignored integration test");
            assert!(!api_key.trim().is_empty(), "API key must not be empty");
            Self {
                secret_env,
                base_url: std::env::var("HAMBUR_E2E_OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://opencode.ai/zen/v1".to_string()),
                model_id: std::env::var("HAMBUR_E2E_OPENAI_MODEL")
                    .unwrap_or_else(|_| "deepseek-v4-flash-free".to_string()),
            }
        }
    }

    fn configure_env_secret_provider(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
        secret_env: &str,
    ) {
        configure_env_secret_provider_with_capabilities(
            runtime,
            provider_id,
            model_id,
            base_url,
            secret_env,
            false,
            false,
        );
    }

    fn configure_env_secret_provider_with_capabilities(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
        secret_env: &str,
        supports_tool_call: bool,
        supports_image_input: bool,
    ) {
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_env_{provider_id}"),
            idempotency_key: format!("{provider_id}:env-provider:e2e"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("E2E Provider {provider_id}"),
            chunk: base_url.to_string(),
            payload_json: format!("env://{secret_env}"),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_env_{provider_id}"),
            idempotency_key: format!("{provider_id}:env-models:e2e"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: serde_json::json!({
                "data": [{
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": false,
                    "supports_tool_call": supports_tool_call,
                    "supports_image_input": supports_image_input,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 1024
                }]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    fn wait_for_event(runtime: &RuntimeEngine, kind: &str) {
        for _ in 0..64 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind {
                return;
            }
        }
        panic!("missing event: {kind}");
    }

    fn next_event_with_timeout(runtime: &RuntimeEngine, label: &str) -> RuntimeEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(mut receiver) = runtime.receiver.lock()
                && let Ok(event) = receiver.try_recv()
            {
                return event;
            }
            if Instant::now() >= deadline {
                panic!("timed out waiting for event: {label}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn assistant_markdown_text(runtime: &RuntimeEngine, session_id: &str) -> String {
        let timeline = runtime.get_timeline_page(session_id.to_string(), 0, 100);
        let payload_ids = timeline
            .items
            .iter()
            .filter(|item| item.content_type == "assistant_markdown_block")
            .map(|item| item.payload_ref.as_str())
            .collect::<HashSet<_>>();
        timeline
            .markdown_block_payloads
            .iter()
            .filter(|payload| payload_ids.contains(payload.id.as_str()))
            .map(|payload| payload.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_for_session_event(runtime: &RuntimeEngine, kind: &str, session_id: &str) {
        for _ in 0..128 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind && event.session_id == session_id {
                return;
            }
        }
        panic!("missing event: {kind} for session {session_id}");
    }

    fn wait_for_session_finished(runtime: &RuntimeEngine, session_id: &str, label: &str) {
        for _ in 0..256 {
            let event = runtime.next_event().expect("runtime event");
            if event.session_id != session_id {
                continue;
            }
            match event.kind.as_str() {
                "TurnFinished" => return,
                "TurnFailed" | "TurnCancelled" => {
                    panic!(
                        "{label} ended as {}: {} {}",
                        event.kind.as_str(),
                        event.error_code,
                        event.message
                    );
                }
                _ => {}
            }
        }
        panic!("missing TurnFinished for {label} in session {session_id}");
    }

    fn wait_for_session_event_result(
        runtime: &RuntimeEngine,
        kind: &str,
        session_id: &str,
    ) -> super::RuntimeEvent {
        for _ in 0..128 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind && event.session_id == session_id {
                return event;
            }
        }
        panic!("missing event: {kind} for session {session_id}");
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            size += dir_size(&entry.path());
        }
    }
    size
}

fn get_rootfs_backend(settings: &[hambur_db::AppSettingRecord]) -> &str {
    for s in settings {
        if s.key == "rootfsBackend" || s.key == "rootfs_setting:rootfsBackend" {
            let val = s.value.trim_matches('"');
            if val == "chroot" || val == "proot" {
                return val;
            }
        }
    }
    "proot"
}
