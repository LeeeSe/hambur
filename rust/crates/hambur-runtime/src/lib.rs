use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct AppBootstrap {
    pub app_files_dir: String,
}

#[derive(Debug, Clone)]
pub enum RuntimeCommand {
    Initialize,
    Shutdown,
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

#[derive(Debug, Clone)]
pub enum RuntimeEventKind {
    RuntimeReady,
}

impl RuntimeEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RuntimeReady => "RuntimeReady",
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
}

pub struct RuntimeEngine {
    _tokio: Runtime,
    bootstrap: AppBootstrap,
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
        let (sender, receiver) = mpsc::channel(64);
        let engine = Arc::new(Self {
            _tokio: tokio,
            bootstrap,
            sender,
            receiver: Mutex::new(receiver),
            sequence: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });

        engine.emit(RuntimeEventKind::RuntimeReady)?;
        Ok(engine)
    }

    pub fn dispatch(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        match command {
            RuntimeCommand::Initialize => RuntimeCommandAck {
                command_id: new_id("cmd"),
                idempotency_key: "runtime:initialize".to_string(),
                accepted: true,
                duplicate: false,
                rejection_code: String::new(),
                message: String::new(),
            },
            RuntimeCommand::Shutdown => {
                self.shutdown();
                RuntimeCommandAck {
                    command_id: new_id("cmd"),
                    idempotency_key: "runtime:shutdown".to_string(),
                    accepted: true,
                    duplicate: false,
                    rejection_code: String::new(),
                    message: String::new(),
                }
            }
        }
    }

    pub fn next_event(&self) -> Option<RuntimeEvent> {
        let mut receiver = self.receiver.lock().ok()?;
        receiver.blocking_recv()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn app_files_dir(&self) -> &str {
        &self.bootstrap.app_files_dir
    }

    fn emit(&self, kind: RuntimeEventKind) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id: String::new(),
            turn_id: String::new(),
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
}
