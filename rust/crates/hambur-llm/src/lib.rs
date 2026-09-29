use std::collections::HashMap;

use hambur_core::{HamburError, HamburResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const OPENAI_COMPATIBLE_PROTOCOL: &str = "OpenAiCompatible";
pub const OPENAI_RESPONSES_PROTOCOL: &str = "OpenAiResponses";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub secret_ref: String,
    pub enabled: bool,
}

pub fn is_official_deepseek_url(url: &str) -> bool {
    let trimmed = url.trim();
    let stripped = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = stripped
        .split(&['/', ':', '?', '#'][..])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    host == "api.deepseek.com" || host == "deepseek.com" || host.ends_with(".deepseek.com")
}

impl ProviderConfig {
    pub fn is_official_deepseek(&self) -> bool {
        is_official_deepseek_url(&self.base_url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub supports_tool_call: bool,
    pub supports_reasoning: bool,
    pub supports_image_input: bool,
    pub supports_structured_output: bool,
    pub supports_temperature: bool,
    pub context_limit: u32,
    pub output_limit: u32,
    pub reasoning_field: String,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            supports_tool_call: false,
            supports_reasoning: false,
            supports_image_input: false,
            supports_structured_output: false,
            supports_temperature: false,
            context_limit: 32_000,
            output_limit: 4096,
            reasoning_field: "reasoning_content".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModel {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
    pub metadata_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderTarget {
    pub provider: ProviderConfig,
    pub model: ProviderModel,
    pub model_group_id: String,
    pub model_group_name: String,
    pub position: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutePlan {
    pub group_id: String,
    pub routing_strategy: RoutingStrategy,
    pub fallback_policy: FallbackPolicy,
    pub targets: Vec<ProviderTarget>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RoutingStrategy {
    #[default]
    Fallback,
    LoadBalance,
}

impl RoutingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::LoadBalance => "load_balance",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "load_balance" => Self::LoadBalance,
            _ => Self::Fallback,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FallbackPolicy {
    #[default]
    Default,
    Always,
}

impl FallbackPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Always => "always",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            _ => Self::Default,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteRequirements {
    pub requires_tool_protocol: bool,
    pub requires_image_input: bool,
    pub requires_structured_output: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModelRouter {
    load_balance_offsets: HashMap<String, usize>,
}

impl ModelRouter {
    pub fn resolve(
        &mut self,
        mut plan: RoutePlan,
        requirements: RouteRequirements,
    ) -> HamburResult<RoutePlan> {
        plan.targets.retain(|target| {
            capability_matches(&target.model.capabilities, &requirements)
                && target.provider.enabled
                && (target.provider.protocol == OPENAI_COMPATIBLE_PROTOCOL
                    || target.provider.protocol == OPENAI_RESPONSES_PROTOCOL)
        });
        if plan.targets.is_empty() {
            return Err(HamburError::CapabilityMismatch(
                "no routed model satisfies the request capabilities".to_string(),
            ));
        }

        if plan.routing_strategy == RoutingStrategy::LoadBalance && plan.targets.len() > 1 {
            let offset = self
                .load_balance_offsets
                .entry(plan.group_id.clone())
                .or_insert(0);
            let rotate_by = *offset % plan.targets.len();
            plan.targets.rotate_left(rotate_by);
            *offset = offset.saturating_add(1);
        }

        Ok(plan)
    }
}

pub fn should_fallback(
    policy: FallbackPolicy,
    semantic_delta_started: bool,
    attempt_index: usize,
    target_count: usize,
    error_code: &str,
) -> bool {
    if semantic_delta_started || attempt_index + 1 >= target_count {
        return false;
    }

    match policy {
        FallbackPolicy::Always => true,
        FallbackPolicy::Default => matches!(
            error_code,
            "Http429" | "Http5xx" | "NetworkTimeout" | "NetworkError"
        ),
    }
}

fn capability_matches(capabilities: &ModelCapabilities, requirements: &RouteRequirements) -> bool {
    if requirements.requires_tool_protocol && !capabilities.supports_tool_call {
        return false;
    }
    if requirements.requires_image_input && !capabilities.supports_image_input {
        return false;
    }
    if requirements.requires_structured_output && !capabilities.supports_structured_output {
        return false;
    }
    true
}

fn model_modalities_include_image(item: &Value) -> bool {
    let Some(modalities) = item
        .get("input_modalities")
        .or_else(|| item.get("inputModalities"))
        .or_else(|| item.get("modalities"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    modalities
        .iter()
        .filter_map(Value::as_str)
        .any(|value| value.eq_ignore_ascii_case("image") || value.eq_ignore_ascii_case("vision"))
}

fn model_id_implies_image_input(model_id: &str) -> bool {
    let normalized = model_id.to_ascii_lowercase();
    normalized.contains("vision")
        || normalized.contains("gpt-4o")
        || normalized.contains("gpt-4.1")
        || normalized.contains("gpt-5")
        || normalized.contains("o3")
        || normalized.contains("o4")
        || normalized.contains("gemini")
        || normalized.contains("claude-3")
        || normalized.contains("claude-4")
        || normalized.contains("qwen-vl")
        || normalized.contains("qwen2.5-vl")
        || normalized.contains("llava")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub request_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub purpose: String,
    pub stream: bool,
    pub system_blocks: Vec<String>,
    pub messages: Vec<ModelMessage>,
    pub reasoning_mode: ReasoningMode,
    pub max_output_tokens: u32,
    pub temperature: Option<f32>,
    #[serde(default)]
    pub tools_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelImagePart {
    pub mime_type: String,
    pub data_base64: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning_content: String,
    #[serde(default)]
    pub tool_calls_json: String,
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub images: Vec<ModelImagePart>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningMode {
    Enabled,
    #[default]
    Disabled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpRequestSpec {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body_json: String,
}

impl HttpRequestSpec {
    pub fn redacted_debug(&self) -> String {
        let headers = self
            .headers
            .iter()
            .map(|(name, value)| {
                if name.eq_ignore_ascii_case("authorization") || value.starts_with("Bearer ") {
                    (name.clone(), "Bearer <redacted>".to_string())
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect::<Vec<_>>();
        format!(
            "HttpRequestSpec {{ method: {}, url: {}, headers: {:?}, body_json: {} }}",
            self.method, self.url, headers, self.body_json
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamPayload {
    pub data: String,
    pub event_name: String,
    pub event_id: String,
    pub raw_frame_meta: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: u32,
        id: String,
        name: String,
        arguments_delta: String,
    },
    ToolCallDone {
        index: u32,
        id: String,
        name: String,
        arguments_json: String,
    },
    Finish {
        finish_reason: String,
        native_finish_reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

pub struct OpenAiCompatibleAdapter;

impl OpenAiCompatibleAdapter {
    pub fn build_stream_request(
        request: &ModelRequest,
        target: &ProviderTarget,
        api_key: &str,
    ) -> HamburResult<HttpRequestSpec> {
        if target.provider.protocol != OPENAI_COMPATIBLE_PROTOCOL {
            return Err(HamburError::ProviderUnavailable(format!(
                "unsupported provider protocol: {}",
                target.provider.protocol
            )));
        }
        if api_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "OpenAI-compatible request requires a resolved API key".to_string(),
            ));
        }

        let mut messages = Vec::new();
        for system in &request.system_blocks {
            if !system.trim().is_empty() {
                messages.push(json!({
                    "role": "system",
                    "content": system,
                }));
            }
        }
        for message in &request.messages {
            let mut object = serde_json::Map::new();
            object.insert("role".to_string(), json!(message.role));
            if message.images.is_empty() {
                object.insert("content".to_string(), json!(message.content));
            } else {
                let mut parts = Vec::new();
                if !message.content.trim().is_empty() {
                    parts.push(json!({
                        "type": "text",
                        "text": message.content,
                    }));
                }
                for image in &message.images {
                    let mut image_url = json!({
                        "url": format!("data:{};base64,{}", image.mime_type, image.data_base64),
                    });
                    if !image.detail.trim().is_empty() {
                        image_url["detail"] = json!(image.detail);
                    }
                    parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url,
                    }));
                }
                object.insert("content".to_string(), Value::Array(parts));
            }
            if !message.reasoning_content.trim().is_empty() {
                object.insert(
                    "reasoning_content".to_string(),
                    json!(message.reasoning_content),
                );
            }
            if !message.tool_calls_json.trim().is_empty() {
                let tool_calls: Value = serde_json::from_str(message.tool_calls_json.trim())
                    .map_err(|error| {
                        HamburError::InvalidCommand(format!(
                            "invalid OpenAI assistant tool_calls JSON: {error}"
                        ))
                    })?;
                match tool_calls {
                    Value::Array(items) if !items.is_empty() => {
                        object.insert("tool_calls".to_string(), Value::Array(items));
                    }
                    Value::Array(_) => {}
                    _ => {
                        return Err(HamburError::InvalidCommand(
                            "OpenAI assistant tool_calls JSON must be an array".to_string(),
                        ));
                    }
                }
            }
            if !message.tool_call_id.trim().is_empty() {
                object.insert("tool_call_id".to_string(), json!(message.tool_call_id));
            }
            messages.push(Value::Object(object));
        }

        let mut body = json!({
            "model": target.model.model_id,
            "stream": true,
            "messages": messages,
        });
        let tools_json = request.tools_json.trim();
        if !tools_json.is_empty() {
            let tools: Value = serde_json::from_str(tools_json).map_err(|error| {
                HamburError::InvalidCommand(format!("invalid OpenAI tools JSON: {error}"))
            })?;
            match &tools {
                Value::Array(items) if !items.is_empty() => {
                    body["tools"] = tools;
                    body["tool_choice"] = json!("auto");
                }
                Value::Array(_) => {}
                _ => {
                    return Err(HamburError::InvalidCommand(
                        "OpenAI tools JSON must be an array".to_string(),
                    ));
                }
            }
        }
        if request.max_output_tokens > 0 {
            body["max_tokens"] = json!(request.max_output_tokens);
        }
        if let Some(temperature) = request.temperature
            && target.model.capabilities.supports_temperature
        {
            body["temperature"] = json!(temperature);
        }
        if target.model.capabilities.supports_reasoning {
            body["thinking"] = json!({
                "type": if request.reasoning_mode == ReasoningMode::Enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            });
            if request.reasoning_mode == ReasoningMode::Enabled {
                body["reasoning_effort"] = json!("high");
            }
        }
        eprintln!(
            "ThinkingToggle build_openai_request session={} model={} supports_reasoning={} reasoning_mode={:?} thinking={} reasoning_effort={}",
            request.session_id,
            target.model.model_id,
            target.model.capabilities.supports_reasoning,
            request.reasoning_mode,
            body.get("thinking").map(Value::to_string).unwrap_or_else(|| "null".to_string()),
            body.get("reasoning_effort").map(Value::to_string).unwrap_or_else(|| "null".to_string()),
        );

        Ok(HttpRequestSpec {
            method: "POST".to_string(),
            url: format!(
                "{}/chat/completions",
                target.provider.base_url.trim_end_matches('/')
            ),
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {}", api_key.trim()),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body_json: body.to_string(),
        })
    }

    pub fn parse_models_response(
        provider_id: &str,
        response_body: &str,
    ) -> HamburResult<Vec<ProviderModel>> {
        let value: Value = serde_json::from_str(response_body).map_err(|error| {
            HamburError::Internal(format!("parse OpenAI-compatible models response: {error}"))
        })?;
        let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
            HamburError::Internal("models response missing data array".to_string())
        })?;

        let mut models = Vec::new();
        for item in data {
            let Some(model_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            let display_name = item
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(model_id)
                .to_string();
            let mut capabilities = ModelCapabilities::default();
            capabilities.supports_reasoning = item
                .get("supports_reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            capabilities.supports_tool_call = item
                .get("supports_tool_call")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            capabilities.supports_image_input = item
                .get("supports_image_input")
                .or_else(|| item.get("supportsImageInput"))
                .or_else(|| item.get("supports_vision"))
                .or_else(|| item.get("supportsVision"))
                .and_then(Value::as_bool)
                .unwrap_or_else(|| model_modalities_include_image(item) || model_id_implies_image_input(model_id));
            capabilities.supports_structured_output = item
                .get("supports_structured_output")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            capabilities.supports_temperature = item
                .get("supports_temperature")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            capabilities.context_limit = item
                .get("context_limit")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(capabilities.context_limit);
            capabilities.output_limit = item
                .get("output_limit")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(capabilities.output_limit);

            models.push(ProviderModel {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                display_name,
                capabilities,
                metadata_json: strip_pricing_fields(item).to_string(),
            });
        }

        Ok(models)
    }

    pub fn parse_stream_payload(payload: &StreamPayload) -> HamburResult<Vec<ProviderStreamEvent>> {
        if payload.data.trim() == "[DONE]" {
            return Ok(vec![ProviderStreamEvent::Finish {
                finish_reason: "stop".to_string(),
                native_finish_reason: "done".to_string(),
            }]);
        }

        let value: Value = serde_json::from_str(&payload.data).map_err(|error| {
            HamburError::SseParse(format!("parse OpenAI-compatible stream payload: {error}"))
        })?;
        if let Some(error) = value.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .or_else(|| error.get("type").and_then(Value::as_str))
                .unwrap_or("ProviderError")
                .to_string();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("provider stream error")
                .to_string();
            return Ok(vec![ProviderStreamEvent::Error { code, message }]);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| HamburError::SseParse("stream payload missing choices".to_string()))?;

        let mut events = Vec::new();
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(content) = delta.get("content").and_then(Value::as_str)
                    && !content.is_empty()
                {
                    events.push(ProviderStreamEvent::ContentDelta(content.to_string()));
                }
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str)
                    && !reasoning.is_empty()
                {
                    events.push(ProviderStreamEvent::ReasoningDelta(reasoning.to_string()));
                }
                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tool_call in tool_calls {
                        let index = tool_call
                            .get("index")
                            .and_then(Value::as_u64)
                            .and_then(|value| u32::try_from(value).ok())
                            .unwrap_or_default();
                        let id = tool_call
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let function = tool_call.get("function").unwrap_or(&Value::Null);
                        let name = function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let arguments_delta = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        events.push(ProviderStreamEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta,
                        });
                    }
                }
            }

            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                let native = choice
                    .get("native_finish_reason")
                    .and_then(Value::as_str)
                    .unwrap_or(reason)
                    .to_string();
                events.push(ProviderStreamEvent::Finish {
                    finish_reason: reason.to_string(),
                    native_finish_reason: native,
                });
            }
        }

        Ok(events)
    }
}

pub struct ResponsesApiAdapter;

impl ResponsesApiAdapter {
    pub fn build_stream_request(
        request: &ModelRequest,
        target: &ProviderTarget,
        api_key: &str,
    ) -> HamburResult<HttpRequestSpec> {
        if target.provider.protocol != OPENAI_RESPONSES_PROTOCOL && !target.provider.is_official_deepseek() {
            return Err(HamburError::ProviderUnavailable(format!(
                "unsupported provider protocol: {}",
                target.provider.protocol
            )));
        }
        if api_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "Responses API request requires a resolved API key".to_string(),
            ));
        }

        let mut input = Vec::new();
        let mut instructions_parts = Vec::new();

        for system in &request.system_blocks {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                instructions_parts.push(trimmed);
            }
        }

        for message in &request.messages {
            match message.role.as_str() {
                "user" => {
                    if message.images.is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "user",
                            "content": message.content,
                        }));
                    } else {
                        let mut content_parts = Vec::new();
                        if !message.content.trim().is_empty() {
                            content_parts.push(json!({
                                "type": "input_text",
                                "text": message.content,
                            }));
                        }
                        for img in &message.images {
                            let data_url = format!("data:{};base64,{}", img.mime_type, img.data_base64);
                            let mut img_part = json!({
                                "type": "input_image",
                                "image_url": data_url,
                            });
                            if !img.detail.trim().is_empty() {
                                img_part["detail"] = json!(img.detail);
                            }
                            content_parts.push(img_part);
                        }
                        input.push(json!({
                            "type": "message",
                            "role": "user",
                            "content": content_parts,
                        }));
                    }
                }
                "assistant" => {
                    if !message.content.trim().is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": message.content,
                        }));
                    }
                    if !message.tool_calls_json.trim().is_empty() {
                        if let Ok(tool_calls) = serde_json::from_str::<Value>(message.tool_calls_json.trim()) {
                            if let Some(items) = tool_calls.as_array() {
                                for item in items {
                                    let call_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                                    let function = item.get("function").unwrap_or(&Value::Null);
                                    let name = function.get("name").and_then(Value::as_str).unwrap_or_default();
                                    let arguments = if let Some(s) = function.get("arguments").and_then(Value::as_str) {
                                        s.to_string()
                                    } else if let Some(obj) = function.get("arguments") {
                                        obj.to_string()
                                    } else {
                                        String::new()
                                    };
                                    input.push(json!({
                                        "type": "function_call",
                                        "call_id": call_id,
                                        "name": name,
                                        "arguments": arguments,
                                    }));
                                }
                            }
                        }
                    }
                }
                "tool" => {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": message.tool_call_id,
                        "output": message.content,
                    }));
                }
                _ => {
                    input.push(json!({
                        "type": "message",
                        "role": message.role,
                        "content": message.content,
                    }));
                }
            }
        }

        let mut body = json!({
            "model": target.model.model_id,
            "stream": true,
            "input": input,
        });

        if !instructions_parts.is_empty() {
            body["instructions"] = json!(instructions_parts.join("\n\n"));
        }

        let tools_json = request.tools_json.trim();
        let is_official_deepseek = target.provider.is_official_deepseek();
        let mut responses_tools = Vec::new();

        if !tools_json.is_empty() {
            let tools: Value = serde_json::from_str(tools_json).map_err(|error| {
                HamburError::InvalidCommand(format!("invalid Responses API tools JSON: {error}"))
            })?;
            if let Some(items) = tools.as_array() {
                for item in items {
                    if let Some(function) = item.get("function") {
                        let name = function.get("name").and_then(Value::as_str).unwrap_or_default();
                        if is_official_deepseek && name == "web_search" {
                            responses_tools.push(json!({ "type": "web_search" }));
                        } else {
                            let mut tool = json!({ "type": "function" });
                            if let Some(name) = function.get("name") { tool["name"] = name.clone(); }
                            if let Some(desc) = function.get("description") { tool["description"] = desc.clone(); }
                            if let Some(params) = function.get("parameters") { tool["parameters"] = params.clone(); }
                            if let Some(strict) = function.get("strict") { tool["strict"] = strict.clone(); }
                            responses_tools.push(tool);
                        }
                    } else {
                        let tool_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                        if is_official_deepseek && tool_type == "web_search" {
                            responses_tools.push(json!({ "type": "web_search" }));
                        } else {
                            responses_tools.push(item.clone());
                        }
                    }
                }
            }
        }

        if is_official_deepseek {
            let has_web_search = responses_tools.iter().any(|t| {
                t.get("type").and_then(Value::as_str) == Some("web_search")
            });
            if !has_web_search {
                responses_tools.push(json!({ "type": "web_search" }));
            }
        }

        if !responses_tools.is_empty() {
            body["tools"] = Value::Array(responses_tools);
            body["tool_choice"] = json!("auto");
        }

        if request.max_output_tokens > 0 {
            body["max_output_tokens"] = json!(request.max_output_tokens);
        }
        if let Some(temperature) = request.temperature
            && target.model.capabilities.supports_temperature
        {
            body["temperature"] = json!(temperature);
        }
        if target.model.capabilities.supports_reasoning {
            body["reasoning"] = json!({
                "effort": if request.reasoning_mode == ReasoningMode::Enabled {
                    "high"
                } else {
                    "low"
                }
            });
        }

        let mut base_url = target.provider.base_url.trim().trim_end_matches('/').to_string();
        if base_url.ends_with("/chat/completions") {
            base_url = base_url.trim_end_matches("/chat/completions").trim_end_matches('/').to_string();
        }
        if base_url.ends_with("/responses") {
            base_url = base_url.trim_end_matches("/responses").trim_end_matches('/').to_string();
        }
        if target.provider.is_official_deepseek() {
            if base_url.ends_with("/v1") {
                base_url = base_url.trim_end_matches("/v1").trim_end_matches('/').to_string();
            }
            if base_url.ends_with("/beta") {
                base_url = base_url.trim_end_matches("/beta").trim_end_matches('/').to_string();
            }
        }
        let url = if base_url == "https://api.openai.com" {
            "https://api.openai.com/v1/responses".to_string()
        } else {
            format!("{}/responses", base_url)
        };

        Ok(HttpRequestSpec {
            method: "POST".to_string(),
            url,
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {}", api_key.trim()),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body_json: body.to_string(),
        })
    }

    pub fn parse_stream_payload(payload: &StreamPayload) -> HamburResult<Vec<ProviderStreamEvent>> {
        if payload.data.trim() == "[DONE]" {
            return Ok(vec![ProviderStreamEvent::Finish {
                finish_reason: "stop".to_string(),
                native_finish_reason: "done".to_string(),
            }]);
        }

        let value: Value = serde_json::from_str(&payload.data).map_err(|error| {
            HamburError::SseParse(format!("parse Responses API stream payload: {error}"))
        })?;

        let error = value.get("error").or_else(|| {
            value
                .get("response")
                .and_then(|r| r.get("status_details"))
                .and_then(|sd| sd.get("error"))
        });
        if let Some(error) = error {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .or_else(|| error.get("type").and_then(Value::as_str))
                .unwrap_or("ProviderError")
                .to_string();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("provider stream error")
                .to_string();
            return Ok(vec![ProviderStreamEvent::Error { code, message }]);
        }

        if value.get("choices").is_some() {
            return OpenAiCompatibleAdapter::parse_stream_payload(payload);
        }

        let event_type = if !payload.event_name.trim().is_empty() {
            payload.event_name.trim()
        } else {
            value.get("type").and_then(Value::as_str).unwrap_or_default()
        };

        let mut events = Vec::new();
        match event_type {
            "response.output_text.delta"
            | "response.text.delta"
            | "response.content_part.delta"
            | "response.output_item.delta" => {
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("delta").and_then(|d| d.get("text")).and_then(Value::as_str))
                    .or_else(|| value.get("text").and_then(Value::as_str))
                    .or_else(|| value.get("part").and_then(|p| p.get("text")).and_then(Value::as_str))
                    .unwrap_or_default();
                if !delta.is_empty() {
                    events.push(ProviderStreamEvent::ContentDelta(delta.to_string()));
                }
            }
            "response.reasoning_text.delta"
            | "response.reasoning.delta"
            | "response.reasoning_content.delta"
            | "response.reasoning_summary_text.delta" => {
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("delta").and_then(|d| d.get("text")).and_then(Value::as_str))
                    .or_else(|| value.get("text").and_then(Value::as_str))
                    .or_else(|| value.get("summary").and_then(Value::as_str))
                    .unwrap_or_default();
                if !delta.is_empty() {
                    events.push(ProviderStreamEvent::ReasoningDelta(delta.to_string()));
                }
            }
            "response.web_search_call.in_progress" => {}
            "response.web_search_call.searching" => {
                let query = value
                    .get("query")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("item").and_then(|i| i.get("query")).and_then(Value::as_str))
                    .unwrap_or_default();
                if !query.is_empty() {
                    events.push(ProviderStreamEvent::ReasoningDelta(format!(
                        "\n🔍 正在联网搜索：{}\n",
                        query
                    )));
                } else {
                    events.push(ProviderStreamEvent::ReasoningDelta(
                        "\n🔍 正在联网搜索...\n".to_string(),
                    ));
                }
            }
            "response.web_search_call.completed" => {}
            "response.output_item.added" => {
                if let Some(item) = value.get("item") {
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                    if item_type == "function_call" {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or_default();
                        let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                        let id = if !call_id.is_empty() { call_id } else { item_id };
                        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                        let index = value
                            .get("output_index")
                            .or_else(|| value.get("index"))
                            .or_else(|| item.get("output_index"))
                            .or_else(|| item.get("index"))
                            .and_then(Value::as_u64)
                            .and_then(|v| u32::try_from(v).ok())
                            .unwrap_or_default();
                        let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or_default();
                        if !arguments.is_empty() && serde_json::from_str::<Value>(arguments).is_ok() {
                            events.push(ProviderStreamEvent::ToolCallDone {
                                index,
                                id: id.to_string(),
                                name: name.to_string(),
                                arguments_json: arguments.to_string(),
                            });
                        } else {
                            events.push(ProviderStreamEvent::ToolCallDelta {
                                index,
                                id: id.to_string(),
                                name: name.to_string(),
                                arguments_delta: String::new(),
                            });
                        }
                    } else if item_type == "web_search_call" {
                        let query = item
                            .get("query")
                            .or_else(|| item.get("action").and_then(|a| a.get("query")))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !query.is_empty() {
                            events.push(ProviderStreamEvent::ReasoningDelta(format!(
                                "\n🔍 正在联网搜索：{}\n",
                                query
                            )));
                        }
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let call_id = value.get("call_id").and_then(Value::as_str).unwrap_or_default();
                let item_id = value.get("item_id").and_then(Value::as_str).unwrap_or_default();
                let id = if !call_id.is_empty() { call_id } else { item_id };
                let delta = value.get("delta").and_then(Value::as_str).unwrap_or_default();
                let index = value
                    .get("output_index")
                    .or_else(|| value.get("index"))
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or_default();
                if !delta.is_empty() || !id.is_empty() {
                    events.push(ProviderStreamEvent::ToolCallDelta {
                        index,
                        id: id.to_string(),
                        name: String::new(),
                        arguments_delta: delta.to_string(),
                    });
                }
            }
            "response.function_call_arguments.done" => {
                let call_id = value.get("call_id").and_then(Value::as_str).unwrap_or_default();
                let item_id = value.get("item_id").and_then(Value::as_str).unwrap_or_default();
                let id = if !call_id.is_empty() { call_id } else { item_id };
                let arguments = value.get("arguments").and_then(Value::as_str).unwrap_or_default();
                let index = value
                    .get("output_index")
                    .or_else(|| value.get("index"))
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or_default();
                events.push(ProviderStreamEvent::ToolCallDone {
                    index,
                    id: id.to_string(),
                    name: String::new(),
                    arguments_json: arguments.to_string(),
                });
            }
            "response.output_item.done" => {
                if let Some(item) = value.get("item") {
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                    let index = value
                        .get("output_index")
                        .or_else(|| value.get("index"))
                        .or_else(|| item.get("output_index"))
                        .or_else(|| item.get("index"))
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or_default();
                    if item_type == "function_call" {
                        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or_default();
                        let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                        let id = if !call_id.is_empty() { call_id } else { item_id };
                        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                        let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or_default();
                        events.push(ProviderStreamEvent::ToolCallDone {
                            index,
                            id: id.to_string(),
                            name: name.to_string(),
                            arguments_json: arguments.to_string(),
                        });
                    }
                }
            }
            "response.failed" => {
                let error = value.get("error").or_else(|| {
                    value
                        .get("response")
                        .and_then(|r| r.get("status_details"))
                        .and_then(|sd| sd.get("error"))
                });
                let (code, message) = if let Some(err) = error {
                    (
                        err.get("code").and_then(Value::as_str).unwrap_or("ProviderError").to_string(),
                        err.get("message").and_then(Value::as_str).unwrap_or("response failed").to_string(),
                    )
                } else {
                    ("ProviderError".to_string(), "response failed".to_string())
                };
                events.push(ProviderStreamEvent::Error { code, message });
            }
            "response.done" | "response.completed" => {
                let response_obj = value.get("response");
                let status = response_obj
                    .and_then(|r| r.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                if status == "failed" {
                    let error = response_obj
                        .and_then(|r| r.get("status_details"))
                        .and_then(|sd| sd.get("error"));
                    let (code, message) = if let Some(err) = error {
                        (
                            err.get("code").and_then(Value::as_str).unwrap_or("ProviderError").to_string(),
                            err.get("message").and_then(Value::as_str).unwrap_or("response failed").to_string(),
                        )
                    } else {
                        ("ProviderError".to_string(), "response failed".to_string())
                    };
                    events.push(ProviderStreamEvent::Error { code, message });
                } else {
                    events.push(ProviderStreamEvent::Finish {
                        finish_reason: "stop".to_string(),
                        native_finish_reason: status.to_string(),
                    });
                }
            }
            _ => {}
        }

        Ok(events)
    }

    pub fn parse_models_response(
        provider_id: &str,
        payload_json: &str,
    ) -> HamburResult<Vec<ProviderModel>> {
        OpenAiCompatibleAdapter::parse_models_response(provider_id, payload_json)
    }
}

fn strip_pricing_fields(item: &Value) -> Value {
    let mut value = item.clone();
    if let Some(object) = value.as_object_mut() {
        for key in [
            "price",
            "pricing",
            "billing",
            "prompt_price",
            "completion_price",
            "input_cost",
            "output_cost",
            "cost",
        ] {
            object.remove(key);
        }
    }
    value
}

#[derive(Debug, Clone, Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    malformed_frames: u32,
}

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> HamburResult<Vec<StreamPayload>> {
        self.buffer.extend_from_slice(bytes);
        let mut payloads = Vec::new();

        while let Some((frame_end, delimiter_len)) = find_frame_boundary(&self.buffer) {
            let frame_bytes = self.buffer[..frame_end].to_vec();
            self.buffer.drain(..frame_end + delimiter_len);
            if frame_bytes.is_empty() {
                continue;
            }
            match parse_sse_frame(&frame_bytes) {
                Ok(Some(payload)) => payloads.push(payload),
                Ok(None) => {}
                Err(error) => {
                    self.malformed_frames = self.malformed_frames.saturating_add(1);
                    return Err(error);
                }
            }
        }

        Ok(payloads)
    }

    pub fn malformed_frames(&self) -> u32 {
        self.malformed_frames
    }
}

fn find_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for index in 0..buffer.len().saturating_sub(1) {
        if buffer[index] == b'\n' && buffer[index + 1] == b'\n' {
            best = Some((index, 2));
            break;
        }
    }
    for index in 0..buffer.len().saturating_sub(3) {
        if buffer[index..index + 4] == *b"\r\n\r\n" {
            if best.is_none_or(|(best_index, _)| index < best_index) {
                best = Some((index, 4));
            }
            break;
        }
    }
    best
}

fn parse_sse_frame(frame_bytes: &[u8]) -> HamburResult<Option<StreamPayload>> {
    let frame = std::str::from_utf8(frame_bytes)
        .map_err(|error| HamburError::SseParse(format!("invalid UTF-8 SSE frame: {error}")))?;
    let mut data_lines = Vec::<String>::new();
    let mut event_name = String::new();
    let mut event_id = String::new();
    let mut meta_lines = Vec::<String>::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" => data_lines.push(value.to_string()),
            "event" => event_name = value.to_string(),
            "id" => event_id = value.to_string(),
            _ => meta_lines.push(line.to_string()),
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    Ok(Some(StreamPayload {
        data: data_lines.join("\n"),
        event_name,
        event_id,
        raw_frame_meta: meta_lines.join("\n"),
    }))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompleteToolCall {
    pub index: u32,
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCallAccumulator {
    calls: HashMap<u32, PartialToolCall>,
}

impl ToolCallAccumulator {
    pub fn apply(&mut self, event: &ProviderStreamEvent) -> Vec<CompleteToolCall> {
        let (index, id, name, delta, done_json) = match event {
            ProviderStreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => (*index, id.as_str(), name.as_str(), Some(arguments_delta.as_str()), None),
            ProviderStreamEvent::ToolCallDone {
                index,
                id,
                name,
                arguments_json,
            } => (*index, id.as_str(), name.as_str(), None, Some(arguments_json.as_str())),
            _ => return Vec::new(),
        };

        let call = if !id.is_empty() {
            if let Some((_, c)) = self.calls.iter_mut().find(|(_, c)| c.id == id || (!c.item_id.is_empty() && c.item_id == id)) {
                c
            } else {
                self.calls.entry(index).or_default()
            }
        } else {
            self.calls.entry(index).or_default()
        };

        if !id.is_empty() {
            if id.starts_with("item_") {
                call.item_id = id.to_string();
                if call.id.is_empty() {
                    call.id = id.to_string();
                }
            } else if id.starts_with("call_") || call.id.is_empty() || call.id.starts_with("item_") {
                call.id = id.to_string();
            }
        }
        if !name.is_empty() {
            call.name = name.to_string();
        }
        if let Some(delta) = delta {
            call.arguments.push_str(delta);
        }
        if let Some(done) = done_json {
            if !done.is_empty() && (call.arguments.is_empty() || serde_json::from_str::<Value>(&call.arguments).is_err()) {
                call.arguments = done.to_string();
            }
        }

        if call.completed {
            return Vec::new();
        }
        if !call.is_executable() {
            return Vec::new();
        }

        call.completed = true;
        vec![CompleteToolCall {
            index,
            id: call.id.clone(),
            name: call.name.clone(),
            arguments_json: call.arguments.clone(),
        }]
    }

    pub fn completed_calls(&self) -> Vec<CompleteToolCall> {
        let mut calls = self
            .calls
            .iter()
            .filter_map(|(index, call)| {
                if call.is_executable() {
                    Some(CompleteToolCall {
                        index: *index,
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments_json: call.arguments.clone(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        calls.sort_by_key(|call| call.index);
        calls
    }

    pub fn has_incomplete_calls(&self) -> bool {
        self.calls.values().any(|call| !call.is_executable())
    }
}

#[derive(Debug, Clone, Default)]
struct PartialToolCall {
    id: String,
    item_id: String,
    name: String,
    arguments: String,
    completed: bool,
}

impl PartialToolCall {
    fn is_executable(&self) -> bool {
        !self.id.is_empty()
            && !self.name.is_empty()
            && serde_json::from_str::<Value>(&self.arguments).is_ok()
    }
}

pub fn scripted_openai_sse_chunks(content: &str, reasoning: &str) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    if !reasoning.is_empty() {
        frames.push(format!(
            "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":{}}}}}]}}\n\n",
            serde_json::to_string(reasoning).unwrap_or_else(|_| "\"\"".to_string())
        ));
    }
    for word in content.split_inclusive(' ') {
        if word.is_empty() {
            continue;
        }
        frames.push(format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(word).unwrap_or_else(|_| "\"\"".to_string())
        ));
    }
    frames.push("data: [DONE]\n\n".to_string());
    frames.into_iter().map(String::into_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_responses_api_build_request() {
        let target = ProviderTarget {
            provider: ProviderConfig {
                id: "test-provider".to_string(),
                name: "Test Provider".to_string(),
                protocol: OPENAI_RESPONSES_PROTOCOL.to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                secret_ref: "secret".to_string(),
                enabled: true,
            },
            model: ProviderModel {
                provider_id: "test-provider".to_string(),
                model_id: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                capabilities: ModelCapabilities {
                    supports_tool_call: true,
                    supports_reasoning: true,
                    supports_image_input: true,
                    supports_structured_output: false,
                    supports_temperature: true,
                    context_limit: 128000,
                    output_limit: 4096,
                    reasoning_field: "reasoning_content".to_string(),
                },
                metadata_json: "{}".to_string(),
            },
            model_group_id: "default".to_string(),
            model_group_name: "Default".to_string(),
            position: 0,
        };

        let request = ModelRequest {
            request_id: "req_1".to_string(),
            session_id: "sess_1".to_string(),
            turn_id: "turn_1".to_string(),
            system_blocks: vec!["You are an AI assistant.".to_string()],
            messages: vec![
                ModelMessage {
                    role: "user".to_string(),
                    content: "Hello!".to_string(),
                    ..Default::default()
                },
            ],
            tools_json: r#"[{"type":"function","function":{"name":"search","description":"Search the web","parameters":{"type":"object"}}}]"#.to_string(),
            temperature: Some(0.7),
            max_output_tokens: 2048,
            reasoning_mode: ReasoningMode::Enabled,
            ..Default::default()
        };

        let spec = ResponsesApiAdapter::build_stream_request(&request, &target, "sk-test-key")
            .expect("build request");
        assert_eq!(spec.url, "https://api.openai.com/v1/responses");
        assert_eq!(spec.method, "POST");

        let body: Value = serde_json::from_str(&spec.body_json).expect("valid json body");
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], "You are an AI assistant.");

        let input = body["input"].as_array().expect("input array");
        assert_eq!(input.len(), 1); // 1 user message
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "Hello!");

        let tools = body["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "search");
    }

    #[test]
    fn test_responses_api_parse_stream_events() {
        // Output text delta (Responses API primary event)
        let payload0 = StreamPayload {
            data: r#"{"type":"response.output_text.delta","delta":"Hi!"}"#.to_string(),
            event_name: "response.output_text.delta".to_string(),
            event_id: "0".to_string(),
            raw_frame_meta: String::new(),
        };
        let events0 = ResponsesApiAdapter::parse_stream_payload(&payload0).expect("parse output text delta");
        assert_eq!(events0, vec![ProviderStreamEvent::ContentDelta("Hi!".to_string())]);

        // Text delta (fallback event)
        let payload1 = StreamPayload {
            data: r#"{"type":"response.text.delta","delta":"Hello world"}"#.to_string(),
            event_name: "response.text.delta".to_string(),
            event_id: "1".to_string(),
            raw_frame_meta: String::new(),
        };
        let events1 = ResponsesApiAdapter::parse_stream_payload(&payload1).expect("parse text delta");
        assert_eq!(events1, vec![ProviderStreamEvent::ContentDelta("Hello world".to_string())]);

        // Reasoning delta
        let payload_reasoning = StreamPayload {
            data: r#"{"type":"response.reasoning_text.delta","delta":"Thinking..."}"#.to_string(),
            event_name: "response.reasoning_text.delta".to_string(),
            event_id: "1r".to_string(),
            raw_frame_meta: String::new(),
        };
        let events_r = ResponsesApiAdapter::parse_stream_payload(&payload_reasoning).expect("parse reasoning delta");
        assert_eq!(events_r, vec![ProviderStreamEvent::ReasoningDelta("Thinking...".to_string())]);

        // Function call item added (with top-level output_index)
        let payload2 = StreamPayload {
            data: r#"{"type":"response.output_item.added","output_index":1,"item":{"id":"item_call_1","type":"function_call","name":"view_image","call_id":"call_999"}}"#.to_string(),
            event_name: "response.output_item.added".to_string(),
            event_id: "2".to_string(),
            raw_frame_meta: String::new(),
        };
        let events2 = ResponsesApiAdapter::parse_stream_payload(&payload2).expect("parse call added");
        assert_eq!(events2, vec![ProviderStreamEvent::ToolCallDelta {
            index: 1,
            id: "call_999".to_string(),
            name: "view_image".to_string(),
            arguments_delta: String::new(),
        }]);

        // Function call arguments delta
        let payload3 = StreamPayload {
            data: r#"{"type":"response.function_call_arguments.delta","output_index":1,"call_id":"call_999","delta":"{\"path\":\"foo.png\"}"}"#.to_string(),
            event_name: "response.function_call_arguments.delta".to_string(),
            event_id: "3".to_string(),
            raw_frame_meta: String::new(),
        };
        let events3 = ResponsesApiAdapter::parse_stream_payload(&payload3).expect("parse args delta");
        assert_eq!(events3, vec![ProviderStreamEvent::ToolCallDelta {
            index: 1,
            id: "call_999".to_string(),
            name: String::new(),
            arguments_delta: "{\"path\":\"foo.png\"}".to_string(),
        }]);

        // Accumulator should complete the call
        let mut acc = ToolCallAccumulator::default();
        let _ = acc.apply(&events2[0]);
        let completed = acc.apply(&events3[0]);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].index, 1);
        assert_eq!(completed[0].id, "call_999");
        assert_eq!(completed[0].name, "view_image");
        assert_eq!(completed[0].arguments_json, "{\"path\":\"foo.png\"}");

        // Output item done with full arguments
        let payload_done = StreamPayload {
            data: r#"{"type":"response.output_item.done","output_index":1,"item":{"id":"item_call_1","type":"function_call","name":"view_image","call_id":"call_999","arguments":"{\"path\":\"foo.png\"}"}}"#.to_string(),
            event_name: "response.output_item.done".to_string(),
            event_id: "4".to_string(),
            raw_frame_meta: String::new(),
        };
        let events_done = ResponsesApiAdapter::parse_stream_payload(&payload_done).expect("parse item done");
        assert_eq!(events_done, vec![ProviderStreamEvent::ToolCallDone {
            index: 1,
            id: "call_999".to_string(),
            name: "view_image".to_string(),
            arguments_json: "{\"path\":\"foo.png\"}".to_string(),
        }]);

        // Response done
        let payload4 = StreamPayload {
            data: r#"{"type":"response.done","response":{"status":"completed"}}"#.to_string(),
            event_name: "response.done".to_string(),
            event_id: "5".to_string(),
            raw_frame_meta: String::new(),
        };
        let events4 = ResponsesApiAdapter::parse_stream_payload(&payload4).expect("parse done");
        assert_eq!(events4, vec![ProviderStreamEvent::Finish {
            finish_reason: "stop".to_string(),
            native_finish_reason: "completed".to_string(),
        }]);
    }

    #[test]
    fn test_is_official_deepseek() {
        let make_provider = |url: &str| ProviderConfig {
            base_url: url.to_string(),
            ..Default::default()
        };

        assert!(make_provider("https://api.deepseek.com").is_official_deepseek());
        assert!(make_provider("https://api.deepseek.com/").is_official_deepseek());
        assert!(make_provider("https://api.deepseek.com/v1").is_official_deepseek());
        assert!(make_provider("https://api.deepseek.com:443/beta").is_official_deepseek());
        assert!(make_provider("http://deepseek.com").is_official_deepseek());
        assert!(make_provider("https://chat.deepseek.com").is_official_deepseek());

        // Non-official providers
        assert!(!make_provider("https://api.siliconflow.cn/v1").is_official_deepseek());
        assert!(!make_provider("https://openrouter.ai/api/v1").is_official_deepseek());
        assert!(!make_provider("https://notdeepseek.com").is_official_deepseek());
        assert!(!make_provider("https://deepseek.com.attacker.com").is_official_deepseek());
        assert!(!make_provider("https://api.openai.com/v1").is_official_deepseek());
        assert!(!make_provider("").is_official_deepseek());
    }

    #[test]
    fn test_official_deepseek_responses_api_web_search_injection() {
        let deepseek_provider = ProviderConfig {
            id: "deepseek".to_string(),
            name: "DeepSeek".to_string(),
            protocol: OPENAI_RESPONSES_PROTOCOL.to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            secret_ref: "secret_deepseek".to_string(),
            enabled: true,
        };
        let deepseek_target = ProviderTarget {
            provider: deepseek_provider,
            model: ProviderModel {
                provider_id: "deepseek".to_string(),
                model_id: "deepseek-chat".to_string(),
                display_name: "DeepSeek-V3".to_string(),
                capabilities: ModelCapabilities {
                    supports_tool_call: true,
                    supports_reasoning: false,
                    ..Default::default()
                },
                metadata_json: "{}".to_string(),
            },
            model_group_id: "default".to_string(),
            model_group_name: "Default".to_string(),
            position: 0,
        };

        // Case 1: tools_json has function web_search -> converted to {"type": "web_search"}
        let request_with_web_search = ModelRequest {
            system_blocks: vec![],
            messages: vec![ModelMessage {
                role: "user".to_string(),
                content: "Latest news today?".to_string(),
                ..Default::default()
            }],
            tools_json: r#"[
                {"type":"function","function":{"name":"web_search","description":"Search","parameters":{"type":"object"}}},
                {"type":"function","function":{"name":"read_file","description":"Read","parameters":{"type":"object"}}}
            ]"#.to_string(),
            ..Default::default()
        };

        let spec = ResponsesApiAdapter::build_stream_request(&request_with_web_search, &deepseek_target, "sk-ds-key")
            .expect("build request");
        let body: Value = serde_json::from_str(&spec.body_json).expect("valid json");
        let tools = body["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0], json!({ "type": "web_search" }));
        assert_eq!(tools[1]["type"], "function");
        assert_eq!(tools[1]["name"], "read_file");

        // Case 2: tools_json is empty -> official deepseek still gets {"type": "web_search"}
        let request_empty_tools = ModelRequest {
            system_blocks: vec![],
            messages: vec![ModelMessage {
                role: "user".to_string(),
                content: "Who won the game yesterday?".to_string(),
                ..Default::default()
            }],
            tools_json: String::new(),
            ..Default::default()
        };
        let spec2 = ResponsesApiAdapter::build_stream_request(&request_empty_tools, &deepseek_target, "sk-ds-key")
            .expect("build request");
        let body2: Value = serde_json::from_str(&spec2.body_json).expect("valid json");
        let tools2 = body2["tools"].as_array().expect("tools array");
        assert_eq!(tools2.len(), 1);
        assert_eq!(tools2[0], json!({ "type": "web_search" }));

        // Case 3: Non-official provider should NOT convert or inject web_search
        let mut non_official_target = deepseek_target.clone();
        non_official_target.provider.base_url = "https://api.siliconflow.cn/v1".to_string();
        let spec3 = ResponsesApiAdapter::build_stream_request(&request_with_web_search, &non_official_target, "sk-sf-key")
            .expect("build request");
        let body3: Value = serde_json::from_str(&spec3.body_json).expect("valid json");
        let tools3 = body3["tools"].as_array().expect("tools array");
        assert_eq!(tools3.len(), 2);
        assert_eq!(tools3[0]["type"], "function");
        assert_eq!(tools3[0]["name"], "web_search");
    }

    #[test]
    fn test_responses_api_parse_web_search_stream_events() {
        // response.web_search_call.searching
        let payload_searching = StreamPayload {
            data: r#"{"type":"response.web_search_call.searching","query":"Rust 2024 features"}"#.to_string(),
            event_name: "response.web_search_call.searching".to_string(),
            event_id: "ws_1".to_string(),
            raw_frame_meta: String::new(),
        };
        let events = ResponsesApiAdapter::parse_stream_payload(&payload_searching).expect("parse search");
        assert_eq!(events, vec![ProviderStreamEvent::ReasoningDelta("\n🔍 正在联网搜索：Rust 2024 features\n".to_string())]);

        // response.output_item.added with web_search_call
        let payload_item_added = StreamPayload {
            data: r#"{"type":"response.output_item.added","item":{"id":"ws_item_1","type":"web_search_call","query":"DeepSeek responses API"}}"#.to_string(),
            event_name: "response.output_item.added".to_string(),
            event_id: "ws_2".to_string(),
            raw_frame_meta: String::new(),
        };
        let events_added = ResponsesApiAdapter::parse_stream_payload(&payload_item_added).expect("parse item added");
        assert_eq!(events_added, vec![ProviderStreamEvent::ReasoningDelta("\n🔍 正在联网搜索：DeepSeek responses API\n".to_string())]);

        // response.output_item.done with web_search_call must NOT emit ToolCallDone
        let payload_item_done = StreamPayload {
            data: r#"{"type":"response.output_item.done","item":{"id":"ws_item_1","type":"web_search_call","status":"completed"}}"#.to_string(),
            event_name: "response.output_item.done".to_string(),
            event_id: "ws_3".to_string(),
            raw_frame_meta: String::new(),
        };
        let events_done = ResponsesApiAdapter::parse_stream_payload(&payload_item_done).expect("parse item done");
        assert!(events_done.is_empty(), "web_search_call output_item.done should not emit any ToolCallDone");
    }
}
