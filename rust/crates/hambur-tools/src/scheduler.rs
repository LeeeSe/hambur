//! Executes batches of builtin tools with bounded parallelism and normalises results.
//!
//! Runtime-hosted tools (`ToolHost::Runtime`) are dispatched by the runtime engine; if
//! one reaches this scheduler it is reported as unavailable rather than silently ignored.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use hambur_core::{HamburError, HamburResult, now_ms};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::invocation::{RawToolOutput, ToolCallBatch, ToolExecutionRecord, ToolInvocation, ToolResult};
use crate::normalizer::ToolResultNormalizer;
use crate::spec::{ToolHost, ToolKind, ToolRegistry};

pub const MAX_PARALLEL_TOOL_CALLS: usize = 3;

#[derive(Clone)]
pub struct ToolScheduler {
    registry: ToolRegistry,
    normalizer: ToolResultNormalizer,
    max_parallel_tool_calls: usize,
}

impl ToolScheduler {
    pub fn new(offload_dir: impl AsRef<Path>) -> HamburResult<Self> {
        Ok(Self {
            registry: ToolRegistry::with_builtin_tools()?,
            normalizer: ToolResultNormalizer::new(offload_dir.as_ref().to_path_buf())?,
            max_parallel_tool_calls: MAX_PARALLEL_TOOL_CALLS,
        })
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Backwards-compatible alias for [`ToolScheduler::registry`].
    pub fn schemas(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn with_parallel_limit(mut self, limit: usize) -> Self {
        self.max_parallel_tool_calls = limit.clamp(1, MAX_PARALLEL_TOOL_CALLS);
        self
    }

    pub fn with_app_files_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.normalizer = self.normalizer.with_app_files_dir(dir.as_ref().to_path_buf());
        self
    }

    pub fn normalize_raw(&self, raw: RawToolOutput) -> HamburResult<ToolResult> {
        self.normalizer.normalize(raw)
    }

    pub fn normalize_raw_with_session(
        &self,
        raw: RawToolOutput,
        session_id: Option<&str>,
    ) -> HamburResult<ToolResult> {
        self.normalizer.normalize_with_session(raw, session_id)
    }

    /// Validate arguments and run a single builtin tool, producing a finished record.
    pub fn execute_builtin(&self, invocation: ToolInvocation) -> HamburResult<ToolExecutionRecord> {
        execute_one_tool(&self.registry, &self.normalizer, invocation)
    }

    pub async fn execute_batch(
        &self,
        batch: ToolCallBatch,
    ) -> HamburResult<Vec<ToolExecutionRecord>> {
        let semaphore = Arc::new(Semaphore::new(self.max_parallel_tool_calls));
        let mut queue = VecDeque::from(batch.calls);
        let mut join_set = JoinSet::new();
        let mut results = Vec::new();

        while !queue.is_empty() || !join_set.is_empty() {
            while let Some(invocation) = queue.pop_front() {
                let is_serial = !invocation.kind.is_some_and(ToolKind::is_parallel);
                if is_serial && !join_set.is_empty() {
                    queue.push_front(invocation);
                    break;
                }

                let permit =
                    semaphore.clone().acquire_owned().await.map_err(|error| {
                        HamburError::Internal(format!("tool semaphore: {error}"))
                    })?;
                let registry = self.registry.clone();
                let normalizer = self.normalizer.clone();
                join_set.spawn(async move {
                    let _permit = permit;
                    execute_one_tool(&registry, &normalizer, invocation)
                });

                if is_serial {
                    break;
                }
            }

            if let Some(joined) = join_set.join_next().await {
                let record = joined
                    .map_err(|error| HamburError::Internal(format!("tool task join: {error}")))??;
                results.push(record);
            }
        }

        results.sort_by_key(|record| record.invocation.index);
        Ok(results)
    }
}

fn execute_one_tool(
    registry: &ToolRegistry,
    normalizer: &ToolResultNormalizer,
    invocation: ToolInvocation,
) -> HamburResult<ToolExecutionRecord> {
    let started_at_ms = now_ms();
    let raw = match invocation.arguments_value() {
        Ok(arguments) => match registry.validate_arguments(&invocation.name, &arguments) {
            Ok(()) => run_builtin_tool(&invocation, &arguments),
            Err(error) => failure(
                &invocation,
                error.to_string(),
                "Tool validation failed",
                error.code().as_str(),
            ),
        },
        Err(error) => failure(
            &invocation,
            error.to_string(),
            "Tool arguments were invalid",
            error.code().as_str(),
        ),
    };
    let result = normalizer.normalize_with_session(raw, Some(&invocation.session_id))?;
    Ok(ToolExecutionRecord {
        invocation,
        result,
        started_at_ms,
        ended_at_ms: now_ms(),
    })
}

fn run_builtin_tool(invocation: &ToolInvocation, arguments: &Value) -> RawToolOutput {
    match invocation.kind {
        Some(ToolKind::Echo) => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                summary: text.chars().take(120).collect(),
                content: text,
                trust_level: "trusted".to_string(),
                command_or_url: "echo".to_string(),
                status: "ok".to_string(),
            }
        }
        Some(kind) if kind.host() == ToolHost::Runtime => failure(
            invocation,
            format!("{} must be executed by the runtime host", kind.name()),
            "Tool unavailable outside the runtime",
            "ToolUnavailable",
        ),
        Some(_) | None => failure(
            invocation,
            format!("unknown tool: {}", invocation.name),
            "Unknown tool",
            "InvalidCommand",
        ),
    }
}

fn failure(invocation: &ToolInvocation, content: String, summary: &str, status: &str) -> RawToolOutput {
    RawToolOutput {
        tool_call_id: invocation.tool_call_id.clone(),
        tool_name: invocation.name.clone(),
        is_error: true,
        content,
        summary: summary.to_string(),
        trust_level: "trusted".to_string(),
        command_or_url: invocation.arguments_json.clone(),
        status: status.to_string(),
    }
}
