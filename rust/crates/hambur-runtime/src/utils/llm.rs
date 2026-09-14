use crate::*;

pub(crate) fn route_plan_from_records(records: Vec<ModelRouteSnapshot>) -> RoutePlan {
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

pub(crate) fn provider_target_from_route(route: ModelRouteSnapshot) -> ProviderTarget {
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

pub(crate) fn route_snapshot_from_target(target: &ProviderTarget) -> ModelRouteSnapshot {
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

pub(crate) fn append_transcript_entry_to_context(
    messages: &mut Vec<ModelMessage>,
    open_tool_call_ids: &mut HashSet<String>,
    entry: hambur_db::ChatTranscriptEntry,
    attachments: &[AttachmentRecord],
) -> HamburResult<()> {
    let message = entry.message;
    match message.role.as_str() {
        "user" => {
            if message.status == "completed" && !message.content_text.trim().is_empty() {
                let content = format_user_content_with_prefix(
                    &message.prompt_prefix,
                    &message.content_text,
                    attachments,
                );
                messages.push(ModelMessage {
                    role: "user".to_string(),
                    content,
                    ..Default::default()
                });
            }
        }
        "assistant" => {
            if !assistant_message_is_context_eligible(&message, &entry.tool_calls) {
                return Ok(());
            }
            let tool_calls_json = if entry.tool_calls.is_empty() {
                String::new()
            } else {
                tool_calls_json_from_records(&entry.tool_calls)?
            };
            for call in &entry.tool_calls {
                open_tool_call_ids.insert(call.id.clone());
            }
            messages.push(ModelMessage {
                role: "assistant".to_string(),
                content: message.content_text,
                reasoning_content: message.reasoning_content,
                tool_calls_json,
                tool_call_id: String::new(),
                ..Default::default()
            });
        }
        "tool" => {
            if message.status != "completed"
                || message.tool_call_id.trim().is_empty()
                || !open_tool_call_ids.remove(&message.tool_call_id)
            {
                return Ok(());
            }
            messages.push(ModelMessage {
                role: "tool".to_string(),
                content: message.content_text,
                reasoning_content: String::new(),
                tool_calls_json: String::new(),
                tool_call_id: message.tool_call_id,
                ..Default::default()
            });
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn assistant_message_is_context_eligible(
    message: &MessageRecord,
    tool_calls: &[hambur_db::ToolCallRecord],
) -> bool {
    if matches!(
        message.status.as_str(),
        "failed" | "failed_partial" | "cancelled" | "deleted"
    ) {
        return false;
    }
    !message.content_text.trim().is_empty() || !tool_calls.is_empty()
}

pub(crate) fn tool_calls_json_from_records(
    calls: &[hambur_db::ToolCallRecord],
) -> HamburResult<String> {
    let values = calls
        .iter()
        .map(|call| {
            if call.id.trim().is_empty() || call.name.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "stored tool call is missing id or name".to_string(),
                ));
            }
            let arguments = if call.arguments_json.trim().is_empty() {
                "{}"
            } else {
                call.arguments_json.trim()
            };
            Ok(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments,
                }
            }))
        })
        .collect::<HamburResult<Vec<_>>>()?;
    serde_json::to_string(&values)
        .map_err(|error| HamburError::Internal(format!("serialize stored tool calls: {error}")))
}

pub(crate) fn stream_source_for_command(
    command: &RuntimeCommand,
    turn_id: &str,
    content: &str,
    messages: Vec<ModelMessage>,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let provider_source = provider_stream_source(
        &command.session_id,
        turn_id,
        messages,
        route,
        tools_json,
        skills_index_prompt,
        memory_system_prompt,
        deep_thinking_enabled,
        search_enabled,
    );
    let request = match &provider_source {
        RouteStreamSource::Provider(request) => request.clone(),
        RouteStreamSource::Scripted { request, .. } => request.clone(),
    };
    let payload = command.payload_json.trim();
    if payload.starts_with("data:") {
        return scripted_stream_source(request, payload.to_string(), Vec::new());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Some(route_value) = scripted_route_value(&value, route) {
            let continuation_sse = scripted_continuation_sse(route_value);
            if let Some(sse) = route_value.get("sse").and_then(serde_json::Value::as_str) {
                return scripted_stream_source(request, sse.to_string(), continuation_sse);
            }
            let response = route_value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = route_value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
        let continuation_sse = scripted_continuation_sse(&value);
        if let Some(sse) = value.get("sse").and_then(serde_json::Value::as_str) {
            return scripted_stream_source(request, sse.to_string(), continuation_sse);
        }
        if value.get("content").is_some() || value.get("reasoning").is_some() {
            let response = value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
    }

    provider_source
}

pub(crate) fn scripted_stream_source(
    request: ModelRequest,
    sse: String,
    continuation_sse: Vec<String>,
) -> RouteStreamSource {
    RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&sse),
        continuation_sse,
    }
}

pub(crate) fn scripted_continuation_sse(value: &Value) -> Vec<String> {
    if let Some(items) = value
        .get("sse_sequence")
        .or_else(|| value.get("continuation_sse"))
        .and_then(Value::as_array)
    {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    value
        .get("continuationSse")
        .and_then(Value::as_str)
        .map(|sse| vec![sse.to_string()])
        .unwrap_or_default()
}

pub(crate) fn provider_stream_source(
    session_id: &str,
    turn_id: &str,
    messages: Vec<ModelMessage>,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let mut system_blocks = vec![
        "You are Hambur, a concise assistant.".to_string(),
        HAMBUR_FILE_LINK_SYSTEM_PROMPT.to_string(),
        HAMBUR_CONFIG_SYSTEM_PROMPT.to_string(),
    ];
    if !skills_index_prompt.trim().is_empty() {
        system_blocks.push(skills_index_prompt.to_string());
    }
    if !memory_system_prompt.trim().is_empty() {
        system_blocks.push(memory_system_prompt.to_string());
    }
    if search_enabled {
        let has_web_tools = tools_json.contains("\"web_search\"") || tools_json.contains("\"browser_use\"");
        if has_web_tools {
            system_blocks.push(
                "Web assistance is enabled for this turn. Use available web search or browser tools when current external information is needed."
                    .to_string(),
            );
        }
    }
    RouteStreamSource::Provider(ModelRequest {
        request_id: new_id("llm_req"),
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        purpose: "chat".to_string(),
        stream: true,
        system_blocks,
        messages,
        reasoning_mode: if route.supports_reasoning && deep_thinking_enabled {
            ReasoningMode::Enabled
        } else {
            ReasoningMode::Disabled
        },
        max_output_tokens: route.output_limit,
        temperature: Some(0.7),
        tools_json: if route.supports_tool_call {
            tools_json.to_string()
        } else {
            String::new()
        },
    })
}

pub(crate) fn tool_continuation_stream_source(
    mut request: ModelRequest,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    assistant_content: &str,
    assistant_reasoning: &str,
    tool_calls: Vec<CompleteToolCall>,
    tool_result_messages: Vec<ModelMessage>,
    continuation_user_messages: Vec<ModelMessage>,
    mut continuation_sse: Vec<String>,
    scripted_source: bool,
) -> HamburResult<RouteStreamSource> {
    request.request_id = new_id("llm_req");
    request.max_output_tokens = route.output_limit;
    request.tools_json = if route.supports_tool_call {
        tools_json.to_string()
    } else {
        String::new()
    };
    request.messages.push(ModelMessage {
        role: "assistant".to_string(),
        content: assistant_content.to_string(),
        reasoning_content: assistant_reasoning.to_string(),
        tool_calls_json: complete_tool_calls_json(&tool_calls)?,
        tool_call_id: String::new(),
        ..Default::default()
    });
    request.messages.extend(tool_result_messages);
    request.messages.extend(continuation_user_messages);

    if continuation_sse.is_empty() {
        if scripted_source {
            return Err(HamburError::InvalidCommand(
                "scripted tool loop requires a continuation SSE".to_string(),
            ));
        }
        return Ok(RouteStreamSource::Provider(request));
    }
    let next_sse = continuation_sse.remove(0);
    Ok(RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&next_sse),
        continuation_sse,
    })
}

pub(crate) fn complete_tool_calls_json(calls: &[CompleteToolCall]) -> HamburResult<String> {
    let values = calls
        .iter()
        .map(|call| {
            let arguments_value: Value =
                serde_json::from_str(&call.arguments_json).unwrap_or_else(|_| json!({}));
            let arguments_json = serde_json::to_string(&arguments_value).map_err(|error| {
                HamburError::Internal(format!("serialize tool call arguments: {error}"))
            })?;
            Ok(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments_json
                }
            }))
        })
        .collect::<HamburResult<Vec<_>>>()?;
    serde_json::to_string(&values)
        .map_err(|error| HamburError::Internal(format!("serialize tool calls: {error}")))
}

pub(crate) fn openai_non_stream_request(
    request: &ModelRequest,
    target: &ProviderTarget,
    api_key: &str,
) -> HamburResult<hambur_llm::HttpRequestSpec> {
    let mut spec = match target.provider.protocol.as_str() {
        OPENAI_RESPONSES_PROTOCOL => {
            ResponsesApiAdapter::build_stream_request(request, target, api_key)?
        }
        _ => {
            OpenAiCompatibleAdapter::build_stream_request(request, target, api_key)?
        }
    };
    let mut body: Value = serde_json::from_str(&spec.body_json)
        .map_err(|error| HamburError::InvalidCommand(format!("invalid request body: {error}")))?;
    body["stream"] = json!(false);
    spec.body_json = body.to_string();
    Ok(spec)
}

pub(crate) async fn reqwest_json(spec: hambur_llm::HttpRequestSpec) -> HamburResult<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_provider_http_status(status));
    }
    response
        .text()
        .await
        .map_err(|error| HamburError::ProviderUnavailable(format!("NetworkError: {error}")))
}

pub(crate) fn parse_openai_non_stream_message(
    body: &str,
) -> HamburResult<MemoryReviewAssistantMessage> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| HamburError::SseParse(format!("parse chat completion JSON: {error}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("provider error");
        return Err(HamburError::ProviderUnavailable(message.to_string()));
    }
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| HamburError::SseParse("chat completion missing message".to_string()))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(items) = message.get("tool_calls").and_then(Value::as_array) {
        for (fallback_index, item) in items.iter().enumerate() {
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(fallback_index as u32);
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let function = item.get("function").unwrap_or(&Value::Null);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments_json = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            if !id.is_empty() && !name.is_empty() {
                tool_calls.push(CompleteToolCall {
                    index,
                    id,
                    name,
                    arguments_json,
                });
            }
        }
    }
    Ok(MemoryReviewAssistantMessage {
        content,
        tool_calls,
    })
}

pub(crate) fn compile_named_tools_json(tools_json: &str, names: &[&str]) -> HamburResult<String> {
    let allowed = names.iter().copied().collect::<HashSet<_>>();
    let value: Value = serde_json::from_str(tools_json.trim()).map_err(|error| {
        HamburError::InvalidCommand(format!("invalid OpenAI tools JSON: {error}"))
    })?;
    let Value::Array(items) = value else {
        return Err(HamburError::InvalidCommand(
            "OpenAI tools JSON must be an array".to_string(),
        ));
    };
    let filtered = items
        .into_iter()
        .filter(|item| {
            item.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| allowed.contains(name))
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(filtered).to_string())
}

pub(crate) async fn reqwest_stream(
    spec: hambur_llm::HttpRequestSpec,
) -> HamburResult<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        Err(map_provider_http_status(status))
    }
}

pub(crate) fn map_provider_http_status(status: StatusCode) -> HamburError {
    if status == StatusCode::TOO_MANY_REQUESTS {
        HamburError::ProviderUnavailable(format!("Http429: HTTP {}", status.as_u16()))
    } else if status.is_server_error() {
        HamburError::ProviderUnavailable(format!("Http5xx: HTTP {}", status.as_u16()))
    } else {
        HamburError::ProviderUnavailable(format!(
            "Http{}: HTTP {}",
            status.as_u16(),
            status.as_u16()
        ))
    }
}

pub(crate) fn markdown_block_summary(node: &hambur_markdown::MarkdownBlockNode) -> String {
    node.text
        .trim()
        .to_string()
        .if_blank(node.raw.trim().to_string())
        .chars()
        .take(160)
        .collect()
}

pub(crate) fn normalize_image_detail(detail: &str) -> &'static str {
    match detail {
        "original" => "original",
        _ => "high",
    }
}

pub(crate) fn vision_handoff_target(
    route_candidates: &[ModelRouteSnapshot],
    active_route: &ModelRouteSnapshot,
) -> Option<ModelRouteSnapshot> {
    route_candidates
        .iter()
        .find(|candidate| {
            candidate.supports_image_input
                && (candidate.provider_id != active_route.provider_id
                    || candidate.model_id != active_route.model_id)
        })
        .cloned()
}

pub(crate) fn select_view_image_handoff_route(
    active_route: &ModelRouteSnapshot,
    route_candidates: &[ModelRouteSnapshot],
    records: &[ToolExecutionRecord],
) -> Option<ModelRouteSnapshot> {
    if active_route.supports_image_input {
        return None;
    }
    let needs_handoff = records.iter().any(|record| {
        record.invocation.name == "view_image"
            && !record.result.is_error
            && serde_json::from_str::<Value>(&record.result.content_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("imageAttachedToNextRequest")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false)
    });
    if needs_handoff {
        vision_handoff_target(route_candidates, active_route)
    } else {
        None
    }
}

pub(crate) fn scripted_route_value<'a>(
    value: &'a serde_json::Value,
    route: &ModelRouteSnapshot,
) -> Option<&'a serde_json::Value> {
    let routes = value.get("routes")?.as_object()?;
    routes
        .get(&route.provider_id)
        .or_else(|| routes.get(&route.model_id))
        .or_else(|| routes.get(&route.position.to_string()))
}

pub(crate) fn split_scripted_sse(sse: &str) -> Vec<Vec<u8>> {
    sse.as_bytes()
        .chunks(13)
        .map(|chunk| chunk.to_vec())
        .collect()
}

pub(crate) fn stream_error_from_provider(code: String, message: String) -> HamburError {
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

pub(crate) fn fallback_error_code(error: &HamburError) -> &'static str {
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

pub(crate) fn default_models_response(model_id: &str) -> String {
    let model_id = if model_id.trim().is_empty() {
        "hambur-openai-compatible-text"
    } else {
        model_id.trim()
    };
    let supports_image_input = model_id.to_ascii_lowercase().contains("vision");
    format!(
        r#"{{"data":[{{"id":"{model_id}","display_name":"{model_id}","supports_reasoning":true,"supports_tool_call":true,"supports_image_input":{supports_image_input},"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}}]}}"#
    )
}

pub(crate) async fn reqwest_text_url(url: &str, timeout_secs: u64) -> HamburResult<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "Hambur/0.1")
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
            } else {
                HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_provider_http_status(status));
    }
    response
        .text()
        .await
        .map_err(|error| HamburError::ProviderUnavailable(format!("NetworkError: {error}")))
}
