use std::sync::Arc;

use hambur_core::{new_id, now_ms};
use hambur_db::{
    AppSnapshot, AttachmentRecord, MessageRecord, SessionSummary, TimelineItemSnapshot,
};
use hambur_markdown::{
    MarkdownBlockNode, MarkdownInlineNode, MarkdownRenderUpdate, MarkdownTableRow,
};
use hambur_runtime::{
    AppBootstrap, RuntimeCommand, RuntimeCommandAck, RuntimeEngine, RuntimeEvent,
    RuntimeMessageSnapshot, RuntimeSearchSnapshot, RuntimeSessionListSnapshot,
    RuntimeSessionSnapshot, RuntimeTimelinePage,
};

uniffi::include_scaffolding!("hambur_uniffi");

pub struct AppBootstrapConfig {
    pub app_files_dir: String,
}

pub struct CommandAck {
    pub command_id: String,
    pub idempotency_key: String,
    pub accepted: bool,
    pub duplicate: bool,
    pub rejection_code: String,
    pub message: String,
}

pub struct BackendCommand {
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

pub struct BackendEvent {
    pub event_id: String,
    pub schema_version: u32,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub kind: String,
    pub session_id: String,
    pub turn_id: String,
    pub snapshot: AppSnapshotDTO,
    pub markdown_render_update: MarkdownRenderUpdateDTO,
    pub error_code: String,
    pub message: String,
}

pub struct MarkdownInlineNodeDTO {
    pub kind: String,
    pub text: String,
    pub destination: String,
    pub title: String,
    pub alt: String,
    pub children: Vec<MarkdownInlineNodeDTO>,
}

pub struct MarkdownTableRowDTO {
    pub cells: Vec<String>,
}

pub struct MarkdownBlockNodeDTO {
    pub message_id: String,
    pub block_id: u64,
    pub stable_key: String,
    pub source_kind: String,
    pub node_kind: String,
    pub committed: bool,
    pub level: u8,
    pub inlines: Vec<MarkdownInlineNodeDTO>,
    pub language: String,
    pub text: String,
    pub raw: String,
    pub children_json: String,
    pub items_json: String,
    pub table_header: Vec<String>,
    pub table_rows: Vec<MarkdownTableRowDTO>,
    pub path: String,
    pub file_kind: String,
}

pub struct MarkdownRenderUpdateDTO {
    pub message_id: String,
    pub reset: bool,
    pub committed_nodes: Vec<MarkdownBlockNodeDTO>,
    pub pending_node: Option<MarkdownBlockNodeDTO>,
    pub invalidated_block_ids: Vec<u64>,
}

pub struct SessionSummaryDTO {
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub message_count: u32,
    pub latest_preview: String,
}

pub struct TimelineItemDTO {
    pub id: String,
    pub stable_key: String,
    pub content_type: String,
    pub display_sequence: u64,
    pub version_sequence: u64,
    pub payload_ref: String,
    pub small_summary: String,
    pub kind: String,
    pub trace_title: String,
    pub trace_content: String,
    pub trace_status: String,
    pub tool_call_id: String,
    pub tool_name: String,
}

pub struct AttachmentDTO {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub kind: String,
    pub display_name: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub origin_type: String,
    pub original_uri: String,
    pub file_id: String,
    pub sandbox_path: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub struct AppSnapshotDTO {
    pub sessions: Vec<SessionSummaryDTO>,
    pub selected_session_id: String,
    pub timeline_items: Vec<TimelineItemDTO>,
    pub pending_attachments: Vec<AttachmentDTO>,
}

pub struct SessionListSnapshotDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub sessions: Vec<SessionSummaryDTO>,
    pub selected_session_id: String,
}

pub struct SessionSnapshotDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session: Option<SessionSummaryDTO>,
    pub timeline_items: Vec<TimelineItemDTO>,
}

pub struct TimelinePageDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session_id: String,
    pub items: Vec<TimelineItemDTO>,
    pub next_before_cursor: u64,
    pub has_more: bool,
}

pub struct MessageDTO {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content_text: String,
    pub reasoning_content: String,
    pub status: String,
    pub turn_id: String,
    pub created_at_ms: u64,
    pub version_sequence: u64,
    pub provider_id_snapshot: String,
    pub provider_name_snapshot: String,
    pub provider_protocol: String,
    pub model_id_snapshot: String,
    pub model_name_snapshot: String,
    pub model_group_id: String,
    pub finish_reason: String,
    pub native_finish_reason: String,
}

pub struct MessageSnapshotDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub message: Option<MessageDTO>,
}

pub struct SearchSnapshotDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub query: String,
    pub sessions: Vec<SessionSummaryDTO>,
}

pub struct BackendRuntime {
    engine: Arc<RuntimeEngine>,
}

pub fn create_runtime(config: AppBootstrapConfig) -> Arc<BackendRuntime> {
    let bootstrap = AppBootstrap {
        app_files_dir: config.app_files_dir,
    };

    match RuntimeEngine::create(bootstrap) {
        Ok(engine) => Arc::new(BackendRuntime { engine }),
        Err(error) => {
            let fallback = RuntimeEngine::create(AppBootstrap {
                app_files_dir: ".".to_string(),
            })
            .expect("fallback runtime must be constructible");
            let _ = fallback.app_files_dir();
            let _ = error;
            Arc::new(BackendRuntime { engine: fallback })
        }
    }
}

impl BackendRuntime {
    pub fn dispatch(&self, command: BackendCommand) -> CommandAck {
        self.engine.dispatch(command.into()).into()
    }

    pub fn next_event(&self) -> Option<BackendEvent> {
        self.engine.next_event().map(BackendEvent::from)
    }

    pub fn get_session_list_snapshot(&self, limit: u32, offset: u32) -> SessionListSnapshotDTO {
        self.engine.get_session_list_snapshot(limit, offset).into()
    }

    pub fn get_session_snapshot(&self, session_id: String) -> SessionSnapshotDTO {
        self.engine.get_session_snapshot(session_id).into()
    }

    pub fn get_timeline_page(
        &self,
        session_id: String,
        before_cursor: u64,
        limit: u32,
    ) -> TimelinePageDTO {
        self.engine
            .get_timeline_page(session_id, before_cursor, limit)
            .into()
    }

    pub fn get_message_snapshot(&self, message_id: String) -> MessageSnapshotDTO {
        self.engine.get_message_snapshot(message_id).into()
    }

    pub fn search_sessions(&self, query: String, limit: u32) -> SearchSnapshotDTO {
        self.engine.search_sessions(query, limit).into()
    }

    pub fn create_session(&self, title: String) -> CommandAck {
        self.engine.create_session(title).into()
    }

    pub fn open_session(&self, session_id: String) -> CommandAck {
        self.engine.open_session(session_id).into()
    }

    pub fn delete_session(&self, session_id: String) -> CommandAck {
        self.engine.delete_session(session_id).into()
    }

    pub fn append_markdown_delta(
        &self,
        session_id: String,
        message_id: String,
        chunk: String,
        finalize: bool,
    ) -> CommandAck {
        self.engine
            .append_markdown_delta(session_id, message_id, chunk, finalize)
            .into()
    }

    pub fn shutdown(&self) {
        self.engine.shutdown();
    }
}

impl From<RuntimeCommandAck> for CommandAck {
    fn from(value: RuntimeCommandAck) -> Self {
        Self {
            command_id: value.command_id,
            idempotency_key: value.idempotency_key,
            accepted: value.accepted,
            duplicate: value.duplicate,
            rejection_code: value.rejection_code,
            message: value.message,
        }
    }
}

impl From<BackendCommand> for RuntimeCommand {
    fn from(value: BackendCommand) -> Self {
        Self {
            command_id: if value.command_id.trim().is_empty() {
                new_id("cmd")
            } else {
                value.command_id
            },
            idempotency_key: value.idempotency_key,
            created_at_ms: if value.created_at_ms == 0 {
                now_ms()
            } else {
                value.created_at_ms
            },
            kind: value.kind,
            session_id: value.session_id,
            turn_id: value.turn_id,
            title: value.title,
            message_id: value.message_id,
            chunk: value.chunk,
            content: value.content,
            reasoning: value.reasoning,
            provider_id: value.provider_id,
            model_id: value.model_id,
            source_message_id: value.source_message_id,
            payload_json: value.payload_json,
            finalize: value.finalize,
        }
    }
}

impl From<RuntimeEvent> for BackendEvent {
    fn from(value: RuntimeEvent) -> Self {
        Self {
            event_id: value.event_id,
            schema_version: value.schema_version,
            sequence: value.sequence,
            created_at_ms: value.created_at_ms,
            kind: value.kind.as_str().to_string(),
            session_id: value.session_id,
            turn_id: value.turn_id,
            snapshot: value.snapshot.into(),
            markdown_render_update: value.markdown_render_update.into(),
            error_code: value.error_code,
            message: value.message,
        }
    }
}

impl From<RuntimeSessionListSnapshot> for SessionListSnapshotDTO {
    fn from(value: RuntimeSessionListSnapshot) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            sessions: value
                .sessions
                .into_iter()
                .map(SessionSummaryDTO::from)
                .collect(),
            selected_session_id: value.selected_session_id,
        }
    }
}

impl From<RuntimeSessionSnapshot> for SessionSnapshotDTO {
    fn from(value: RuntimeSessionSnapshot) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            session: value.session.map(SessionSummaryDTO::from),
            timeline_items: value
                .timeline_items
                .into_iter()
                .map(TimelineItemDTO::from)
                .collect(),
        }
    }
}

impl From<RuntimeTimelinePage> for TimelinePageDTO {
    fn from(value: RuntimeTimelinePage) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            session_id: value.session_id,
            items: value.items.into_iter().map(TimelineItemDTO::from).collect(),
            next_before_cursor: value.next_before_cursor,
            has_more: value.has_more,
        }
    }
}

impl From<RuntimeMessageSnapshot> for MessageSnapshotDTO {
    fn from(value: RuntimeMessageSnapshot) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            message: value.message.map(MessageDTO::from),
        }
    }
}

impl From<RuntimeSearchSnapshot> for SearchSnapshotDTO {
    fn from(value: RuntimeSearchSnapshot) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            query: value.query,
            sessions: value
                .sessions
                .into_iter()
                .map(SessionSummaryDTO::from)
                .collect(),
        }
    }
}

impl From<MarkdownInlineNode> for MarkdownInlineNodeDTO {
    fn from(value: MarkdownInlineNode) -> Self {
        Self {
            kind: value.kind,
            text: value.text,
            destination: value.destination,
            title: value.title,
            alt: value.alt,
            children: value
                .children
                .into_iter()
                .map(MarkdownInlineNodeDTO::from)
                .collect(),
        }
    }
}

impl From<MarkdownTableRow> for MarkdownTableRowDTO {
    fn from(value: MarkdownTableRow) -> Self {
        Self { cells: value.cells }
    }
}

impl From<MarkdownBlockNode> for MarkdownBlockNodeDTO {
    fn from(value: MarkdownBlockNode) -> Self {
        Self {
            message_id: value.message_id,
            block_id: value.block_id,
            stable_key: value.stable_key,
            source_kind: value.source_kind,
            node_kind: value.node_kind,
            committed: value.committed,
            level: value.level,
            inlines: value
                .inlines
                .into_iter()
                .map(MarkdownInlineNodeDTO::from)
                .collect(),
            language: value.language,
            text: value.text,
            raw: value.raw,
            children_json: value.children_json,
            items_json: value.items_json,
            table_header: value.table_header,
            table_rows: value
                .table_rows
                .into_iter()
                .map(MarkdownTableRowDTO::from)
                .collect(),
            path: value.path,
            file_kind: value.file_kind,
        }
    }
}

impl From<MarkdownRenderUpdate> for MarkdownRenderUpdateDTO {
    fn from(value: MarkdownRenderUpdate) -> Self {
        Self {
            message_id: value.message_id,
            reset: value.reset,
            committed_nodes: value
                .committed_nodes
                .into_iter()
                .map(MarkdownBlockNodeDTO::from)
                .collect(),
            pending_node: value.pending_node.map(MarkdownBlockNodeDTO::from),
            invalidated_block_ids: value.invalidated_block_ids,
        }
    }
}

impl From<AppSnapshot> for AppSnapshotDTO {
    fn from(value: AppSnapshot) -> Self {
        Self {
            sessions: value
                .sessions
                .into_iter()
                .map(SessionSummaryDTO::from)
                .collect(),
            selected_session_id: value.selected_session_id,
            timeline_items: value
                .timeline_items
                .into_iter()
                .map(TimelineItemDTO::from)
                .collect(),
            pending_attachments: value
                .pending_attachments
                .into_iter()
                .map(AttachmentDTO::from)
                .collect(),
        }
    }
}

impl From<SessionSummary> for SessionSummaryDTO {
    fn from(value: SessionSummary) -> Self {
        Self {
            id: value.id,
            title: value.title,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            message_count: value.message_count,
            latest_preview: value.latest_preview,
        }
    }
}

impl From<TimelineItemSnapshot> for TimelineItemDTO {
    fn from(value: TimelineItemSnapshot) -> Self {
        Self {
            id: value.id,
            stable_key: value.stable_key,
            content_type: value.content_type,
            display_sequence: value.display_sequence,
            version_sequence: value.version_sequence,
            payload_ref: value.payload_ref,
            small_summary: value.small_summary,
            kind: value.kind,
            trace_title: value.trace_title,
            trace_content: value.trace_content,
            trace_status: value.trace_status,
            tool_call_id: value.tool_call_id,
            tool_name: value.tool_name,
        }
    }
}

impl From<AttachmentRecord> for AttachmentDTO {
    fn from(value: AttachmentRecord) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            message_id: value.message_id,
            kind: value.kind,
            display_name: value.display_name,
            mime_type: value.mime_type,
            byte_size: value.byte_size,
            origin_type: value.origin_type,
            original_uri: value.original_uri,
            file_id: value.file_id,
            sandbox_path: value.sandbox_path,
            width: value.width,
            height: value.height,
            sha256: value.sha256,
            status: value.status,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<MessageRecord> for MessageDTO {
    fn from(value: MessageRecord) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            role: value.role,
            content_text: value.content_text,
            reasoning_content: value.reasoning_content,
            status: value.status,
            turn_id: value.turn_id,
            created_at_ms: value.created_at_ms,
            version_sequence: value.version_sequence,
            provider_id_snapshot: value.provider_id_snapshot,
            provider_name_snapshot: value.provider_name_snapshot,
            provider_protocol: value.provider_protocol,
            model_id_snapshot: value.model_id_snapshot,
            model_name_snapshot: value.model_name_snapshot,
            model_group_id: value.model_group_id,
            finish_reason: value.finish_reason,
            native_finish_reason: value.native_finish_reason,
        }
    }
}
