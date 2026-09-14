use crate::*;

impl RuntimeEngine {

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_tool_batch_and_continue(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        cancel: &Arc<AtomicBool>,
        content: &str,
        reasoning: &str,
        finish_reason: String,
        native_finish_reason: String,
        complete_tool_calls: Vec<CompleteToolCall>,
        tool_iteration: u32,
        current_request: ModelRequest,
        continuation_sse: Vec<String>,
        scripted_source: bool,
    ) -> HamburResult<Option<ToolContinuation>> {
        if tool_iteration >= MAX_TOOL_ITERATIONS_PER_TURN {
            return Err(HamburError::InvalidCommand(format!(
                "max tool iterations exceeded: {}",
                MAX_TOOL_ITERATIONS_PER_TURN
            )));
        }

        if let Some(update) =
            self.append_stream_markdown(session_id, assistant_message_id, "", true)
        {
            let _ = self
                .emit_markdown_persisted(session_id.to_string(), turn_id.to_string(), update);
        }

        self.database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "requires_tool",
                &finish_reason.if_blank("tool_calls".to_string()),
                &native_finish_reason.if_blank("tool_calls".to_string()),
            )?;
        self.persist_reasoning_block_for_timeline(
            session_id,
            turn_id,
            assistant_message_id,
            reasoning,
        )?;

        self.database
            .update_turn_status(turn_id, "ExecutingTools", false)?;
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .unwrap_or_default();
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnStateChanged,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "Executing tools".to_string(),
            None,
        );

        let assistant_tool_calls = complete_tool_calls.clone();
        let mut invocations = Vec::new();
        let mut trace_ids = HashMap::<String, String>::new();
        for call in complete_tool_calls {
            let invocation = ToolInvocation::from_model_call(
                call.index,
                call.id,
                turn_id.to_string(),
                session_id.to_string(),
                call.name,
                call.arguments_json,
            )?;
            self.database
                .insert_tool_call(NewToolCall {
                    id: invocation.tool_call_id.clone(),
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    assistant_message_id: assistant_message_id.to_string(),
                    name: invocation.name.clone(),
                    arguments_json: invocation.arguments_json.clone(),
                    display_title: invocation.display_title.clone(),
                    status: "running".to_string(),
                    requires_approval: invocation.requires_approval,
                    call_index: invocation.index,
                })?;
            let trace = self
                .database
                .insert_trace_span(NewTraceSpan {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    kind: "tool".to_string(),
                    title: invocation.display_title.clone(),
                    content: invocation.arguments_json.clone(),
                    status: "running".to_string(),
                    tool_call_id: invocation.tool_call_id.clone(),
                    payload_json: invocation.arguments_json.clone(),
                    visible: true,
                    ..Default::default()
                })?;
            trace_ids.insert(invocation.tool_call_id.clone(), trace.id);
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .unwrap_or_default();
            let _ = self.emit_with_snapshot(
                RuntimeEventKind::ToolCallStarted,
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                invocation.display_title.clone(),
                None,
            );
            invocations.push(invocation);
        }

        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                content,
                reasoning,
            );
            return Ok(None);
        }

        let records = self
            .execute_runtime_tool_batch(
                session_id,
                turn_id,
                assistant_message_id,
                route,
                route_candidates,
                invocations,
            )
            .await?;
        let view_image_handoff = select_view_image_handoff_route(route, route_candidates, &records);
        let mut tool_result_messages = Vec::new();
        let mut context_stubs = Vec::new();
        for record in &records {
            let tool_message = self
                .database
                .insert_tool_result_message(
                    session_id,
                    turn_id,
                    &record.invocation.tool_call_id,
                    &record.invocation.name,
                    &record.result.context_stub,
                    route,
                )?;
            let result = self
                .database
                .insert_tool_result(NewToolResult {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    tool_call_id: record.invocation.tool_call_id.clone(),
                    message_id: tool_message.id,
                    is_error: record.result.is_error,
                    content_json: record.result.content_json.clone(),
                    summary: record.result.summary.clone(),
                    artifacts_json: record.result.artifacts_json.clone(),
                    trust_level: record.result.trust_level.clone(),
                    truncated: record.result.truncated,
                    offloaded_file_id: record.result.offloaded_file_id.clone(),
                    offloaded_path: record.result.offloaded_path.clone(),
                    context_stub: record.result.context_stub.clone(),
                })?;
            let status = if record.result.is_error {
                "failed"
            } else {
                "completed"
            };
            self.database
                .update_tool_call_status(
                    &record.invocation.tool_call_id,
                    status,
                    &result.id,
                    if record.result.is_error {
                        "ToolError"
                    } else {
                        ""
                    },
                    if record.result.is_error {
                        &record.result.summary
                    } else {
                        ""
                    },
                    true,
                )?;
            self.update_latest_tool_trace(
                trace_ids
                    .get(&record.invocation.tool_call_id)
                    .map(String::as_str),
                &record.invocation.tool_call_id,
                status,
                &record.result.summary,
            )?;

            let snapshot = self
                .database
                .session_snapshot(session_id)
                .unwrap_or_default();
            let _ = self.emit_with_snapshot(
                if record.result.is_error {
                    RuntimeEventKind::ToolCallFailed
                } else {
                    RuntimeEventKind::ToolCallFinished
                },
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                record.result.summary.clone(),
                None,
            );
            context_stubs.push(record.result.context_stub.clone());
            tool_result_messages.push(ModelMessage {
                role: "tool".to_string(),
                content: record.result.context_stub.clone(),
                reasoning_content: String::new(),
                tool_calls_json: String::new(),
                tool_call_id: record.invocation.tool_call_id.clone(),
                ..Default::default()
            });
        }

        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                content,
                reasoning,
            );
            return Ok(None);
        }

        self.database
            .update_turn_status(turn_id, "ContinuingAfterTools", false)?;
        let continuation_route = view_image_handoff.unwrap_or_else(|| route.clone());
        if continuation_route.provider_id != route.provider_id
            || continuation_route.model_id != route.model_id
        {
            self.database
                .update_turn_route_snapshot(turn_id, &continuation_route)?;
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .unwrap_or_default();
            let _ = self.emit_with_snapshot(
                RuntimeEventKind::TurnStateChanged,
                session_id.to_string(),
                turn_id.to_string(),
                snapshot,
                "RouteHandoff(ImageInspectionRequired)".to_string(),
                None,
            );
        }
        if context_stubs
            .iter()
            .any(|stub| stub.contains("ImagePart(fileId="))
        {
            let _synthetic = self
                .database
                .insert_message_with_route(
                    session_id,
                    "user",
                    &format_synthetic_view_image_message(&context_stubs),
                    "",
                    "completed",
                    turn_id,
                    &continuation_route,
                )?;
        }

        let mut continuation_images = Vec::new();
        for record in &records {
            if record.invocation.name == "view_image" && !record.result.is_error {
                if let Ok(value) = serde_json::from_str::<Value>(&record.result.content_json) {
                    let relative_path = value
                        .get("relativePath")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let mime_type = value
                        .get("mimeType")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png");
                    let detail = value
                        .get("detail")
                        .and_then(Value::as_str)
                        .unwrap_or("high");
                    let host_path = value
                        .get("hostPath")
                        .and_then(Value::as_str)
                        .map(std::path::PathBuf::from)
                        .filter(|p| p.is_file())
                        .or_else(|| {
                            if !relative_path.is_empty() {
                                self.filestore.host_path_for_relative(relative_path).ok()
                            } else {
                                None
                            }
                        });
                    if let Some(host_path) = host_path {
                        if let Ok(bytes) = std::fs::read(&host_path) {
                            continuation_images.push(ModelImagePart {
                                mime_type: mime_type.to_string(),
                                data_base64: BASE64_STANDARD.encode(&bytes),
                                detail: detail.to_string(),
                            });
                        }
                    }
                }
            }
        }

        let mut continuation_user_messages = Vec::new();
        if !continuation_images.is_empty() {
            continuation_user_messages.push(ModelMessage {
                role: "user".to_string(),
                content: format_synthetic_view_image_message(&context_stubs),
                images: continuation_images,
                ..Default::default()
            });
        }

        let continuation_message = self
            .database
            .insert_message_with_route(
                session_id,
                "assistant",
                "",
                "",
                "streaming",
                turn_id,
                &continuation_route,
            )?;
        let engine = self.self_ref.lock().ok().and_then(|value| value.upgrade()).ok_or_else(|| {
            HamburError::Internal("runtime self reference unavailable".to_string())
        })?;
        engine.insert_initial_pending_markdown_block(
            session_id,
            turn_id,
            &continuation_message.id,
        )?;

        let continuation_source = tool_continuation_stream_source(
            current_request,
            &continuation_route,
            &self.compile_enabled_main_tools_json(),
            content,
            reasoning,
            assistant_tool_calls,
            tool_result_messages,
            continuation_user_messages,
            continuation_sse,
            scripted_source,
        )?;
        Ok(Some(ToolContinuation {
            assistant_message_id: continuation_message.id,
            route: continuation_route,
            stream_source: continuation_source,
            tool_iteration: tool_iteration.saturating_add(1),
        }))
    }

    pub(crate) fn execute_memory_review_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolResult {
        if invocation.name != "memory" {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "Only the memory tool is enabled during automatic memory review.",
            );
        }
        let arguments = match invocation.arguments_value() {
            Ok(arguments) => arguments,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let old_text = arguments
            .get("old_text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if is_review_status_entry(content) || is_review_status_entry(old_text) {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "Review status JSON is not memory. Return it as final assistant content without calling tools.",
            );
        }
        self.resolve_knowledge_tool_result(&invocation, &arguments)
    }

    pub(crate) async fn execute_runtime_tool_batch(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocations: Vec<ToolInvocation>,
    ) -> HamburResult<Vec<ToolExecutionRecord>> {
        const MAX_DELEGATE_TASKS_PER_BATCH: usize = 3;
        let ctx = ToolTurnContext {
            session_id,
            turn_id,
            route,
            route_candidates,
        };
        let mut builtin = Vec::new();
        let mut records = Vec::new();
        let disabled_tools = self.disabled_tool_names();
        let delegate_task_count = invocations
            .iter()
            .filter(|invocation| invocation.kind == Some(ToolKind::DelegateTask))
            .count();
        for invocation in invocations {
            if disabled_tools.contains(invocation.name.as_str()) {
                records.push(failed_record(
                    invocation,
                    |invocation| format!("tool is disabled: {}", invocation.name),
                ));
                continue;
            }
            let Some(kind) = invocation.kind else {
                // Unknown names fall through to the scheduler, which reports them as unknown.
                builtin.push(invocation);
                continue;
            };
            if kind == ToolKind::DelegateTask && delegate_task_count > MAX_DELEGATE_TASKS_PER_BATCH {
                records.push(failed_record(invocation, |_| {
                    "delegate batch limit exceeded".to_string()
                }));
                continue;
            }
            match kind.host() {
                ToolHost::Builtin => builtin.push(invocation),
                ToolHost::Runtime => {
                    records.push(self.execute_runtime_tool(&ctx, kind, invocation).await);
                }
            }
        }

        if !builtin.is_empty() {
            let batch = ToolCallBatch::new(
                turn_id.to_string(),
                assistant_message_id.to_string(),
                builtin,
            );
            records.extend(self.tools.execute_batch(batch).await?);
        }
        records.sort_by_key(|record| record.invocation.index);
        Ok(records)
    }

    /// Parse arguments, dispatch to the resolver for `kind`, and wrap the outcome in a record.
    pub(crate) async fn execute_runtime_tool(
        &self,
        ctx: &ToolTurnContext<'_>,
        kind: ToolKind,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_runtime_tool(ctx, kind, &invocation, &arguments)
                    .await
            }
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        };
        ToolExecutionRecord {
            invocation,
            result,
            started_at_ms,
            ended_at_ms: now_ms(),
        }
    }

    /// The single place that maps a tool kind to its runtime resolver.
    async fn resolve_runtime_tool(
        &self,
        ctx: &ToolTurnContext<'_>,
        kind: ToolKind,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        match kind {
            ToolKind::ViewImage => {
                self.resolve_view_image_result(
                    ctx.session_id,
                    ctx.route,
                    ctx.route_candidates,
                    invocation,
                    arguments,
                )
            }
            ToolKind::SessionSearch => {
                self.resolve_session_search_result(invocation, arguments)
            }
            ToolKind::Terminal | ToolKind::Process => {
                self.resolve_sandbox_tool_result(invocation, arguments)
            }
            ToolKind::ReadFile | ToolKind::WriteFile | ToolKind::Patch | ToolKind::SearchFiles => {
                self.resolve_file_tool_result(invocation, arguments)
            }
            ToolKind::Memory | ToolKind::SkillsList | ToolKind::SkillView => {
                self.resolve_knowledge_tool_result(invocation, arguments)
            }
            ToolKind::HamburConfig => {
                self.resolve_hambur_config_tool_result(invocation, arguments)
            }
            ToolKind::BrowserUse => {
                self.resolve_browser_tool_result(ctx.session_id, ctx.turn_id, invocation, arguments)
                    .await
            }
            ToolKind::AndroidCli => {
                self.resolve_android_cli_tool_result(
                    ctx.session_id,
                    ctx.turn_id,
                    invocation,
                    arguments,
                )
                .await
            }
            ToolKind::WebSearch => self.resolve_web_tool_result(invocation, arguments),
            ToolKind::DelegateTask | ToolKind::SubmitDelegateResult => {
                self.resolve_delegate_result(
                    ctx.session_id,
                    ctx.turn_id,
                    ctx.route,
                    ctx.route_candidates,
                    invocation,
                    arguments,
                )
                .await
            }
            ToolKind::Echo => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "echo is a builtin tool and is not hosted by the runtime",
            ),
        }
    }

    pub(crate) fn execute_sandbox_command(
        &self,
        session_id: &str,
        command: &str,
        cwd_sandbox: &str,
        timeout_ms: u64,
    ) -> HamburResult<hambur_sandbox::SandboxExecResult> {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self.database.settings_snapshot()
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        self.sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)?;
        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let status = self.sandbox.rootfs_status();
        if !status.available {
            return Err(HamburError::Internal(format!(
                "Sandbox rootfs not available: {}",
                status.reason
            )));
        }
        self.sandbox
            .execute(session_id, command, cwd_sandbox, timeout_ms)
    }
}

/// Everything a runtime-hosted tool may need from the turn that invoked it.
pub(crate) struct ToolTurnContext<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) turn_id: &'a str,
    pub(crate) route: &'a ModelRouteSnapshot,
    pub(crate) route_candidates: &'a [ModelRouteSnapshot],
}

fn failed_record(
    invocation: ToolInvocation,
    message: impl FnOnce(&ToolInvocation) -> String,
) -> ToolExecutionRecord {
    let now = now_ms();
    let message = message(&invocation);
    ToolExecutionRecord {
        result: ToolResult::failed(&invocation.tool_call_id, &invocation.name, message),
        invocation,
        started_at_ms: now,
        ended_at_ms: now,
    }
}
