use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use hambur_core::{DTO_SCHEMA_VERSION, HamburError, HamburResult, new_id, now_ms};
use hambur_db::{
    AppSnapshot, AttachmentRecord, HamburDatabase, MessageBlockPayloadRecord, MessageRecord,
    ModelRouteSnapshot, NewAttachment, NewFileRecord, NewMessageBlockPayload, NewTimelineItem,
    NewToolCall, NewToolResult, NewTraceSpan, ProviderModelOverride, ProviderModelUpsert,
    ProviderUpsert, SessionReviewRecord, SessionSummary, SettingsSnapshot, TimelineItemSnapshot,
};
use hambur_filestore::FileStore;
use hambur_llm::{
    CompleteToolCall, FallbackPolicy, ModelCapabilities, ModelImagePart, ModelMessage, ModelRequest, ModelRouter,
    OPENAI_COMPATIBLE_PROTOCOL, OPENAI_RESPONSES_PROTOCOL, OpenAiCompatibleAdapter, ProviderConfig, ProviderModel,
    ProviderStreamEvent, ProviderTarget, ReasoningMode, ResponsesApiAdapter, RoutePlan, RouteRequirements,
    RoutingStrategy, SseDecoder, ToolCallAccumulator, scripted_openai_sse_chunks, should_fallback,
};
use hambur_markdown::{MarkdownBlockNode, MarkdownPipeline, MarkdownRenderUpdate};
use hambur_sandbox::{SandboxAccess, SandboxService};
use hambur_tools::{
    MAX_TOOL_ITERATIONS_PER_TURN, RawToolOutput, ToolCallBatch, ToolExecutionRecord, ToolHost,
    ToolInvocation, ToolKind, ToolResult, ToolScheduler,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, sleep, timeout};

// Module map:
//   types    - public DTOs exposed over UniFFI (commands, acks, snapshots, events)
//   state    - RuntimeEngine struct and private in-flight state (turns, delegates, processes, streams)
//   engine   - RuntimeEngine behaviour, one file per concern (chat streaming, tool resolvers, memory, skills, events)
//   commands - RuntimeCommand dispatch and execute_* handlers
//   queries  - read-only snapshot accessors
//   utils    - free helper functions grouped by domain (fs, llm, config, prompts, ...)
mod commands;
mod engine;
mod queries;
mod state;
mod types;
mod utils;

pub use types::*;
pub use state::RuntimeEngine;
pub(crate) use state::*;
pub(crate) use utils::*;
