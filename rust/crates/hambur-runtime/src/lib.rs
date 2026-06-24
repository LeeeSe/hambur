use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use hambur_db::{
    AppSnapshot, HamburDatabase, MessageRecord, ModelRouteSnapshot, NewTimelineItem,
    ProviderModelUpsert, ProviderUpsert, SessionSummary, TimelineItemSnapshot,
};
use hambur_llm::{
    FallbackPolicy, ModelCapabilities, ModelRouter, OPENAI_COMPATIBLE_PROTOCOL,
    OpenAiCompatibleAdapter, ProviderConfig, ProviderModel, ProviderStreamEvent, ProviderTarget,
    RoutePlan, RouteRequirements, RoutingStrategy, SseDecoder, scripted_openai_sse_chunks,
    should_fallback,
};
use hambur_markdown::{MarkdownPipeline, MarkdownRenderUpdate};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

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
    pub error_code: String,
    pub message: String,
}

pub struct RuntimeEngine {
    tokio: Runtime,
    self_ref: Mutex<Weak<RuntimeEngine>>,
    bootstrap: AppBootstrap,
    database: HamburDatabase,
    markdown_streams: Mutex<HashMap<String, MarkdownPipeline>>,
    active_turns: Mutex<HashMap<String, ActiveTurn>>,
    router: Mutex<ModelRouter>,
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
        let snapshot = tokio.block_on(database.bootstrap_snapshot())?;
        let (sender, receiver) = mpsc::channel(64);
        let engine = Arc::new(Self {
            tokio,
            self_ref: Mutex::new(Weak::new()),
            bootstrap,
            database,
            markdown_streams: Mutex::new(HashMap::new()),
            active_turns: Mutex::new(HashMap::new()),
            router: Mutex::new(ModelRouter::default()),
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
            "RefreshProviderModels" => self.execute_refresh_provider_models(command),
            "SendMessage" => self.execute_send_message(command, "SendMessage"),
            "RetryTurn" => self.execute_send_message(command, "RetryTurn"),
            "RegenerateMessage" => self.execute_send_message(command, "RegenerateMessage"),
            "EditMessage" => self.execute_send_message(command, "EditMessage"),
            "CancelTurn" => self.execute_cancel_turn(command),
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
        let result = self
            .tokio
            .block_on(self.database.upsert_provider(ProviderUpsert {
                id: provider_id,
                name: command.title,
                icon_name: "sparkles".to_string(),
                api_type: OPENAI_COMPATIBLE_PROTOCOL.to_string(),
                base_url: command.chunk,
                secret_ref: command.payload_json,
                enabled: true,
            }));

        match result {
            Ok(_) => {
                let snapshot = self
                    .tokio
                    .block_on(self.database.bootstrap_snapshot())
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::ModelsUpdated,
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

        let content = match self.resolve_turn_content(&command, command_kind) {
            Ok(content) => content,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        if content.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("message content must not be empty".to_string()),
            );
        }

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
            requires_image_input: false,
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
                    &content,
                    "",
                    "completed",
                    &turn.id,
                    &route,
                )
                .await?;
            self.database
                .upsert_timeline_item(
                    &command.session_id,
                    NewTimelineItem {
                        stable_key: user_message.id.clone(),
                        content_type: "message".to_string(),
                        display_sequence: user_message.created_at_ms,
                        payload_ref: user_message.id.clone(),
                        small_summary: content.chars().take(160).collect(),
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
            content.clone(),
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
        if let Some(update) = result.1 {
            if !update.committed_nodes.is_empty()
                || update.pending_node.is_some()
                || update.reset
                || !update.invalidated_block_ids.is_empty()
            {
                let _ = self.emit_markdown(command.session_id.clone(), update);
            }
        }

        accepted_ack(command.command_id, command.idempotency_key)
    }

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

    async fn run_chat_stream_attempt(
        self: Arc<Self>,
        session_id: String,
        turn_id: String,
        assistant_message_id: String,
        route: ModelRouteSnapshot,
        cancel: Arc<AtomicBool>,
        stream_chunks: Vec<Vec<u8>>,
    ) -> StreamAttemptResult {
        let mut decoder = SseDecoder::default();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut semantic_delta_started = false;
        let mut finish_reason = String::new();
        let mut native_finish_reason = String::new();

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
        if let Ok(mut turns) = self.active_turns.lock() {
            if turns
                .get(session_id)
                .is_some_and(|active| active.turn_id == turn_id)
            {
                turns.remove(session_id);
            }
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
        "RefreshProviderModels" => {
            if command.provider_id.is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
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
    format!(
        r#"{{"data":[{{"id":"{model_id}","display_name":"{model_id}","supports_reasoning":true,"supports_tool_call":false,"supports_image_input":false,"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}}]}}"#
    )
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
    use std::{fs, path::PathBuf};

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
                    assert_eq!(event.markdown_render_update.message_id.is_empty(), false);
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

        let models_json = format!(
            r#"{{"data":[{{"id":"{model_id}","display_name":"{model_id}","supports_reasoning":true,"supports_tool_call":false,"supports_image_input":false,"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}}]}}"#
        );
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
