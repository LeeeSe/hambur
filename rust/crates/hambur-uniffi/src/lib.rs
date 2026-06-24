use std::sync::Arc;

use hambur_core::{new_id, now_ms};
use hambur_db::{
    AppSettingRecord, AppSnapshot, AttachmentRecord, ConfigAuditRecord, DefaultModelGroupRecord,
    MessageRecord, ModelGroupMemberRecord, ModelGroupRecord, ProviderModelRecord,
    PublicProviderRecord, SessionSummary, TimelineItemSnapshot,
};
use hambur_markdown::{
    MarkdownBlockNode, MarkdownInlineNode, MarkdownRenderUpdate, MarkdownTableRow,
};
use hambur_runtime::{
    AppBootstrap, RuntimeCommand, RuntimeCommandAck, RuntimeEngine, RuntimeEvent,
    RuntimeMessageSnapshot, RuntimeSearchSnapshot, RuntimeSessionListSnapshot,
    RuntimeSessionSnapshot, RuntimeSettingsSnapshot, RuntimeTimelinePage,
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

pub struct PublicProviderDTO {
    pub id: String,
    pub name: String,
    pub icon_name: String,
    pub api_type: String,
    pub base_url: String,
    pub secret_label: String,
    pub enabled: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub struct ProviderModelDTO {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub supports_tool_call: bool,
    pub supports_reasoning: bool,
    pub supports_image_input: bool,
    pub supports_structured_output: bool,
    pub supports_temperature: bool,
    pub context_limit: u32,
    pub output_limit: u32,
    pub reasoning_field: String,
    pub metadata_json: String,
    pub synced_at_ms: u64,
}

pub struct ModelGroupDTO {
    pub id: String,
    pub name: String,
    pub routing_strategy: String,
    pub fallback_policy: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub struct ModelGroupMemberDTO {
    pub id: String,
    pub group_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_display_name: String,
    pub position: u32,
    pub enabled: bool,
}

pub struct DefaultModelGroupDTO {
    pub key: String,
    pub group_id: String,
    pub updated_at_ms: u64,
}

pub struct AppSettingDTO {
    pub key: String,
    pub value: String,
    pub updated_at_ms: u64,
}

pub struct ConfigAuditDTO {
    pub id: String,
    pub command_id: String,
    pub actor: String,
    pub action: String,
    pub target_kind: String,
    pub target_id: String,
    pub redacted_summary: String,
    pub approval_required: bool,
    pub approval_token: String,
    pub created_at_ms: u64,
}

pub struct SettingsSnapshotDTO {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub providers: Vec<PublicProviderDTO>,
    pub provider_models: Vec<ProviderModelDTO>,
    pub model_groups: Vec<ModelGroupDTO>,
    pub model_group_members: Vec<ModelGroupMemberDTO>,
    pub default_model_groups: Vec<DefaultModelGroupDTO>,
    pub settings: Vec<AppSettingDTO>,
    pub config_audits: Vec<ConfigAuditDTO>,
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

    pub fn get_settings_snapshot(&self) -> SettingsSnapshotDTO {
        self.engine.get_settings_snapshot().into()
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

impl From<RuntimeSettingsSnapshot> for SettingsSnapshotDTO {
    fn from(value: RuntimeSettingsSnapshot) -> Self {
        Self {
            snapshot_sequence: value.snapshot_sequence,
            created_at_ms: value.created_at_ms,
            providers: value
                .settings
                .providers
                .into_iter()
                .map(PublicProviderDTO::from)
                .collect(),
            provider_models: value
                .settings
                .provider_models
                .into_iter()
                .map(ProviderModelDTO::from)
                .collect(),
            model_groups: value
                .settings
                .model_groups
                .into_iter()
                .map(ModelGroupDTO::from)
                .collect(),
            model_group_members: value
                .settings
                .model_group_members
                .into_iter()
                .map(ModelGroupMemberDTO::from)
                .collect(),
            default_model_groups: value
                .settings
                .default_model_groups
                .into_iter()
                .map(DefaultModelGroupDTO::from)
                .collect(),
            settings: value
                .settings
                .settings
                .into_iter()
                .map(AppSettingDTO::from)
                .collect(),
            config_audits: value
                .settings
                .config_audits
                .into_iter()
                .map(ConfigAuditDTO::from)
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

impl From<PublicProviderRecord> for PublicProviderDTO {
    fn from(value: PublicProviderRecord) -> Self {
        Self {
            id: value.id,
            name: value.name,
            icon_name: value.icon_name,
            api_type: value.api_type,
            base_url: value.base_url,
            secret_label: value.secret_label,
            enabled: value.enabled,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<ProviderModelRecord> for ProviderModelDTO {
    fn from(value: ProviderModelRecord) -> Self {
        Self {
            id: value.id,
            provider_id: value.provider_id,
            model_id: value.model_id,
            display_name: value.display_name,
            supports_tool_call: value.supports_tool_call,
            supports_reasoning: value.supports_reasoning,
            supports_image_input: value.supports_image_input,
            supports_structured_output: value.supports_structured_output,
            supports_temperature: value.supports_temperature,
            context_limit: value.context_limit,
            output_limit: value.output_limit,
            reasoning_field: value.reasoning_field,
            metadata_json: value.metadata_json,
            synced_at_ms: value.synced_at_ms,
        }
    }
}

impl From<ModelGroupRecord> for ModelGroupDTO {
    fn from(value: ModelGroupRecord) -> Self {
        Self {
            id: value.id,
            name: value.name,
            routing_strategy: value.routing_strategy,
            fallback_policy: value.fallback_policy,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<ModelGroupMemberRecord> for ModelGroupMemberDTO {
    fn from(value: ModelGroupMemberRecord) -> Self {
        Self {
            id: value.id,
            group_id: value.group_id,
            provider_id: value.provider_id,
            provider_name: value.provider_name,
            model_id: value.model_id,
            model_display_name: value.model_display_name,
            position: value.position,
            enabled: value.enabled,
        }
    }
}

impl From<DefaultModelGroupRecord> for DefaultModelGroupDTO {
    fn from(value: DefaultModelGroupRecord) -> Self {
        Self {
            key: value.key,
            group_id: value.group_id,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<AppSettingRecord> for AppSettingDTO {
    fn from(value: AppSettingRecord) -> Self {
        Self {
            key: value.key,
            value: value.value,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<ConfigAuditRecord> for ConfigAuditDTO {
    fn from(value: ConfigAuditRecord) -> Self {
        Self {
            id: value.id,
            command_id: value.command_id,
            actor: value.actor,
            action: value.action,
            target_kind: value.target_kind,
            target_id: value.target_id,
            redacted_summary: value.redacted_summary,
            approval_required: value.approval_required,
            approval_token: value.approval_token,
            created_at_ms: value.created_at_ms,
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
