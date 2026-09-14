use crate::*;

impl RuntimeEngine {
    pub(crate) fn maybe_complete_delegate_session_from_assistant(
        &self,
        session_id: &str,
        assistant_message_id: &str,
        fallback_summary: &str,
    ) {
        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(session_id))
        else {
            return;
        };

        let content_text = self
            .database
            .message_snapshot(assistant_message_id)
            .ok()
            .flatten()
            .map(|message| message.content_text.trim().to_string())
            .unwrap_or_default();
        let summary = content_text
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().chars().take(240).collect::<String>())
            .unwrap_or_else(|| fallback_summary.to_string());
        let content = json!({
            "summary": summary,
            "findings": if content_text.is_empty() {
                json!([])
            } else {
                json!([content_text])
            },
            "changedFiles": [],
            "artifactPaths": [],
            "artifactMappings": [],
            "risks": [],
            "nextSteps": [],
            "autoSubmitted": true
        });
        let _ = self
            .database
            .update_trace_span_status(&state.trace_id, "completed", &summary, true);
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: false,
            content,
            summary,
        });
    }

    pub(crate) async fn resolve_delegate_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
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
        if invocation.name == "submit_delegate_result" {
            return self
                .resolve_submit_delegate_result(session_id, invocation, arguments);
        }
        if self
            .delegate_sessions
            .lock()
            .map(|sessions| sessions.contains(session_id))
            .unwrap_or(false)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate_task is disabled inside delegate sessions",
            );
        }

        self.resolve_delegate_task_result(
            session_id,
            turn_id,
            route,
            route_candidates,
            invocation,
            arguments,
        )
        .await
    }

    pub(crate) fn resolve_submit_delegate_result(
        &self,
        session_id: &str,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let mut child_paths = Vec::new();
        for path in arguments
            .get("artifact_paths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let child_path = match self.sandbox.resolve(session_id, path, SandboxAccess::Read) {
                Ok(resolved) => resolved,
                Err(error) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        error.to_string(),
                    );
                }
            };
            child_paths.push(child_path);
        }

        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|tasks| tasks.get(session_id).map(DelegateTaskState::snapshot))
        else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "submit_delegate_result is available only inside delegate sessions",
            );
        };
        let mut artifact_mappings = Vec::new();
        for child_path in child_paths {
            if child_path.host_path.exists() {
                let Ok(metadata) = fs::metadata(&child_path.host_path) else {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "delegate artifact metadata unavailable",
                    );
                };
                if metadata.len() > 10 * 1024 * 1024 {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "delegate artifact is too large",
                    );
                }
                let relative_name = child_path
                    .sandbox_path
                    .trim_start_matches("/var/hambur/workspace/")
                    .trim_start_matches('/')
                    .to_string()
                    .if_blank("artifact".to_string());
                let parent_sandbox_path = format!(
                    "/var/hambur/workspace/delegates/{}/{}",
                    state.child_session_id, relative_name
                );
                let parent_path = match self.sandbox.resolve(
                    &state.parent_session_id,
                    &parent_sandbox_path,
                    SandboxAccess::Write,
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        return ToolResult::failed(
                            &invocation.tool_call_id,
                            &invocation.name,
                            error.to_string(),
                        );
                    }
                };
                if parent_path.host_path.exists() {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!(
                            "delegate artifact target already exists: {}",
                            parent_path.sandbox_path
                        ),
                    );
                }
                if let Some(parent) = parent_path.host_path.parent()
                    && let Err(error) = fs::create_dir_all(parent)
                {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("create delegate artifact directory: {error}"),
                    );
                }
                if let Err(error) = fs::copy(&child_path.host_path, &parent_path.host_path) {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("copy delegate artifact: {error}"),
                    );
                }
                artifact_mappings.push(json!({
                    "childPath": child_path.sandbox_path,
                    "parentPath": parent_path.sandbox_path,
                    "bytes": metadata.len()
                }));
            }
        }

        let content = json!({
            "summary": arguments.get("summary").and_then(Value::as_str).unwrap_or_default(),
            "findings": arguments.get("findings").cloned().unwrap_or_else(|| json!([])),
            "changedFiles": arguments.get("changed_files").or_else(|| arguments.get("changedFiles")).cloned().unwrap_or_else(|| json!([])),
            "artifactPaths": arguments.get("artifact_paths").or_else(|| arguments.get("artifactPaths")).cloned().unwrap_or_else(|| json!([])),
            "artifactMappings": artifact_mappings,
            "risks": arguments.get("risks").cloned().unwrap_or_else(|| json!([])),
            "nextSteps": arguments.get("next_steps").or_else(|| arguments.get("nextSteps")).cloned().unwrap_or_else(|| json!([]))
        });
        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(session_id))
        else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "submit_delegate_result is available only inside delegate sessions",
            );
        };
        let summary = arguments
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("Delegate result submitted")
            .to_string();
        let _ = self
            .database
            .update_trace_span_status(&state.trace_id, "completed", &summary, true);
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: false,
            content: content.clone(),
            summary: summary.clone(),
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Delegate result submitted".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    pub(crate) async fn resolve_delegate_task_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let pending_count = self
            .delegate_tasks
            .lock()
            .map(|tasks| {
                tasks
                    .values()
                    .filter(|task| task.parent_turn_id == turn_id)
                    .count()
            })
            .unwrap_or(usize::MAX);
        if pending_count >= 3 {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate batch limit exceeded",
            );
        }

        let top_role = arguments
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("leaf")
            .trim();
        if top_role != "leaf" {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate_task currently supports role='leaf' only",
            );
        }

        if let Some(tasks) = arguments.get("tasks").and_then(Value::as_array) {
            if !tasks.is_empty() {
                if tasks.len() > 3 {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "delegate_task supports at most 3 parallel tasks per call. Split larger batches into multiple calls.",
                    );
                }
                return self
                    .resolve_delegate_task_batch_result(
                        session_id,
                        turn_id,
                        route,
                        route_candidates,
                        invocation,
                        arguments,
                        tasks,
                    )
                    .await;
            }
        }

        let task = delegate_task_prompt(arguments, None, 0, 1);
        if task.trim().is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate_task requires goal",
            );
        }
        self.resolve_single_delegate_task_result(
            session_id,
            turn_id,
            route,
            route_candidates,
            invocation,
            arguments,
            task,
            arguments
                .get("toolsets")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .await
    }

    pub(crate) async fn resolve_delegate_task_batch_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
        tasks: &[Value],
    ) -> ToolResult {
        let batch_started_at = now_ms();
        let mut results = Vec::new();
        for (index, task_value) in tasks.iter().enumerate() {
            let started_at = now_ms();
            let role = task_value
                .get("role")
                .and_then(Value::as_str)
                .or_else(|| arguments.get("role").and_then(Value::as_str))
                .unwrap_or("leaf")
                .trim();
            let goal = task_value
                .get("goal")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if goal.is_empty() {
                results.push(json!({
                    "index": index,
                    "status": "error",
                    "error": "task goal is required",
                    "duration_ms": now_ms().saturating_sub(started_at)
                }));
                continue;
            }
            if role != "leaf" {
                results.push(json!({
                    "index": index,
                    "goal": goal,
                    "status": "error",
                    "error": "delegate_task currently supports role='leaf' only",
                    "duration_ms": now_ms().saturating_sub(started_at)
                }));
                continue;
            }
            let task = delegate_task_prompt(arguments, Some(task_value), index, tasks.len());
            let task_toolsets = task_value
                .get("toolsets")
                .cloned()
                .or_else(|| arguments.get("toolsets").cloned())
                .unwrap_or_else(|| json!([]));
            let result = self
                .resolve_single_delegate_task_result(
                    session_id,
                    turn_id,
                    route,
                    route_candidates,
                    invocation,
                    arguments,
                    task,
                    task_toolsets,
                )
                .await;
            let parsed_result =
                serde_json::from_str::<Value>(&result.content_json).unwrap_or_else(|_| {
                    json!({
                        "summary": result.summary,
                        "content": result.content_json
                    })
                });
            results.push(json!({
                "index": index,
                "goal": goal,
                "status": if result.is_error { "error" } else { "completed" },
                "is_error": result.is_error,
                "duration_ms": now_ms().saturating_sub(started_at),
                "result": parsed_result
            }));
        }

        let content = json!({
            "mode": "batch",
            "parallel": true,
            "max_concurrent_children": 3,
            "duration_ms": now_ms().saturating_sub(batch_started_at),
            "tasks": results
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Delegate batch completed".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn resolve_single_delegate_task_result(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
        task: String,
        toolsets: Value,
    ) -> ToolResult {
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(600_000)
            .clamp(1_000, 600_000);
        let payload_json = arguments
            .get("payload_json")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let delegate_session_id = match self
            .database
            .create_internal_session(
                &format!(
                "Delegate: {}",
                task.chars().take(72).collect::<String>()
                ),
                "delegate",
            )
        {
            Ok(session_id) => session_id,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        if let Err(error) = self.sandbox.prepare_session(&delegate_session_id) {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let trace = match self
            .database
            .insert_trace_span(NewTraceSpan {
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                kind: "tool".to_string(),
                title: "Delegate session".to_string(),
                content: format!("delegateSessionId={delegate_session_id}\ntask={task}"),
                status: "running".to_string(),
                tool_call_id: invocation.tool_call_id.clone(),
                payload_json: json!({
                    "delegateSessionId": delegate_session_id,
                    "task": task,
                    "toolsets": toolsets
                })
                .to_string(),
                visible: true,
                ..Default::default()
            })
        {
            Ok(trace) => trace,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };

        let prepared = match self
            .prepare_delegate_child_turn(
                session_id,
                turn_id,
                &delegate_session_id,
                &task,
                payload_json,
                route.clone(),
                route_candidates.to_vec(),
            )
        {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = self
                    .database
                    .update_trace_span_status(&trace.id, "failed", &error.to_string(), true);
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        let (sender, receiver) = oneshot::channel();
        let state = DelegateTaskState {
            parent_session_id: session_id.to_string(),
            parent_turn_id: turn_id.to_string(),
            child_session_id: delegate_session_id.clone(),
            trace_id: trace.id.clone(),
            sender,
        };
        if let Ok(mut tasks) = self.delegate_tasks.lock() {
            tasks.insert(delegate_session_id.clone(), state);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "delegate registry unavailable",
            );
        }
        if let Ok(mut sessions) = self.delegate_sessions.lock() {
            sessions.insert(delegate_session_id.clone());
        }
        self.spawn_prepared_delegate_turn(delegate_session_id.clone(), prepared);

        match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(completion)) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: completion.is_error,
                content_json: completion.content.to_string(),
                summary: completion.summary.clone(),
                artifacts_json: completion
                    .content
                    .get("artifactMappings")
                    .cloned()
                    .unwrap_or_else(|| json!([]))
                    .to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: completion.content.to_string(),
            },
            _ => {
                let _ = self
                    .delegate_tasks
                    .lock()
                    .ok()
                    .and_then(|mut tasks| tasks.remove(&delegate_session_id));
                let message = "delegate task timed out before submit_delegate_result";
                let _ = self
                    .database
                    .update_trace_span_status(&trace.id, "failed", message, true);
                ToolResult::failed(&invocation.tool_call_id, &invocation.name, message)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_delegate_child_turn(
        &self,
        parent_session_id: &str,
        parent_turn_id: &str,
        delegate_session_id: &str,
        task: &str,
        payload_json: String,
        route: ModelRouteSnapshot,
        route_candidates: Vec<ModelRouteSnapshot>,
    ) -> HamburResult<PreparedDelegateTurn> {
        let child_content = format!(
            "Delegate task from parent session {parent_session_id}, turn {parent_turn_id}.\n\n{task}\n\nFinish by calling submit_delegate_result."
        );
        let turn = self
            .database
            .create_turn_with_route(delegate_session_id, "StreamingAssistant", &route)?;
        let user_message = self
            .database
            .insert_message_with_route(
                delegate_session_id,
                "user",
                &child_content,
                "",
                "completed",
                &turn.id,
                &route,
            )?;
        self.database
            .upsert_timeline_item(
                delegate_session_id,
                NewTimelineItem {
                    stable_key: user_message.id.clone(),
                    content_type: "user_message".to_string(),
                    display_sequence: user_message.created_at_ms,
                    payload_ref: user_message.id.clone(),
                    small_summary: child_content.chars().take(160).collect(),
                    kind: "UserMessage".to_string(),
                },
            )?;
        let assistant_message = self
            .database
            .insert_message_with_route(
                delegate_session_id,
                "assistant",
                "",
                "",
                "streaming",
                &turn.id,
                &route,
            )?;
        self.insert_initial_pending_markdown_block(
            delegate_session_id,
            &turn.id,
            &assistant_message.id,
        )?;
        let snapshot = self.database.session_snapshot(delegate_session_id)?;
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut active_turns) = self.active_turns.lock() {
            active_turns.insert(
                delegate_session_id.to_string(),
                ActiveTurn {
                    turn_id: turn.id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnStarted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            "DelegateTask".to_string(),
            None,
        );
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::MessageUpserted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            child_content.clone(),
            None,
        );
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::AssistantMessageStarted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot,
            route.model_display_name.clone(),
            None,
        );

        let route_candidates = if route_candidates.is_empty() {
            vec![route.clone()]
        } else {
            route_candidates
        };
        let fallback_policy = FallbackPolicy::parse(&route.fallback_policy);
        let stream_command = RuntimeCommand {
            session_id: delegate_session_id.to_string(),
            payload_json,
            ..RuntimeCommand::default()
        };
        let tools_json = self.compile_enabled_delegate_tools_json();
        let skills_index_prompt = self.build_skills_index_prompt();
        let memory_system_prompt = self.build_memory_system_prompt();
        let child_model_content = format!(
            "[Current Time: {}]\n\n{}",
            format_beijing_timestamp_with_weekday(now_ms()),
            child_content.trim()
        );
        let stream_sources_by_route = route_candidates
            .iter()
            .map(|candidate| {
                stream_source_for_command(
                    &stream_command,
                    &turn.id,
                    &child_content,
                    vec![ModelMessage {
                        role: "user".to_string(),
                        content: child_model_content.clone(),
                        ..Default::default()
                    }],
                    candidate,
                    &tools_json,
                    &skills_index_prompt,
                    &memory_system_prompt,
                    false,
                    false,
                )
            })
            .collect::<Vec<_>>();
        Ok(PreparedDelegateTurn {
            turn_id: turn.id,
            assistant_message_id: assistant_message.id,
            route_candidates,
            fallback_policy,
            cancel,
            stream_sources_by_route,
        })
    }

    pub(crate) fn spawn_prepared_delegate_turn(
        &self,
        delegate_session_id: String,
        prepared: PreparedDelegateTurn,
    ) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .run_chat_turn(
                    delegate_session_id,
                    prepared.turn_id,
                    prepared.assistant_message_id,
                    prepared.route_candidates,
                    prepared.fallback_policy,
                    prepared.cancel,
                    prepared.stream_sources_by_route,
                    0,
                )
                .await;
        });
    }

    pub(crate) fn maybe_fail_delegate_session(&self, session_id: &str, message: &str) {
        let Some(state) = self
            .delegate_tasks
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(session_id))
        else {
            return;
        };
        let summary = {
            let trimmed = message.trim();
            if trimmed.is_empty() {
                "delegate task failed".to_string()
            } else {
                trimmed.to_string()
            }
        };
        let content = json!({
            "summary": summary,
            "findings": [],
            "changedFiles": [],
            "artifactPaths": [],
            "artifactMappings": [],
            "risks": [summary],
            "nextSteps": [],
            "autoSubmitted": true
        });
        let _ = self
            .database
            .update_trace_span_status(&state.trace_id, "failed", &summary, true);
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: true,
            content,
            summary,
        });
    }
}
