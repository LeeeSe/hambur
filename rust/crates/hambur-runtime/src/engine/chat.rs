use crate::*;

impl RuntimeEngine {
    pub(crate) fn maybe_title_session_from_first_user_message(
        &self,
        session_id: &str,
        user_content: &str,
    ) -> HamburResult<()> {
        let summary = self.database.session_summary(session_id)?;
        if summary.message_count != 1 || !is_default_session_title(&summary.title) {
            return Ok(());
        }

        let title = title_from_first_user_message(user_content);
        if title.is_empty() {
            return Ok(());
        }

        let _ = self.database.rename_session(session_id, &title)?;
        Ok(())
    }

    pub(crate) fn resolve_turn_content(
        &self,
        command: &RuntimeCommand,
        command_kind: &str,
    ) -> HamburResult<String> {
        let explicit_content = command.content.clone().if_blank(command.chunk.clone());
        if command_kind == "SendMessage" || command_kind == "EditMessage" {
            return Ok(explicit_content);
        }
        if !explicit_content.trim().is_empty() {
            return Ok(explicit_content);
        }

        let source_message_id = command
            .source_message_id
            .clone()
            .if_blank(command.message_id.clone());
        if source_message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "source_message_id must not be empty".to_string(),
            ));
        }

        self.database
                    .source_user_message_for(&command.session_id, &source_message_id)
            .map(|message| message.content_text)
    }

    pub(crate) fn prepare_visible_branch_for_command(
        &self,
        command: &RuntimeCommand,
        command_kind: &str,
    ) -> HamburResult<Option<MessageRecord>> {
        let source_message_id = command
            .source_message_id
            .clone()
            .if_blank(command.message_id.clone());
        if source_message_id.trim().is_empty() {
            return Ok(None);
        }

        match command_kind {
            "EditMessage" => {
                self.database
                    .hide_visible_timeline_after_message(
                        &command.session_id,
                        &source_message_id,
                        true,
                    )?;
                Ok(None)
            }
            "RetryTurn" | "RegenerateMessage" => {
                let source_message = self
                    .database
                    .message_snapshot(&source_message_id)?
                    .ok_or_else(|| {
                        HamburError::InvalidCommand(format!(
                            "message not found: {source_message_id}"
                        ))
                    })?;
                let is_user = source_message.role == "user";
                let source_user = self
                    .database
                    .source_user_message_for(&command.session_id, &source_message_id)?;
                self.database
                    .hide_visible_timeline_after_message(
                        &command.session_id,
                        &source_message_id,
                        !is_user,
                    )?;
                Ok(Some(source_user))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn build_chat_context_messages(
        &self,
        session_id: &str,
        current_user_message: &MessageRecord,
        _current_attachments: &[AttachmentRecord],
        route: &ModelRouteSnapshot,
        append_current_user: bool,
        current_user_model_content: &str,
    ) -> HamburResult<Vec<ModelMessage>> {
        let transcript = self
            .database
            .visible_chat_transcript_before_message(session_id, &current_user_message.id)?;
        let mut messages = Vec::new();
        let mut open_tool_call_ids = HashSet::<String>::new();

        for entry in transcript {
            let attachments = if entry.message.role == "user" {
                self.database
                    .attachments_for_message(&entry.message.id)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            append_transcript_entry_to_context(
                &mut messages,
                &mut open_tool_call_ids,
                entry,
                &attachments,
            )?;
        }

        if !open_tool_call_ids.is_empty() {
            for call_id in open_tool_call_ids {
                messages.push(ModelMessage {
                    role: "tool".to_string(),
                    content: "Tool execution was cancelled or interrupted before completion.".to_string(),
                    tool_call_id: call_id,
                    ..Default::default()
                });
            }
        }

        if append_current_user {
            messages.push(ModelMessage {
                role: "user".to_string(),
                content: current_user_model_content
                    .trim()
                    .to_string()
                    .if_blank(current_user_message.content_text.clone()),
                ..Default::default()
            });
        }

        if !route.supports_tool_call {
            messages.retain(|message| {
                message.role != "tool" && message.tool_calls_json.trim().is_empty()
            });
        }

        Ok(messages)
    }

    pub(crate) fn load_pending_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> HamburResult<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = HashSet::new();
        let mut attachments = Vec::new();
        for attachment_id in attachment_ids {
            if !seen.insert(attachment_id.clone()) {
                continue;
            }
            let attachment = self.database.attachment_by_id(attachment_id)?;
            if attachment.session_id != session_id {
                return Err(HamburError::InvalidCommand(format!(
                    "attachment does not belong to session: {attachment_id}"
                )));
            }
            if attachment.status != "pending" || !attachment.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(format!(
                    "attachment is not pending: {attachment_id}"
                )));
            }
            attachments.push(attachment);
        }
        Ok(attachments)
    }

    pub(crate) async fn resolve_provider_api_key(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        cancel: &Arc<AtomicBool>,
    ) -> HamburResult<String> {
        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::Cancelled);
        }
        let secret_ref = route.secret_ref.trim();
        if let Some(env_name) = secret_ref.strip_prefix("env://") {
            let value = std::env::var(env_name).map_err(|_| {
                HamburError::ProviderUnavailable(format!(
                    "provider secret env var is not available: {env_name}"
                ))
            })?;
            if value.trim().is_empty() {
                return Err(HamburError::ProviderUnavailable(format!(
                    "provider secret env var is empty: {env_name}"
                )));
            }
            return Ok(value);
        }
        if !secret_ref.starts_with("android-secret://") {
            return Err(HamburError::ProviderUnavailable(
                "unsupported provider secret_ref".to_string(),
            ));
        }

        let request_id = new_id("platform_req");
        let timeout_ms = 30_000;
        let request = PlatformRequest {
            request_id: request_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            kind: "ResolveSecret".to_string(),
            payload_json: json!({
                "secretRef": secret_ref,
                "providerId": route.provider_id
            })
            .to_string(),
            timeout_ms,
            cancellable: true,
        };
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut requests) = self.platform_requests.lock() {
            requests.insert(request_id.clone(), sender);
        } else {
            return Err(HamburError::Internal(
                "platform request registry unavailable".to_string(),
            ));
        }
        if let Err(error) = self.emit_platform_request(request) {
            let _ = self
                .platform_requests
                .lock()
                .ok()
                .and_then(|mut requests| requests.remove(&request_id));
            return Err(error);
        }
        let result = match timeout(Duration::from_millis(timeout_ms), receiver).await {
            Ok(Ok(result)) => result,
            _ => {
                let _ = self
                    .platform_requests
                    .lock()
                    .ok()
                    .and_then(|mut requests| requests.remove(&request_id));
                return Err(HamburError::ProviderUnavailable(
                    "ResolveSecret platform request timed out".to_string(),
                ));
            }
        };
        if result.is_error {
            return Err(HamburError::ProviderUnavailable(
                result
                    .message
                    .if_blank(result.error_code)
                    .if_blank("ResolveSecret failed".to_string()),
            ));
        }
        let value = config_payload_value(&result.payload_json);
        let api_key = config_string(&value, "apiKey")
            .if_blank(config_string(&value, "secret"))
            .if_blank(config_string(&value, "value"));
        if api_key.trim().is_empty() {
            return Err(HamburError::ProviderUnavailable(
                "ResolveSecret returned an empty secret".to_string(),
            ));
        }
        Ok(api_key)
    }

    pub(crate) async fn run_chat_turn(
        self: Arc<Self>,
        session_id: String,
        turn_id: String,
        assistant_message_id: String,
        routes: Vec<ModelRouteSnapshot>,
        fallback_policy: FallbackPolicy,
        cancel: Arc<AtomicBool>,
        stream_sources_by_route: Vec<RouteStreamSource>,
        tool_iteration: u32,
    ) {
        let mut current_assistant_message_id = assistant_message_id;
        let mut current_routes = routes;
        let mut current_stream_sources_by_route = stream_sources_by_route;
        let mut current_tool_iteration = tool_iteration;
        'agent_loop: loop {
            let target_count = current_routes.len();
            let route_candidates = current_routes.clone();
            let mut last_error = None;

            for (attempt_index, route) in current_routes.iter().cloned().enumerate() {
                if attempt_index > 0 {
                    if let Err(error) = self
                        .database
                        .update_turn_route_snapshot(&turn_id, &route)
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        );
                        return;
                    }
                    if let Err(error) = self
                        .database
                        .update_message_route_snapshot(&current_assistant_message_id, &route)
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        );
                        return;
                    }

                    let snapshot = self
                        .database
                        .session_snapshot(&session_id)
                        .unwrap_or_default();
                    let _ = self.emit_with_snapshot(
                        RuntimeEventKind::TurnStateChanged,
                        session_id.clone(),
                        turn_id.clone(),
                        snapshot,
                        format!("Fallback to {}", route.model_display_name),
                        None,
                    );
                }

                let stream_source = current_stream_sources_by_route
                    .get(attempt_index)
                    .cloned()
                    .unwrap_or_else(|| {
                        let tools_json = self.compile_enabled_main_tools_json();
                        provider_stream_source(
                            &session_id,
                            &turn_id,
                            Vec::new(),
                            &route,
                            &tools_json,
                            "",
                            "",
                            false,
                            false,
                        )
                    });
                match self
                    .clone()
                    .run_chat_stream_attempt(
                        session_id.clone(),
                        turn_id.clone(),
                        current_assistant_message_id.clone(),
                        route,
                        route_candidates.clone(),
                        cancel.clone(),
                        stream_source,
                        current_tool_iteration,
                    )
                    .await
                {
                    StreamAttemptResult::Completed | StreamAttemptResult::Cancelled => return,
                    StreamAttemptResult::Continue(continuation) => {
                        current_assistant_message_id = continuation.assistant_message_id;
                        current_routes = vec![continuation.route];
                        current_stream_sources_by_route = vec![continuation.stream_source];
                        current_tool_iteration = continuation.tool_iteration;
                        continue 'agent_loop;
                    }
                    StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started,
                    } => {
                        let can_fallback = should_fallback(
                            fallback_policy,
                            semantic_delta_started,
                            attempt_index,
                            target_count,
                            fallback_error_code(&error),
                        );
                        if can_fallback {
                            last_error = Some(error);
                            continue;
                        }
                        if !semantic_delta_started {
                            self.finish_failed_turn(
                                &session_id,
                                &turn_id,
                                &current_assistant_message_id,
                                "",
                                "",
                                error,
                            );
                        }
                        return;
                    }
                }
            }

            if let Some(error) = last_error {
                self.finish_failed_turn(
                    &session_id,
                    &turn_id,
                    &current_assistant_message_id,
                    "",
                    "",
                    error,
                );
            }
            return;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_chat_stream_attempt(
        self: Arc<Self>,
        session_id: String,
        turn_id: String,
        assistant_message_id: String,
        route: ModelRouteSnapshot,
        route_candidates: Vec<ModelRouteSnapshot>,
        cancel: Arc<AtomicBool>,
        stream_source: RouteStreamSource,
        tool_iteration: u32,
    ) -> StreamAttemptResult {
        let mut state = StreamAttemptState::default();
        let mut current_request = match &stream_source {
            RouteStreamSource::Scripted { request, .. } => request.clone(),
            RouteStreamSource::Provider(request) => request.clone(),
        };
        let continuation_sse = match &stream_source {
            RouteStreamSource::Scripted {
                continuation_sse, ..
            } => continuation_sse.clone(),
            RouteStreamSource::Provider(_) => Vec::new(),
        };
        let scripted_source = matches!(&stream_source, RouteStreamSource::Scripted { .. });
        match stream_source {
            RouteStreamSource::Scripted { chunks, .. } => {
                for chunk in chunks {
                    if let Some(result) = self
                        .clone()
                        .process_stream_chunk(
                            &mut state,
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &cancel,
                            &chunk,
                            &route.provider_protocol,
                            true,
                        )
                        .await
                    {
                        return result;
                    }
                }
            }
            RouteStreamSource::Provider(request) => {
                current_request = request.clone();
                let api_key = match self
                    .resolve_provider_api_key(&session_id, &turn_id, &route, &cancel)
                    .await
                {
                    Ok(api_key) => api_key,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                let target = provider_target_from_route(route.clone());
                let spec_result = match target.provider.protocol.as_str() {
                    OPENAI_RESPONSES_PROTOCOL => {
                        ResponsesApiAdapter::build_stream_request(&request, &target, &api_key)
                    }
                    _ => {
                        OpenAiCompatibleAdapter::build_stream_request(&request, &target, &api_key)
                    }
                };
                let spec = match spec_result {
                    Ok(spec) => spec,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                let thinking_trace = openai_request_thinking_trace(
                    &request,
                    &route,
                    &spec.body_json,
                );
                let snapshot = self
                    .database
                    .session_snapshot(&session_id)
                    .unwrap_or_default();
                let _ = self.emit_with_snapshot(
                    RuntimeEventKind::TurnStateChanged,
                    session_id.clone(),
                    turn_id.clone(),
                    snapshot,
                    thinking_trace,
                    None,
                );
                let mut response = match reqwest_stream(spec).await {
                    Ok(response) => response,
                    Err(error) => {
                        return StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: false,
                        };
                    }
                };
                loop {
                    if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                        self.finish_cancelled_turn(
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &state.content,
                            &state.reasoning,
                        );
                        return StreamAttemptResult::Cancelled;
                    }
                    let chunk = match response.chunk().await {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            let mapped = HamburError::ProviderUnavailable(format!(
                                "NetworkError: provider stream read failed: {error}"
                            ));
                            if state.semantic_delta_started {
                                self.finish_failed_turn(
                                    &session_id,
                                    &turn_id,
                                    &assistant_message_id,
                                    &state.content,
                                    &state.reasoning,
                                    mapped.clone(),
                                );
                            }
                            return StreamAttemptResult::Failed {
                                error: mapped,
                                semantic_delta_started: state.semantic_delta_started,
                            };
                        }
                    };
                    let Some(chunk) = chunk else {
                        break;
                    };
                    if let Some(result) = self
                        .clone()
                        .process_stream_chunk(
                            &mut state,
                            &session_id,
                            &turn_id,
                            &assistant_message_id,
                            &cancel,
                            chunk.as_ref(),
                            &route.provider_protocol,
                            false,
                        )
                        .await
                    {
                        return result;
                    }
                }
            }
        }

        if !state.semantic_delta_started {
            return StreamAttemptResult::Failed {
                error: HamburError::ProviderUnavailable(format!(
                    "provider {} produced no semantic output",
                    route.provider_id
                )),
                semantic_delta_started: state.semantic_delta_started,
            };
        }

        if let Some(update) =
            self.append_stream_markdown(&session_id, &assistant_message_id, "", true)
        {
            let _ = self
                .emit_markdown_persisted(session_id.clone(), turn_id.clone(), update);
        }

        let final_finish_reason = state.finish_reason.if_blank("stop".to_string());
        let final_native_finish_reason = state
            .native_finish_reason
            .if_blank(final_finish_reason.clone());
        if state.saw_tool_delta {
            state.complete_tool_calls = state.tool_accumulator.completed_calls();
        }
        if state.saw_tool_delta
            && (state.complete_tool_calls.is_empty()
                || state.tool_accumulator.has_incomplete_calls())
        {
            let error = HamburError::SseParse("incomplete tool call stream".to_string());
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            );
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        if !state.complete_tool_calls.is_empty() {
            state.complete_tool_calls.sort_by_key(|call| call.index);
            let result = self
                .execute_tool_batch_and_continue(
                    &session_id,
                    &turn_id,
                    &assistant_message_id,
                    &route,
                    &route_candidates,
                    &cancel,
                    &state.content,
                    &state.reasoning,
                    final_finish_reason,
                    final_native_finish_reason,
                    state.complete_tool_calls,
                    tool_iteration,
                    current_request,
                    continuation_sse,
                    scripted_source,
                )
                .await;
            return match result {
                Ok(Some(continuation)) => StreamAttemptResult::Continue(continuation),
                Ok(None) => StreamAttemptResult::Cancelled,
                Err(error) => {
                    self.finish_failed_turn(
                        &session_id,
                        &turn_id,
                        &assistant_message_id,
                        &state.content,
                        &state.reasoning,
                        error.clone(),
                    );
                    StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started: true,
                    }
                }
            };
        }

        if let Err(error) = self
            .database
            .update_message_stream_result(
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                "completed",
                &final_finish_reason,
                &final_native_finish_reason,
            )
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            );
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        let _ = self
            .persist_reasoning_block_for_timeline(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.reasoning,
            );
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .unwrap_or_default();
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnStateChanged,
            session_id.clone(),
            turn_id.clone(),
            snapshot,
            format!(
                "ThinkingToggle db final_write message={} status=completed contentLen={} reasoningLen={} finishReason={} nativeFinishReason={}",
                assistant_message_id,
                state.content.chars().count(),
                state.reasoning.chars().count(),
                final_finish_reason,
                final_native_finish_reason,
            ),
            None,
        );
        if let Err(error) = self
            .database
            .update_turn_status(&turn_id, "Finished", true)
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            );
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .unwrap_or_default();

        self.maybe_complete_delegate_session_from_assistant(
            &session_id,
            &assistant_message_id,
            "Delegate completed without submit_delegate_result.",
        );
        self.clear_active_turn(&session_id, &turn_id);
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::AssistantMessageFinished,
            session_id.clone(),
            turn_id.clone(),
            snapshot.clone(),
            final_finish_reason,
            None,
        );
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnFinished,
            session_id,
            turn_id,
            snapshot,
            final_native_finish_reason,
            None,
        );
        StreamAttemptResult::Completed
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn process_stream_chunk(
        self: Arc<Self>,
        state: &mut StreamAttemptState,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        cancel: &Arc<AtomicBool>,
        chunk: &[u8],
        provider_protocol: &str,
        delay_scripted_chunk: bool,
    ) -> Option<StreamAttemptResult> {
        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                &state.content,
                &state.reasoning,
            );
            return Some(StreamAttemptResult::Cancelled);
        }

        if delay_scripted_chunk {
            sleep(Duration::from_millis(24)).await;
        }
        let payloads = match state.decoder.push(chunk) {
            Ok(payloads) => payloads,
            Err(error) => {
                if state.semantic_delta_started {
                    self.finish_failed_turn(
                        session_id,
                        turn_id,
                        assistant_message_id,
                        &state.content,
                        &state.reasoning,
                        error.clone(),
                    );
                }
                return Some(StreamAttemptResult::Failed {
                    error,
                    semantic_delta_started: state.semantic_delta_started,
                });
            }
        };

        for payload in payloads {
            if let Some(_trace) =
                provider_payload_thinking_trace(&payload.data, state.thinking_raw_trace_count + 1)
            {
                state.thinking_raw_trace_count += 1;
            }
            let parsed_events = match provider_protocol {
                OPENAI_RESPONSES_PROTOCOL => {
                    ResponsesApiAdapter::parse_stream_payload(&payload)
                }
                _ => {
                    OpenAiCompatibleAdapter::parse_stream_payload(&payload)
                }
            };
            let events = match parsed_events {
                Ok(events) => events,
                Err(error) => {
                    if state.semantic_delta_started {
                        self.finish_failed_turn(
                            session_id,
                            turn_id,
                            assistant_message_id,
                            &state.content,
                            &state.reasoning,
                            error.clone(),
                        );
                    }
                    return Some(StreamAttemptResult::Failed {
                        error,
                        semantic_delta_started: state.semantic_delta_started,
                    });
                }
            };

            for event in events {
                let newly_complete_tool_calls = state.tool_accumulator.apply(&event);
                if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
                    self.finish_cancelled_turn(
                        session_id,
                        turn_id,
                        assistant_message_id,
                        &state.content,
                        &state.reasoning,
                    );
                    return Some(StreamAttemptResult::Cancelled);
                }

                match event {
                    ProviderStreamEvent::ContentDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.content.push_str(&delta);
                        state.thinking_parsed_trace_count += 1;
                        if !state.reasoning.is_empty() && !state.reasoning_persisted {
                            state.reasoning_persisted = true;
                            let _ = self
                                .persist_reasoning_block_for_timeline(
                                    session_id,
                                    turn_id,
                                    assistant_message_id,
                                    &state.reasoning,
                                );
                        }
                        if let Some(update) = self.append_stream_markdown(
                            session_id,
                            assistant_message_id,
                            &delta,
                            false,
                        ) {
                            if !update.committed_nodes.is_empty() {
                                let _ = self
                                    .emit_markdown_persisted(
                                        session_id.to_string(),
                                        turn_id.to_string(),
                                        update,
                                    );
                            } else {
                                let _ = self.emit_markdown(session_id.to_string(), turn_id.to_string(), update);
                            }
                        }
                    }
                    ProviderStreamEvent::ReasoningDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.reasoning.push_str(&delta);
                        state.thinking_parsed_trace_count += 1;
                        let should_persist =
                            !state.reasoning_persisted && !state.reasoning.is_empty();
                        if should_persist {
                            state.reasoning_persisted = true;
                            let _ = self
                                .persist_reasoning_block_for_timeline(
                                    session_id,
                                    turn_id,
                                    assistant_message_id,
                                    &state.reasoning,
                                );
                            let snapshot = self
                                .database
                                .session_snapshot(session_id)
                                .unwrap_or_default();
                            let _ = self.emit_with_snapshot(

                                RuntimeEventKind::TurnStateChanged,

                                session_id.to_string(),

                                turn_id.to_string(),

                                snapshot,

                                "ReasoningStarted".to_string(),

                                None,

                            );
                        }
                        let _ = self.emit_delta(
                            RuntimeEventKind::AssistantReasoningDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            assistant_message_id.to_string(),
                            delta,
                        );
                    }
                    ProviderStreamEvent::ToolCallDelta { .. }
                    | ProviderStreamEvent::ToolCallDone { .. } => {
                        state.semantic_delta_started = true;
                        state.saw_tool_delta = true;
                        let _ = self.emit_plain(
                            RuntimeEventKind::ToolCallDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            "Tool call delta",
                            );
                    }
                    ProviderStreamEvent::Finish {
                        finish_reason,
                        native_finish_reason,
                    } => {
                        state.finish_reason = finish_reason;
                        state.native_finish_reason = native_finish_reason;
                    }
                    ProviderStreamEvent::Error { code, message } => {
                        let error = stream_error_from_provider(code, message);
                        if state.semantic_delta_started {
                            self.finish_failed_turn(
                                session_id,
                                turn_id,
                                assistant_message_id,
                                &state.content,
                                &state.reasoning,
                                error.clone(),
                            );
                        }
                        return Some(StreamAttemptResult::Failed {
                            error,
                            semantic_delta_started: state.semantic_delta_started,
                        });
                    }
                }

                for complete in newly_complete_tool_calls {
                    if !state
                        .complete_tool_calls
                        .iter()
                        .any(|existing| existing.index == complete.index)
                    {
                        state.complete_tool_calls.push(complete);
                    }
                }
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_latest_tool_trace(
        &self,
        trace_id: Option<&str>,
        tool_call_id: &str,
        status: &str,
        summary: &str,
    ) -> HamburResult<()> {
        let Some(trace_id) = trace_id else {
            return Err(HamburError::Internal(format!(
                "tool trace missing for: {tool_call_id}"
            )));
        };
        self.database
            .update_trace_span_status(trace_id, status, summary, true)?;
        Ok(())
    }

    pub(crate) fn finish_cancelled_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        content: &str,
        reasoning: &str,
    ) {
        let _ = self
            .database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "cancelled",
                "cancelled",
                "cancelled",
            );
        let _ = self
            .persist_reasoning_block_for_timeline(
                session_id,
                turn_id,
                assistant_message_id,
                reasoning,
            );
        let _ = self
            .database
            .fail_turn(turn_id, "Cancelled", "Cancelled", "turn cancelled");
        self.maybe_fail_delegate_session(session_id, "delegate task was cancelled");
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .unwrap_or_default();
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnCancelled,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "turn cancelled".to_string(),
            Some(&HamburError::Cancelled),
        );
    }

    pub(crate) fn finish_failed_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        assistant_message_id: &str,
        content: &str,
        reasoning: &str,
        error: HamburError,
    ) {
        let _ = self
            .database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "failed_partial",
                "error",
                error.code().as_str(),
            );
        let _ = self
            .persist_reasoning_block_for_timeline(
                session_id,
                turn_id,
                assistant_message_id,
                reasoning,
            );
        let _ = self
            .database
            .fail_turn(turn_id, "Failed", error.code().as_str(), &error.to_string());
        self.maybe_fail_delegate_session(session_id, &error.to_string());
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .unwrap_or_default();
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnFailed,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            error.to_string(),
            Some(&error),
        );
    }
}

fn openai_request_thinking_trace(
    request: &ModelRequest,
    route: &ModelRouteSnapshot,
    body_json: &str,
) -> String {
    let body = serde_json::from_str::<serde_json::Value>(body_json)
        .unwrap_or(serde_json::Value::Null);
    let thinking = body
        .get("thinking")
        .map(serde_json::Value::to_string)
        .unwrap_or_else(|| "null".to_string());
    let reasoning_effort = body
        .get("reasoning_effort")
        .map(serde_json::Value::to_string)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "ThinkingToggle rust request session={} model={} supports_reasoning={} reasoning_mode={:?} thinking={} reasoning_effort={}",
        request.session_id,
        route.model_id,
        route.supports_reasoning,
        request.reasoning_mode,
        thinking,
        reasoning_effort,
    )
}

fn provider_payload_thinking_trace(data: &str, raw_index: u32) -> Option<String> {
    if data.trim().is_empty() || data.trim() == "[DONE]" {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(data).ok()?;
    let choice = value.get("choices")?.as_array()?.first()?;
    let delta = choice.get("delta").unwrap_or(&serde_json::Value::Null);
    let delta_keys = delta
        .as_object()
        .map(|object| {
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys.join("|")
        })
        .unwrap_or_default();
    let reasoning_content_len = json_string_len(delta.get("reasoning_content"));
    let reasoning_len = json_string_len(delta.get("reasoning"));
    let reasoning_content_camel_len = json_string_len(delta.get("reasoningContent"));
    let content_len = json_string_len(delta.get("content"));
    let finish_reason = choice
        .get("finish_reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let sample = delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .or_else(|| delta.get("reasoningContent"))
        .or_else(|| delta.get("content"))
        .and_then(serde_json::Value::as_str)
        .map(short_log_sample)
        .unwrap_or_default();
    Some(format!(
        "ThinkingToggle sse raw rawIndex={} deltaKeys={} reasoning_content_len={} reasoning_len={} reasoningContent_len={} content_len={} finishReason={} sample={}",
        raw_index,
        delta_keys,
        reasoning_content_len,
        reasoning_len,
        reasoning_content_camel_len,
        content_len,
        finish_reason,
        sample,
    ))
}

fn json_string_len(value: Option<&serde_json::Value>) -> usize {
    value
        .and_then(serde_json::Value::as_str)
        .map(|text| text.chars().count())
        .unwrap_or(0)
}

fn short_log_sample(text: &str) -> String {
    text.chars()
        .take(24)
        .collect::<String>()
        .replace('\n', "\\n")
}
