use crate::*;

pub struct RuntimeEngine {
    pub(crate) tokio: Runtime,
    pub(crate) self_ref: Mutex<Weak<RuntimeEngine>>,
    pub(crate) bootstrap: AppBootstrap,
    pub(crate) database: HamburDatabase,
    pub(crate) filestore: FileStore,
    pub(crate) sandbox: SandboxService,
    pub(crate) markdown_streams: Mutex<HashMap<String, MarkdownPipeline>>,
    pub(crate) active_turns: Mutex<HashMap<String, ActiveTurn>>,
    pub(crate) platform_requests: Mutex<HashMap<String, oneshot::Sender<PlatformResultPayload>>>,
    pub(crate) delegate_tasks: Mutex<HashMap<String, DelegateTaskState>>,
    pub(crate) delegate_sessions: Mutex<HashSet<String>>,
    pub(crate) process_sessions: Mutex<HashMap<String, BackgroundProcessSession>>,
    pub(crate) completed_process_sessions: Mutex<HashMap<String, CompletedProcessSession>>,
    pub(crate) memory_review_sessions: Mutex<HashSet<String>>,
    pub(crate) router: Mutex<ModelRouter>,
    pub(crate) tools: ToolScheduler,
    pub(crate) idempotency: Mutex<HashMap<String, RuntimeCommandAck>>,
    pub(crate) sender: mpsc::Sender<RuntimeEvent>,
    pub(crate) receiver: Mutex<mpsc::Receiver<RuntimeEvent>>,
    pub(crate) sequence: AtomicU64,
    pub(crate) shutdown: AtomicBool,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveTurn {
    pub(crate) turn_id: String,
    pub(crate) cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) struct PlatformResultPayload {
    pub(crate) is_error: bool,
    pub(crate) payload_json: String,
    pub(crate) error_code: String,
    pub(crate) message: String,
}

pub(crate) struct DelegateTaskState {
    pub(crate) parent_session_id: String,
    pub(crate) parent_turn_id: String,
    pub(crate) child_session_id: String,
    pub(crate) trace_id: String,
    pub(crate) sender: oneshot::Sender<DelegateCompletionPayload>,
}

#[derive(Debug, Clone)]
pub(crate) struct DelegateCompletionPayload {
    pub(crate) is_error: bool,
    pub(crate) content: Value,
    pub(crate) summary: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DelegateTaskSnapshot {
    pub(crate) parent_session_id: String,
    pub(crate) child_session_id: String,
}

impl DelegateTaskState {
    pub(crate) fn snapshot(&self) -> DelegateTaskSnapshot {
        DelegateTaskSnapshot {
            parent_session_id: self.parent_session_id.clone(),
            child_session_id: self.child_session_id.clone(),
        }
    }
}

pub(crate) struct PreparedDelegateTurn {
    pub(crate) turn_id: String,
    pub(crate) assistant_message_id: String,
    pub(crate) route_candidates: Vec<ModelRouteSnapshot>,
    pub(crate) fallback_policy: FallbackPolicy,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) stream_sources_by_route: Vec<RouteStreamSource>,
}

pub(crate) struct BackgroundProcessSession {
    pub(crate) session_id: String,
    pub(crate) process_session_id: String,
    pub(crate) backend: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) started_at_ms: u64,
    pub(crate) pid: u32,
    pub(crate) pid_file: Option<PathBuf>,
    pub(crate) child: Child,
    pub(crate) output: Arc<Mutex<ProcessOutputBuffer>>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) finished_at_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletedProcessSession {
    pub(crate) session_id: String,
    pub(crate) process_session_id: String,
    pub(crate) backend: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) started_at_ms: u64,
    pub(crate) pid: u32,
    pub(crate) output: ProcessOutputSnapshot,
    pub(crate) exit_code: Option<i32>,
    pub(crate) finished_at_ms: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ProcessOutputBuffer {
    pub(crate) stdout: VecDeque<u8>,
    pub(crate) stderr: VecDeque<u8>,
    pub(crate) stdout_total_bytes: u64,
    pub(crate) stderr_total_bytes: u64,
}

impl ProcessOutputBuffer {
    pub(crate) fn push_stdout(&mut self, bytes: &[u8]) {
        push_ring(&mut self.stdout, bytes);
        self.stdout_total_bytes += bytes.len() as u64;
    }

    pub(crate) fn push_stderr(&mut self, bytes: &[u8]) {
        push_ring(&mut self.stderr, bytes);
        self.stderr_total_bytes += bytes.len() as u64;
    }

    pub(crate) fn snapshot(&self) -> ProcessOutputSnapshot {
        ProcessOutputSnapshot {
            stdout: String::from_utf8_lossy(&self.stdout.iter().copied().collect::<Vec<_>>())
                .to_string(),
            stderr: String::from_utf8_lossy(&self.stderr.iter().copied().collect::<Vec<_>>())
                .to_string(),
            stdout_total_bytes: self.stdout_total_bytes,
            stderr_total_bytes: self.stderr_total_bytes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessOutputSnapshot {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_total_bytes: u64,
    pub(crate) stderr_total_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MemorySnapshot {
    pub(crate) memory_block: String,
    pub(crate) user_block: String,
}

impl MemorySnapshot {
    pub(crate) fn is_empty(&self) -> bool {
        self.memory_block.trim().is_empty() && self.user_block.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MemoryReviewAssistantMessage {
    pub(crate) content: String,
    pub(crate) tool_calls: Vec<CompleteToolCall>,
}

pub(crate) enum StreamAttemptResult {
    Completed,
    Cancelled,
    Continue(ToolContinuation),
    Failed {
        error: HamburError,
        semantic_delta_started: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ToolContinuation {
    pub(crate) assistant_message_id: String,
    pub(crate) route: ModelRouteSnapshot,
    pub(crate) stream_source: RouteStreamSource,
    pub(crate) tool_iteration: u32,
}

#[derive(Debug, Clone)]
pub(crate) enum RouteStreamSource {
    Scripted {
        request: ModelRequest,
        chunks: Vec<Vec<u8>>,
        continuation_sse: Vec<String>,
    },
    Provider(ModelRequest),
}

#[derive(Debug, Default)]
pub(crate) struct StreamAttemptState {
    pub(crate) decoder: SseDecoder,
    pub(crate) content: String,
    pub(crate) reasoning: String,
    pub(crate) reasoning_persisted: bool,
    pub(crate) thinking_raw_trace_count: u32,
    pub(crate) thinking_parsed_trace_count: u32,
    pub(crate) semantic_delta_started: bool,
    pub(crate) finish_reason: String,
    pub(crate) native_finish_reason: String,
    pub(crate) saw_tool_delta: bool,
    pub(crate) tool_accumulator: ToolCallAccumulator,
    pub(crate) complete_tool_calls: Vec<CompleteToolCall>,
}
