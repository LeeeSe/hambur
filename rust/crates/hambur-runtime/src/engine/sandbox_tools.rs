use crate::*;

impl RuntimeEngine {
    pub(crate) fn resolve_session_search_result(
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
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(5)
            .clamp(1, 20);
        let sessions = self
            .database
            .search_sessions(query, limit)
            .unwrap_or_default();
        let matches = sessions
            .iter()
            .map(|session| {
                json!({
                    "sessionId": session.id,
                    "title": session.title,
                    "latestPreview": session.latest_preview,
                    "messageCount": session.message_count,
                    "updatedAtMs": session.updated_at_ms
                })
            })
            .collect::<Vec<_>>();
        let content = json!({
            "query": query,
            "limit": limit,
            "matches": matches
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: format!("Session search returned {} matches", sessions.len()),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    pub(crate) fn resolve_sandbox_tool_result(
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
        if invocation.name == "process" {
            return self.resolve_process_tool_result(invocation, arguments);
        }
        let command = arguments
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if command.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "terminal command must not be empty",
            );
        }
        let cwd = arguments
            .get("workdir")
            .or_else(|| arguments.get("cwd"))
            .and_then(Value::as_str)
            .unwrap_or("/var/hambur/workspace");
        let cwd = match self
            .sandbox
            .resolve(&invocation.session_id, cwd, SandboxAccess::Read)
        {
            Ok(resolved) => resolved,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        if arguments
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return self.start_background_process(invocation, command, &cwd.sandbox_path);
        }

        let timeout_ms = argument_seconds_or_ms(arguments, "timeout", "timeout_ms", 30_000)
            .clamp(1_000, 300_000);
        let raw = match self.execute_sandbox_command(
            &invocation.session_id,
            command,
            &cwd.sandbox_path,
            timeout_ms,
        ) {
            Ok(exec_result) => {
                let is_error = exec_result.exit_code != 0 || exec_result.timed_out;
                let status = if exec_result.timed_out {
                    "timeout"
                } else if exec_result.exit_code != 0 {
                    "failed"
                } else {
                    "completed"
                };
                let summary = if exec_result.timed_out {
                    "terminal timed out".to_string()
                } else {
                    format!(
                        "terminal completed with exit code {}",
                        exec_result.exit_code
                    )
                };
                RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error,
                    content: serde_json::to_string(&exec_result).unwrap_or_default(),
                    summary,
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: status.to_string(),
                }
            }
            Err(error) => RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: error.to_string(),
                summary: "sandbox execution error".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: "failed".to_string(),
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

    pub(crate) fn resolve_file_tool_result(
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
        let result = match invocation.name.as_str() {
            "read_file" => self.read_sandbox_file(invocation, arguments),
            "write_file" => self.write_sandbox_file(invocation, arguments),
            "patch" => self.patch_sandbox_file(invocation, arguments),
            "search_files" => self.search_sandbox_files(invocation, arguments),
            _ => Err(HamburError::InvalidCommand(format!(
                "unknown file tool: {}",
                invocation.name
            ))),
        };
        match result {
            Ok(value) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                summary: value
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("file tool completed")
                    .to_string(),
                content_json: value.to_string(),
                artifacts_json: "[]".to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: value.to_string(),
            },
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        }
    }

    pub(crate) fn resolve_tool_sandbox_path(
        &self,
        session_id: &str,
        raw_path: &str,
        access: SandboxAccess,
    ) -> HamburResult<hambur_sandbox::SandboxPathResolution> {
        let path = normalize_tool_sandbox_path(raw_path);
        self.sandbox.resolve(session_id, &path, access)
    }

    pub(crate) fn read_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let offset = arguments
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(500)
            .min(2000);
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Read)?;
        let content = fs::read_to_string(&resolved.host_path).map_err(|error| {
            HamburError::InvalidCommand(format!("read file {}: {error}", resolved.sandbox_path))
        })?;
        let lines = content.lines().collect::<Vec<_>>();
        let start = usize::try_from(offset.saturating_sub(1)).unwrap_or(usize::MAX);
        let limit = usize::try_from(limit).unwrap_or(2000);
        let rendered = lines
            .iter()
            .enumerate()
            .skip(start)
            .take(limit)
            .map(|(index, line)| format!("{}|{}", index + 1, line))
            .collect::<Vec<_>>();
        Ok(json!({
            "path": resolved.sandbox_path,
            "offset": offset,
            "limit": limit,
            "totalLines": lines.len(),
            "content": rendered.join("\n"),
            "summary": format!("read {} lines", rendered.len())
        }))
    }

    pub(crate) fn write_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Write)?;
        if let Some(parent) = resolved.host_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| HamburError::Internal(format!("create parent: {error}")))?;
        }
        fs::write(&resolved.host_path, content.as_bytes())
            .map_err(|error| HamburError::Internal(format!("write file: {error}")))?;
        Ok(json!({
            "path": resolved.sandbox_path,
            "bytes": content.len(),
            "summary": "file written"
        }))
    }

    pub(crate) fn patch_sandbox_file(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("replace");
        if mode == "patch" {
            return Err(HamburError::InvalidCommand(
                "patch mode='patch' is not implemented; use mode='replace'".to_string(),
            ));
        }
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let old = arguments
            .get("old_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let new = arguments
            .get("new_string")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if old.is_empty() {
            return Err(HamburError::InvalidCommand(
                "old_string must not be empty".to_string(),
            ));
        }
        let replace_all = arguments
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resolved =
            self.resolve_tool_sandbox_path(&invocation.session_id, path, SandboxAccess::Write)?;
        let content = fs::read_to_string(&resolved.host_path)
            .map_err(|error| HamburError::InvalidCommand(format!("read file: {error}")))?;
        let count = content.matches(old).count();
        if count == 0 {
            return Err(HamburError::InvalidCommand(
                "old_string was not found".to_string(),
            ));
        }
        if !replace_all && count > 1 {
            return Err(HamburError::InvalidCommand(format!(
                "old_string matched {count} times; pass replace_all=true or add context"
            )));
        }
        let updated = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        fs::write(&resolved.host_path, updated.as_bytes())
            .map_err(|error| HamburError::Internal(format!("write patched file: {error}")))?;
        Ok(json!({
            "path": resolved.sandbox_path,
            "replacements": if replace_all { count } else { 1 },
            "summary": "file patched"
        }))
    }
}
