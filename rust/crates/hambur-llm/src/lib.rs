use std::collections::HashMap;

use hambur_core::{HamburError, HamburResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const OPENAI_COMPATIBLE_PROTOCOL: &str = "OpenAiCompatible";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub secret_ref: String,
    pub enabled: bool,
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
                && target.provider.protocol == OPENAI_COMPATIBLE_PROTOCOL
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
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
            messages.push(json!({
                "role": message.role,
                "content": message.content,
            }));
        }

        let mut body = json!({
            "model": target.model.model_id,
            "stream": true,
            "messages": messages,
        });
        if request.max_output_tokens > 0 {
            body["max_tokens"] = json!(request.max_output_tokens);
        }
        if let Some(temperature) = request.temperature
            && target.model.capabilities.supports_temperature
        {
            body["temperature"] = json!(temperature);
        }
        if request.reasoning_mode == ReasoningMode::Enabled
            && target.model.capabilities.supports_reasoning
        {
            body["reasoning"] = json!({"enabled": true});
        }

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
                .and_then(Value::as_bool)
                .unwrap_or(false);
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
        let ProviderStreamEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta,
        } = event
        else {
            return Vec::new();
        };

        let call = self.calls.entry(*index).or_default();
        if !id.is_empty() {
            call.id = id.clone();
        }
        if !name.is_empty() {
            call.name = name.clone();
        }
        call.arguments.push_str(arguments_delta);

        if call.completed {
            return Vec::new();
        }
        if !call.is_executable() {
            return Vec::new();
        }

        call.completed = true;
        vec![CompleteToolCall {
            index: *index,
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
    fn openai_request_json_contains_model_and_redacts_secret() {
        let target = test_target("gpt-test", ModelCapabilities::default());
        let request = ModelRequest {
            request_id: "req".to_string(),
            session_id: "ses".to_string(),
            turn_id: "turn".to_string(),
            purpose: "chat".to_string(),
            stream: true,
            system_blocks: vec!["You are concise.".to_string()],
            messages: vec![ModelMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            max_output_tokens: 128,
            ..Default::default()
        };

        let spec = OpenAiCompatibleAdapter::build_stream_request(&request, &target, "sk-secret")
            .expect("request spec");
        assert_eq!(spec.method, "POST");
        assert_eq!(spec.url, "https://api.test/v1/chat/completions");
        assert!(spec.body_json.contains("\"model\":\"gpt-test\""));
        assert!(spec.body_json.contains("\"stream\":true"));
        assert!(
            spec.headers
                .iter()
                .any(|(_, value)| value == "Bearer sk-secret")
        );
        assert!(!spec.redacted_debug().contains("sk-secret"));
    }

    #[test]
    fn model_refresh_parses_models_and_strips_pricing() {
        let models = OpenAiCompatibleAdapter::parse_models_response(
            "provider",
            r#"{"data":[{"id":"gpt-a","display_name":"GPT A","supports_reasoning":true,"price":12}]}"#,
        )
        .expect("models");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "gpt-a");
        assert!(models[0].capabilities.supports_reasoning);
        assert!(!models[0].metadata_json.contains("price"));
    }

    #[test]
    fn sse_decoder_handles_split_chunks_and_multiline_data() {
        let mut decoder = SseDecoder::default();
        let first = decoder
            .push(b"id: 1\nevent: delta\ndata: {\"a\":")
            .expect("first chunk");
        assert!(first.is_empty());
        let second = decoder
            .push(b"1}\ndata: {\"b\":2}\n\n")
            .expect("second chunk");

        assert_eq!(second.len(), 1);
        assert_eq!(second[0].event_id, "1");
        assert_eq!(second[0].event_name, "delta");
        assert_eq!(second[0].data, "{\"a\":1}\n{\"b\":2}");
    }

    #[test]
    fn openai_stream_parses_content_and_reasoning() {
        let mut decoder = SseDecoder::default();
        let payloads = decoder
            .push(
                b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n",
            )
            .expect("payloads");
        let events = payloads
            .iter()
            .flat_map(|payload| OpenAiCompatibleAdapter::parse_stream_payload(payload).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::ReasoningDelta("thinking".to_string()),
                ProviderStreamEvent::ContentDelta("answer".to_string()),
                ProviderStreamEvent::Finish {
                    finish_reason: "stop".to_string(),
                    native_finish_reason: "stop".to_string(),
                },
            ]
        );
    }

    #[test]
    fn openai_stream_maps_error_payload() {
        let payload = StreamPayload {
            data: r#"{"error":{"code":"Http5xx","message":"upstream unavailable"}}"#.to_string(),
            ..Default::default()
        };
        let events = OpenAiCompatibleAdapter::parse_stream_payload(&payload).expect("events");

        assert_eq!(
            events,
            vec![ProviderStreamEvent::Error {
                code: "Http5xx".to_string(),
                message: "upstream unavailable".to_string(),
            }]
        );
    }

    #[test]
    fn tool_call_accumulator_joins_argument_deltas() {
        let mut accumulator = ToolCallAccumulator::default();
        let first = ProviderStreamEvent::ToolCallDelta {
            index: 0,
            id: "call_1".to_string(),
            name: "search".to_string(),
            arguments_delta: "{\"q\"".to_string(),
        };
        let second = ProviderStreamEvent::ToolCallDelta {
            index: 0,
            id: String::new(),
            name: String::new(),
            arguments_delta: ":\"rust\"}".to_string(),
        };

        assert!(accumulator.apply(&first).is_empty());
        let complete = accumulator.apply(&second);
        assert_eq!(complete.len(), 1);
        assert_eq!(complete[0].arguments_json, "{\"q\":\"rust\"}");
    }

    #[test]
    fn router_filters_capabilities_and_rotates_load_balance() {
        let capable = ModelCapabilities {
            supports_tool_call: true,
            ..Default::default()
        };
        let target_a = test_target("a", capable.clone());
        let target_b = test_target("b", capable);
        let mut router = ModelRouter::default();
        let plan = RoutePlan {
            group_id: "primary".to_string(),
            routing_strategy: RoutingStrategy::LoadBalance,
            fallback_policy: FallbackPolicy::Default,
            targets: vec![target_a.clone(), target_b.clone()],
        };
        let requirements = RouteRequirements {
            requires_tool_protocol: true,
            ..Default::default()
        };

        let first = router
            .resolve(plan.clone(), requirements.clone())
            .expect("first");
        let second = router.resolve(plan, requirements).expect("second");
        assert_eq!(first.targets[0].model.model_id, "a");
        assert_eq!(second.targets[0].model.model_id, "b");
    }

    #[test]
    fn fallback_stops_after_semantic_delta() {
        assert!(should_fallback(
            FallbackPolicy::Default,
            false,
            0,
            2,
            "Http5xx"
        ));
        assert!(!should_fallback(
            FallbackPolicy::Default,
            true,
            0,
            2,
            "Http5xx"
        ));
    }

    fn test_target(model_id: &str, capabilities: ModelCapabilities) -> ProviderTarget {
        ProviderTarget {
            provider: ProviderConfig {
                id: "provider".to_string(),
                name: "Provider".to_string(),
                protocol: OPENAI_COMPATIBLE_PROTOCOL.to_string(),
                base_url: "https://api.test/v1".to_string(),
                secret_ref: "secret://provider".to_string(),
                enabled: true,
            },
            model: ProviderModel {
                provider_id: "provider".to_string(),
                model_id: model_id.to_string(),
                display_name: model_id.to_string(),
                capabilities,
                metadata_json: "{}".to_string(),
            },
            model_group_id: "primary".to_string(),
            model_group_name: "Primary".to_string(),
            position: 0,
        }
    }
}
