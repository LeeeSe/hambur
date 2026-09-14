//! Data types that flow through a tool call: invocation, batch, result, record.

use hambur_core::{HamburError, HamburResult, new_id, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::spec::ToolKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub index: u32,
    /// Resolved tool identity; `None` for names the registry does not know.
    pub kind: Option<ToolKind>,
    pub tool_call_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub name: String,
    pub arguments_json: String,
    pub display_title: String,
    pub risk_level: String,
    pub requires_approval: bool,
    pub timeout_ms: u64,
    pub cancellable: bool,
}

impl ToolInvocation {
    pub fn from_model_call(
        index: u32,
        tool_call_id: String,
        turn_id: String,
        session_id: String,
        name: String,
        arguments_json: String,
    ) -> HamburResult<Self> {
        if tool_call_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "tool_call_id must not be empty".to_string(),
            ));
        }
        if name.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "tool name must not be empty".to_string(),
            ));
        }
        serde_json::from_str::<Value>(&arguments_json).map_err(|error| {
            HamburError::InvalidCommand(format!("tool arguments are not valid JSON: {error}"))
        })?;
        let kind = ToolKind::from_name(&name);
        Ok(Self {
            index,
            tool_call_id,
            turn_id,
            session_id,
            display_title: kind

                .map(|kind| kind.display_title(&arguments_json))

                .unwrap_or_else(|| name.replace('_', " ")),

            risk_level: kind.map(ToolKind::risk).unwrap_or(crate::spec::RiskLevel::Low).as_str().to_string(),

            requires_approval: kind.is_some_and(ToolKind::requires_approval),

            timeout_ms: kind.map(ToolKind::timeout_ms).unwrap_or(30_000),

            kind,
            cancellable: true,
            name,
            arguments_json,
        })
    }

    pub fn arguments_value(&self) -> HamburResult<Value> {
        serde_json::from_str::<Value>(&self.arguments_json).map_err(|error| {
            HamburError::InvalidCommand(format!(
                "tool arguments are not valid JSON for {}: {error}",
                self.name
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallBatch {
    pub batch_id: String,
    pub turn_id: String,
    pub assistant_message_id: String,
    pub calls: Vec<ToolInvocation>,
    pub status: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

impl ToolCallBatch {
    pub fn new(
        turn_id: String,
        assistant_message_id: String,
        mut calls: Vec<ToolInvocation>,
    ) -> Self {
        calls.sort_by_key(|call| call.index);
        Self {
            batch_id: new_id("tool_batch"),
            turn_id,
            assistant_message_id,
            calls,
            status: "pending".to_string(),
            started_at_ms: now_ms(),
            ended_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionRecord {
    pub invocation: ToolInvocation,
    pub result: ToolResult,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
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

impl ToolResult {
    pub fn failed(tool_call_id: &str, tool_name: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            is_error: true,
            content_json: json!({"error": message}).to_string(),
            summary: message.clone(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToolOutput {
    pub tool_call_id: String,
    pub tool_name: String,
    pub is_error: bool,
    pub content: String,
    pub summary: String,
    pub trust_level: String,
    pub command_or_url: String,
    pub status: String,
}
