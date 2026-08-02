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
    AppSnapshot, AttachmentRecord, HamburDatabase, MessageBlockPayloadRecord, MessageRecord,
    ModelRouteSnapshot, NewAttachment, NewFileRecord, NewMessageBlockPayload, NewTimelineItem,
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
use hambur_markdown::{MarkdownBlockNode, MarkdownPipeline, MarkdownRenderUpdate};
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

const MODEL_CATALOG_CACHE_KEY: &str = "models_dev_api";
const MODEL_CATALOG_CACHE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

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
    pub message_block_payloads: Vec<MessageBlockPayloadRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeTimelinePage {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session_id: String,
    pub items: Vec<TimelineItemSnapshot>,
    pub message_block_payloads: Vec<MessageBlockPayloadRecord>,
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
    completed_process_sessions: Mutex<HashMap<String, CompletedProcessSession>>,
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

#[derive(Debug, Clone)]
struct CompletedProcessSession {
    session_id: String,
    process_session_id: String,
    backend: String,
    command: String,
    cwd: String,
    started_at_ms: u64,
    pid: u32,
    output: ProcessOutputSnapshot,
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
    thinking_raw_trace_count: u32,
    thinking_parsed_trace_count: u32,
    semantic_delta_started: bool,
    finish_reason: String,
    native_finish_reason: String,
    saw_tool_delta: bool,
    tool_accumulator: ToolCallAccumulator,
    complete_tool_calls: Vec<CompleteToolCall>,
}

mod commands;
mod engine;
mod queries;
mod utils;
pub(crate) use utils::*;

pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
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

pub(crate) fn get_rootfs_backend(settings: &[hambur_db::AppSettingRecord]) -> &str {
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
