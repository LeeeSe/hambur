use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use hambur_db::{
    AppSnapshot, HamburDatabase, MessageRecord, SessionSummary, TimelineItemSnapshot,
};
use hambur_markdown::{MarkdownPipeline, MarkdownRenderUpdate};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

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
    MarkdownRenderUpdate,
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
            Self::MarkdownRenderUpdate => "MarkdownRenderUpdate",
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
    bootstrap: AppBootstrap,
    database: HamburDatabase,
    markdown_streams: Mutex<HashMap<String, MarkdownPipeline>>,
    idempotency: Mutex<HashMap<String, RuntimeCommandAck>>,
    sender: mpsc::Sender<RuntimeEvent>,
    receiver: Mutex<mpsc::Receiver<RuntimeEvent>>,
    sequence: AtomicU64,
    shutdown: AtomicBool,
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
            bootstrap,
            database,
            markdown_streams: Mutex::new(HashMap::new()),
            idempotency: Mutex::new(HashMap::new()),
            sender,
            receiver: Mutex::new(receiver),
            sequence: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });

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

        match self.tokio.block_on(self.database.create_session(&command.title)) {
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

        match self.tokio.block_on(self.database.open_session(&command.session_id)) {
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

    pub fn get_session_list_snapshot(
        &self,
        limit: u32,
        offset: u32,
    ) -> RuntimeSessionListSnapshot {
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
            .block_on(self.database.timeline_page(&session_id, before_cursor, limit))
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
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind: RuntimeEventKind::MarkdownRenderUpdate,
            session_id,
            turn_id: String::new(),
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

    fn temp_app_dir() -> PathBuf {
        std::env::temp_dir().join(new_id("hambur_runtime_test"))
    }
}
