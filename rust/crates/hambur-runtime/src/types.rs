use crate::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeEventKind {
    RuntimeReady,
    RuntimeClosed,
    RuntimeError,
    SessionCreated,
    SessionOpened,
    SessionDeleted,
    SessionRenamed,
    SessionPinnedChanged,
    ModelsUpdated,
    SettingsChanged,
    AttachmentImported,
    PendingAttachmentRemoved,
    PendingAttachmentsCleaned,
    TurnStarted,
    TurnStateChanged,
    TurnFinished,
    TurnFailed,
    TurnCancelled,
    MessageUpserted,
    AssistantMessageStarted,
    AssistantMessageFinished,
    AssistantReasoningDelta,
    MarkdownRenderUpdate,
    ToolCallStarted,
    ToolCallDelta,
    ToolCallFinished,
    ToolCallFailed,
    PlatformRequest,
    PlatformRequestTimedOut,
}

impl RuntimeEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RuntimeReady => "RuntimeReady",
            Self::RuntimeClosed => "RuntimeClosed",
            Self::RuntimeError => "RuntimeError",
            Self::SessionCreated => "SessionCreated",
            Self::SessionOpened => "SessionOpened",
            Self::SessionDeleted => "SessionDeleted",
            Self::SessionRenamed => "SessionRenamed",
            Self::SessionPinnedChanged => "SessionPinnedChanged",
            Self::ModelsUpdated => "ModelsUpdated",
            Self::SettingsChanged => "SettingsChanged",
            Self::AttachmentImported => "AttachmentImported",
            Self::PendingAttachmentRemoved => "PendingAttachmentRemoved",
            Self::PendingAttachmentsCleaned => "PendingAttachmentsCleaned",
            Self::TurnStarted => "TurnStarted",
            Self::TurnStateChanged => "TurnStateChanged",
            Self::TurnFinished => "TurnFinished",
            Self::TurnFailed => "TurnFailed",
            Self::TurnCancelled => "TurnCancelled",
            Self::MessageUpserted => "MessageUpserted",
            Self::AssistantMessageStarted => "AssistantMessageStarted",
            Self::AssistantMessageFinished => "AssistantMessageFinished",
            Self::AssistantReasoningDelta => "AssistantReasoningDelta",
            Self::MarkdownRenderUpdate => "MarkdownRenderUpdate",
            Self::ToolCallStarted => "ToolCallStarted",
            Self::ToolCallDelta => "ToolCallDelta",
            Self::ToolCallFinished => "ToolCallFinished",
            Self::ToolCallFailed => "ToolCallFailed",
            Self::PlatformRequest => "PlatformRequest",
            Self::PlatformRequestTimedOut => "PlatformRequestTimedOut",
        }
    }

    /// High-frequency streaming events carry no snapshot and may be coalesced by the UI.
    pub fn is_stream_delta(&self) -> bool {
        matches!(
            self,
            Self::AssistantReasoningDelta | Self::MarkdownRenderUpdate | Self::ToolCallDelta
        )
    }
}

/// Typed event payload. Each event kind carries only the data the UI needs for it:
/// structural changes carry a session snapshot, streaming deltas carry just the delta.
#[derive(Debug, Clone)]
pub enum RuntimeEventPayload {
    None,
    Snapshot { snapshot: AppSnapshot },
    Delta { message_id: String, delta: String },
    Markdown { update: MarkdownRenderUpdate },
    Attachments { attachments: Vec<AttachmentRecord> },
    PlatformRequest { request: PlatformRequest },
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
    pub payload: RuntimeEventPayload,
    pub error_code: String,
    pub message: String,
}
