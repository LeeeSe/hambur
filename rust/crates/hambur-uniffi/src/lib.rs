use std::sync::Arc;

use hambur_runtime::{
    AppBootstrap, RuntimeCommand, RuntimeCommandAck, RuntimeEngine, RuntimeEvent,
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

pub struct BackendEvent {
    pub event_id: String,
    pub schema_version: u32,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub kind: String,
    pub session_id: String,
    pub turn_id: String,
}

pub enum BackendCommand {
    Initialize,
    Shutdown,
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
        let command = match command {
            BackendCommand::Initialize => RuntimeCommand::Initialize,
            BackendCommand::Shutdown => RuntimeCommand::Shutdown,
        };
        self.engine.dispatch(command).into()
    }

    pub fn next_event(&self) -> Option<BackendEvent> {
        self.engine.next_event().map(BackendEvent::from)
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
        }
    }
}
