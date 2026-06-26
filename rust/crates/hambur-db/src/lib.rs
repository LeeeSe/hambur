use std::future::{Ready, ready};
use std::path::Path;
use std::sync::Mutex;

use hambur_core::{HamburError, HamburResult, new_id, now_ms};
use rusqlite::types::{Value, ValueRef};

macro_rules! params {
    () => {
        Vec::<Value>::new()
    };
    ($($value:expr),+ $(,)?) => {
        vec![$(IntoSqlValue::into_sql_value($value)),+]
    };
}

const DEFAULT_SESSION_TITLE: &str = "Untitled session";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub pinned_at_ms: u64,
    pub message_count: u32,
    pub latest_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineItemSnapshot {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlockPayloadRecord {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub block_id: u64,
    pub stable_key: String,
    pub committed: bool,
    pub payload_json: String,
    pub raw: String,
    pub small_summary: String,
    pub version_sequence: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
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
    pub attachments: Vec<AttachmentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRecord {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub finished_at_ms: u64,
    pub selected_provider_id: String,
    pub selected_provider_name: String,
    pub provider_protocol: String,
    pub selected_model_id: String,
    pub selected_model_name: String,
    pub model_group_id: String,
    pub error_code: String,
    pub error_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTimelineItem {
    pub stable_key: String,
    pub content_type: String,
    pub display_sequence: u64,
    pub payload_ref: String,
    pub small_summary: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMarkdownBlockPayload {
    pub id: String,
    pub message_id: String,
    pub block_id: u64,
    pub stable_key: String,
    pub committed: bool,
    pub payload_json: String,
    pub raw: String,
    pub small_summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewTraceSpan {
    pub session_id: String,
    pub turn_id: String,
    pub parent_span_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub status: String,
    pub tool_call_id: String,
    pub payload_json: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceSpanRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub parent_span_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub status: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub tool_call_id: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewToolCall {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub assistant_message_id: String,
    pub name: String,
    pub arguments_json: String,
    pub display_title: String,
    pub status: String,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub assistant_message_id: String,
    pub name: String,
    pub arguments_json: String,
    pub display_title: String,
    pub status: String,
    pub requires_approval: bool,
    pub approval_status: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub result_id: String,
    pub error_code: String,
    pub error_message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewToolResult {
    pub session_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub message_id: String,
    pub is_error: bool,
    pub content_json: String,
    pub summary: String,
    pub artifacts_json: String,
    pub trust_level: String,
    pub truncated: bool,
    pub offloaded_file_id: String,
    pub offloaded_path: String,
    pub context_stub: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolResultRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub message_id: String,
    pub is_error: bool,
    pub content_json: String,
    pub summary: String,
    pub artifacts_json: String,
    pub trust_level: String,
    pub truncated: bool,
    pub offloaded_file_id: String,
    pub offloaded_path: String,
    pub context_stub: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewFileRecord {
    pub id: String,
    pub scope: String,
    pub session_id: String,
    pub relative_path: String,
    pub sandbox_path: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub retention_policy: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileRecord {
    pub id: String,
    pub scope: String,
    pub session_id: String,
    pub relative_path: String,
    pub sandbox_path: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub retention_policy: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewAttachment {
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentRecord {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileCleanupJobRecord {
    pub id: String,
    pub file_id: String,
    pub relative_path: String,
    pub reason: String,
    pub status: String,
    pub attempts: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderUpsert {
    pub id: String,
    pub name: String,
    pub icon_name: String,
    pub api_type: String,
    pub base_url: String,
    pub secret_ref: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub icon_name: String,
    pub api_type: String,
    pub base_url: String,
    pub secret_ref: String,
    pub enabled: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicProviderRecord {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderModelUpsert {
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderModelRecord {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderModelOverride {
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelGroupRecord {
    pub id: String,
    pub name: String,
    pub routing_strategy: String,
    pub fallback_policy: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelGroupMemberRecord {
    pub id: String,
    pub group_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_display_name: String,
    pub position: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultModelGroupRecord {
    pub key: String,
    pub group_id: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSettingRecord {
    pub key: String,
    pub value: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigAuditRecord {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub providers: Vec<PublicProviderRecord>,
    pub provider_models: Vec<ProviderModelRecord>,
    pub model_groups: Vec<ModelGroupRecord>,
    pub model_group_members: Vec<ModelGroupMemberRecord>,
    pub default_model_groups: Vec<DefaultModelGroupRecord>,
    pub settings: Vec<AppSettingRecord>,
    pub config_audits: Vec<ConfigAuditRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelRouteSnapshot {
    pub provider_id: String,
    pub provider_name: String,
    pub provider_protocol: String,
    pub base_url: String,
    pub secret_ref: String,
    pub model_id: String,
    pub model_display_name: String,
    pub model_group_id: String,
    pub model_group_name: String,
    pub routing_strategy: String,
    pub fallback_policy: String,
    pub position: u32,
    pub supports_tool_call: bool,
    pub supports_reasoning: bool,
    pub supports_image_input: bool,
    pub supports_structured_output: bool,
    pub supports_temperature: bool,
    pub context_limit: u32,
    pub output_limit: u32,
    pub reasoning_field: String,
}

impl ModelRouteSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSnapshot {
    pub sessions: Vec<SessionSummary>,
    pub selected_session_id: String,
    pub timeline_items: Vec<TimelineItemSnapshot>,
    pub markdown_block_payloads: Vec<MarkdownBlockPayloadRecord>,
    pub pending_attachments: Vec<AttachmentRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelinePageData {
    pub items: Vec<TimelineItemSnapshot>,
    pub markdown_block_payloads: Vec<MarkdownBlockPayloadRecord>,
    pub next_before_cursor: u64,
    pub has_more: bool,
}

pub struct HamburDatabase {
    connection: Connection,
}

impl HamburDatabase {
    pub async fn open(path: impl AsRef<Path>) -> HamburResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HamburError::Internal(format!("create database directory: {error}"))
            })?;
        }

        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.migrate().await?;
        Ok(database)
    }

    pub async fn bootstrap_snapshot(&self) -> HamburResult<AppSnapshot> {
        self.snapshot_for_selected(None).await
    }

    pub async fn create_session(&self, title: &str) -> HamburResult<AppSnapshot> {
        let id = new_id("ses");
        let now = now_ms();
        let title = normalize_title(title);

        self.connection
            .execute(
                "INSERT INTO sessions (id, title, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id.clone(), title, now as i64, now as i64],
            )
            .await
            .map_err(database_error)?;

        self.set_active_session(Some(&id)).await?;
        self.snapshot_for_selected(Some(&id)).await
    }

    pub async fn open_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        if !self.session_exists(session_id).await? {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        let now = now_ms();
        self.connection
            .execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        self.set_active_session(Some(session_id)).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn rename_session(&self, session_id: &str, title: &str) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        let now = now_ms();
        let title = normalize_title(title);
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET title = ?1, updated_at_ms = ?2
                 WHERE id = ?3 AND deleted_at_ms IS NULL",
                params![title, now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn set_session_pinned(
        &self,
        session_id: &str,
        pinned: bool,
    ) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        let now = now_ms();
        let pinned_at_ms = if pinned { now } else { 0 };
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET pinned_at_ms = ?1, updated_at_ms = ?2
                 WHERE id = ?3 AND deleted_at_ms IS NULL",
                params![pinned_at_ms as i64, now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn delete_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET deleted_at_ms = ?1, updated_at_ms = ?1
                 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        if self.active_session_id().await?.as_deref() == Some(session_id) {
            self.set_active_session(None).await?;
        }
        self.snapshot_for_selected(None).await
    }

    pub async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        let role = normalize_role(role)?;
        self.connection
            .execute(
                "INSERT INTO messages
                    (id, session_id, role, content_text, created_at_ms, version_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![
                    id.clone(),
                    session_id,
                    role.clone(),
                    content_text,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(MessageRecord {
            id,
            session_id: session_id.to_string(),
            role,
            content_text: content_text.to_string(),
            reasoning_content: String::new(),
            status: "completed".to_string(),
            turn_id: String::new(),
            created_at_ms: now,
            version_sequence: 1,
            provider_id_snapshot: String::new(),
            provider_name_snapshot: String::new(),
            provider_protocol: String::new(),
            model_id_snapshot: String::new(),
            model_name_snapshot: String::new(),
            model_group_id: String::new(),
            finish_reason: String::new(),
            native_finish_reason: String::new(),
            attachments: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_message_with_route(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
        reasoning_content: &str,
        status: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        let role = normalize_role(role)?;
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO messages
                    (
                        id,
                        session_id,
                        turn_id,
                        role,
                        status,
                        content_text,
                        reasoning_content,
                        created_at_ms,
                        version_sequence,
                        provider_id_snapshot,
                        provider_name_snapshot,
                        provider_protocol,
                        model_id_snapshot,
                        model_name_snapshot,
                        model_group_id,
                        finish_reason,
                        native_finish_reason
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11, ?12, ?13, ?14, '', '')",
                params![
                    id.clone(),
                    session_id,
                    turn_id,
                    role.clone(),
                    status.clone(),
                    content_text,
                    reasoning_content,
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone()
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(MessageRecord {
            id,
            session_id: session_id.to_string(),
            role,
            content_text: content_text.to_string(),
            reasoning_content: reasoning_content.to_string(),
            status,
            turn_id: turn_id.to_string(),
            created_at_ms: now,
            version_sequence: 1,
            provider_id_snapshot: route.provider_id.clone(),
            provider_name_snapshot: route.provider_name.clone(),
            provider_protocol: route.provider_protocol.clone(),
            model_id_snapshot: route.model_id.clone(),
            model_name_snapshot: route.model_display_name.clone(),
            model_group_id: route.model_group_id.clone(),
            finish_reason: String::new(),
            native_finish_reason: String::new(),
            attachments: Vec::new(),
        })
    }

    pub async fn update_message_stream_result(
        &self,
        message_id: &str,
        content_text: &str,
        reasoning_content: &str,
        status: &str,
        finish_reason: &str,
        native_finish_reason: &str,
    ) -> HamburResult<MessageRecord> {
        let status = normalize_status(status)?;
        let changed = self
            .connection
            .execute(
                "UPDATE messages
                 SET content_text = ?1,
                     reasoning_content = ?2,
                     status = ?3,
                     finish_reason = ?4,
                     native_finish_reason = ?5,
                     version_sequence = version_sequence + 1
                 WHERE id = ?6",
                params![
                    content_text,
                    reasoning_content,
                    status,
                    finish_reason,
                    native_finish_reason,
                    message_id
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "message not found: {message_id}"
            )));
        }
        self.message_by_id(message_id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("message disappeared: {message_id}")))
    }

    pub async fn update_message_route_snapshot(
        &self,
        message_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        let changed = self
            .connection
            .execute(
                "UPDATE messages
                 SET provider_id_snapshot = ?1,
                     provider_name_snapshot = ?2,
                     provider_protocol = ?3,
                     model_id_snapshot = ?4,
                     model_name_snapshot = ?5,
                     model_group_id = ?6,
                     version_sequence = version_sequence + 1
                 WHERE id = ?7",
                params![
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    message_id,
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "message not found: {message_id}"
            )));
        }
        self.message_by_id(message_id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("message disappeared: {message_id}")))
    }

    pub async fn insert_tool_result_message(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        content_text: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO messages
                    (
                        id,
                        session_id,
                        turn_id,
                        role,
                        status,
                        content_text,
                        reasoning_content,
                        created_at_ms,
                        version_sequence,
                        provider_id_snapshot,
                        provider_name_snapshot,
                        provider_protocol,
                        model_id_snapshot,
                        model_name_snapshot,
                        model_group_id,
                        finish_reason,
                        native_finish_reason,
                        tool_call_id,
                        tool_name,
                        tool_title
                    )
                 VALUES (?1, ?2, ?3, 'tool', 'completed', ?4, '', ?5, 1, ?6, ?7, ?8, ?9, ?10, ?11, 'tool_result', 'tool_result', ?12, ?13, ?13)",
                params![
                    id.clone(),
                    session_id,
                    turn_id,
                    content_text,
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    tool_call_id,
                    tool_name,
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        self.message_by_id(&id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("tool message disappeared: {id}")))
    }

    pub async fn insert_trace_span(&self, input: NewTraceSpan) -> HamburResult<TraceSpanRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = new_id("trace");
        let now = now_ms();
        let status = normalize_status(&input.status)?;
        self.connection
            .execute(
                "INSERT INTO trace_spans
                    (
                        id,
                        session_id,
                        turn_id,
                        parent_span_id,
                        kind,
                        title,
                        content,
                        status,
                        started_at_ms,
                        ended_at_ms,
                        tool_call_id,
                        payload_json,
                        visible
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12)",
                params![
                    id.clone(),
                    input.session_id.clone(),
                    input.turn_id,
                    input.parent_span_id,
                    input.kind,
                    input.title,
                    input.content,
                    status,
                    now as i64,
                    input.tool_call_id,
                    input.payload_json,
                    input.visible,
                ],
            )
            .await
            .map_err(database_error)?;

        if input.visible {
            let trace = self.trace_span_by_id(&id).await?;
            self.upsert_timeline_item(
                &input.session_id,
                NewTimelineItem {
                    stable_key: trace.id.clone(),
                    content_type: "trace".to_string(),
                    display_sequence: trace.started_at_ms,
                    payload_ref: trace.id.clone(),
                    small_summary: trace.title.clone(),
                    kind: match trace.kind.as_str() {
                        "tool" => "ToolTrace".to_string(),
                        "error" => "ErrorTrace".to_string(),
                        _ => "TraceSpan".to_string(),
                    },
                },
            )
            .await?;
        }

        self.trace_span_by_id(&id).await
    }

    pub async fn update_trace_span_status(
        &self,
        trace_id: &str,
        status: &str,
        content: &str,
        ended: bool,
    ) -> HamburResult<TraceSpanRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE trace_spans
                 SET status = ?1,
                     content = ?2,
                     ended_at_ms = CASE WHEN ?3 THEN ?4 ELSE ended_at_ms END
                 WHERE id = ?5",
                params![status, content, ended, now as i64, trace_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "trace span not found: {trace_id}"
            )));
        }
        let trace = self.trace_span_by_id(trace_id).await?;
        self.upsert_timeline_item(
            &trace.session_id,
            NewTimelineItem {
                stable_key: trace.id.clone(),
                content_type: "trace".to_string(),
                display_sequence: trace.started_at_ms,
                payload_ref: trace.id.clone(),
                small_summary: trace.title.clone(),
                kind: match trace.kind.as_str() {
                    "tool" => "ToolTrace".to_string(),
                    "error" => "ErrorTrace".to_string(),
                    _ => "TraceSpan".to_string(),
                },
            },
        )
        .await?;
        Ok(trace)
    }

    pub async fn insert_tool_call(&self, input: NewToolCall) -> HamburResult<ToolCallRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = if input.id.trim().is_empty() {
            new_id("tool_call")
        } else {
            input.id
        };
        let now = now_ms();
        let status = normalize_status(&input.status)?;
        self.connection
            .execute(
                "INSERT INTO tool_calls
                    (
                        id,
                        session_id,
                        turn_id,
                        assistant_message_id,
                        name,
                        arguments_json,
                        display_title,
                        status,
                        requires_approval,
                        approval_status,
                        started_at_ms,
                        ended_at_ms,
                        result_id,
                        error_code,
                        error_message
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'not_required', ?10, NULL, '', '', '')
                 ON CONFLICT(id) DO UPDATE SET
                    status = excluded.status,
                    display_title = excluded.display_title,
                    arguments_json = excluded.arguments_json,
                    requires_approval = excluded.requires_approval,
                    started_at_ms = CASE
                        WHEN tool_calls.started_at_ms = 0 THEN excluded.started_at_ms
                        ELSE tool_calls.started_at_ms
                    END",
                params![
                    id.clone(),
                    input.session_id,
                    input.turn_id,
                    input.assistant_message_id,
                    input.name,
                    input.arguments_json,
                    input.display_title,
                    status,
                    input.requires_approval,
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;

        self.tool_call_by_id(&id).await
    }

    pub async fn update_tool_call_status(
        &self,
        tool_call_id: &str,
        status: &str,
        result_id: &str,
        error_code: &str,
        error_message: &str,
        ended: bool,
    ) -> HamburResult<ToolCallRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE tool_calls
                 SET status = ?1,
                     result_id = ?2,
                     error_code = ?3,
                     error_message = ?4,
                     ended_at_ms = CASE WHEN ?5 THEN ?6 ELSE ended_at_ms END
                 WHERE id = ?7",
                params![
                    status,
                    result_id,
                    error_code,
                    error_message,
                    ended,
                    now as i64,
                    tool_call_id,
                ],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "tool call not found: {tool_call_id}"
            )));
        }
        self.tool_call_by_id(tool_call_id).await
    }

    pub async fn insert_tool_result(&self, input: NewToolResult) -> HamburResult<ToolResultRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = new_id("tool_result");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO tool_results
                    (
                        id,
                        session_id,
                        turn_id,
                        tool_call_id,
                        message_id,
                        is_error,
                        content_json,
                        summary,
                        artifacts_json,
                        trust_level,
                        truncated,
                        offloaded_file_id,
                        offloaded_path,
                        context_stub,
                        created_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    id.clone(),
                    input.session_id,
                    input.turn_id,
                    input.tool_call_id,
                    input.message_id,
                    input.is_error,
                    input.content_json,
                    input.summary,
                    input.artifacts_json,
                    input.trust_level,
                    input.truncated,
                    input.offloaded_file_id,
                    input.offloaded_path,
                    input.context_stub,
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;

        self.tool_result_by_id(&id).await
    }

    pub async fn upsert_file_record(&self, input: NewFileRecord) -> HamburResult<FileRecord> {
        let id = if input.id.trim().is_empty() {
            new_id("file")
        } else {
            input.id.trim().chars().take(160).collect()
        };
        let scope = normalize_file_scope(&input.scope)?;
        let session_id = input.session_id.trim().to_string();
        if scope == "session" {
            self.ensure_session_exists(&session_id).await?;
        }
        let relative_path = normalize_db_path(&input.relative_path, "relative_path")?;
        let sandbox_path = normalize_db_path(&input.sandbox_path, "sandbox_path")?;
        let mime_type = normalize_mime_type(&input.mime_type);
        let retention_policy = normalize_retention_policy(&input.retention_policy)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO files
                    (
                        id,
                        scope,
                        session_id,
                        relative_path,
                        sandbox_path,
                        mime_type,
                        byte_size,
                        sha256,
                        created_at_ms,
                        updated_at_ms,
                        retention_policy
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    scope = excluded.scope,
                    session_id = excluded.session_id,
                    relative_path = excluded.relative_path,
                    sandbox_path = excluded.sandbox_path,
                    mime_type = excluded.mime_type,
                    byte_size = excluded.byte_size,
                    sha256 = excluded.sha256,
                    updated_at_ms = excluded.updated_at_ms,
                    retention_policy = excluded.retention_policy",
                params![
                    id.clone(),
                    scope,
                    session_id,
                    relative_path,
                    sandbox_path,
                    mime_type,
                    input.byte_size as i64,
                    input.sha256.trim().chars().take(128).collect::<String>(),
                    now as i64,
                    retention_policy,
                ],
            )
            .await
            .map_err(database_error)?;

        self.file_by_id(&id).await
    }

    pub async fn create_pending_attachment(
        &self,
        input: NewAttachment,
    ) -> HamburResult<AttachmentRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        self.file_by_id(&input.file_id).await?;
        let id = if input.id.trim().is_empty() {
            new_id("att")
        } else {
            input.id.trim().chars().take(160).collect()
        };
        let kind = normalize_attachment_kind(&input.kind, &input.mime_type);
        let status = normalize_attachment_status(&input.status)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO attachments
                    (
                        id,
                        session_id,
                        message_id,
                        kind,
                        display_name,
                        mime_type,
                        byte_size,
                        origin_type,
                        original_uri,
                        file_id,
                        sandbox_path,
                        width,
                        height,
                        sha256,
                        status,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                    display_name = excluded.display_name,
                    mime_type = excluded.mime_type,
                    byte_size = excluded.byte_size,
                    original_uri = excluded.original_uri,
                    sandbox_path = excluded.sandbox_path,
                    width = excluded.width,
                    height = excluded.height,
                    sha256 = excluded.sha256,
                    status = excluded.status,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id.clone(),
                    input.session_id,
                    input.message_id,
                    kind,
                    normalize_display_name(&input.display_name),
                    normalize_mime_type(&input.mime_type),
                    input.byte_size as i64,
                    normalize_origin_type(&input.origin_type),
                    input
                        .original_uri
                        .trim()
                        .chars()
                        .take(1024)
                        .collect::<String>(),
                    input.file_id,
                    normalize_db_path(&input.sandbox_path, "sandbox_path")?,
                    input.width as i64,
                    input.height as i64,
                    input.sha256.trim().chars().take(128).collect::<String>(),
                    status,
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;
        self.attachment_by_id(&id).await
    }

    pub async fn pending_attachments_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<AttachmentRecord>> {
        self.ensure_session_exists(session_id).await?;
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    kind,
                    display_name,
                    mime_type,
                    byte_size,
                    origin_type,
                    original_uri,
                    file_id,
                    sandbox_path,
                    width,
                    height,
                    sha256,
                    status,
                    created_at_ms,
                    updated_at_ms
                FROM attachments
                WHERE session_id = ?1
                  AND message_id = ''
                  AND status = 'pending'
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            attachments.push(attachment_from_row(&row)?);
        }
        Ok(attachments)
    }

    pub async fn attach_pending_to_message(
        &self,
        session_id: &str,
        message_id: &str,
        attachment_ids: &[String],
    ) -> HamburResult<Vec<AttachmentRecord>> {
        self.ensure_session_exists(session_id).await?;
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_ms();
        let mut attached = Vec::new();
        for attachment_id in attachment_ids {
            let attachment = self.attachment_by_id(attachment_id).await?;
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
            self.connection
                .execute(
                    "UPDATE attachments
                     SET message_id = ?1,
                         status = 'attached',
                         updated_at_ms = ?2
                     WHERE id = ?3",
                    params![message_id, now as i64, attachment_id.clone()],
                )
                .await
                .map_err(database_error)?;
            attached.push(self.attachment_by_id(attachment_id).await?);
        }
        Ok(attached)
    }

    pub async fn remove_pending_attachment(
        &self,
        session_id: &str,
        attachment_id: &str,
    ) -> HamburResult<(AttachmentRecord, Option<FileCleanupJobRecord>)> {
        self.ensure_session_exists(session_id).await?;
        let attachment = self.attachment_by_id(attachment_id).await?;
        if attachment.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "attachment does not belong to session: {attachment_id}"
            )));
        }
        if attachment.status != "pending" || !attachment.message_id.is_empty() {
            return Err(HamburError::InvalidCommand(format!(
                "attachment is not removable pending state: {attachment_id}"
            )));
        }
        let now = now_ms();
        self.connection
            .execute(
                "UPDATE attachments
                 SET status = 'removed',
                     updated_at_ms = ?1
                 WHERE id = ?2",
                params![now as i64, attachment_id],
            )
            .await
            .map_err(database_error)?;
        let removed = self.attachment_by_id(attachment_id).await?;
        let cleanup = self
            .schedule_file_cleanup(&removed.file_id, "pending_attachment_removed")
            .await
            .ok();
        Ok((removed, cleanup))
    }

    pub async fn cleanup_pending_attachments(
        &self,
        older_than_ms: u64,
    ) -> HamburResult<Vec<FileCleanupJobRecord>> {
        let cutoff = now_ms().saturating_sub(older_than_ms);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    kind,
                    display_name,
                    mime_type,
                    byte_size,
                    origin_type,
                    original_uri,
                    file_id,
                    sandbox_path,
                    width,
                    height,
                    sha256,
                    status,
                    created_at_ms,
                    updated_at_ms
                FROM attachments
                WHERE status = 'pending'
                  AND message_id = ''
                  AND created_at_ms < ?1
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![cutoff as i64],
            )
            .await
            .map_err(database_error)?;

        let mut expired = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            expired.push(attachment_from_row(&row)?);
        }

        let now = now_ms();
        let mut jobs = Vec::new();
        for attachment in expired {
            self.connection
                .execute(
                    "UPDATE attachments
                     SET status = 'expired',
                         updated_at_ms = ?1
                     WHERE id = ?2 AND status = 'pending'",
                    params![now as i64, attachment.id],
                )
                .await
                .map_err(database_error)?;
            if let Ok(job) = self
                .schedule_file_cleanup(&attachment.file_id, "pending_attachment_expired")
                .await
            {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    pub async fn schedule_file_cleanup(
        &self,
        file_id: &str,
        reason: &str,
    ) -> HamburResult<FileCleanupJobRecord> {
        let file = self.file_by_id(file_id).await?;
        let id = new_id("cleanup");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO file_cleanup_jobs
                    (
                        id,
                        file_id,
                        relative_path,
                        reason,
                        status,
                        attempts,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5)",
                params![
                    id.clone(),
                    file.id,
                    file.relative_path,
                    reason.trim().chars().take(120).collect::<String>(),
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;
        self.file_cleanup_job_by_id(&id).await
    }

    pub async fn pending_file_cleanup_jobs(
        &self,
        limit: u32,
    ) -> HamburResult<Vec<FileCleanupJobRecord>> {
        let limit = clamp_limit(limit, 1, 100);
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, file_id, relative_path, reason, status, attempts, created_at_ms, updated_at_ms
                FROM file_cleanup_jobs
                WHERE status = 'pending'
                ORDER BY created_at_ms ASC, id ASC
                LIMIT ?1
                ",
                params![limit as i64],
            )
            .await
            .map_err(database_error)?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            jobs.push(file_cleanup_job_from_row(&row)?);
        }
        Ok(jobs)
    }

    pub async fn mark_file_cleanup_done(&self, job_id: &str) -> HamburResult<FileCleanupJobRecord> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE file_cleanup_jobs
                 SET status = 'completed',
                     updated_at_ms = ?1
                 WHERE id = ?2",
                params![now as i64, job_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "file cleanup job not found: {job_id}"
            )));
        }
        self.file_cleanup_job_by_id(job_id).await
    }

    pub async fn resolve_file_by_sandbox_path(
        &self,
        session_id: &str,
        sandbox_path: &str,
    ) -> HamburResult<FileRecord> {
        self.ensure_session_exists(session_id).await?;
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    scope,
                    session_id,
                    relative_path,
                    sandbox_path,
                    mime_type,
                    byte_size,
                    sha256,
                    created_at_ms,
                    updated_at_ms,
                    retention_policy
                FROM files
                WHERE sandbox_path = ?1
                  AND (scope != 'session' OR session_id = ?2)
                LIMIT 1
                ",
                params![sandbox_path, session_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file not found for sandbox path: {sandbox_path}"
            )));
        };
        file_from_row(&row)
    }

    pub async fn upsert_timeline_item(
        &self,
        session_id: &str,
        item: NewTimelineItem,
    ) -> HamburResult<TimelineItemSnapshot> {
        self.ensure_session_exists(session_id).await?;

        let stable_key = item.stable_key;
        if stable_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline stable_key must not be empty".to_string(),
            ));
        }
        if item.content_type.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline content_type must not be empty".to_string(),
            ));
        }

        let id = new_id("tl");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO timeline_items
                    (
                        id,
                        session_id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind,
                        visible,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, 1, ?9, ?9)
                 ON CONFLICT(session_id, stable_key) DO UPDATE SET
                    content_type = excluded.content_type,
                    display_sequence = excluded.display_sequence,
                    version_sequence = timeline_items.version_sequence + 1,
                    payload_ref = excluded.payload_ref,
                    small_summary = excluded.small_summary,
                    kind = excluded.kind,
                    visible = 1,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id,
                    session_id,
                    stable_key.clone(),
                    item.content_type,
                    item.display_sequence as i64,
                    item.payload_ref,
                    item.small_summary,
                    item.kind,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        self.timeline_item_by_stable_key(session_id, &stable_key)
            .await
    }

    pub async fn upsert_markdown_block_payload(
        &self,
        session_id: &str,
        _turn_id: &str,
        input: NewMarkdownBlockPayload,
    ) -> HamburResult<MarkdownBlockPayloadRecord> {
        self.ensure_session_exists(session_id).await?;
        if input.message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "markdown block message_id must not be empty".to_string(),
            ));
        }
        if input.stable_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "markdown block stable_key must not be empty".to_string(),
            ));
        }

        let id = if input.id.trim().is_empty() {
            new_id("mdb")
        } else {
            input.id.clone()
        };
        let now = now_ms();
        let stable_key = input.stable_key.clone();
        self.connection
            .execute(
                "INSERT INTO markdown_blocks
                    (
                        id,
                        session_id,
                        message_id,
                        block_id,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10)
                 ON CONFLICT(session_id, stable_key) DO UPDATE SET
                    message_id = excluded.message_id,
                    block_id = excluded.block_id,
                    committed = excluded.committed,
                    payload_json = excluded.payload_json,
                    raw = excluded.raw,
                    small_summary = excluded.small_summary,
                    version_sequence = markdown_blocks.version_sequence + 1,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id.clone(),
                    session_id,
                    input.message_id,
                    input.block_id as i64,
                    stable_key.clone(),
                    if input.committed { 1_i64 } else { 0_i64 },
                    input.payload_json,
                    input.raw,
                    input.small_summary,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        let record = self.markdown_block_by_stable_key(session_id, &stable_key).await?;
        self.upsert_timeline_item(
            session_id,
            NewTimelineItem {
                stable_key: record.stable_key.clone(),
                content_type: if record.committed {
                    "assistant_markdown_block".to_string()
                } else {
                    "assistant_pending_block".to_string()
                },
                display_sequence: record.created_at_ms.saturating_add(record.block_id),
                payload_ref: record.id.clone(),
                small_summary: record.small_summary.clone(),
                kind: if record.committed {
                    "AssistantMarkdownBlock".to_string()
                } else {
                    "AssistantPendingBlock".to_string()
                },
            },
        )
        .await?;
        if record.committed {
            self.hide_timeline_item(session_id, &pending_markdown_stable_key(&record.message_id))
                .await?;
        }
        Ok(record)
    }

    pub async fn remove_pending_markdown_block(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> HamburResult<()> {
        self.hide_timeline_item(session_id, &pending_markdown_stable_key(message_id))
            .await
    }

    async fn hide_timeline_item(&self, session_id: &str, stable_key: &str) -> HamburResult<()> {
        if stable_key.trim().is_empty() {
            return Ok(());
        }
        let now = now_ms();
        self.connection
            .execute(
                "UPDATE timeline_items
                 SET visible = 0,
                     version_sequence = version_sequence + 1,
                     updated_at_ms = ?3
                 WHERE session_id = ?1 AND stable_key = ?2",
                params![session_id, stable_key, now as i64],
            )
            .await
            .map(|_| ())
            .map_err(database_error)
    }

    async fn markdown_block_by_stable_key(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<MarkdownBlockPayloadRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    block_id,
                    stable_key,
                    committed,
                    payload_json,
                    raw,
                    small_summary,
                    version_sequence,
                    created_at_ms,
                    updated_at_ms
                FROM markdown_blocks
                WHERE session_id = ?1 AND stable_key = ?2
                LIMIT 1
                ",
                params![session_id, stable_key],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::Internal(format!(
                "markdown block not found after upsert: {stable_key}"
            )));
        };
        markdown_block_from_row(&row)
    }

    async fn markdown_blocks_for_payload_refs(
        &self,
        session_id: &str,
        payload_refs: &[String],
    ) -> HamburResult<Vec<MarkdownBlockPayloadRecord>> {
        let mut blocks = Vec::new();
        for payload_ref in payload_refs {
            if payload_ref.trim().is_empty() {
                continue;
            }
            let mut rows = self
                .connection
                .query(
                    "
                    SELECT
                        id,
                        session_id,
                        message_id,
                        block_id,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    FROM markdown_blocks
                    WHERE session_id = ?1 AND id = ?2
                    LIMIT 1
                    ",
                    params![session_id, payload_ref],
                )
                .await
                .map_err(database_error)?;
            if let Some(row) = rows.next().await.map_err(database_error)? {
                blocks.push(markdown_block_from_row(&row)?);
            }
        }
        Ok(blocks)
    }

    pub async fn create_turn(&self, session_id: &str, status: &str) -> HamburResult<TurnRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("turn");
        let now = now_ms();
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO turns
                    (id, session_id, status, created_at_ms, updated_at_ms, finished_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?4, NULL)",
                params![id.clone(), session_id, status.clone(), now as i64],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            status,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: 0,
            selected_provider_id: String::new(),
            selected_provider_name: String::new(),
            provider_protocol: String::new(),
            selected_model_id: String::new(),
            selected_model_name: String::new(),
            model_group_id: String::new(),
            error_code: String::new(),
            error_message: String::new(),
        })
    }

    pub async fn create_turn_with_route(
        &self,
        session_id: &str,
        status: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<TurnRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("turn");
        let now = now_ms();
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO turns
                    (
                        id,
                        session_id,
                        status,
                        created_at_ms,
                        updated_at_ms,
                        finished_at_ms,
                        selected_provider_id,
                        selected_provider_name,
                        provider_protocol,
                        selected_model_id,
                        selected_model_name,
                        model_group_id,
                        error_code,
                        error_message
                    )
                 VALUES (?1, ?2, ?3, ?4, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, '', '')",
                params![
                    id.clone(),
                    session_id,
                    status.clone(),
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone()
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            status,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: 0,
            selected_provider_id: route.provider_id.clone(),
            selected_provider_name: route.provider_name.clone(),
            provider_protocol: route.provider_protocol.clone(),
            selected_model_id: route.model_id.clone(),
            selected_model_name: route.model_display_name.clone(),
            model_group_id: route.model_group_id.clone(),
            error_code: String::new(),
            error_message: String::new(),
        })
    }

    pub async fn update_turn_status(
        &self,
        turn_id: &str,
        status: &str,
        finished: bool,
    ) -> HamburResult<TurnRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET status = ?1,
                     updated_at_ms = ?2,
                     finished_at_ms = CASE WHEN ?3 THEN ?2 ELSE finished_at_ms END
                 WHERE id = ?4",
                params![status, now as i64, finished, turn_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }

        self.turn_by_id(turn_id).await
    }

    pub async fn update_turn_route_snapshot(
        &self,
        turn_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<TurnRecord> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET updated_at_ms = ?1,
                     selected_provider_id = ?2,
                     selected_provider_name = ?3,
                     provider_protocol = ?4,
                     selected_model_id = ?5,
                     selected_model_name = ?6,
                     model_group_id = ?7
                 WHERE id = ?8",
                params![
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    turn_id,
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }
        self.turn_by_id(turn_id).await
    }

    pub async fn fail_turn(
        &self,
        turn_id: &str,
        status: &str,
        error_code: &str,
        error_message: &str,
    ) -> HamburResult<TurnRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET status = ?1,
                     updated_at_ms = ?2,
                     finished_at_ms = ?2,
                     error_code = ?3,
                     error_message = ?4
                 WHERE id = ?5",
                params![status, now as i64, error_code, error_message, turn_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }

        self.turn_by_id(turn_id).await
    }

    pub async fn session_snapshot(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn session_list(&self, limit: u32, offset: u32) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(limit, offset).await
    }

    pub async fn session_summary(&self, session_id: &str) -> HamburResult<SessionSummary> {
        self.ensure_session_exists(session_id).await?;
        self.session_summary_by_id(session_id).await
    }

    pub async fn timeline_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        self.ensure_session_exists(session_id).await?;
        let mut page = self.timeline_items_page(session_id, before_cursor, limit).await?;
        let payload_refs = markdown_payload_refs(&page.items);
        page.markdown_block_payloads = self
            .markdown_blocks_for_payload_refs(session_id, &payload_refs)
            .await?;
        Ok(page)
    }

    pub async fn message_snapshot(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        if message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "message_id must not be empty".to_string(),
            ));
        }
        let Some(mut message) = self.message_by_id(message_id).await? else {
            return Ok(None);
        };
        message.attachments = self.attachments_for_message(message_id).await?;
        Ok(Some(message))
    }

    pub async fn source_user_message_for(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;
        let source = self.message_snapshot(message_id).await?.ok_or_else(|| {
            HamburError::InvalidCommand(format!("message not found: {message_id}"))
        })?;

        if source.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "message does not belong to session: {message_id}"
            )));
        }
        if source.role == "user" {
            return Ok(source);
        }

        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    role,
                    content_text,
                    reasoning_content,
                    status,
                    turn_id,
                    created_at_ms,
                    version_sequence,
                    provider_id_snapshot,
                    provider_name_snapshot,
                    provider_protocol,
                    model_id_snapshot,
                    model_name_snapshot,
                    model_group_id,
                    finish_reason,
                    native_finish_reason
                FROM messages
                WHERE session_id = ?1
                  AND role = 'user'
                  AND created_at_ms <= ?2
                ORDER BY created_at_ms DESC, id DESC
                LIMIT 1
                ",
                params![session_id, source.created_at_ms as i64],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "source user message not found for: {message_id}"
            )));
        };

        message_from_row(&row)
    }

    pub async fn search_sessions(
        &self,
        query: &str,
        limit: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let query = query.trim();
        if query.is_empty() {
            return self.list_sessions_page(limit, 0).await;
        }

        let limit = clamp_limit(limit, 1, 100);
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                  AND s.title LIKE ?1 ESCAPE '\\'
                ORDER BY s.pinned_at_ms DESC, s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?2
                ",
                params![pattern, limit as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }

    pub async fn upsert_provider(&self, input: ProviderUpsert) -> HamburResult<ProviderRecord> {
        let provider_id = normalize_provider_id(&input.id);
        let name = normalize_title(&input.name);
        let icon_name = input.icon_name.trim().chars().take(80).collect::<String>();
        let api_type = input.api_type.trim();
        if api_type != "OpenAiCompatible" {
            return Err(HamburError::InvalidCommand(format!(
                "unsupported provider api_type: {api_type}"
            )));
        }
        let base_url = normalize_base_url(&input.base_url)?;
        let secret_ref = normalize_secret_ref(&input.secret_ref)?;
        let now = now_ms();

        self.connection
            .execute(
                "INSERT INTO providers
                    (id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    icon_name = excluded.icon_name,
                    api_type = excluded.api_type,
                    base_url = excluded.base_url,
                    secret_ref = excluded.secret_ref,
                    enabled = excluded.enabled,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    provider_id.clone(),
                    name,
                    icon_name,
                    api_type,
                    base_url,
                    secret_ref,
                    input.enabled,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;

        self.provider_by_id(&provider_id).await
    }

    pub async fn provider_by_id(&self, provider_id: &str) -> HamburResult<ProviderRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms
                FROM providers
                WHERE id = ?1
                LIMIT 1
                ",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ProviderUnavailable(format!(
                "provider not found: {provider_id}"
            )));
        };
        provider_record_from_row(&row)
    }

    pub async fn delete_provider(&self, provider_id: &str) -> HamburResult<()> {
        let provider_id = provider_id.trim();
        if provider_id.is_empty() {
            return Err(HamburError::InvalidCommand(
                "provider_id must not be empty".to_string(),
            ));
        }
        let changed = self
            .connection
            .execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::ProviderUnavailable(format!(
                "provider not found: {provider_id}"
            )));
        }
        Ok(())
    }

    pub async fn replace_provider_models(
        &self,
        provider_id: &str,
        models: Vec<ProviderModelUpsert>,
    ) -> HamburResult<Vec<ProviderModelRecord>> {
        self.provider_by_id(provider_id).await?;
        if models.is_empty() {
            return Err(HamburError::InvalidCommand(
                "model refresh returned no usable models".to_string(),
            ));
        }

        self.connection
            .execute(
                "DELETE FROM provider_models WHERE provider_id = ?1",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let now = now_ms();
        for model in models {
            let model_id = model.model_id.trim();
            if model_id.is_empty() {
                continue;
            }
            let display_name = if model.display_name.trim().is_empty() {
                model_id.to_string()
            } else {
                model.display_name.trim().chars().take(160).collect()
            };
            let row_id = new_id("pmod");
            self.connection
                .execute(
                    "INSERT INTO provider_models
                        (
                            id,
                            provider_id,
                            model_id,
                            display_name,
                            supports_tool_call,
                            supports_reasoning,
                            supports_image_input,
                            supports_structured_output,
                            supports_temperature,
                            context_limit,
                            output_limit,
                            reasoning_field,
                            metadata_json,
                            synced_at_ms
                        )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        row_id,
                        provider_id,
                        model_id,
                        display_name,
                        model.supports_tool_call,
                        model.supports_reasoning,
                        model.supports_image_input,
                        model.supports_structured_output,
                        model.supports_temperature,
                        model.context_limit.max(1) as i64,
                        model.output_limit.max(1) as i64,
                        model.reasoning_field,
                        model.metadata_json,
                        now as i64
                    ],
                )
                .await
                .map_err(database_error)?;
        }

        let saved = self.provider_models(provider_id).await?;
        if let Some(first) = saved.first() {
            self.ensure_default_model_groups(provider_id, &first.model_id)
                .await?;
        }
        Ok(saved)
    }

    pub async fn upsert_provider_model_override(
        &self,
        input: ProviderModelOverride,
    ) -> HamburResult<ProviderModelRecord> {
        let provider_id = input.provider_id.trim().to_string();
        let model_id = input.model_id.trim().to_string();
        if provider_id.is_empty() || model_id.is_empty() {
            return Err(HamburError::InvalidCommand(
                "provider_id and model_id must not be empty".to_string(),
            ));
        }
        self.provider_by_id(&provider_id).await?;
        let display_name = if input.display_name.trim().is_empty() {
            model_id.clone()
        } else {
            input.display_name.trim().chars().take(160).collect()
        };
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO provider_models
                    (
                        id,
                        provider_id,
                        model_id,
                        display_name,
                        supports_tool_call,
                        supports_reasoning,
                        supports_image_input,
                        supports_structured_output,
                        supports_temperature,
                        context_limit,
                        output_limit,
                        reasoning_field,
                        metadata_json,
                        synced_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(provider_id, model_id) DO UPDATE SET
                    display_name = excluded.display_name,
                    supports_tool_call = excluded.supports_tool_call,
                    supports_reasoning = excluded.supports_reasoning,
                    supports_image_input = excluded.supports_image_input,
                    supports_structured_output = excluded.supports_structured_output,
                    supports_temperature = excluded.supports_temperature,
                    context_limit = excluded.context_limit,
                    output_limit = excluded.output_limit,
                    reasoning_field = excluded.reasoning_field,
                    metadata_json = excluded.metadata_json,
                    synced_at_ms = excluded.synced_at_ms",
                params![
                    new_id("pmod"),
                    provider_id.clone(),
                    model_id.clone(),
                    display_name,
                    input.supports_tool_call,
                    input.supports_reasoning,
                    input.supports_image_input,
                    input.supports_structured_output,
                    input.supports_temperature,
                    input.context_limit.max(1) as i64,
                    input.output_limit.max(1) as i64,
                    input.reasoning_field,
                    input.metadata_json,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;

        self.provider_model_by_key(&provider_id, &model_id).await
    }

    pub async fn provider_model_by_key(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<ProviderModelRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    provider_id,
                    model_id,
                    display_name,
                    supports_tool_call,
                    supports_reasoning,
                    supports_image_input,
                    supports_structured_output,
                    supports_temperature,
                    context_limit,
                    output_limit,
                    reasoning_field,
                    metadata_json,
                    synced_at_ms
                FROM provider_models
                WHERE provider_id = ?1 AND model_id = ?2
                LIMIT 1
                ",
                params![provider_id, model_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "provider model not found: {provider_id}/{model_id}"
            )));
        };
        provider_model_from_row(&row)
    }

    pub async fn provider_models(
        &self,
        provider_id: &str,
    ) -> HamburResult<Vec<ProviderModelRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    provider_id,
                    model_id,
                    display_name,
                    supports_tool_call,
                    supports_reasoning,
                    supports_image_input,
                    supports_structured_output,
                    supports_temperature,
                    context_limit,
                    output_limit,
                    reasoning_field,
                    metadata_json,
                    synced_at_ms
                FROM provider_models
                WHERE provider_id = ?1
                ORDER BY model_id ASC
                ",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let mut models = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            models.push(provider_model_from_row(&row)?);
        }
        Ok(models)
    }

    pub async fn ensure_default_model_groups(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<()> {
        let now = now_ms();
        for (group_id, name, default_key) in [
            ("grp_primary_chat", "Primary Chat", "primary"),
            (
                "grp_secondary_background",
                "Secondary Background",
                "secondary",
            ),
        ] {
            self.connection
                .execute(
                    "INSERT INTO model_groups
                        (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, 'fallback', 'default', ?3, ?3)
                     ON CONFLICT(id) DO UPDATE SET updated_at_ms = excluded.updated_at_ms",
                    params![group_id, name, now as i64],
                )
                .await
                .map_err(database_error)?;
            self.connection
                .execute(
                    "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET
                        group_id = excluded.group_id,
                        updated_at_ms = excluded.updated_at_ms",
                    params![default_key, group_id, now as i64],
                )
                .await
                .map_err(database_error)?;
        }

        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, 'grp_primary_chat', ?2, ?3, 0, 1)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET enabled = 1",
                params![new_id("mgm"), provider_id, model_id],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn upsert_primary_chat_member(
        &self,
        provider_id: &str,
        model_id: &str,
        position: u32,
    ) -> HamburResult<()> {
        self.provider_by_id(provider_id).await?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO model_groups
                    (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                 VALUES ('grp_primary_chat', 'Primary Chat', 'fallback', 'default', ?1, ?1)
                 ON CONFLICT(id) DO UPDATE SET updated_at_ms = excluded.updated_at_ms",
                params![now as i64],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                 VALUES ('primary', 'grp_primary_chat', ?1)
                 ON CONFLICT(key) DO UPDATE SET
                    group_id = excluded.group_id,
                    updated_at_ms = excluded.updated_at_ms",
                params![now as i64],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, 'grp_primary_chat', ?2, ?3, ?4, 1)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET
                    position = excluded.position,
                    enabled = 1",
                params![new_id("mgm"), provider_id, model_id, position as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn upsert_model_group(
        &self,
        group_id: &str,
        name: &str,
        routing_strategy: &str,
        fallback_policy: &str,
    ) -> HamburResult<ModelGroupRecord> {
        let group_id = normalize_setting_id(group_id, "grp");
        let name = normalize_title(name);
        let routing_strategy = normalize_routing_strategy(routing_strategy)?;
        let fallback_policy = normalize_fallback_policy(fallback_policy)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO model_groups
                    (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    routing_strategy = excluded.routing_strategy,
                    fallback_policy = excluded.fallback_policy,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    group_id.clone(),
                    name,
                    routing_strategy,
                    fallback_policy,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.model_group_by_id(&group_id).await
    }

    pub async fn upsert_model_group_member(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
        position: u32,
        enabled: bool,
    ) -> HamburResult<ModelGroupMemberRecord> {
        self.model_group_by_id(group_id).await?;
        self.provider_model_by_key(provider_id, model_id).await?;
        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET
                    position = excluded.position,
                    enabled = excluded.enabled",
                params![
                    new_id("mgm"),
                    group_id,
                    provider_id,
                    model_id,
                    position as i64,
                    enabled
                ],
            )
            .await
            .map_err(database_error)?;
        self.model_group_member_by_key(group_id, provider_id, model_id)
            .await
    }

    pub async fn delete_model_group_member(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<()> {
        let group_id = normalize_setting_id(group_id, "grp");
        let provider_id = normalize_provider_id(provider_id);
        self.connection
            .execute(
                "DELETE FROM model_group_members WHERE group_id = ?1 AND provider_id = ?2 AND model_id = ?3",
                params![group_id, provider_id, model_id.trim()],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn set_default_model_group(
        &self,
        key: &str,
        group_id: &str,
    ) -> HamburResult<DefaultModelGroupRecord> {
        let key = normalize_default_group_key(key)?;
        self.model_group_by_id(group_id).await?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    group_id = excluded.group_id,
                    updated_at_ms = excluded.updated_at_ms",
                params![key.clone(), group_id, now as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(DefaultModelGroupRecord {
            key,
            group_id: group_id.to_string(),
            updated_at_ms: now,
        })
    }

    pub async fn upsert_app_setting(
        &self,
        key: &str,
        value: &str,
    ) -> HamburResult<AppSettingRecord> {
        let key = normalize_app_setting_key(key)?;
        let value = normalize_app_setting_value(&key, value)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO app_settings (key, value, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at_ms = excluded.updated_at_ms",
                params![key.clone(), value.clone(), now as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(AppSettingRecord {
            key,
            value,
            updated_at_ms: now,
        })
    }

    pub async fn delete_model_group(&self, group_id: &str) -> HamburResult<()> {
        let group_id = normalize_setting_id(group_id, "grp");
        self.connection
            .execute(
                "DELETE FROM default_model_groups WHERE group_id = ?1",
                params![group_id.clone()],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute("DELETE FROM model_groups WHERE id = ?1", params![group_id])
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn delete_app_setting(&self, key: &str) -> HamburResult<()> {
        let key = normalize_app_setting_key(key)?;
        self.connection
            .execute("DELETE FROM app_settings WHERE key = ?1", params![key])
            .await
            .map_err(database_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_config_audit(
        &self,
        command_id: &str,
        actor: &str,
        action: &str,
        target_kind: &str,
        target_id: &str,
        redacted_summary: &str,
        approval_required: bool,
        approval_token: &str,
    ) -> HamburResult<ConfigAuditRecord> {
        let id = new_id("audit");
        let now = now_ms();
        let record = ConfigAuditRecord {
            id: id.clone(),
            command_id: command_id.trim().chars().take(120).collect(),
            actor: actor.trim().chars().take(80).collect(),
            action: action.trim().chars().take(80).collect(),
            target_kind: target_kind.trim().chars().take(80).collect(),
            target_id: target_id.trim().chars().take(180).collect(),
            redacted_summary: redacted_summary.trim().chars().take(500).collect(),
            approval_required,
            approval_token: approval_token.trim().chars().take(160).collect(),
            created_at_ms: now,
        };
        self.connection
            .execute(
                "INSERT INTO config_audit
                    (id, command_id, actor, action, target_kind, target_id, redacted_summary, approval_required, approval_token, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    record.id.clone(),
                    record.command_id.clone(),
                    record.actor.clone(),
                    record.action.clone(),
                    record.target_kind.clone(),
                    record.target_id.clone(),
                    record.redacted_summary.clone(),
                    record.approval_required,
                    record.approval_token.clone(),
                    record.created_at_ms as i64
                ],
            )
            .await
            .map_err(database_error)?;
        Ok(record)
    }

    pub async fn settings_snapshot(&self) -> HamburResult<SettingsSnapshot> {
        Ok(SettingsSnapshot {
            providers: self.public_providers().await?,
            provider_models: self.all_provider_models().await?,
            model_groups: self.model_groups().await?,
            model_group_members: self.model_group_members().await?,
            default_model_groups: self.default_model_groups().await?,
            settings: self.app_settings().await?,
            config_audits: self.config_audits(40).await?,
        })
    }

    pub async fn primary_chat_route(&self) -> HamburResult<Vec<ModelRouteSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    p.id,
                    p.name,
                    p.api_type,
                    p.base_url,
                    p.secret_ref,
                    pm.model_id,
                    pm.display_name,
                    mg.id,
                    mg.name,
                    mg.routing_strategy,
                    mg.fallback_policy,
                    mgm.position,
                    pm.supports_tool_call,
                    pm.supports_reasoning,
                    pm.supports_image_input,
                    pm.supports_structured_output,
                    pm.supports_temperature,
                    pm.context_limit,
                    pm.output_limit,
                    pm.reasoning_field
                FROM default_model_groups d
                JOIN model_groups mg ON mg.id = d.group_id
                JOIN model_group_members mgm ON mgm.group_id = mg.id
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = p.id AND pm.model_id = mgm.model_id
                WHERE d.key = 'primary'
                  AND p.enabled = 1
                  AND mgm.enabled = 1
                ORDER BY mgm.position ASC, p.id ASC, pm.model_id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;

        let mut targets = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            targets.push(model_route_from_row(&row)?);
        }

        if targets.is_empty() {
            let fallback = self.first_enabled_model_route().await?;
            targets.extend(fallback);
        }
        Ok(targets)
    }

    async fn first_enabled_model_route(&self) -> HamburResult<Vec<ModelRouteSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    p.id,
                    p.name,
                    p.api_type,
                    p.base_url,
                    p.secret_ref,
                    pm.model_id,
                    pm.display_name,
                    'grp_primary_chat',
                    'Primary Chat',
                    'fallback',
                    'default',
                    0,
                    pm.supports_tool_call,
                    pm.supports_reasoning,
                    pm.supports_image_input,
                    pm.supports_structured_output,
                    pm.supports_temperature,
                    pm.context_limit,
                    pm.output_limit,
                    pm.reasoning_field
                FROM providers p
                JOIN provider_models pm ON pm.provider_id = p.id
                WHERE p.enabled = 1
                ORDER BY p.updated_at_ms DESC, pm.model_id ASC
                LIMIT 1
                ",
                params![],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ProviderUnavailable(
                "no enabled provider model is configured".to_string(),
            ));
        };
        Ok(vec![model_route_from_row(&row)?])
    }

    async fn migrate(&self) -> HamburResult<()> {
        self.connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    pinned_at_ms INTEGER NOT NULL DEFAULT 0,
                    deleted_at_ms INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_active_updated
                    ON sessions(deleted_at_ms, pinned_at_ms DESC, updated_at_ms DESC, created_at_ms DESC);

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    turn_id TEXT NOT NULL DEFAULT '',
                    role TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'completed',
                    content_text TEXT NOT NULL,
                    reasoning_content TEXT NOT NULL DEFAULT '',
                    created_at_ms INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL DEFAULT 1,
                    provider_id_snapshot TEXT NOT NULL DEFAULT '',
                    provider_name_snapshot TEXT NOT NULL DEFAULT '',
                    provider_protocol TEXT NOT NULL DEFAULT '',
                    model_id_snapshot TEXT NOT NULL DEFAULT '',
                    model_name_snapshot TEXT NOT NULL DEFAULT '',
                    model_group_id TEXT NOT NULL DEFAULT '',
                    finish_reason TEXT NOT NULL DEFAULT '',
                    native_finish_reason TEXT NOT NULL DEFAULT '',
                    tool_call_id TEXT NOT NULL DEFAULT '',
                    tool_name TEXT NOT NULL DEFAULT '',
                    tool_title TEXT NOT NULL DEFAULT ''
                );

                CREATE INDEX IF NOT EXISTS idx_messages_session_order
                    ON messages(session_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS timeline_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    stable_key TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    display_sequence INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    payload_ref TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    visible INTEGER NOT NULL DEFAULT 1,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_timeline_items_session_order
                    ON timeline_items(session_id, display_sequence, id);

                CREATE TABLE IF NOT EXISTS markdown_blocks (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL,
                    block_id INTEGER NOT NULL,
                    stable_key TEXT NOT NULL,
                    committed INTEGER NOT NULL,
                    payload_json TEXT NOT NULL,
                    raw TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_markdown_blocks_session_message
                    ON markdown_blocks(session_id, message_id, block_id);

                CREATE TABLE IF NOT EXISTS turns (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    status TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    finished_at_ms INTEGER,
                    selected_provider_id TEXT NOT NULL DEFAULT '',
                    selected_provider_name TEXT NOT NULL DEFAULT '',
                    provider_protocol TEXT NOT NULL DEFAULT '',
                    selected_model_id TEXT NOT NULL DEFAULT '',
                    selected_model_name TEXT NOT NULL DEFAULT '',
                    model_group_id TEXT NOT NULL DEFAULT '',
                    error_code TEXT NOT NULL DEFAULT '',
                    error_message TEXT NOT NULL DEFAULT ''
                );

                CREATE INDEX IF NOT EXISTS idx_turns_session_updated
                    ON turns(session_id, updated_at_ms DESC, id);

                CREATE TABLE IF NOT EXISTS trace_spans (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    parent_span_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER,
                    tool_call_id TEXT NOT NULL DEFAULT '',
                    payload_json TEXT NOT NULL DEFAULT '',
                    visible INTEGER NOT NULL DEFAULT 1
                );

                CREATE INDEX IF NOT EXISTS idx_trace_spans_session_started
                    ON trace_spans(session_id, started_at_ms, id);

                CREATE TABLE IF NOT EXISTS tool_calls (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    assistant_message_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    arguments_json TEXT NOT NULL,
                    display_title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    requires_approval INTEGER NOT NULL,
                    approval_status TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER,
                    result_id TEXT NOT NULL DEFAULT '',
                    error_code TEXT NOT NULL DEFAULT '',
                    error_message TEXT NOT NULL DEFAULT ''
                );

                CREATE INDEX IF NOT EXISTS idx_tool_calls_turn_started
                    ON tool_calls(turn_id, started_at_ms, id);

                CREATE TABLE IF NOT EXISTS tool_results (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    tool_call_id TEXT NOT NULL REFERENCES tool_calls(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL,
                    is_error INTEGER NOT NULL,
                    content_json TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    artifacts_json TEXT NOT NULL,
                    trust_level TEXT NOT NULL,
                    truncated INTEGER NOT NULL,
                    offloaded_file_id TEXT NOT NULL DEFAULT '',
                    offloaded_path TEXT NOT NULL DEFAULT '',
                    context_stub TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_tool_results_tool_call
                    ON tool_results(tool_call_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS files (
                    id TEXT PRIMARY KEY NOT NULL,
                    scope TEXT NOT NULL,
                    session_id TEXT NOT NULL DEFAULT '',
                    relative_path TEXT NOT NULL,
                    sandbox_path TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    sha256 TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    retention_policy TEXT NOT NULL
                );

                CREATE UNIQUE INDEX IF NOT EXISTS idx_files_sandbox_path
                    ON files(sandbox_path);

                CREATE INDEX IF NOT EXISTS idx_files_session_scope
                    ON files(session_id, scope, created_at_ms);

                CREATE TABLE IF NOT EXISTS attachments (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    origin_type TEXT NOT NULL,
                    original_uri TEXT NOT NULL,
                    file_id TEXT NOT NULL REFERENCES files(id),
                    sandbox_path TEXT NOT NULL,
                    width INTEGER NOT NULL DEFAULT 0,
                    height INTEGER NOT NULL DEFAULT 0,
                    sha256 TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_attachments_session_pending
                    ON attachments(session_id, message_id, status, created_at_ms);

                CREATE INDEX IF NOT EXISTS idx_attachments_message
                    ON attachments(message_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS file_cleanup_jobs (
                    id TEXT PRIMARY KEY NOT NULL,
                    file_id TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    status TEXT NOT NULL,
                    attempts INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_file_cleanup_jobs_status
                    ON file_cleanup_jobs(status, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS providers (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    icon_name TEXT NOT NULL,
                    api_type TEXT NOT NULL,
                    base_url TEXT NOT NULL,
                    secret_ref TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS provider_models (
                    id TEXT PRIMARY KEY NOT NULL,
                    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                    model_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    supports_tool_call INTEGER NOT NULL,
                    supports_reasoning INTEGER NOT NULL,
                    supports_image_input INTEGER NOT NULL,
                    supports_structured_output INTEGER NOT NULL,
                    supports_temperature INTEGER NOT NULL,
                    context_limit INTEGER NOT NULL,
                    output_limit INTEGER NOT NULL,
                    reasoning_field TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    synced_at_ms INTEGER NOT NULL,
                    UNIQUE(provider_id, model_id)
                );

                CREATE TABLE IF NOT EXISTS model_groups (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    routing_strategy TEXT NOT NULL,
                    fallback_policy TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS model_group_members (
                    id TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL REFERENCES model_groups(id) ON DELETE CASCADE,
                    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                    model_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    enabled INTEGER NOT NULL,
                    UNIQUE(group_id, provider_id, model_id)
                );

                CREATE TABLE IF NOT EXISTS default_model_groups (
                    key TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS app_settings (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS config_audit (
                    id TEXT PRIMARY KEY NOT NULL,
                    command_id TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    action TEXT NOT NULL,
                    target_kind TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    redacted_summary TEXT NOT NULL,
                    approval_required INTEGER NOT NULL,
                    approval_token TEXT NOT NULL DEFAULT '',
                    created_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_config_audit_created
                    ON config_audit(created_at_ms DESC, id DESC);
                ",
            )
            .await
            .map_err(database_error)?;

        self.add_column_if_missing("sessions", "pinned_at_ms", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.add_column_if_missing("messages", "turn_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "status", "TEXT NOT NULL DEFAULT 'completed'")
            .await?;
        self.add_column_if_missing("messages", "reasoning_content", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "provider_id_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing(
            "messages",
            "provider_name_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "provider_protocol", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "model_id_snapshot", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "model_name_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "model_group_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "finish_reason", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "native_finish_reason",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "tool_call_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "tool_name", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "tool_title", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("timeline_items", "visible", "INTEGER NOT NULL DEFAULT 1")
            .await?;
        self.add_column_if_missing("turns", "selected_provider_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "turns",
            "selected_provider_name",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("turns", "provider_protocol", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "selected_model_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "selected_model_name", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "model_group_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "error_code", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "error_message", "TEXT NOT NULL DEFAULT ''")
            .await?;

        Ok(())
    }

    async fn add_column_if_missing(
        &self,
        table: &str,
        column: &str,
        definition: &str,
    ) -> HamburResult<()> {
        let pragma = format!("PRAGMA table_info({table})");
        let mut rows = self
            .connection
            .query(pragma.as_str(), params![])
            .await
            .map_err(database_error)?;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let name = row.get::<String>(1).map_err(database_error)?;
            if name == column {
                return Ok(());
            }
        }

        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        self.connection
            .execute(sql.as_str(), params![])
            .await
            .map(|_| ())
            .map_err(database_error)
    }

    async fn snapshot_for_selected(
        &self,
        requested_session_id: Option<&str>,
    ) -> HamburResult<AppSnapshot> {
        let sessions = self.list_sessions().await?;
        let active_session_id = self.active_session_id().await?;
        let selected_session_id = match requested_session_id {
            Some(session_id) if sessions.iter().any(|session| session.id == session_id) => {
                session_id.to_string()
            }
            _ if active_session_id.as_ref().is_some_and(|session_id| {
                sessions.iter().any(|session| session.id == *session_id)
            }) =>
            {
                active_session_id.unwrap_or_default()
            }
            _ => sessions
                .first()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
        };
        let timeline_items = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            self.timeline_items_for_session(&selected_session_id)
                .await?
        };
        let markdown_block_payloads = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            let payload_refs = markdown_payload_refs(&timeline_items);
            self.markdown_blocks_for_payload_refs(&selected_session_id, &payload_refs)
                .await?
        };
        let pending_attachments = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            self.pending_attachments_for_session(&selected_session_id)
                .await?
        };

        Ok(AppSnapshot {
            sessions,
            selected_session_id,
            timeline_items,
            markdown_block_payloads,
            pending_attachments,
        })
    }

    async fn list_sessions(&self) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(100, 0).await
    }

    async fn list_sessions_page(
        &self,
        limit: u32,
        offset: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let limit = clamp_limit(limit, 1, 200);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id AND ti.visible = 1
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                ORDER BY s.pinned_at_ms DESC, s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?1 OFFSET ?2
                ",
                params![limit as i64, offset as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }

    async fn timeline_items_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<TimelineItemSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    ti.id,
                    ti.stable_key,
                    ti.content_type,
                    ti.display_sequence,
                    ti.version_sequence,
                    ti.payload_ref,
                    ti.small_summary,
                    ti.kind,
                    COALESCE(ts.title, ''),
                    COALESCE(ts.content, ''),
                    COALESCE(ts.status, ''),
                    COALESCE(ts.tool_call_id, ''),
                    COALESCE(tc.name, '')
                FROM timeline_items ti
                LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                WHERE ti.session_id = ?1 AND ti.visible = 1
                ORDER BY ti.display_sequence ASC, ti.id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(TimelineItemSnapshot {
                id: row.get::<String>(0).map_err(database_error)?,
                stable_key: row.get::<String>(1).map_err(database_error)?,
                content_type: row.get::<String>(2).map_err(database_error)?,
                display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
                version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
                payload_ref: row.get::<String>(5).map_err(database_error)?,
                small_summary: row.get::<String>(6).map_err(database_error)?,
                kind: row.get::<String>(7).map_err(database_error)?,
                trace_title: row.get::<String>(8).map_err(database_error)?,
                trace_content: row.get::<String>(9).map_err(database_error)?,
                trace_status: row.get::<String>(10).map_err(database_error)?,
                tool_call_id: row.get::<String>(11).map_err(database_error)?,
                tool_name: row.get::<String>(12).map_err(database_error)?,
            });
        }

        Ok(items)
    }

    async fn timeline_items_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        let limit = clamp_limit(limit, 1, 100);
        let fetch_limit = limit.saturating_add(1);
        let mut rows = if before_cursor == 0 {
            self.connection
                .query(
                    "
                    SELECT
                        ti.id,
                        ti.stable_key,
                        ti.content_type,
                        ti.display_sequence,
                        ti.version_sequence,
                        ti.payload_ref,
                        ti.small_summary,
                        ti.kind,
                        COALESCE(ts.title, ''),
                        COALESCE(ts.content, ''),
                        COALESCE(ts.status, ''),
                        COALESCE(ts.tool_call_id, ''),
                        COALESCE(tc.name, '')
                    FROM timeline_items ti
                    LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                    LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                    WHERE ti.session_id = ?1 AND ti.visible = 1
                    ORDER BY ti.display_sequence DESC, ti.id DESC
                    LIMIT ?2
                    ",
                    params![session_id, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        } else {
            self.connection
                .query(
                    "
                    SELECT
                        ti.id,
                        ti.stable_key,
                        ti.content_type,
                        ti.display_sequence,
                        ti.version_sequence,
                        ti.payload_ref,
                        ti.small_summary,
                        ti.kind,
                        COALESCE(ts.title, ''),
                        COALESCE(ts.content, ''),
                        COALESCE(ts.status, ''),
                        COALESCE(ts.tool_call_id, ''),
                        COALESCE(tc.name, '')
                    FROM timeline_items ti
                    LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                    LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                    WHERE ti.session_id = ?1
                      AND ti.visible = 1
                      AND ti.display_sequence < ?2
                    ORDER BY ti.display_sequence DESC, ti.id DESC
                    LIMIT ?3
                    ",
                    params![session_id, before_cursor as i64, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        };

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(timeline_item_from_row(&row)?);
        }

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        items.reverse();
        let next_before_cursor = if has_more {
            items
                .first()
                .map(|item| item.display_sequence)
                .unwrap_or_default()
        } else {
            0
        };

        Ok(TimelinePageData {
            items,
            markdown_block_payloads: Vec::new(),
            next_before_cursor,
            has_more,
        })
    }

    async fn session_exists(&self, session_id: &str) -> HamburResult<bool> {
        let mut rows = self
            .connection
            .query(
                "SELECT id FROM sessions WHERE id = ?1 AND deleted_at_ms IS NULL LIMIT 1",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        Ok(rows.next().await.map_err(database_error)?.is_some())
    }

    async fn session_summary_by_id(&self, session_id: &str) -> HamburResult<SessionSummary> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.id = ?1 AND s.deleted_at_ms IS NULL
                LIMIT 1
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        };

        session_summary_from_row(&row)
    }

    async fn ensure_session_exists(&self, session_id: &str) -> HamburResult<()> {
        if self.session_exists(session_id).await? {
            Ok(())
        } else {
            Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )))
        }
    }

    async fn touch_session(&self, session_id: &str, now: u64) -> HamburResult<()> {
        self.connection
            .execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn set_active_session(&self, session_id: Option<&str>) -> HamburResult<()> {
        let now = now_ms();
        match session_id {
            Some(session_id) => self
                .connection
                .execute(
                    "INSERT INTO app_state (key, value, updated_at_ms)
                     VALUES ('active_session_id', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET
                        value = excluded.value,
                        updated_at_ms = excluded.updated_at_ms",
                    params![session_id, now as i64],
                )
                .await
                .map(|_| ())
                .map_err(database_error),
            None => self
                .connection
                .execute(
                    "DELETE FROM app_state WHERE key = 'active_session_id'",
                    params![],
                )
                .await
                .map(|_| ())
                .map_err(database_error),
        }
    }

    async fn active_session_id(&self) -> HamburResult<Option<String>> {
        let mut rows = self
            .connection
            .query(
                "SELECT value FROM app_state WHERE key = 'active_session_id' LIMIT 1",
                params![],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };
        let value = row.get::<String>(0).map_err(database_error)?;
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    async fn timeline_item_by_stable_key(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<TimelineItemSnapshot> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    ti.id,
                    ti.stable_key,
                    ti.content_type,
                    ti.display_sequence,
                    ti.version_sequence,
                    ti.payload_ref,
                    ti.small_summary,
                    ti.kind,
                    COALESCE(ts.title, ''),
                    COALESCE(ts.content, ''),
                    COALESCE(ts.status, ''),
                    COALESCE(ts.tool_call_id, ''),
                    COALESCE(tc.name, '')
                FROM timeline_items ti
                LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                WHERE ti.session_id = ?1 AND ti.stable_key = ?2 AND ti.visible = 1
                LIMIT 1
                ",
                params![session_id, stable_key],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::Internal(format!(
                "timeline item not found after upsert: {stable_key}"
            )));
        };

        timeline_item_from_row(&row)
    }

    async fn message_by_id(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    role,
                    content_text,
                    reasoning_content,
                    status,
                    turn_id,
                    created_at_ms,
                    version_sequence,
                    provider_id_snapshot,
                    provider_name_snapshot,
                    provider_protocol,
                    model_id_snapshot,
                    model_name_snapshot,
                    model_group_id,
                    finish_reason,
                    native_finish_reason
                FROM messages
                WHERE id = ?1
                LIMIT 1
                ",
                params![message_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };

        Ok(Some(message_from_row(&row)?))
    }

    async fn turn_by_id(&self, turn_id: &str) -> HamburResult<TurnRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    status,
                    created_at_ms,
                    updated_at_ms,
                    finished_at_ms,
                    selected_provider_id,
                    selected_provider_name,
                    provider_protocol,
                    selected_model_id,
                    selected_model_name,
                    model_group_id,
                    error_code,
                    error_message
                FROM turns
                WHERE id = ?1
                LIMIT 1
                ",
                params![turn_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        };

        Ok(TurnRecord {
            id: row.get::<String>(0).map_err(database_error)?,
            session_id: row.get::<String>(1).map_err(database_error)?,
            status: row.get::<String>(2).map_err(database_error)?,
            created_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
            updated_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
            finished_at_ms: row
                .get::<Option<i64>>(5)
                .map_err(database_error)?
                .map(unsigned_ms)
                .unwrap_or_default(),
            selected_provider_id: row.get::<String>(6).map_err(database_error)?,
            selected_provider_name: row.get::<String>(7).map_err(database_error)?,
            provider_protocol: row.get::<String>(8).map_err(database_error)?,
            selected_model_id: row.get::<String>(9).map_err(database_error)?,
            selected_model_name: row.get::<String>(10).map_err(database_error)?,
            model_group_id: row.get::<String>(11).map_err(database_error)?,
            error_code: row.get::<String>(12).map_err(database_error)?,
            error_message: row.get::<String>(13).map_err(database_error)?,
        })
    }

    async fn trace_span_by_id(&self, trace_id: &str) -> HamburResult<TraceSpanRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    turn_id,
                    parent_span_id,
                    kind,
                    title,
                    content,
                    status,
                    started_at_ms,
                    ended_at_ms,
                    tool_call_id,
                    payload_json
                FROM trace_spans
                WHERE id = ?1
                LIMIT 1
                ",
                params![trace_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "trace span not found: {trace_id}"
            )));
        };
        trace_span_from_row(&row)
    }

    async fn tool_call_by_id(&self, tool_call_id: &str) -> HamburResult<ToolCallRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    turn_id,
                    assistant_message_id,
                    name,
                    arguments_json,
                    display_title,
                    status,
                    requires_approval,
                    approval_status,
                    started_at_ms,
                    ended_at_ms,
                    result_id,
                    error_code,
                    error_message
                FROM tool_calls
                WHERE id = ?1
                LIMIT 1
                ",
                params![tool_call_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "tool call not found: {tool_call_id}"
            )));
        };
        tool_call_from_row(&row)
    }

    async fn tool_result_by_id(&self, result_id: &str) -> HamburResult<ToolResultRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    turn_id,
                    tool_call_id,
                    message_id,
                    is_error,
                    content_json,
                    summary,
                    artifacts_json,
                    trust_level,
                    truncated,
                    offloaded_file_id,
                    offloaded_path,
                    context_stub,
                    created_at_ms
                FROM tool_results
                WHERE id = ?1
                LIMIT 1
                ",
                params![result_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "tool result not found: {result_id}"
            )));
        };
        tool_result_from_row(&row)
    }

    pub async fn file_by_id(&self, file_id: &str) -> HamburResult<FileRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    scope,
                    session_id,
                    relative_path,
                    sandbox_path,
                    mime_type,
                    byte_size,
                    sha256,
                    created_at_ms,
                    updated_at_ms,
                    retention_policy
                FROM files
                WHERE id = ?1
                LIMIT 1
                ",
                params![file_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file not found: {file_id}"
            )));
        };
        file_from_row(&row)
    }

    pub async fn attachment_by_id(&self, attachment_id: &str) -> HamburResult<AttachmentRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    kind,
                    display_name,
                    mime_type,
                    byte_size,
                    origin_type,
                    original_uri,
                    file_id,
                    sandbox_path,
                    width,
                    height,
                    sha256,
                    status,
                    created_at_ms,
                    updated_at_ms
                FROM attachments
                WHERE id = ?1
                LIMIT 1
                ",
                params![attachment_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "attachment not found: {attachment_id}"
            )));
        };
        attachment_from_row(&row)
    }

    pub async fn attachments_for_message(
        &self,
        message_id: &str,
    ) -> HamburResult<Vec<AttachmentRecord>> {
        if message_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    kind,
                    display_name,
                    mime_type,
                    byte_size,
                    origin_type,
                    original_uri,
                    file_id,
                    sandbox_path,
                    width,
                    height,
                    sha256,
                    status,
                    created_at_ms,
                    updated_at_ms
                FROM attachments
                WHERE message_id = ?1
                  AND status = 'attached'
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![message_id],
            )
            .await
            .map_err(database_error)?;
        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            attachments.push(attachment_from_row(&row)?);
        }
        Ok(attachments)
    }

    async fn file_cleanup_job_by_id(&self, job_id: &str) -> HamburResult<FileCleanupJobRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, file_id, relative_path, reason, status, attempts, created_at_ms, updated_at_ms
                FROM file_cleanup_jobs
                WHERE id = ?1
                LIMIT 1
                ",
                params![job_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file cleanup job not found: {job_id}"
            )));
        };
        file_cleanup_job_from_row(&row)
    }

    async fn public_providers(&self) -> HamburResult<Vec<PublicProviderRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms
                FROM providers
                ORDER BY updated_at_ms DESC, id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut providers = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            providers.push(public_provider_from_row(&row)?);
        }
        Ok(providers)
    }

    async fn all_provider_models(&self) -> HamburResult<Vec<ProviderModelRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    provider_id,
                    model_id,
                    display_name,
                    supports_tool_call,
                    supports_reasoning,
                    supports_image_input,
                    supports_structured_output,
                    supports_temperature,
                    context_limit,
                    output_limit,
                    reasoning_field,
                    metadata_json,
                    synced_at_ms
                FROM provider_models
                ORDER BY provider_id ASC, model_id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut models = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            models.push(provider_model_from_row(&row)?);
        }
        Ok(models)
    }

    async fn model_groups(&self) -> HamburResult<Vec<ModelGroupRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms
                FROM model_groups
                ORDER BY id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut groups = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            groups.push(model_group_from_row(&row)?);
        }
        Ok(groups)
    }

    async fn model_group_by_id(&self, group_id: &str) -> HamburResult<ModelGroupRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms
                FROM model_groups
                WHERE id = ?1
                LIMIT 1
                ",
                params![group_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "model group not found: {group_id}"
            )));
        };
        model_group_from_row(&row)
    }

    async fn model_group_members(&self) -> HamburResult<Vec<ModelGroupMemberRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    mgm.id,
                    mgm.group_id,
                    mgm.provider_id,
                    p.name,
                    mgm.model_id,
                    pm.display_name,
                    mgm.position,
                    mgm.enabled
                FROM model_group_members mgm
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = mgm.provider_id AND pm.model_id = mgm.model_id
                ORDER BY mgm.group_id ASC, mgm.position ASC, mgm.provider_id ASC, mgm.model_id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut members = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            members.push(model_group_member_from_row(&row)?);
        }
        Ok(members)
    }

    async fn model_group_member_by_key(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<ModelGroupMemberRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    mgm.id,
                    mgm.group_id,
                    mgm.provider_id,
                    p.name,
                    mgm.model_id,
                    pm.display_name,
                    mgm.position,
                    mgm.enabled
                FROM model_group_members mgm
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = mgm.provider_id AND pm.model_id = mgm.model_id
                WHERE mgm.group_id = ?1 AND mgm.provider_id = ?2 AND mgm.model_id = ?3
                LIMIT 1
                ",
                params![group_id, provider_id, model_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "model group member not found: {group_id}/{provider_id}/{model_id}"
            )));
        };
        model_group_member_from_row(&row)
    }

    async fn default_model_groups(&self) -> HamburResult<Vec<DefaultModelGroupRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT key, group_id, updated_at_ms
                FROM default_model_groups
                ORDER BY key ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut defaults = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            defaults.push(DefaultModelGroupRecord {
                key: row.get::<String>(0).map_err(database_error)?,
                group_id: row.get::<String>(1).map_err(database_error)?,
                updated_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
            });
        }
        Ok(defaults)
    }

    async fn app_settings(&self) -> HamburResult<Vec<AppSettingRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT key, value, updated_at_ms
                FROM app_settings
                ORDER BY key ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut settings = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            settings.push(AppSettingRecord {
                key: row.get::<String>(0).map_err(database_error)?,
                value: row.get::<String>(1).map_err(database_error)?,
                updated_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
            });
        }
        Ok(settings)
    }

    async fn config_audits(&self, limit: u32) -> HamburResult<Vec<ConfigAuditRecord>> {
        let limit = clamp_limit(limit, 1, 100);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    command_id,
                    actor,
                    action,
                    target_kind,
                    target_id,
                    redacted_summary,
                    approval_required,
                    approval_token,
                    created_at_ms
                FROM config_audit
                ORDER BY created_at_ms DESC, id DESC
                LIMIT ?1
                ",
                params![limit as i64],
            )
            .await
            .map_err(database_error)?;
        let mut audits = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            audits.push(config_audit_from_row(&row)?);
        }
        Ok(audits)
    }
}

fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        DEFAULT_SESSION_TITLE.to_string()
    } else {
        title.chars().take(120).collect()
    }
}

fn normalize_setting_id(value: &str, prefix: &str) -> String {
    let normalized = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        .take(120)
        .collect::<String>();
    if normalized.is_empty() {
        new_id(prefix)
    } else {
        normalized
    }
}

fn normalize_routing_strategy(value: &str) -> HamburResult<String> {
    let value = value.trim();
    match value {
        "" | "fallback" | "priority" => Ok("fallback".to_string()),
        "load_balance" | "round_robin" => Ok("load_balance".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid routing_strategy: {value}"
        ))),
    }
}

fn normalize_fallback_policy(value: &str) -> HamburResult<String> {
    let value = value.trim();
    match value {
        "" | "default" | "never" => Ok("default".to_string()),
        "always" | "always_before_output" => Ok("always".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid fallback_policy: {value}"
        ))),
    }
}

fn normalize_default_group_key(value: &str) -> HamburResult<String> {
    let key = value.trim();
    match key {
        "primary" | "secondary" | "vision" | "tools" => Ok(key.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid default model group key: {key}"
        ))),
    }
}

fn normalize_app_setting_key(value: &str) -> HamburResult<String> {
    let key = value.trim();
    if key.starts_with("skill_enabled:")
        || key.starts_with("startup_task:")
        || key.starts_with("rootfs_setting:")
    {
        return normalize_prefixed_setting_key(key);
    }

    let allowed = [
        "themeMode",
        "fontScale",
        "startupChatMode",
        "lastSelectedSessionId",
        "loggingEnabled",
        "predictiveBackEnabled",
        "fpsOverlayEnabled",
        "rootfsBackend",
        "webFetchBackend",
        "viewImageScaleMode",
        "defaultDeepThinkingEnabled",
        "startupTasksEnabled",
        "tool_settings",
        "skills",
        "memory_projections",
        "startup_tasks",
        "rootfs_settings",
        "browser_tool_settings",
        "appearance",
        "logs",
        "token_usage",
        "persona",
        "environment_variables",
        "deep_thinking",
        "search",
    ];
    if allowed.contains(&key) {
        Ok(key.to_string())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "invalid app setting key: {key}"
        )))
    }
}

fn normalize_prefixed_setting_key(key: &str) -> HamburResult<String> {
    let mut parts = key.splitn(2, ':');
    let prefix = parts.next().unwrap_or_default();
    let id = parts.next().unwrap_or_default();
    if id.trim().is_empty() {
        return Err(HamburError::InvalidCommand(format!(
            "invalid app setting key: {key}"
        )));
    }
    let id = if prefix == "skill_enabled" {
        normalize_skill_setting_key_suffix(id)?
    } else {
        normalize_setting_key_suffix(id)
    };
    Ok(format!("{prefix}:{id}"))
}

fn normalize_skill_setting_key_suffix(value: &str) -> HamburResult<String> {
    let value = value
        .trim()
        .trim_start_matches("/var/hambur/skills/")
        .trim_start_matches('/');
    if value.is_empty() || value.contains("..") || value.contains('\\') {
        return Err(HamburError::InvalidCommand(
            "invalid skill setting key".to_string(),
        ));
    }
    if !value.ends_with("/SKILL.md") {
        return Ok(normalize_setting_key_suffix(value));
    }
    Ok(value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
        .take(240)
        .collect())
}

fn normalize_setting_key_suffix(value: &str) -> String {
    let mut output = String::new();
    let mut previous_separator = false;
    for ch in value.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            ch
        } else {
            '-'
        };
        if next == '-' {
            if !previous_separator {
                output.push(next);
            }
            previous_separator = true;
        } else {
            output.push(next);
            previous_separator = false;
        }
        if output.len() >= 120 {
            break;
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        new_id("setting")
    } else {
        output
    }
}

fn normalize_app_setting_value(key: &str, value: &str) -> HamburResult<String> {
    let value = value.trim();
    match key {
        "themeMode" => normalize_enum_setting(key, value, &["system", "light", "dark"]),
        "fontScale" => {
            normalize_enum_setting(key, value, &["small", "default", "large", "extra_large"])
        }
        "startupChatMode" => normalize_enum_setting(key, value, &["new_chat", "last_chat"]),
        "rootfsBackend" => normalize_enum_setting(key, value, &["chroot", "proot"]),
        "webFetchBackend" => normalize_enum_setting(key, value, &["local", "tinyfish"]),
        "viewImageScaleMode" => normalize_enum_setting(key, value, &["original", "resize_fit"]),
        "loggingEnabled"
        | "predictiveBackEnabled"
        | "fpsOverlayEnabled"
        | "defaultDeepThinkingEnabled"
        | "startupTasksEnabled" => normalize_bool_setting(key, value),
        "lastSelectedSessionId" => Ok(value.chars().take(160).collect()),
        "browser_tool_settings" => normalize_browser_tool_settings(value),
        key if key.starts_with("skill_enabled:") => normalize_bool_setting(key, value),
        "tool_settings"
        | "skills"
        | "memory_projections"
        | "startup_tasks"
        | "rootfs_settings"
        | "appearance"
        | "logs"
        | "token_usage"
        | "persona"
        | "environment_variables"
        | "deep_thinking"
        | "search" => normalize_json_or_text_setting(value),
        key if key.starts_with("startup_task:") || key.starts_with("rootfs_setting:") => {
            normalize_json_or_text_setting(value)
        }
        _ => Ok(value.chars().take(8000).collect()),
    }
}

fn normalize_enum_setting(key: &str, value: &str, allowed: &[&str]) -> HamburResult<String> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "invalid {key} value: {value}"
        )))
    }
}

fn normalize_bool_setting(key: &str, value: &str) -> HamburResult<String> {
    match value {
        "true" | "false" => Ok(value.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid {key} value: expected true or false"
        ))),
    }
}

fn normalize_json_or_text_setting(value: &str) -> HamburResult<String> {
    if value.len() > 8000 {
        return Err(HamburError::InvalidCommand(
            "setting value must be at most 8000 characters".to_string(),
        ));
    }
    if value.starts_with('{') || value.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(value).map_err(|error| {
            HamburError::InvalidCommand(format!("setting value must be valid JSON: {error}"))
        })?;
    }
    Ok(value.to_string())
}

fn normalize_browser_tool_settings(value: &str) -> HamburResult<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(value).map_err(|error| {
        HamburError::InvalidCommand(format!("browser tool settings must be JSON: {error}"))
    })?;
    let max_fetch_bytes = parsed
        .get("maxFetchBytes")
        .or_else(|| parsed.get("max_fetch_bytes"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1_000_000);
    if !(250_000..=10_000_000).contains(&max_fetch_bytes) {
        return Err(HamburError::InvalidCommand(
            "browser maxFetchBytes must be between 250000 and 10000000".to_string(),
        ));
    }
    let auto_close_minutes = parsed
        .get("autoCloseMinutes")
        .or_else(|| parsed.get("auto_close_minutes"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if auto_close_minutes > 240 {
        return Err(HamburError::InvalidCommand(
            "browser autoCloseMinutes must be between 0 and 240".to_string(),
        ));
    }
    let accept_cookies = parsed
        .get("acceptCookies")
        .or_else(|| parsed.get("accept_cookies"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let accept_third_party = parsed
        .get("acceptThirdPartyCookies")
        .or_else(|| parsed.get("accept_third_party_cookies"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !accept_cookies && accept_third_party {
        return Err(HamburError::InvalidCommand(
            "browser acceptThirdPartyCookies must be false when acceptCookies is false".to_string(),
        ));
    }
    normalize_json_or_text_setting(value)
}

fn normalize_role(role: &str) -> HamburResult<String> {
    let role = role.trim();
    match role {
        "system" | "user" | "assistant" | "tool" => Ok(role.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid message role: {role}"
        ))),
    }
}

fn normalize_status(status: &str) -> HamburResult<String> {
    let status = status.trim();
    if status.is_empty() {
        return Err(HamburError::InvalidCommand(
            "status must not be empty".to_string(),
        ));
    }

    Ok(status.chars().take(80).collect())
}

fn unsigned_ms(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn unsigned_count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn clamp_limit(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

fn normalize_provider_id(id: &str) -> String {
    let id = id.trim();
    if id.is_empty() {
        new_id("provider")
    } else {
        id.chars().take(120).collect()
    }
}

fn normalize_base_url(base_url: &str) -> HamburResult<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(HamburError::InvalidCommand(
            "provider base_url must not be empty".to_string(),
        ));
    }
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(HamburError::InvalidCommand(
            "provider base_url must be http or https".to_string(),
        ));
    }
    Ok(base_url.chars().take(512).collect())
}

fn normalize_secret_ref(secret_ref: &str) -> HamburResult<String> {
    let secret_ref = secret_ref.trim();
    if secret_ref.is_empty() {
        return Err(HamburError::InvalidCommand(
            "provider secret_ref must not be empty".to_string(),
        ));
    }
    let lower = secret_ref.to_ascii_lowercase();
    if secret_ref.starts_with("sk-")
        || lower.starts_with("bearer ")
        || lower.contains("api_key=")
        || lower.contains("apikey=")
    {
        return Err(HamburError::InvalidCommand(
            "provider secret_ref must reference Android Secret Store, not a raw API key"
                .to_string(),
        ));
    }
    Ok(secret_ref.chars().take(256).collect())
}

fn secret_label(secret_ref: &str) -> String {
    let secret_ref = secret_ref.trim();
    if secret_ref.is_empty() {
        "Not configured".to_string()
    } else if secret_ref.starts_with("android-secret://") {
        "Android Secret Store".to_string()
    } else if secret_ref.starts_with("env://") {
        "Environment Secret".to_string()
    } else {
        "Secret reference".to_string()
    }
}

fn normalize_file_scope(scope: &str) -> HamburResult<String> {
    let scope = scope.trim();
    match scope {
        "session" | "global" | "cache" => Ok(scope.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid file scope: {scope}"
        ))),
    }
}

fn normalize_retention_policy(policy: &str) -> HamburResult<String> {
    let policy = policy.trim();
    match policy {
        "keep" | "delete_with_session" | "cache" => Ok(policy.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid retention policy: {policy}"
        ))),
    }
}

fn normalize_db_path(path: &str, label: &str) -> HamburResult<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(HamburError::InvalidCommand(format!(
            "{label} must not be empty"
        )));
    }
    if path.contains('\0') || path.contains("..") {
        return Err(HamburError::InvalidCommand(format!(
            "{label} must not contain traversal"
        )));
    }
    Ok(path.chars().take(1024).collect())
}

fn normalize_mime_type(mime_type: &str) -> String {
    let mime_type = mime_type.trim().to_ascii_lowercase();
    let value = if mime_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime_type
    };
    value.chars().take(160).collect()
}

fn normalize_origin_type(origin_type: &str) -> String {
    let origin_type = origin_type.trim();
    match origin_type {
        "content_uri" | "file" | "camera" | "share" | "sandbox" | "generated" => {
            origin_type.to_string()
        }
        _ => "content_uri".to_string(),
    }
}

fn normalize_display_name(display_name: &str) -> String {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        "attachment".to_string()
    } else {
        display_name.chars().take(160).collect()
    }
}

fn normalize_attachment_kind(kind: &str, mime_type: &str) -> String {
    let kind = kind.trim();
    match kind {
        "image" | "file" | "audio" | "video" | "other" => kind.to_string(),
        _ => {
            let mime_type = mime_type.trim().to_ascii_lowercase();
            if mime_type.starts_with("image/") {
                "image".to_string()
            } else if mime_type.starts_with("audio/") {
                "audio".to_string()
            } else if mime_type.starts_with("video/") {
                "video".to_string()
            } else {
                "file".to_string()
            }
        }
    }
}

fn normalize_attachment_status(status: &str) -> HamburResult<String> {
    let status = status.trim();
    match status {
        "pending" | "attached" | "removed" | "expired" => Ok(status.to_string()),
        "" => Ok("pending".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid attachment status: {status}"
        ))),
    }
}

fn sql_bool(value: i64) -> bool {
    value != 0
}

fn session_summary_from_row(row: &Row) -> HamburResult<SessionSummary> {
    Ok(SessionSummary {
        id: row.get::<String>(0).map_err(database_error)?,
        title: row.get::<String>(1).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        pinned_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        message_count: unsigned_count(row.get::<i64>(5).map_err(database_error)?),
        latest_preview: row.get::<String>(6).map_err(database_error)?,
    })
}

fn provider_record_from_row(row: &Row) -> HamburResult<ProviderRecord> {
    Ok(ProviderRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        icon_name: row.get::<String>(2).map_err(database_error)?,
        api_type: row.get::<String>(3).map_err(database_error)?,
        base_url: row.get::<String>(4).map_err(database_error)?,
        secret_ref: row.get::<String>(5).map_err(database_error)?,
        enabled: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
    })
}

fn public_provider_from_row(row: &Row) -> HamburResult<PublicProviderRecord> {
    let secret_ref = row.get::<String>(5).map_err(database_error)?;
    Ok(PublicProviderRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        icon_name: row.get::<String>(2).map_err(database_error)?,
        api_type: row.get::<String>(3).map_err(database_error)?,
        base_url: row.get::<String>(4).map_err(database_error)?,
        secret_label: secret_label(&secret_ref),
        enabled: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
    })
}

fn provider_model_from_row(row: &Row) -> HamburResult<ProviderModelRecord> {
    Ok(ProviderModelRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        provider_id: row.get::<String>(1).map_err(database_error)?,
        model_id: row.get::<String>(2).map_err(database_error)?,
        display_name: row.get::<String>(3).map_err(database_error)?,
        supports_tool_call: sql_bool(row.get::<i64>(4).map_err(database_error)?),
        supports_reasoning: sql_bool(row.get::<i64>(5).map_err(database_error)?),
        supports_image_input: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        supports_structured_output: sql_bool(row.get::<i64>(7).map_err(database_error)?),
        supports_temperature: sql_bool(row.get::<i64>(8).map_err(database_error)?),
        context_limit: unsigned_count(row.get::<i64>(9).map_err(database_error)?),
        output_limit: unsigned_count(row.get::<i64>(10).map_err(database_error)?),
        reasoning_field: row.get::<String>(11).map_err(database_error)?,
        metadata_json: row.get::<String>(12).map_err(database_error)?,
        synced_at_ms: unsigned_ms(row.get::<i64>(13).map_err(database_error)?),
    })
}

fn model_group_from_row(row: &Row) -> HamburResult<ModelGroupRecord> {
    Ok(ModelGroupRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        routing_strategy: row.get::<String>(2).map_err(database_error)?,
        fallback_policy: row.get::<String>(3).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(5).map_err(database_error)?),
    })
}

fn model_group_member_from_row(row: &Row) -> HamburResult<ModelGroupMemberRecord> {
    Ok(ModelGroupMemberRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        group_id: row.get::<String>(1).map_err(database_error)?,
        provider_id: row.get::<String>(2).map_err(database_error)?,
        provider_name: row.get::<String>(3).map_err(database_error)?,
        model_id: row.get::<String>(4).map_err(database_error)?,
        model_display_name: row.get::<String>(5).map_err(database_error)?,
        position: unsigned_count(row.get::<i64>(6).map_err(database_error)?),
        enabled: sql_bool(row.get::<i64>(7).map_err(database_error)?),
    })
}

fn config_audit_from_row(row: &Row) -> HamburResult<ConfigAuditRecord> {
    Ok(ConfigAuditRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        command_id: row.get::<String>(1).map_err(database_error)?,
        actor: row.get::<String>(2).map_err(database_error)?,
        action: row.get::<String>(3).map_err(database_error)?,
        target_kind: row.get::<String>(4).map_err(database_error)?,
        target_id: row.get::<String>(5).map_err(database_error)?,
        redacted_summary: row.get::<String>(6).map_err(database_error)?,
        approval_required: sql_bool(row.get::<i64>(7).map_err(database_error)?),
        approval_token: row.get::<String>(8).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
    })
}

fn model_route_from_row(row: &Row) -> HamburResult<ModelRouteSnapshot> {
    Ok(ModelRouteSnapshot {
        provider_id: row.get::<String>(0).map_err(database_error)?,
        provider_name: row.get::<String>(1).map_err(database_error)?,
        provider_protocol: row.get::<String>(2).map_err(database_error)?,
        base_url: row.get::<String>(3).map_err(database_error)?,
        secret_ref: row.get::<String>(4).map_err(database_error)?,
        model_id: row.get::<String>(5).map_err(database_error)?,
        model_display_name: row.get::<String>(6).map_err(database_error)?,
        model_group_id: row.get::<String>(7).map_err(database_error)?,
        model_group_name: row.get::<String>(8).map_err(database_error)?,
        routing_strategy: row.get::<String>(9).map_err(database_error)?,
        fallback_policy: row.get::<String>(10).map_err(database_error)?,
        position: unsigned_count(row.get::<i64>(11).map_err(database_error)?),
        supports_tool_call: sql_bool(row.get::<i64>(12).map_err(database_error)?),
        supports_reasoning: sql_bool(row.get::<i64>(13).map_err(database_error)?),
        supports_image_input: sql_bool(row.get::<i64>(14).map_err(database_error)?),
        supports_structured_output: sql_bool(row.get::<i64>(15).map_err(database_error)?),
        supports_temperature: sql_bool(row.get::<i64>(16).map_err(database_error)?),
        context_limit: unsigned_count(row.get::<i64>(17).map_err(database_error)?),
        output_limit: unsigned_count(row.get::<i64>(18).map_err(database_error)?),
        reasoning_field: row.get::<String>(19).map_err(database_error)?,
    })
}

fn timeline_item_from_row(row: &Row) -> HamburResult<TimelineItemSnapshot> {
    Ok(TimelineItemSnapshot {
        id: row.get::<String>(0).map_err(database_error)?,
        stable_key: row.get::<String>(1).map_err(database_error)?,
        content_type: row.get::<String>(2).map_err(database_error)?,
        display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        payload_ref: row.get::<String>(5).map_err(database_error)?,
        small_summary: row.get::<String>(6).map_err(database_error)?,
        kind: row.get::<String>(7).map_err(database_error)?,
        trace_title: row.get::<String>(8).map_err(database_error)?,
        trace_content: row.get::<String>(9).map_err(database_error)?,
        trace_status: row.get::<String>(10).map_err(database_error)?,
        tool_call_id: row.get::<String>(11).map_err(database_error)?,
        tool_name: row.get::<String>(12).map_err(database_error)?,
    })
}

fn markdown_block_from_row(row: &Row) -> HamburResult<MarkdownBlockPayloadRecord> {
    Ok(MarkdownBlockPayloadRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        message_id: row.get::<String>(2).map_err(database_error)?,
        block_id: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        stable_key: row.get::<String>(4).map_err(database_error)?,
        committed: row.get::<i64>(5).map_err(database_error)? != 0,
        payload_json: row.get::<String>(6).map_err(database_error)?,
        raw: row.get::<String>(7).map_err(database_error)?,
        small_summary: row.get::<String>(8).map_err(database_error)?,
        version_sequence: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(10).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(11).map_err(database_error)?),
    })
}

fn markdown_payload_refs(items: &[TimelineItemSnapshot]) -> Vec<String> {
    items
        .iter()
        .filter(|item| {
            item.content_type == "assistant_markdown_block"
                || item.content_type == "assistant_pending_block"
        })
        .map(|item| item.payload_ref.clone())
        .collect()
}

pub fn pending_markdown_stable_key(message_id: &str) -> String {
    format!("{message_id}:pending")
}

fn message_from_row(row: &Row) -> HamburResult<MessageRecord> {
    Ok(MessageRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        role: row.get::<String>(2).map_err(database_error)?,
        content_text: row.get::<String>(3).map_err(database_error)?,
        reasoning_content: row.get::<String>(4).map_err(database_error)?,
        status: row.get::<String>(5).map_err(database_error)?,
        turn_id: row.get::<String>(6).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        version_sequence: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        provider_id_snapshot: row.get::<String>(9).map_err(database_error)?,
        provider_name_snapshot: row.get::<String>(10).map_err(database_error)?,
        provider_protocol: row.get::<String>(11).map_err(database_error)?,
        model_id_snapshot: row.get::<String>(12).map_err(database_error)?,
        model_name_snapshot: row.get::<String>(13).map_err(database_error)?,
        model_group_id: row.get::<String>(14).map_err(database_error)?,
        finish_reason: row.get::<String>(15).map_err(database_error)?,
        native_finish_reason: row.get::<String>(16).map_err(database_error)?,
        attachments: Vec::new(),
    })
}

fn trace_span_from_row(row: &Row) -> HamburResult<TraceSpanRecord> {
    Ok(TraceSpanRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        parent_span_id: row.get::<String>(3).map_err(database_error)?,
        kind: row.get::<String>(4).map_err(database_error)?,
        title: row.get::<String>(5).map_err(database_error)?,
        content: row.get::<String>(6).map_err(database_error)?,
        status: row.get::<String>(7).map_err(database_error)?,
        started_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        ended_at_ms: row
            .get::<Option<i64>>(9)
            .map_err(database_error)?
            .map(unsigned_ms)
            .unwrap_or_default(),
        tool_call_id: row.get::<String>(10).map_err(database_error)?,
        payload_json: row.get::<String>(11).map_err(database_error)?,
    })
}

fn tool_call_from_row(row: &Row) -> HamburResult<ToolCallRecord> {
    Ok(ToolCallRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        assistant_message_id: row.get::<String>(3).map_err(database_error)?,
        name: row.get::<String>(4).map_err(database_error)?,
        arguments_json: row.get::<String>(5).map_err(database_error)?,
        display_title: row.get::<String>(6).map_err(database_error)?,
        status: row.get::<String>(7).map_err(database_error)?,
        requires_approval: sql_bool(row.get::<i64>(8).map_err(database_error)?),
        approval_status: row.get::<String>(9).map_err(database_error)?,
        started_at_ms: unsigned_ms(row.get::<i64>(10).map_err(database_error)?),
        ended_at_ms: row
            .get::<Option<i64>>(11)
            .map_err(database_error)?
            .map(unsigned_ms)
            .unwrap_or_default(),
        result_id: row.get::<String>(12).map_err(database_error)?,
        error_code: row.get::<String>(13).map_err(database_error)?,
        error_message: row.get::<String>(14).map_err(database_error)?,
    })
}

fn tool_result_from_row(row: &Row) -> HamburResult<ToolResultRecord> {
    Ok(ToolResultRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        tool_call_id: row.get::<String>(3).map_err(database_error)?,
        message_id: row.get::<String>(4).map_err(database_error)?,
        is_error: sql_bool(row.get::<i64>(5).map_err(database_error)?),
        content_json: row.get::<String>(6).map_err(database_error)?,
        summary: row.get::<String>(7).map_err(database_error)?,
        artifacts_json: row.get::<String>(8).map_err(database_error)?,
        trust_level: row.get::<String>(9).map_err(database_error)?,
        truncated: sql_bool(row.get::<i64>(10).map_err(database_error)?),
        offloaded_file_id: row.get::<String>(11).map_err(database_error)?,
        offloaded_path: row.get::<String>(12).map_err(database_error)?,
        context_stub: row.get::<String>(13).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(14).map_err(database_error)?),
    })
}

fn file_from_row(row: &Row) -> HamburResult<FileRecord> {
    Ok(FileRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        scope: row.get::<String>(1).map_err(database_error)?,
        session_id: row.get::<String>(2).map_err(database_error)?,
        relative_path: row.get::<String>(3).map_err(database_error)?,
        sandbox_path: row.get::<String>(4).map_err(database_error)?,
        mime_type: row.get::<String>(5).map_err(database_error)?,
        byte_size: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        sha256: row.get::<String>(7).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
        retention_policy: row.get::<String>(10).map_err(database_error)?,
    })
}

fn attachment_from_row(row: &Row) -> HamburResult<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        message_id: row.get::<String>(2).map_err(database_error)?,
        kind: row.get::<String>(3).map_err(database_error)?,
        display_name: row.get::<String>(4).map_err(database_error)?,
        mime_type: row.get::<String>(5).map_err(database_error)?,
        byte_size: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        origin_type: row.get::<String>(7).map_err(database_error)?,
        original_uri: row.get::<String>(8).map_err(database_error)?,
        file_id: row.get::<String>(9).map_err(database_error)?,
        sandbox_path: row.get::<String>(10).map_err(database_error)?,
        width: unsigned_count(row.get::<i64>(11).map_err(database_error)?),
        height: unsigned_count(row.get::<i64>(12).map_err(database_error)?),
        sha256: row.get::<String>(13).map_err(database_error)?,
        status: row.get::<String>(14).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(15).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(16).map_err(database_error)?),
    })
}

fn file_cleanup_job_from_row(row: &Row) -> HamburResult<FileCleanupJobRecord> {
    Ok(FileCleanupJobRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        file_id: row.get::<String>(1).map_err(database_error)?,
        relative_path: row.get::<String>(2).map_err(database_error)?,
        reason: row.get::<String>(3).map_err(database_error)?,
        status: row.get::<String>(4).map_err(database_error)?,
        attempts: unsigned_count(row.get::<i64>(5).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
    })
}

struct Connection {
    inner: Mutex<rusqlite::Connection>,
}

impl Connection {
    fn open(path: &Path) -> HamburResult<Self> {
        let inner = rusqlite::Connection::open(path).map_err(database_error)?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    fn execute(&self, sql: &str, params: SqlParams) -> Ready<Result<usize, rusqlite::Error>> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| connection.execute(sql, rusqlite::params_from_iter(params)));
        ready(result)
    }

    fn execute_batch(&self, sql: &str) -> Ready<Result<(), rusqlite::Error>> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| connection.execute_batch(sql));
        ready(result)
    }

    fn query(&self, sql: &str, params: SqlParams) -> Ready<Result<Rows, rusqlite::Error>> {
        let result = self
            .inner
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)
            .and_then(|connection| {
                let mut statement = connection.prepare(sql)?;
                let column_count = statement.column_count();
                let mut rows = statement.query(rusqlite::params_from_iter(params))?;
                let mut values = Vec::new();
                while let Some(row) = rows.next()? {
                    let mut columns = Vec::with_capacity(column_count);
                    for index in 0..column_count {
                        columns.push(row.get::<_, Value>(index)?);
                    }
                    values.push(Row { columns });
                }
                Ok(Rows {
                    rows: values,
                    next_index: 0,
                })
            });
        ready(result)
    }
}

type SqlParams = Vec<Value>;

trait IntoSqlValue {
    fn into_sql_value(self) -> Value;
}

impl IntoSqlValue for Value {
    fn into_sql_value(self) -> Value {
        self
    }
}

impl IntoSqlValue for String {
    fn into_sql_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoSqlValue for &str {
    fn into_sql_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoSqlValue for &String {
    fn into_sql_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoSqlValue for i64 {
    fn into_sql_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoSqlValue for i32 {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

impl IntoSqlValue for u32 {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

impl IntoSqlValue for bool {
    fn into_sql_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

struct Rows {
    rows: Vec<Row>,
    next_index: usize,
}

impl Rows {
    fn next(&mut self) -> Ready<Result<Option<Row>, rusqlite::Error>> {
        let row = self.rows.get(self.next_index).cloned();
        if row.is_some() {
            self.next_index += 1;
        }
        ready(Ok(row))
    }
}

#[derive(Clone)]
struct Row {
    columns: Vec<Value>,
}

impl Row {
    fn get<T>(&self, index: usize) -> Result<T, rusqlite::Error>
    where
        T: rusqlite::types::FromSql,
    {
        let value = self
            .columns
            .get(index)
            .ok_or(rusqlite::Error::InvalidColumnIndex(index))?;
        rusqlite::types::FromSql::column_result(ValueRef::from(value)).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(index, value.data_type(), Box::new(error))
        })
    }
}

fn database_error(error: rusqlite::Error) -> HamburError {
    HamburError::Internal(format!("rusqlite: {error}"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use hambur_core::new_id;
    use tokio::runtime::Runtime;

    use super::{
        HamburDatabase, ModelRouteSnapshot, NewTimelineItem, NewToolCall, NewToolResult,
        NewTraceSpan,
    };

    #[test]
    fn sessions_survive_restart_and_delete_from_snapshot() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("First")
                .await
                .expect("create session");
            assert_eq!(created.sessions.len(), 1);
            assert_eq!(created.sessions[0].title, "First");
            let session_id = created.selected_session_id.clone();
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let bootstrap = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(bootstrap.sessions.len(), 1);
            assert_eq!(bootstrap.selected_session_id, session_id);

            let deleted = restarted
                .delete_session(&session_id)
                .await
                .expect("delete session");
            assert!(deleted.sessions.is_empty());
            assert!(deleted.selected_session_id.is_empty());
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn active_session_survives_restart_after_opening_older_session() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let first = database
                .create_session("First")
                .await
                .expect("create first");
            let first_id = first.selected_session_id;
            let second = database
                .create_session("Second")
                .await
                .expect("create second");
            assert_eq!(second.selected_session_id, second.sessions[0].id);

            let opened_first = database.open_session(&first_id).await.expect("open first");
            assert_eq!(opened_first.selected_session_id, first_id);
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let snapshot = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(snapshot.selected_session_id, first_id);
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn message_timeline_and_turn_repositories_write_basic_records() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello")
                .await
                .expect("insert message");
            assert_eq!(message.role, "user");
            assert_eq!(message.content_text, "hello");

            let item = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: message.id.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: message.id.clone(),
                        small_summary: "hello".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("upsert timeline item");
            assert_eq!(item.stable_key, message.id);
            assert_eq!(item.version_sequence, 1);

            let updated = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: item.stable_key.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: item.payload_ref.clone(),
                        small_summary: "hello again".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("update timeline item");
            assert_eq!(updated.version_sequence, 2);
            assert_eq!(updated.small_summary, "hello again");

            let turn = database
                .create_turn(&session_id, "Preparing")
                .await
                .expect("create turn");
            let finished = database
                .update_turn_status(&turn.id, "Finished", true)
                .await
                .expect("finish turn");
            assert_eq!(finished.status, "Finished");
            assert!(finished.finished_at_ms > 0);

            let snapshot = database
                .session_snapshot(&session_id)
                .await
                .expect("session snapshot");
            assert_eq!(snapshot.timeline_items.len(), 1);
            assert_eq!(snapshot.timeline_items[0].small_summary, "hello again");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn snapshot_query_methods_are_paginated_and_side_effect_free() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Searchable Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello snapshot")
                .await
                .expect("insert message");

            for index in 0..3u64 {
                database
                    .upsert_timeline_item(
                        &session_id,
                        NewTimelineItem {
                            stable_key: format!("item-{index}"),
                            content_type: "message".to_string(),
                            display_sequence: message.created_at_ms + index,
                            payload_ref: message.id.clone(),
                            small_summary: format!("summary {index}"),
                            kind: "UserMessage".to_string(),
                        },
                    )
                    .await
                    .expect("upsert timeline item");
            }

            let sessions = database.session_list(10, 0).await.expect("session list");
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].title, "Searchable Chat");

            let search = database
                .search_sessions("Searchable", 10)
                .await
                .expect("search sessions");
            assert_eq!(search.len(), 1);

            let page = database
                .timeline_page(&session_id, 0, 2)
                .await
                .expect("timeline page");
            assert_eq!(page.items.len(), 2);
            assert!(page.has_more);
            assert!(page.next_before_cursor > 0);

            let snapshot = database
                .message_snapshot(&message.id)
                .await
                .expect("message snapshot");
            assert_eq!(snapshot.expect("message").content_text, "hello snapshot");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn trace_spans_and_tool_results_render_through_timeline() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Tools")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;
            let route = ModelRouteSnapshot {
                provider_id: "provider".to_string(),
                provider_name: "Provider".to_string(),
                provider_protocol: "OpenAiCompatible".to_string(),
                model_id: "model".to_string(),
                model_display_name: "Model".to_string(),
                model_group_id: "grp".to_string(),
                ..Default::default()
            };
            let turn = database
                .create_turn_with_route(&session_id, "ExecutingTools", &route)
                .await
                .expect("turn");
            let assistant = database
                .insert_message_with_route(
                    &session_id,
                    "assistant",
                    "",
                    "",
                    "streaming",
                    &turn.id,
                    &route,
                )
                .await
                .expect("assistant");
            let tool_call = database
                .insert_tool_call(NewToolCall {
                    id: "call_1".to_string(),
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    assistant_message_id: assistant.id.clone(),
                    name: "echo".to_string(),
                    arguments_json: r#"{"text":"hello"}"#.to_string(),
                    display_title: "Echo".to_string(),
                    status: "running".to_string(),
                    requires_approval: false,
                })
                .await
                .expect("tool call");
            let trace = database
                .insert_trace_span(NewTraceSpan {
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    kind: "tool".to_string(),
                    title: "Echo".to_string(),
                    content: "running".to_string(),
                    status: "running".to_string(),
                    tool_call_id: tool_call.id.clone(),
                    visible: true,
                    ..Default::default()
                })
                .await
                .expect("trace");
            database
                .update_trace_span_status(&trace.id, "completed", "hello", true)
                .await
                .expect("complete trace");
            let tool_message = database
                .insert_tool_result_message(
                    &session_id,
                    &turn.id,
                    &tool_call.id,
                    "echo",
                    "hello",
                    &route,
                )
                .await
                .expect("tool message");
            let result = database
                .insert_tool_result(NewToolResult {
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    tool_call_id: tool_call.id.clone(),
                    message_id: tool_message.id,
                    is_error: false,
                    content_json: r#"{"text":"hello"}"#.to_string(),
                    summary: "hello".to_string(),
                    artifacts_json: "[]".to_string(),
                    trust_level: "trusted".to_string(),
                    context_stub: "hello".to_string(),
                    ..Default::default()
                })
                .await
                .expect("tool result");
            database
                .update_tool_call_status(&tool_call.id, "completed", &result.id, "", "", true)
                .await
                .expect("complete tool call");

            let page = database
                .timeline_page(&session_id, 0, 20)
                .await
                .expect("timeline");
            let trace_item = page
                .items
                .iter()
                .find(|item| item.kind == "ToolTrace")
                .expect("trace item");
            assert_eq!(trace_item.trace_title, "Echo");
            assert_eq!(trace_item.trace_status, "completed");
            assert_eq!(trace_item.tool_call_id, "call_1");
            assert_eq!(trace_item.tool_name, "echo");
        });

        let _ = fs::remove_file(path);
    }

    fn temp_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("{}.db", new_id("hambur_db_test")))
    }
}
