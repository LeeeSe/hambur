use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use hambur_db::{
    AppSnapshot, AttachmentRecord, HamburDatabase, MessageRecord, ModelRouteSnapshot,
    NewAttachment, NewFileRecord, NewTimelineItem, NewToolCall, NewToolResult, NewTraceSpan,
    ProviderModelOverride, ProviderModelUpsert, ProviderUpsert, SessionSummary, SettingsSnapshot,
    TimelineItemSnapshot,
};
use hambur_filestore::FileStore;
use hambur_llm::{
    CompleteToolCall, FallbackPolicy, ModelCapabilities, ModelRouter, OPENAI_COMPATIBLE_PROTOCOL,
    OpenAiCompatibleAdapter, ProviderConfig, ProviderModel, ProviderStreamEvent, ProviderTarget,
    RoutePlan, RouteRequirements, RoutingStrategy, SseDecoder, ToolCallAccumulator,
    scripted_openai_sse_chunks, should_fallback,
};
use hambur_markdown::{MarkdownPipeline, MarkdownRenderUpdate};
use hambur_sandbox::{SandboxAccess, SandboxService};
use hambur_tools::{
    MAX_TOOL_ITERATIONS_PER_TURN, RawToolOutput, ToolCallBatch, ToolExecutionRecord,
    ToolInvocation, ToolResult, ToolScheduler,
};
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, sleep, timeout};

#[derive(Debug, Clone)]
pub struct AppBootstrap {
    pub app_files_dir: String,
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
pub struct RuntimeSessionSnapshot {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session: Option<SessionSummary>,
    pub timeline_items: Vec<TimelineItemSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeTimelinePage {
    pub snapshot_sequence: u64,
    pub created_at_ms: u64,
    pub session_id: String,
    pub items: Vec<TimelineItemSnapshot>,
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

enum StreamAttemptResult {
    Completed,
    Cancelled,
    Failed {
        error: HamburError,
        semantic_delta_started: bool,
    },
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
        let sandbox = SandboxService::new(&bootstrap.app_files_dir)?;
        let jobs = tokio.block_on(database.cleanup_pending_attachments(0))?;
        for job in jobs {
            let _ = filestore.delete_relative_if_exists(&job.relative_path);
            let _ = tokio.block_on(database.mark_file_cleanup_done(&job.id));
        }
        let snapshot = tokio.block_on(database.bootstrap_snapshot())?;
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
        Ok(engine)
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
            "UpdateDefaultModelGroups" => self.execute_update_default_model_groups(command),
            "UpdateToolSettings"
            | "UpdateSkills"
            | "UpdateMemoryProjections"
            | "UpdateStartupTasks"
            | "UpdateRootfsSettings"
            | "UpdateAppSetting"
            | "UpdateBrowserToolSettings"
            | "UpdateSkillEnabled"
            | "UpdateStartupTask"
            | "DeleteStartupTask"
            | "UpdateRootfsSetting" => self.execute_update_app_setting(command),
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

        match self
            .tokio
            .block_on(self.database.create_session(&command.title))
        {
            Ok(snapshot) => {
                let _ = self.emit(RuntimeEventKind::SessionCreated, snapshot, None);
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

        match self
            .tokio
            .block_on(self.database.open_session(&command.session_id))
        {
            Ok(snapshot) => {
                let _ = self.emit(RuntimeEventKind::SessionOpened, snapshot, None);
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

        if !metadata.source_path.trim().is_empty() {
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
        let attachment_ids = parse_attachment_ids(&command.payload_json);
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
            requires_tool_protocol: false,
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
                        content_type: "message".to_string(),
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
            self.database
                .upsert_timeline_item(
                    &command.session_id,
                    NewTimelineItem {
                        stable_key: assistant_message.id.clone(),
                        content_type: "message".to_string(),
                        display_sequence: assistant_message.created_at_ms,
                        payload_ref: assistant_message.id.clone(),
                        small_summary: format!(
                            "{} via {}",
                            route.model_display_name, route.provider_name
                        ),
                        kind: "AssistantMessage".to_string(),
                    },
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
        let stream_chunks_by_route = route_snapshots
            .iter()
            .map(|route| stream_chunks_for_command(&command, &content, route))
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
                    stream_chunks_by_route,
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

    fn execute_rootfs_lifecycle(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

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
            "action": command.kind
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
        stream_chunks_by_route: Vec<Vec<Vec<u8>>>,
    ) {
        let target_count = routes.len();
        let route_candidates = routes.clone();
        let mut last_error = None;

        for (attempt_index, route) in routes.into_iter().enumerate() {
            if attempt_index > 0 {
                if let Err(error) = self
                    .database
                    .update_turn_route_snapshot(&turn_id, &route)
                    .await
                {
                    self.finish_failed_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
                        "",
                        "",
                        error,
                    )
                    .await;
                    return;
                }
                if let Err(error) = self
                    .database
                    .update_message_route_snapshot(&assistant_message_id, &route)
                    .await
                {
                    self.finish_failed_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
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

            let stream_chunks = stream_chunks_by_route
                .get(attempt_index)
                .cloned()
                .unwrap_or_default();
            match self
                .clone()
                .run_chat_stream_attempt(
                    session_id.clone(),
                    turn_id.clone(),
                    assistant_message_id.clone(),
                    route,
                    route_candidates.clone(),
                    cancel.clone(),
                    stream_chunks,
                )
                .await
            {
                StreamAttemptResult::Completed | StreamAttemptResult::Cancelled => return,
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
                            &assistant_message_id,
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
            self.finish_failed_turn(&session_id, &turn_id, &assistant_message_id, "", "", error)
                .await;
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
        stream_chunks: Vec<Vec<u8>>,
    ) -> StreamAttemptResult {
        let mut decoder = SseDecoder::default();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut semantic_delta_started = false;
        let mut finish_reason = String::new();
        let mut native_finish_reason = String::new();
        let mut saw_tool_delta = false;
        let mut tool_accumulator = ToolCallAccumulator::default();
        let mut complete_tool_calls = Vec::<CompleteToolCall>::new();

        for chunk in stream_chunks {
            if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                self.finish_cancelled_turn(
                    &session_id,
                    &turn_id,
                    &assistant_message_id,
                    &content,
                    &reasoning,
                )
                .await;
                return StreamAttemptResult::Cancelled;
            }

            sleep(Duration::from_millis(24)).await;
            let payloads = match decoder.push(&chunk) {
                Ok(payloads) => payloads,
                Err(error) => {
                    if semantic_delta_started {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &content,
                            &reasoning,
                            error.clone(),
                        )
                        .await;
                    }
                    return StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started,
                    };
                }
            };

            for payload in payloads {
                let events = match OpenAiCompatibleAdapter::parse_stream_payload(&payload) {
                    Ok(events) => events,
                    Err(error) => {
                        if semantic_delta_started {
                            self.finish_failed_turn(
                                &session_id,
                                &turn_id,
                                &assistant_message_id,
                                &content,
                                &reasoning,
                                error.clone(),
                            )
                            .await;
                        }
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started,
                        };
                    }
                };

                for event in events {
                    let newly_complete_tool_calls = tool_accumulator.apply(&event);
                    if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                        self.finish_cancelled_turn(
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &content,
                            &reasoning,
                        )
                        .await;
                        return StreamAttemptResult::Cancelled;
                    }

                    match event {
                        ProviderStreamEvent::ContentDelta(delta) => {
                            semantic_delta_started = true;
                            content.push_str(&delta);
                            let snapshot = self
                                .database
                                .session_snapshot(&session_id)
                                .await
                                .unwrap_or_default();
                            let _ = self.emit_session_event(
                                RuntimeEventKind::AssistantContentDelta,
                                session_id.clone(),
                                turn_id.clone(),
                                snapshot.clone(),
                                delta.clone(),
                                None,
                            );
                            if let Some(update) = self.append_stream_markdown(
                                &session_id,
                                &assistant_message_id,
                                &delta,
                                false,
                            ) {
                                let _ = self.emit_markdown_event(
                                    session_id.clone(),
                                    turn_id.clone(),
                                    snapshot,
                                    update,
                                );
                            }
                        }
                        ProviderStreamEvent::ReasoningDelta(delta) => {
                            semantic_delta_started = true;
                            reasoning.push_str(&delta);
                            let snapshot = self
                                .database
                                .session_snapshot(&session_id)
                                .await
                                .unwrap_or_default();
                            let _ = self.emit_session_event(
                                RuntimeEventKind::AssistantReasoningDelta,
                                session_id.clone(),
                                turn_id.clone(),
                                snapshot,
                                delta,
                                None,
                            );
                        }
                        ProviderStreamEvent::ToolCallDelta { .. } => {
                            semantic_delta_started = true;
                            saw_tool_delta = true;
                            let snapshot = self
                                .database
                                .session_snapshot(&session_id)
                                .await
                                .unwrap_or_default();
                            let _ = self.emit_session_event(
                                RuntimeEventKind::ToolCallDelta,
                                session_id.clone(),
                                turn_id.clone(),
                                snapshot,
                                "Tool call delta".to_string(),
                                None,
                            );
                        }
                        ProviderStreamEvent::Finish {
                            finish_reason: reason,
                            native_finish_reason: native,
                        } => {
                            finish_reason = reason;
                            native_finish_reason = native;
                        }
                        ProviderStreamEvent::Error { code, message } => {
                            let error = stream_error_from_provider(code, message);
                            if semantic_delta_started {
                                self.finish_failed_turn(
                                    &session_id,
                                    &turn_id,
                                    &assistant_message_id,
                                    &content,
                                    &reasoning,
                                    error.clone(),
                                )
                                .await;
                            }
                            return StreamAttemptResult::Failed {
                                error,
                                semantic_delta_started,
                            };
                        }
                    }

                    for complete in newly_complete_tool_calls {
                        if !complete_tool_calls
                            .iter()
                            .any(|existing| existing.index == complete.index)
                        {
                            complete_tool_calls.push(complete);
                        }
                    }
                }
            }
        }

        if !semantic_delta_started {
            return StreamAttemptResult::Failed {
                error: HamburError::ProviderUnavailable(format!(
                    "provider {} produced no semantic output",
                    route.provider_id
                )),
                semantic_delta_started,
            };
        }

        if let Some(update) =
            self.append_stream_markdown(&session_id, &assistant_message_id, "", true)
        {
            let snapshot = self
                .database
                .session_snapshot(&session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_markdown_event(session_id.clone(), turn_id.clone(), snapshot, update);
        }

        let final_finish_reason = finish_reason.if_blank("stop".to_string());
        let final_native_finish_reason = native_finish_reason.if_blank(final_finish_reason.clone());
        if saw_tool_delta {
            complete_tool_calls = tool_accumulator.completed_calls();
        }
        if saw_tool_delta
            && (complete_tool_calls.is_empty() || tool_accumulator.has_incomplete_calls())
        {
            let error = HamburError::SseParse("incomplete tool call stream".to_string());
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &content,
                &reasoning,
                error.clone(),
            )
            .await;
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        if !complete_tool_calls.is_empty() {
            complete_tool_calls.sort_by_key(|call| call.index);
            let result = self
                .execute_tool_batch_and_continue(
                    &session_id,
                    &turn_id,
                    &assistant_message_id,
                    &route,
                    &route_candidates,
                    &cancel,
                    &content,
                    &reasoning,
                    final_finish_reason,
                    final_native_finish_reason,
                    complete_tool_calls,
                )
                .await;
            return match result {
                Ok(()) => StreamAttemptResult::Completed,
                Err(error) => {
                    self.finish_failed_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
                        &content,
                        &reasoning,
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

        let message = match self
            .database
            .update_message_stream_result(
                &assistant_message_id,
                &content,
                &reasoning,
                "completed",
                &final_finish_reason,
                &final_native_finish_reason,
            )
            .await
        {
            Ok(message) => message,
            Err(error) => {
                self.finish_failed_turn(
                    &session_id,
                    &turn_id,
                    &assistant_message_id,
                    &content,
                    &reasoning,
                    error.clone(),
                )
                .await;
                return StreamAttemptResult::Failed {
                    error,
                    semantic_delta_started: true,
                };
            }
        };
        let summary = message.content_text.chars().take(160).collect::<String>();
        if let Err(error) = self
            .database
            .upsert_timeline_item(
                &session_id,
                NewTimelineItem {
                    stable_key: message.id,
                    content_type: "message".to_string(),
                    display_sequence: message.created_at_ms,
                    payload_ref: assistant_message_id.clone(),
                    small_summary: summary,
                    kind: "AssistantMessage".to_string(),
                },
            )
            .await
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &content,
                &reasoning,
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
                &content,
                &reasoning,
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
    ) -> HamburResult<()> {
        if complete_tool_calls.len() > MAX_TOOL_ITERATIONS_PER_TURN as usize {
            return Err(HamburError::InvalidCommand(format!(
                "max tool iterations exceeded: {}",
                MAX_TOOL_ITERATIONS_PER_TURN
            )));
        }

        if let Some(update) =
            self.append_stream_markdown(session_id, assistant_message_id, "", true)
        {
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_markdown_event(
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                update,
            );
        }

        let message = self
            .database
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
            .upsert_timeline_item(
                session_id,
                NewTimelineItem {
                    stable_key: message.id,
                    content_type: "message".to_string(),
                    display_sequence: message.created_at_ms,
                    payload_ref: assistant_message_id.to_string(),
                    small_summary: if content.trim().is_empty() {
                        "Tool calls requested".to_string()
                    } else {
                        content.chars().take(160).collect()
                    },
                    kind: "AssistantMessage".to_string(),
                },
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
            return Ok(());
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
            context_stubs.push(record.result.context_stub);
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
            return Ok(());
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
        let continuation_content = format_tool_continuation(&context_stubs);
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
                        content_type: "message".to_string(),
                        display_sequence: synthetic.created_at_ms,
                        payload_ref: synthetic.id.clone(),
                        small_summary: synthetic.content_text.chars().take(160).collect(),
                        kind: "SyntheticUserMessage".to_string(),
                    },
                )
                .await?;
        }
        let continuation = self
            .database
            .insert_message_with_route(
                session_id,
                "assistant",
                &continuation_content,
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
                    stable_key: continuation.id.clone(),
                    content_type: "message".to_string(),
                    display_sequence: continuation.created_at_ms,
                    payload_ref: continuation.id.clone(),
                    small_summary: continuation_content.chars().take(160).collect(),
                    kind: "AssistantMessage".to_string(),
                },
            )
            .await?;
        self.database
            .update_turn_status(turn_id, "Finished", true)
            .await?;

        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        self.clear_active_turn(session_id, turn_id);
        let _ = self.emit_session_event(
            RuntimeEventKind::AssistantMessageFinished,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot.clone(),
            "tool_continuation".to_string(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnFinished,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "tool_continuation".to_string(),
            None,
        );
        Ok(())
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
                    records.push(self.execute_delegate_tool(session_id, invocation).await);
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
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process background sessions are not attached yet",
            );
        }
        if arguments
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "terminal background execution requires process sessions",
            );
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
        let _ = fs::create_dir_all(&cwd.host_path);
        let status = self.sandbox.rootfs_status();
        if !status.available {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("ToolUnavailable({})", status.reason),
            );
        }

        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(30_000)
            .clamp(1_000, 300_000);
        let raw = run_terminal_command(invocation, command, &cwd.host_path, timeout_ms);
        self.tools.normalize_raw(raw).unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
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
                let content = if payload_json.trim().is_empty() {
                    message
                } else {
                    payload_json
                };
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
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_delegate_result(session_id, &invocation, &arguments),
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

    fn resolve_delegate_result(
        &self,
        session_id: &str,
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
            for path in arguments
                .get("artifact_paths")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if let Err(error) = self.sandbox.resolve(session_id, path, SandboxAccess::Read) {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        error.to_string(),
                    );
                }
            }
            let content = json!({
                "summary": arguments.get("summary").and_then(Value::as_str).unwrap_or_default(),
                "findings": arguments.get("findings").cloned().unwrap_or_else(|| json!([])),
                "changedFiles": arguments.get("changed_files").or_else(|| arguments.get("changedFiles")).cloned().unwrap_or_else(|| json!([])),
                "artifactPaths": arguments.get("artifact_paths").or_else(|| arguments.get("artifactPaths")).cloned().unwrap_or_else(|| json!([])),
                "risks": arguments.get("risks").cloned().unwrap_or_else(|| json!([])),
                "nextSteps": arguments.get("next_steps").or_else(|| arguments.get("nextSteps")).cloned().unwrap_or_else(|| json!([]))
            });
            return ToolResult {
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
            };
        }

        let delegate_session_id = new_id("delegate_session");
        let content = json!({
            "delegateSessionId": delegate_session_id,
            "status": "awaiting_submit_delegate_result",
            "task": arguments.get("task").and_then(Value::as_str).unwrap_or_default(),
            "delegateTaskDisabledInChild": true
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Delegate session created".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
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
        let timeline_items = self
            .tokio
            .block_on(self.database.session_snapshot(&session_id))
            .map(|snapshot| snapshot.timeline_items)
            .unwrap_or_default();
        RuntimeSessionSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            session,
            timeline_items,
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
        self.emit_markdown_event(session_id, turn_id, snapshot, markdown_render_update)
    }

    fn emit_markdown_event(
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

fn database_path(bootstrap: &AppBootstrap) -> PathBuf {
    PathBuf::from(&bootstrap.app_files_dir).join("hambur.db")
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
        "OpenSession" | "DeleteSession" | "SoftDeleteSession" | "HardPurgeSession" => {
            require_session_id(command)
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
        "UpdateToolSettings"
        | "UpdateSkills"
        | "UpdateMemoryProjections"
        | "UpdateStartupTasks"
        | "UpdateRootfsSettings"
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
        "RunRootfsWarmup" | "ResetRootfs" => Ok(()),
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

fn stream_chunks_for_command(
    command: &RuntimeCommand,
    content: &str,
    route: &ModelRouteSnapshot,
) -> Vec<Vec<u8>> {
    let payload = command.payload_json.trim();
    if payload.starts_with("data:") {
        return split_scripted_sse(payload);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Some(route_value) = scripted_route_value(&value, route) {
            if let Some(sse) = route_value.get("sse").and_then(serde_json::Value::as_str) {
                return split_scripted_sse(sse);
            }
            let response = route_value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = route_value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return scripted_openai_sse_chunks(response, reasoning);
        }
        if let Some(sse) = value.get("sse").and_then(serde_json::Value::as_str) {
            return split_scripted_sse(sse);
        }
        let response = value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| content.trim());
        let reasoning = value
            .get("reasoning")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(command.reasoning.as_str());
        return scripted_openai_sse_chunks(response, reasoning);
    }

    let response = format!("Echo: {}", content.trim());
    let reasoning = command.reasoning.clone().if_blank(format!(
        "Selected {} through {}.",
        route.model_display_name, route.provider_name
    ));
    scripted_openai_sse_chunks(&response, &reasoning)
}

#[derive(Debug, Clone, Default)]
struct AttachmentImportPayload {
    display_name: String,
    mime_type: String,
    byte_size: u64,
    origin_type: String,
    original_uri: String,
    source_path: String,
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

fn parse_attachment_ids(payload_json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(payload_json) else {
        return Vec::new();
    };
    let Some(values) = value
        .get("attachmentIds")
        .or_else(|| value.get("attachment_ids"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
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

fn format_tool_continuation(context_stubs: &[String]) -> String {
    if context_stubs.is_empty() {
        return "Tool batch completed with no output.".to_string();
    }
    let mut output = String::from("Tool batch completed. Results:\n");
    for (index, stub) in context_stubs.iter().enumerate() {
        output.push_str(&format!("\n{}. {}\n", index + 1, stub.trim()));
    }
    output
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

fn run_terminal_command(
    invocation: &ToolInvocation,
    command: &str,
    cwd: &std::path::Path,
    timeout_ms: u64,
) -> RawToolOutput {
    let shell = if cfg!(target_os = "android") {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    };
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
        match fetch_http_url(&url, max_bytes) {
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
    let parsed = parse_http_url(url)?;
    let mut stream = TcpStream::connect((&*parsed.host, parsed.port))
        .map_err(|error| format!("connect failed: {error}"))?;
    stream
        .set_read_timeout(Some(StdDuration::from_secs(20)))
        .map_err(|error| format!("set read timeout failed: {error}"))?;
    stream
        .set_write_timeout(Some(StdDuration::from_secs(20)))
        .map_err(|error| format!("set write timeout failed: {error}"))?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Hambur/0.1\r\nAccept: text/*, application/json;q=0.9, */*;q=0.1\r\nConnection: close\r\n\r\n",
        parsed.path, parsed.host
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write request failed: {error}"))?;
    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    while response.len() < max_bytes {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("read response failed: {error}"))?;
        if read == 0 {
            break;
        }
        let remaining = max_bytes - response.len();
        response.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    let truncated = response.len() >= max_bytes;
    let text = String::from_utf8_lossy(&response).to_string();
    let (headers, body) = split_http_response(&text);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_default();
    Ok(json!({
        "url": url,
        "status": status,
        "headers": headers,
        "text": body,
        "truncated": truncated
    }))
}

#[derive(Debug)]
struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, String> {
    let Some(rest) = url.strip_prefix("http://") else {
        if url.starts_with("https://") {
            return Err(
                "HTTPS web_fetch requires a configured TLS-capable web provider".to_string(),
            );
        }
        return Err("web_fetch URL must start with http:// or https://".to_string());
    };
    let (authority, path) = rest
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((rest, "/".to_string()));
    let (host, port) = authority
        .rsplit_once(':')
        .and_then(|(host, port)| port.parse::<u16>().ok().map(|port| (host, port)))
        .unwrap_or((authority, 80));
    if host.trim().is_empty() {
        return Err("web_fetch host must not be empty".to_string());
    }
    Ok(ParsedHttpUrl {
        host: host.to_string(),
        port,
        path,
    })
}

fn split_http_response(response: &str) -> (String, String) {
    response
        .split_once("\r\n\r\n")
        .map(|(headers, body)| (headers.to_string(), body.to_string()))
        .unwrap_or_else(|| (String::new(), response.to_string()))
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
    if expected.iter().any(|candidate| token == *candidate) {
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
        fs,
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        thread,
    };

    use hambur_core::new_id;

    use super::{AppBootstrap, RuntimeCommand, RuntimeEngine};

    #[test]
    fn bootstrap_snapshot_survives_restart_without_replayed_session_event() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let first = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        first.shutdown();
        drop(first);

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn send_message_streams_reasoning_content_and_persists_model_snapshot() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        assert_eq!(message.content_text, "hello world");
        assert_eq!(message.reasoning_content, "thinking separately");
        assert_eq!(message.provider_id_snapshot, "provider_test");
        assert_eq!(message.model_id_snapshot, "gpt-test");
        assert_eq!(message.status, "completed");
        assert_eq!(finished.session_id, message.session_id);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn duplicate_send_message_is_rejected_while_turn_is_active() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn cancel_turn_stops_streaming() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn regenerate_message_uses_source_user_content() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        wait_for_event(&runtime, "TurnFinished");

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
        wait_for_event(&runtime, "TurnFinished");

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
    fn fallback_switches_target_before_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn fallback_does_not_switch_after_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn tool_calls_execute_as_one_batch_and_render_traces() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_tools".to_string(),
            idempotency_key: "message:tools:batch".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "use tools".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut finished_tools = 0;
        let mut turn_finished = false;
        for _ in 0..96 {
            let event = runtime.next_event().expect("tool event");
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
        assert_eq!(finished_tools, 2);
        assert!(turn_finished, "missing turn finish");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let trace_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "ToolTrace")
            .count();
        assert!(trace_count >= 2, "missing tool traces: {trace_count}");
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
        assert!(message.content_text.contains("hello tools"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn attachment_import_remove_and_startup_cleanup() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn model_groups_and_app_settings_persist_through_restart() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        {
            let runtime = RuntimeEngine::create(AppBootstrap {
                app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_session_search".to_string(),
            idempotency_key: "message:m7:session-search".to_string(),
            kind: "SendMessage".to_string(),
            session_id: runtime.get_session_list_snapshot(10, 0).selected_session_id,
            content: "search old sessions".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        let selected = snapshot.selected_session_id;
        let timeline = runtime.get_timeline_page(selected, 0, 50);
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
        assert!(message.content_text.contains("Alpha Project"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_browser_use_waits_for_submit_platform_result() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_browser".to_string(),
            idempotency_key: "message:m7:browser".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "read page".to_string(),
            payload_json: serde_json::json!({"sse": sse}).to_string(),
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
        assert!(message.content_text.contains("Example Domain"));
        assert!(message.content_text.contains("<untrusted_tool_result"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_delegate_submit_rejects_artifact_path_escape() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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
    fn milestone7_terminal_uses_sandbox_path_policy_before_execution() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
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

    fn temp_app_dir() -> PathBuf {
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

    fn wait_for_event(runtime: &RuntimeEngine, kind: &str) {
        for _ in 0..64 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind {
                return;
            }
        }
        panic!("missing event: {kind}");
    }
}
