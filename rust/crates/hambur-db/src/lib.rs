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
    pub memory_reviewed: bool,
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
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_title: String,
    pub attachments: Vec<AttachmentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTranscriptEntry {
    pub message: MessageRecord,
    pub tool_calls: Vec<ToolCallRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionReviewRecord {
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub memory_reviewed: bool,
    pub messages: Vec<MessageRecord>,
    pub trace_spans: Vec<TraceSpanRecord>,
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
    pub call_index: u32,
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
    pub call_index: u32,
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
pub struct ModelCatalogCacheRecord {
    pub key: String,
    pub catalog_json: String,
    pub synced_at_ms: u64,
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

mod connection;
mod database;
mod mutations;
mod queries;
mod utils;

#[cfg(test)]
mod tests;

pub(crate) use connection::{Connection, IntoSqlValue, Row, Rows, SqlParams, database_error};
pub use utils::*;

pub struct HamburDatabase {
    connection: Connection,
}
