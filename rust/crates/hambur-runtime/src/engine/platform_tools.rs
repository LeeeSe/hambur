use crate::*;

impl RuntimeEngine {
    pub(crate) async fn resolve_browser_tool_result(
        &self,
        session_id: &str,
        turn_id: &str,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let request_id = new_id("platform_req");
        let timeout_ms = argument_seconds_or_ms(arguments, "timeout", "timeout_ms", 60_000)
            .clamp(1_000, 120_000);
        let request = PlatformRequest {
            request_id: request_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: "BrowserAction".to_string(),
            payload_json: json!({
                "toolCallId": invocation.tool_call_id,
                "action": arguments
            })
            .to_string(),
            timeout_ms,
            cancellable: true,
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut requests) = self.platform_requests.lock() {
            requests.insert(request_id.clone(), sender);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "platform request registry unavailable",
            );
        }
        if let Err(error) = self.emit_platform_request(request) {
            let _ = self
                .platform_requests
                .lock()
                .ok()
                .and_then(|mut requests| requests.remove(&request_id));
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => {
                let is_error = result.is_error;
                let payload_json = result.payload_json;
                let error_code = result.error_code;
                let message = result.message;
                let mut content = if payload_json.trim().is_empty() {
                    message
                } else {
                    payload_json
                };
                if !is_error {
                    match self
                        .materialize_browser_artifacts(
                            session_id,
                            &invocation.tool_call_id,
                            &content,
                        )
                    {
                        Ok(materialized) => {
                            content = materialized;
                        }
                        Err(error) => {
                            return ToolResult::failed(
                                &invocation.tool_call_id,
                                &invocation.name,
                                error.to_string(),
                            );
                        }
                    }
                }
                let summary = if is_error {
                    error_code
                        .clone()
                        .if_blank("Browser action failed".to_string())
                } else {
                    "Browser action completed".to_string()
                };
                let status = if is_error {
                    error_code
                } else {
                    "ok".to_string()
                };
                let raw = RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error,
                    content,
                    summary,
                    trust_level: "untrusted".to_string(),
                    command_or_url: arguments.to_string(),
                    status,
                };
                self.tools
                    .normalize_raw_with_session(raw, Some(&invocation.session_id))
                    .unwrap_or_else(|error| {
                    ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        error.to_string(),
                    )
                })
            }
            _ => {
                let _ = self
                    .platform_requests
                    .lock()
                    .ok()
                    .and_then(|mut requests| requests.remove(&request_id));
                                let _ = self.emit_failure(
                    RuntimeEventKind::PlatformRequestTimedOut,
                    session_id.to_string(),
                    turn_id.to_string(),
                    &HamburError::InvalidCommand(format!("PlatformRequestTimeout {request_id}")),
                );
                ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "PlatformRequestTimeout",
                )
            }
        }
    }

    pub(crate) async fn resolve_android_cli_tool_result(
        &self,
        session_id: &str,
        turn_id: &str,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let request_id = new_id("platform_req");
        let timeout_ms = argument_seconds_or_ms(arguments, "timeout", "timeout_ms", 20_000)
            .clamp(1_000, 60_000);
        let request = PlatformRequest {
            request_id: request_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: "AndroidCliAction".to_string(),
            payload_json: json!({
                "toolCallId": invocation.tool_call_id,
                "action": arguments
            })
            .to_string(),
            timeout_ms,
            cancellable: true,
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut requests) = self.platform_requests.lock() {
            requests.insert(request_id.clone(), sender);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "platform request registry unavailable",
            );
        }
        if let Err(error) = self.emit_platform_request(request) {
            let _ = self
                .platform_requests
                .lock()
                .ok()
                .and_then(|mut requests| requests.remove(&request_id));
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => {
                let is_error = result.is_error;
                let payload_json = result.payload_json;
                let error_code = result.error_code;
                let message = result.message;
                let content = if payload_json.trim().is_empty() {
                    message
                } else {
                    payload_json
                };
                let summary = if is_error {
                    error_code
                        .clone()
                        .if_blank("Android CLI action failed".to_string())
                } else {
                    "Android CLI action completed".to_string()
                };
                let status = if is_error {
                    error_code
                } else {
                    "ok".to_string()
                };
                let raw = RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error,
                    content,
                    summary,
                    trust_level: "trusted".to_string(),
                    command_or_url: arguments.to_string(),
                    status,
                };
                self.tools
                    .normalize_raw_with_session(raw, Some(&invocation.session_id))
                    .unwrap_or_else(|error| {
                        ToolResult::failed(
                            &invocation.tool_call_id,
                            &invocation.name,
                            error.to_string(),
                        )
                    })
            }
            _ => {
                let _ = self
                    .platform_requests
                    .lock()
                    .ok()
                    .and_then(|mut requests| requests.remove(&request_id));
                                let _ = self.emit_failure(
                    RuntimeEventKind::PlatformRequestTimedOut,
                    session_id.to_string(),
                    turn_id.to_string(),
                    &HamburError::InvalidCommand(format!("PlatformRequestTimeout {request_id}")),
                );
                ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "PlatformRequestTimeout",
                )
            }
        }
    }

    pub(crate) fn materialize_browser_artifacts(
        &self,
        session_id: &str,
        tool_call_id: &str,
        content: &str,
    ) -> HamburResult<String> {
        let Ok(mut value) = serde_json::from_str::<Value>(content) else {
            return Ok(content.to_string());
        };
        if value.get("downloaded").and_then(Value::as_bool) == Some(true) {
            if let Some(sandbox_path) = value.get("sandboxPath").and_then(Value::as_str) {
                if let Ok(resolved) = self.sandbox.resolve(session_id, sandbox_path, SandboxAccess::Read) {
                    if let Ok(meta) = fs::metadata(&resolved.host_path) {
                        let mime_type = value
                            .get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or_else(|| detect_mime_type(&resolved.host_path));
                        let file_id = new_id("file");
                        let _ = self
                            .database
                            .upsert_file_record(NewFileRecord {
                                id: file_id.clone(),
                                scope: "session".to_string(),
                                session_id: session_id.to_string(),
                                relative_path: resolved.relative_path.clone(),
                                sandbox_path: resolved.sandbox_path.clone(),
                                mime_type: mime_type.to_string(),
                                byte_size: meta.len(),
                                sha256: String::new(),
                                retention_policy: "delete_with_session".to_string(),
                            });
                        if let Some(object) = value.as_object_mut() {
                            object.insert("fileId".to_string(), json!(file_id));
                            object.insert(
                                "hostPath".to_string(),
                                json!(resolved.host_path.to_string_lossy().to_string()),
                            );
                            object.insert("materialized".to_string(), json!(true));
                        }
                    }
                }
            }
            return Ok(value.to_string());
        }
        let Some(base64_value) = value.get("base64").and_then(Value::as_str) else {
            return Ok(content.to_string());
        };
        if base64_value.trim().is_empty() {
            return Ok(content.to_string());
        }
        let bytes = BASE64_STANDARD
            .decode(base64_value.as_bytes())
            .map_err(|error| {
                HamburError::InvalidCommand(format!("browser artifact base64: {error}"))
            })?;
        let mime_type = value
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("image/png")
            .to_string();
        let extension = match mime_type.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let reserved = self.filestore.reserve_cache_image(session_id, extension)?;
        fs::write(&reserved.host_path, &bytes)
            .map_err(|error| HamburError::Internal(format!("write browser artifact: {error}")))?;
        self.database
            .upsert_file_record(NewFileRecord {
                id: reserved.file_id.clone(),
                scope: "session".to_string(),
                session_id: session_id.to_string(),
                relative_path: reserved.relative_path.clone(),
                sandbox_path: reserved.sandbox_path.clone(),
                mime_type: mime_type.clone(),
                byte_size: bytes.len() as u64,
                sha256: String::new(),
                retention_policy: "delete_with_session".to_string(),
            })?;
        if let Some(object) = value.as_object_mut() {
            object.remove("base64");
            object.insert("fileId".to_string(), json!(reserved.file_id));
            object.insert("sandboxPath".to_string(), json!(reserved.sandbox_path));
            object.insert("relativePath".to_string(), json!(reserved.relative_path));
            object.insert("toolCallId".to_string(), json!(tool_call_id));
            object.insert("materialized".to_string(), json!(true));
        }
        Ok(value.to_string())
    }

    pub(crate) fn resolve_web_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let raw = match invocation.name.as_str() {
            "web_search" => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: "web_search requires a configured search provider".to_string(),
                summary: "Web search provider unavailable".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: arguments.to_string(),
                status: "ProviderUnavailable".to_string(),
            },
            _ => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: format!("unknown web tool: {}", invocation.name),
                summary: "Unknown web tool".to_string(),
                trust_level: "trusted".to_string(),
                command_or_url: arguments.to_string(),
                status: "InvalidCommand".to_string(),
            },
        };

        self.tools
            .normalize_raw_with_session(raw, Some(&invocation.session_id))
            .unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
    }
}
