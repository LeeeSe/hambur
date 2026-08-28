use crate::*;

impl RuntimeEngine {
    pub fn create(bootstrap: AppBootstrap) -> HamburResult<Arc<Self>> {
        if bootstrap.app_files_dir.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "app_files_dir must not be empty".to_string(),
            ));
        }

        let tokio = Runtime::new()
            .map_err(|error| HamburError::Internal(format!("tokio runtime: {error}")))?;
        let database_path = database_path(&bootstrap);
        let database = tokio.block_on(HamburDatabase::open(database_path))?;
        let filestore = FileStore::new(&bootstrap.app_files_dir)?;
        let sandbox = SandboxService::new_with_native_library_dir(
            &bootstrap.app_files_dir,
            &bootstrap.native_library_dir,
        )?;
        seed_bundled_skills(
            &PathBuf::from(&bootstrap.app_files_dir)
                .join("sandbox")
                .join("global")
                .join("skills"),
        )?;
        let jobs = tokio.block_on(database.cleanup_pending_attachments(0))?;
        for job in jobs {
            let _ = filestore.delete_relative_if_exists(&job.relative_path);
            let _ = tokio.block_on(database.mark_file_cleanup_done(&job.id));
        }
        let snapshot = tokio.block_on(database.bootstrap_snapshot())?;
        for session in &snapshot.sessions {
            let _ = sandbox.prepare_session(&session.id);
        }
        let tools = ToolScheduler::new(PathBuf::from(&bootstrap.app_files_dir).join("offloads"))?;
        let (sender, receiver) = mpsc::channel(64);
        let engine = Arc::new(Self {
            tokio,
            self_ref: Mutex::new(Weak::new()),
            bootstrap,
            database,
            filestore,
            sandbox,
            markdown_streams: Mutex::new(HashMap::new()),
            active_turns: Mutex::new(HashMap::new()),
            platform_requests: Mutex::new(HashMap::new()),
            delegate_tasks: Mutex::new(HashMap::new()),
            delegate_sessions: Mutex::new(HashSet::new()),
            process_sessions: Mutex::new(HashMap::new()),
            completed_process_sessions: Mutex::new(HashMap::new()),
            memory_review_sessions: Mutex::new(HashSet::new()),
            router: Mutex::new(ModelRouter::default()),
            tools,
            idempotency: Mutex::new(HashMap::new()),
            sender,
            receiver: Mutex::new(receiver),
            sequence: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });

        if let Ok(mut self_ref) = engine.self_ref.lock() {
            *self_ref = Arc::downgrade(&engine);
        }
        engine.emit(RuntimeEventKind::RuntimeReady, snapshot, None)?;
        engine.schedule_startup_memory_review_check();
        engine.schedule_model_catalog_sync();
        Ok(engine)
    }

    pub(crate) fn safe_block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| self.tokio.block_on(future))
        } else {
            self.tokio.block_on(future)
        }
    }
    pub fn next_event(&self) -> Option<RuntimeEvent> {
        let mut receiver = self.receiver.lock().ok()?;
        receiver.blocking_recv()
    }

    pub(crate) fn finish_settings_command(
        &self,
        command: RuntimeCommand,
        result: HamburResult<AppSnapshot>,
        message: &'static str,
    ) -> RuntimeCommandAck {
        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    message.to_string(),
                    None,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
    pub(crate) async fn maybe_title_session_from_first_user_message(
        &self,
        session_id: &str,
        user_content: &str,
    ) -> HamburResult<()> {
        let summary = self.database.session_summary(session_id).await?;
        if summary.message_count != 1 || !is_default_session_title(&summary.title) {
            return Ok(());
        }

        let title = title_from_first_user_message(user_content);
        if title.is_empty() {
            return Ok(());
        }

        let _ = self.database.rename_session(session_id, &title).await?;
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

        self.tokio
            .block_on(
                self.database
                    .source_user_message_for(&command.session_id, &source_message_id),
            )
            .map(|message| message.content_text)
    }
    pub(crate) async fn prepare_visible_branch_for_command(
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
                    )
                    .await?;
                Ok(None)
            }
            "RetryTurn" | "RegenerateMessage" => {
                let source_message = self
                    .database
                    .message_snapshot(&source_message_id)
                    .await?
                    .ok_or_else(|| {
                        HamburError::InvalidCommand(format!(
                            "message not found: {source_message_id}"
                        ))
                    })?;
                let is_user = source_message.role == "user";
                let source_user = self
                    .database
                    .source_user_message_for(&command.session_id, &source_message_id)
                    .await?;
                self.database
                    .hide_visible_timeline_after_message(
                        &command.session_id,
                        &source_message_id,
                        !is_user,
                    )
                    .await?;
                Ok(Some(source_user))
            }
            _ => Ok(None),
        }
    }
    pub(crate) async fn build_chat_context_messages(
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
            .visible_chat_transcript_before_message(session_id, &current_user_message.id)
            .await?;
        let mut messages = Vec::new();
        let mut open_tool_call_ids = HashSet::<String>::new();

        for entry in transcript {
            append_transcript_entry_to_context(&mut messages, &mut open_tool_call_ids, entry)?;
        }

        if !open_tool_call_ids.is_empty() {
            return Err(HamburError::InvalidCommand(format!(
                "visible transcript has assistant tool calls without tool results: {}",
                open_tool_call_ids.len()
            )));
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
            let attachment = self
                .tokio
                .block_on(self.database.attachment_by_id(attachment_id))?;
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
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        if let Err(error) = self.emit_platform_request_event(request, snapshot) {
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
                        .await
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        )
                        .await;
                        return;
                    }
                    if let Err(error) = self
                        .database
                        .update_message_route_snapshot(&current_assistant_message_id, &route)
                        .await
                    {
                        self.finish_failed_turn(
                            &session_id,
                            &turn_id,
                            &current_assistant_message_id,
                            "",
                            "",
                            error,
                        )
                        .await;
                        return;
                    }

                    let snapshot = self
                        .database
                        .session_snapshot(&session_id)
                        .await
                        .unwrap_or_default();
                    let _ = self.emit_session_event(
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
                            )
                            .await;
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
                )
                .await;
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
                let spec = match OpenAiCompatibleAdapter::build_stream_request(
                    &request, &target, &api_key,
                ) {
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
                    .await
                    .unwrap_or_default();
                let _ = self.emit_session_event(
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
                        )
                        .await;
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
                                )
                                .await;
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
                .emit_markdown_event_async(session_id.clone(), turn_id.clone(), update)
                .await;
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
            )
            .await;
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
                    )
                    .await;
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
            .await
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            )
            .await;
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
            )
            .await;
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
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
            .await
        {
            self.finish_failed_turn(
                &session_id,
                &turn_id,
                &assistant_message_id,
                &state.content,
                &state.reasoning,
                error.clone(),
            )
            .await;
            return StreamAttemptResult::Failed {
                error,
                semantic_delta_started: true,
            };
        }
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .await
            .unwrap_or_default();

        self.maybe_complete_delegate_session_from_assistant(
            &session_id,
            &assistant_message_id,
            "Delegate completed without submit_delegate_result.",
        )
        .await;
        self.clear_active_turn(&session_id, &turn_id);
        let _ = self.emit_session_event(
            RuntimeEventKind::AssistantMessageFinished,
            session_id.clone(),
            turn_id.clone(),
            snapshot.clone(),
            final_finish_reason,
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnFinished,
            session_id,
            turn_id,
            snapshot,
            final_native_finish_reason,
            None,
        );
        StreamAttemptResult::Completed
    }

    pub(crate) async fn maybe_complete_delegate_session_from_assistant(
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
            .await
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
            .update_trace_span_status(&state.trace_id, "completed", &summary, true)
            .await;
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: false,
            content,
            summary,
        });
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
        delay_scripted_chunk: bool,
    ) -> Option<StreamAttemptResult> {
        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                &state.content,
                &state.reasoning,
            )
            .await;
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
                    )
                    .await;
                }
                return Some(StreamAttemptResult::Failed {
                    error,
                    semantic_delta_started: state.semantic_delta_started,
                });
            }
        };

        for payload in payloads {
            if let Some(trace) =
                provider_payload_thinking_trace(&payload.data, state.thinking_raw_trace_count + 1)
            {
                state.thinking_raw_trace_count += 1;
                let snapshot = self
                    .database
                    .session_snapshot(session_id)
                    .await
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::TurnStateChanged,
                    session_id.to_string(),
                    turn_id.to_string(),
                    snapshot,
                    trace,
                    None,
                );
            }
            let events = match OpenAiCompatibleAdapter::parse_stream_payload(&payload) {
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
                        )
                        .await;
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
                    )
                    .await;
                    return Some(StreamAttemptResult::Cancelled);
                }

                match event {
                    ProviderStreamEvent::ContentDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.content.push_str(&delta);
                        state.thinking_parsed_trace_count += 1;
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::TurnStateChanged,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot.clone(),
                            format!(
                                "ThinkingToggle parsed content_delta parsedIndex={} deltaLen={} totalContentLen={} totalReasoningLen={}",
                                state.thinking_parsed_trace_count,
                                delta.chars().count(),
                                state.content.chars().count(),
                                state.reasoning.chars().count(),
                            ),
                            None,
                        );
                        let _ = self.emit_session_event(
                            RuntimeEventKind::AssistantContentDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot.clone(),
                            delta.clone(),
                            None,
                        );
                        if let Some(update) = self.append_stream_markdown(
                            session_id,
                            assistant_message_id,
                            &delta,
                            false,
                        ) {
                            let _ = self
                                .emit_markdown_event_async(
                                    session_id.to_string(),
                                    turn_id.to_string(),
                                    update,
                                )
                                .await;
                        }
                    }
                    ProviderStreamEvent::ReasoningDelta(delta) => {
                        state.semantic_delta_started = true;
                        state.reasoning.push_str(&delta);
                        state.thinking_parsed_trace_count += 1;
                        let _ = self
                            .database
                            .update_message_stream_result(
                                assistant_message_id,
                                &state.content,
                                &state.reasoning,
                                "streaming",
                                "",
                                "",
                            )
                            .await;
                        let _ = self
                            .persist_reasoning_block_for_timeline(
                                session_id,
                                turn_id,
                                assistant_message_id,
                                &state.reasoning,
                            )
                            .await;
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::TurnStateChanged,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot.clone(),
                            format!(
                                "ThinkingToggle parsed reasoning_delta parsedIndex={} deltaLen={} totalReasoningLen={} totalContentLen={}",
                                state.thinking_parsed_trace_count,
                                delta.chars().count(),
                                state.reasoning.chars().count(),
                                state.content.chars().count(),
                            ),
                            None,
                        );
                        let _ = self.emit_session_event(
                            RuntimeEventKind::AssistantReasoningDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot,
                            delta,
                            None,
                        );
                    }
                    ProviderStreamEvent::ToolCallDelta { .. } => {
                        state.semantic_delta_started = true;
                        state.saw_tool_delta = true;
                        let snapshot = self
                            .database
                            .session_snapshot(session_id)
                            .await
                            .unwrap_or_default();
                        let _ = self.emit_session_event(
                            RuntimeEventKind::ToolCallDelta,
                            session_id.to_string(),
                            turn_id.to_string(),
                            snapshot,
                            "Tool call delta".to_string(),
                            None,
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
                            )
                            .await;
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
    pub(crate) async fn update_latest_tool_trace(
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
            .update_trace_span_status(trace_id, status, summary, true)
            .await?;
        Ok(())
    }
    pub(crate) fn spawn_memory_review_if_session_changed(
        &self,
        previous_session_id: String,
        current_session_id: String,
        reason: &str,
    ) {
        if previous_session_id.trim().is_empty() || previous_session_id == current_session_id {
            return;
        }
        if let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) {
            engine.maybe_spawn_memory_review_for_session(previous_session_id, reason);
        }
    }
    pub(crate) fn schedule_startup_memory_review_check(self: &Arc<Self>) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            sleep(Duration::from_secs(30)).await;
            if engine.shutdown.load(Ordering::SeqCst) {
                return;
            }
            let sessions = engine
                .database
                .unreviewed_sessions(50)
                .await
                .unwrap_or_default();
            for session in sessions {
                engine
                    .clone()
                    .maybe_spawn_memory_review_for_session(session.id, "app_startup");
            }
        });
    }

    pub(crate) fn schedule_model_catalog_sync(self: &Arc<Self>) {
        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return;
        };
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            sleep(Duration::from_secs(15)).await;
            if engine.shutdown.load(Ordering::SeqCst) {
                return;
            }
            let _ = engine.models_dev_catalog_json(false).await;
        });
    }

    pub(crate) fn maybe_spawn_memory_review_for_session(
        self: Arc<Self>,
        session_id: String,
        reason: &str,
    ) {
        if session_id.trim().is_empty() || self.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let inserted = self
            .memory_review_sessions
            .lock()
            .map(|mut sessions| sessions.insert(session_id.clone()))
            .unwrap_or(false);
        if !inserted {
            return;
        }
        let reason = reason.to_string();
        let engine = self.clone();
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .clone()
                .review_memory_session_if_needed(session_id.clone(), reason)
                .await;
            if let Ok(mut sessions) = engine.memory_review_sessions.lock() {
                sessions.remove(&session_id);
            }
        });
    }
    pub(crate) async fn review_memory_session_if_needed(
        self: Arc<Self>,
        session_id: String,
        reason: String,
    ) {
        if self.active_turn_for_session(&session_id).is_some() {
            return;
        }
        let review = match self.database.session_review_record(&session_id).await {
            Ok(review) => review,
            Err(_) => return,
        };
        if self
            .delegate_sessions
            .lock()
            .map(|sessions| sessions.contains(&session_id))
            .unwrap_or(false)
            || review.purpose == "delegate"
        {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            return;
        }
        if review.memory_reviewed {
            return;
        }
        if review.messages.is_empty() && review.trace_spans.is_empty() {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            return;
        }
        if self
            .clone()
            .run_automatic_memory_review(review, &reason)
            .await
            .unwrap_or(false)
        {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true)
                .await;
            let snapshot = self
                .database
                .session_snapshot(&session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
                RuntimeEventKind::SettingsChanged,
                session_id,
                String::new(),
                snapshot,
                "Memory reviewed".to_string(),
                None,
            );
        }
    }
    pub(crate) async fn run_automatic_memory_review(
        self: Arc<Self>,
        review: SessionReviewRecord,
        reason: &str,
    ) -> HamburResult<bool> {
        let routes = self.database.memory_review_route().await?;
        let mut plan = route_plan_from_records(routes);
        plan.targets
            .retain(|target| target.model.capabilities.supports_tool_call);
        if plan.targets.is_empty() {
            return Ok(false);
        }
        let requirements = RouteRequirements {
            requires_tool_protocol: true,
            ..Default::default()
        };
        let route_plan = self
            .router
            .lock()
            .map_err(|_| HamburError::Internal("router registry poisoned".to_string()))?
            .resolve(plan, requirements)?;
        let memory_snapshot = self.memory_snapshot_async().await.unwrap_or_default();
        let initial_messages = build_memory_review_messages(&review, reason, &memory_snapshot);
        for target in route_plan.targets {
            let route = route_snapshot_from_target(&target);
            if !route.supports_tool_call {
                continue;
            }
            if self
                .clone()
                .run_memory_review_on_route(&review.id, &route, initial_messages.clone())
                .await
                .unwrap_or(false)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(crate) async fn run_memory_review_on_route(
        self: Arc<Self>,
        session_id: &str,
        route: &ModelRouteSnapshot,
        mut messages: Vec<ModelMessage>,
    ) -> HamburResult<bool> {
        let mut executed_memory_tool = false;
        let tools_json = compile_named_tools_json(
            &self.compile_enabled_main_tools_json(),
            &["memory"],
        )?;
        for _ in 0..MAX_MEMORY_REVIEW_TOOL_ITERATIONS {
            let request = ModelRequest {
                request_id: new_id("llm_req"),
                session_id: session_id.to_string(),
                turn_id: new_id("memory_review"),
                purpose: "memory_review".to_string(),
                stream: false,
                system_blocks: Vec::new(),
                messages: messages.clone(),
                reasoning_mode: ReasoningMode::Disabled,
                max_output_tokens: route.output_limit.min(2048),
                temperature: Some(0.0),
                tools_json: tools_json.clone(),
            };
            let assistant = self.memory_review_completion(&request, route).await?;
            if assistant.tool_calls.is_empty() {
                return Ok(true);
            }
            executed_memory_tool = true;
            messages.push(ModelMessage {
                role: "assistant".to_string(),
                content: assistant.content,
                reasoning_content: String::new(),
                tool_calls_json: complete_tool_calls_json(&assistant.tool_calls)?,
                tool_call_id: String::new(),
            });
            for call in assistant.tool_calls {
                let invocation = ToolInvocation::from_model_call(
                    call.index,
                    call.id,
                    request.turn_id.clone(),
                    session_id.to_string(),
                    call.name,
                    call.arguments_json,
                )?;
                let result = self.execute_memory_review_tool(invocation).await;
                messages.push(ModelMessage {
                    role: "tool".to_string(),
                    content: result.context_stub,
                    reasoning_content: String::new(),
                    tool_calls_json: String::new(),
                    tool_call_id: result.tool_call_id,
                });
            }
        }
        Ok(executed_memory_tool)
    }
    pub(crate) async fn memory_review_completion(
        &self,
        request: &ModelRequest,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MemoryReviewAssistantMessage> {
        let cancel = Arc::new(AtomicBool::new(false));
        let api_key = self
            .resolve_provider_api_key(&request.session_id, &request.turn_id, route, &cancel)
            .await?;
        let target = provider_target_from_route(route.clone());
        let spec = openai_non_stream_request(request, &target, &api_key)?;
        let body = reqwest_json(spec).await?;
        parse_openai_non_stream_message(&body)
    }
    pub(crate) async fn resolve_session_search_result(
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
            .await
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
        self.tools.normalize_raw(raw).unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
    }
    pub(crate) fn start_background_process(
        &self,
        invocation: &ToolInvocation,
        command: &str,
        sandbox_cwd: &str,
    ) -> ToolResult {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .safe_block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        if let Err(error) = self
            .sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("sandbox initialization failed: {error}"),
            );
        }
        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let process_session_id = new_id("proc");
        let backend = self.sandbox.rootfs_status().backend;
        let pid_file = if backend == "chroot" {
            match self.sandbox.chroot_process_pid_file(&process_session_id) {
                Ok(path) => Some(path),
                Err(error) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("build execution command failed: {error}"),
                    );
                }
            }
        } else {
            None
        };

        let (program, args, envs) = match self.sandbox.build_execution_command(
            &invocation.session_id,
            command,
            sandbox_cwd,
            &process_session_id,
        ) {
            Ok(res) => res,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("build execution command failed: {error}"),
                );
            }
        };

        let mut cmd = Command::new(&program);
        cmd.args(&args);
        for (k, v) in envs {
            cmd.env(k, v);
        }

        let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("terminal background spawn failed: {error}"),
                );
            }
        };
        let started_at_ms = now_ms();
        let pid = child.id();
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::default()));
        if let Some(stdout) = child.stdout.take() {
            spawn_process_pipe_reader(stdout, output.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_process_pipe_reader(stderr, output.clone(), false);
        }

        let state = BackgroundProcessSession {
            session_id: invocation.session_id.clone(),
            process_session_id: process_session_id.clone(),
            backend: self.sandbox.rootfs_status().backend.clone(),
            command: command.to_string(),
            cwd: sandbox_cwd.to_string(),
            started_at_ms,
            pid,
            pid_file,
            child,
            output,
            exit_code: None,
            finished_at_ms: 0,
        };
        if let Ok(mut sessions) = self.process_sessions.lock() {
            sessions.insert(process_session_id.clone(), state);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process registry unavailable",
            );
        }

        let content = json!({
            "processSessionId": process_session_id,
            "backend": backend,
            "command": command,
            "cwd": sandbox_cwd,
            "startedAt": started_at_ms,
            "pid": pid
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "background process started".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }
    pub(crate) fn resolve_process_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let process_session_id = arguments
            .get("session_id")
            .or_else(|| arguments.get("process_session_id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        match action {
            "list" => self.list_process_sessions(invocation),
            "poll" | "log" => self.snapshot_process_session(invocation, process_session_id, false),
            "wait" => {
                let timeout_ms = argument_seconds_or_ms(arguments, "timeout", "timeout_ms", 30_000)
                    .clamp(1_000, 300_000);
                self.wait_process_session(invocation, process_session_id, timeout_ms)
            }
            "kill" | "close" => self.kill_process_session(invocation, process_session_id),
            "write" | "submit" => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process stdin is not attached for this backend",
            ),
            _ => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("unsupported process action: {action}"),
            ),
        }
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
    pub(crate) async fn resolve_hambur_config_tool_result(
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

        let value = match self.hambur_config_response(arguments).await {
            Ok(value) => value,
            Err(error) => json!({
                "ok": false,
                "error": "validation_failed",
                "reason": error.to_string()
            }),
        };
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: value
                .get("ok")
                .and_then(Value::as_bool)
                .map(|ok| !ok)
                .unwrap_or(false),
            summary: value
                .get("user_message")
                .or_else(|| value.get("reason"))
                .or_else(|| value.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or("hambur_config completed")
                .to_string(),
            content_json: value.to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: value.to_string(),
        }
    }
    pub(crate) async fn hambur_config_response(&self, arguments: &Value) -> HamburResult<Value> {
        let action = config_string(arguments, "action");
        match action.as_str() {
            "list_topics" => Ok(json!({
                "ok": true,
                "data": {
                    "topics": hambur_config_topics()
                }
            })),
            "topic_help" => {
                let topic = normalize_hambur_config_topic(
                    &config_string(arguments, "topic").if_blank(config_string(arguments, "path")),
                );
                if topic.is_empty() {
                    return Ok(config_error("validation_failed", "topic is required."));
                }
                let fields = hambur_config_fields()
                    .into_iter()
                    .filter(|field| field.topic == topic)
                    .map(HamburConfigFieldSpec::to_json)
                    .collect::<Vec<_>>();
                if fields.is_empty() {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No registered topic '{topic}'."),
                    ));
                }
                Ok(json!({
                    "ok": true,
                    "data": {
                        "topic": topic,
                        "fields": fields
                    }
                }))
            }
            "get" => {
                let path = normalize_hambur_config_path(&config_string(arguments, "path"));
                if path.is_empty() {
                    return Ok(config_error("validation_failed", "path is required."));
                }
                let field = hambur_config_field_for(&path);
                let Some(field) = field else {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No registered field at '{path}'."),
                    ));
                };
                if field.access == "write-only" {
                    return Ok(config_error(
                        "permission_denied",
                        &format!(
                            "permission_denied: {} is write-only and cannot be read back.",
                            field.path
                        ),
                    ));
                }
                let snapshot = self.database.settings_snapshot().await?;
                Ok(read_hambur_config_path(
                    &snapshot,
                    &path,
                    &field,
                    &config_string(arguments, "filter"),
                    config_u32(arguments, "page", 1),
                    config_u32(arguments, "page_size", 20),
                ))
            }
            "set" | "append" | "remove" => {
                let mut path = config_string(arguments, "path");
                if action == "append" && !path.ends_with(".append") {
                    path.push_str(".append");
                }
                if action == "remove" && !path.ends_with(".remove") {
                    path.push_str(".remove");
                }
                let result = self
                    .apply_hambur_config_mutation(
                        &path,
                        &config_string(arguments, "value_json"),
                        &config_string(arguments, "actor").if_blank("agent".to_string()),
                        &config_string(arguments, "caption"),
                    )
                    .await?;
                Ok(result)
            }
            "set_batch" => {
                let Some(items) = arguments.get("batch").and_then(Value::as_array) else {
                    return Ok(config_error(
                        "validation_failed",
                        "batch is required and cannot be empty.",
                    ));
                };
                if items.is_empty() {
                    return Ok(config_error(
                        "validation_failed",
                        "batch is required and cannot be empty.",
                    ));
                }
                let actor = config_string(arguments, "actor").if_blank("agent".to_string());
                let caption = config_string(arguments, "caption");
                let mut results = Vec::new();
                for item in items {
                    let item_path = config_string(item, "path");
                    let item_caption = config_string(item, "caption").if_blank(caption.clone());
                    let outcome = self
                        .apply_hambur_config_mutation(
                            &item_path,
                            &config_string(item, "value_json"),
                            &actor,
                            &item_caption,
                        )
                        .await?;
                    if !outcome.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                        return Ok(json!({
                            "ok": false,
                            "error": outcome.get("error").cloned().unwrap_or_else(|| json!("validation_failed")),
                            "reason": format!("Batch failed at '{}': {}", item_path, outcome.get("reason").and_then(Value::as_str).unwrap_or("unknown error")),
                            "results": results
                        }));
                    }
                    results.push(outcome);
                }
                Ok(json!({
                    "ok": true,
                    "count": results.len(),
                    "results": results,
                    "user_message": format!("已更新 {} 项 Hambur 配置。", results.len())
                }))
            }
            "audit_list" => {
                let snapshot = self.database.settings_snapshot().await?;
                let scope = normalize_hambur_config_topic(&config_string(arguments, "scope"));
                let limit = config_u32(arguments, "limit", 50).clamp(1, 200) as usize;
                let entries = snapshot
                    .config_audits
                    .iter()
                    .filter(|entry| {
                        scope.is_empty()
                            || normalize_hambur_config_topic(&entry.target_kind) == scope
                    })
                    .take(limit)
                    .map(config_audit_json)
                    .collect::<Vec<_>>();
                Ok(json!({
                    "ok": true,
                    "count": entries.len(),
                    "capacity": 40,
                    "total_used": snapshot.config_audits.len(),
                    "entries": entries
                }))
            }
            "audit_get" => {
                let audit_id = config_string(arguments, "audit_id");
                if audit_id.is_empty() {
                    return Ok(config_error("validation_failed", "audit_id is required."));
                }
                let snapshot = self.database.settings_snapshot().await?;
                let Some(entry) = snapshot
                    .config_audits
                    .iter()
                    .find(|entry| entry.id == audit_id)
                else {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No audit entry '{audit_id}'."),
                    ));
                };
                Ok(json!({
                    "ok": true,
                    "entry": config_audit_json(entry)
                }))
            }
            "audit_revert" => Ok(config_error(
                "permission_denied",
                "audit_revert is not implemented in the Rust settings backend yet.",
            )),
            _ => Ok(config_error(
                "validation_failed",
                &format!("Unknown hambur_config action: {action}"),
            )),
        }
    }
    pub(crate) async fn apply_hambur_config_mutation(
        &self,
        raw_path: &str,
        value_json: &str,
        actor: &str,
        caption: &str,
    ) -> HamburResult<Value> {
        let (path, operation) = parse_hambur_config_operation(raw_path);
        if path.is_empty() {
            return Ok(config_error("validation_failed", "path is required."));
        }
        let Some(field) = hambur_config_field_for(&path) else {
            return Ok(config_error(
                "unknown_path",
                &format!("No registered field at '{raw_path}'."),
            ));
        };
        if field.access == "readonly" {
            return Ok(config_error(
                "permission_denied",
                &format!("Field '{path}' is readonly."),
            ));
        }
        if operation != ConfigOperation::Set {
            return Ok(config_error(
                "validation_failed",
                &format!("'{path}' does not support {}.", operation.as_str()),
            ));
        }

        let snapshot = self.database.settings_snapshot().await?;
        let old = read_hambur_config_raw_value(&snapshot, &path);
        let setting = match app_setting_for_hambur_config_path(&path, value_json) {
            Some(setting) => setting,
            None => {
                return Ok(config_error(
                    "unknown_path",
                    &format!("No writable field at '{path}'."),
                ));
            }
        };
        let record = self
            .database
            .upsert_app_setting(&setting.0, &setting.1)
            .await?;
        let new_value = serde_json::to_string(&record.value).unwrap_or_else(|_| record.value);
        let summary = format!(
            "{} {}\nPath: {}\nOld: {}\nNew: {}",
            operation.as_str(),
            field.display_name,
            path,
            old.chars().take(240).collect::<String>(),
            new_value.chars().take(240).collect::<String>()
        );
        let audit = self
            .database
            .insert_config_audit(
                &new_id("config_tool"),
                if actor.trim().is_empty() {
                    "agent"
                } else {
                    actor
                },
                "HamburConfigTool",
                field.topic,
                &path,
                if caption.trim().is_empty() {
                    &summary
                } else {
                    caption
                },
                false,
                "",
            )
            .await?;
        let snapshot = self.database.bootstrap_snapshot().await?;
        let _ = self.emit_session_event(
            RuntimeEventKind::SettingsChanged,
            String::new(),
            String::new(),
            snapshot,
            format!("Setting updated: {}", path),
            None,
        );
        Ok(json!({
            "ok": true,
            "path": path,
            "old": old,
            "new": new_value,
            "audit_id": audit.id,
            "user_message": format!("已更新 Hambur 配置：{}", path)
        }))
    }
    pub(crate) async fn resolve_knowledge_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let tool_name = normalize_knowledge_tool_name(&invocation.name);
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(tool_name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let result = match tool_name {
            "skills_list" => {
                let category = arguments
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let disabled = self.disabled_skill_paths_async().await;
                let all_skills = self
                    .list_skills_with_disabled(&disabled)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|skill| skill.enabled)
                    .collect::<Vec<_>>();
                let mut categories = all_skills
                    .iter()
                    .filter_map(|skill| {
                        (!skill.category.is_empty()).then(|| skill.category.clone())
                    })
                    .collect::<Vec<_>>();
                categories.sort();
                categories.dedup();
                let skills = all_skills
                    .into_iter()
                    .filter(|skill| category.is_empty() || skill.category == category)
                    .map(skill_summary_json)
                    .collect::<Vec<_>>();
                let count = skills.len();
                Ok(json!({
                    "success": true,
                    "skills": skills,
                    "categories": categories,
                    "count": count,
                    "hint": "Use skill_view(name) to see full content, tags, and linked files.",
                    "summary": format!("{count} skills")
                }))
            }
            "skill_view" => {
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file_path = arguments
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let disabled = self.disabled_skill_paths_async().await;
                self.get_skill_detail_with_disabled(name, file_path, &disabled)
                    .map(|detail| skill_detail_json(detail, !file_path.trim().is_empty()))
            }
            "memory" => self.memory_tool_result(arguments),
            _ => Err(HamburError::InvalidCommand(format!(
                "unknown knowledge tool: {}",
                invocation.name
            ))),
        };
        match result {
            Ok(value) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !value
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                summary: value
                    .get("summary")
                    .or_else(|| value.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("knowledge tool completed")
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
    pub(crate) fn snapshot_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        remove_finished: bool,
    ) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let Some(state) = sessions.get_mut(process_session_id) else {
            drop(sessions);
            if let Some(completed) = self.completed_process_snapshot(invocation, process_session_id)
            {
                return completed;
            }
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        refresh_process_exit(state);
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(state, Some(output));
        if remove_finished && state.exit_code.is_some() {
            sessions.remove(process_session_id);
        }
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session snapshot".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }
    pub(crate) fn wait_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        timeout_ms: u64,
    ) -> ToolResult {
        let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
        loop {
            {
                let mut sessions = match self.process_sessions.lock() {
                    Ok(sessions) => sessions,
                    Err(_) => {
                        return ToolResult::failed(
                            &invocation.tool_call_id,
                            &invocation.name,
                            "process registry unavailable",
                        );
                    }
                };
                let Some(state) = sessions.get_mut(process_session_id) else {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session not found",
                    );
                };
                if state.session_id != invocation.session_id {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session belongs to a different session",
                    );
                }
                refresh_process_exit(state);
                if state.exit_code.is_some() {
                    let output = state
                        .output
                        .lock()
                        .map(|buffer| buffer.snapshot())
                        .unwrap_or_default();
                    let content = process_status_json(state, Some(output));
                    if let Some(completed) =
                        completed_process_session_from_state(state, process_session_id)
                    {
                        if let Ok(mut completed_sessions) = self.completed_process_sessions.lock() {
                            completed_sessions.insert(process_session_id.to_string(), completed);
                        }
                    }
                    sessions.remove(process_session_id);
                    return ToolResult {
                        tool_call_id: invocation.tool_call_id.clone(),
                        tool_name: invocation.name.clone(),
                        is_error: false,
                        content_json: content.to_string(),
                        summary: "process session finished".to_string(),
                        artifacts_json: "[]".to_string(),
                        trust_level: "untrusted".to_string(),
                        truncated: false,
                        offloaded_file_id: String::new(),
                        offloaded_path: String::new(),
                        context_stub: content.to_string(),
                    };
                }
            }
            if Instant::now() >= deadline {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process wait timed out",
                );
            }
            thread::sleep(StdDuration::from_millis(20));
        }
    }
    pub(crate) fn completed_process_snapshot(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
    ) -> Option<ToolResult> {
        let sessions = self.completed_process_sessions.lock().ok()?;
        let state = sessions.get(process_session_id)?;
        if state.session_id != invocation.session_id {
            return Some(ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            ));
        }
        let content = completed_process_status_json(state);
        Some(ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session snapshot".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        })
    }
    pub(crate) fn kill_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
    ) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let Some(mut state) = sessions.remove(process_session_id) else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            sessions.insert(process_session_id.to_string(), state);
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        terminate_background_process_wrapper(&state);
        let _ = state.child.kill();
        let _ = state.child.wait();
        state.exit_code = Some(-1);
        state.finished_at_ms = now_ms();
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(&state, Some(output));
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session closed".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }
    pub(crate) fn kill_processes_for_session(&self, session_id: &str) {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => return,
        };
        let ids = sessions
            .iter()
            .filter_map(|(id, state)| {
                if state.session_id == session_id {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(mut state) = sessions.remove(&id) {
                terminate_background_process_wrapper(&state);
                let _ = state.child.kill();
                let _ = state.child.wait();
            }
        }
    }
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
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        if let Err(error) = self.emit_platform_request_event(request, snapshot) {
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
                        .await
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
                self.tools.normalize_raw(raw).unwrap_or_else(|error| {
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
                let snapshot = self
                    .database
                    .session_snapshot(session_id)
                    .await
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::PlatformRequestTimedOut,
                    session_id.to_string(),
                    turn_id.to_string(),
                    snapshot,
                    request_id,
                    Some(&HamburError::InvalidCommand(
                        "PlatformRequestTimeout".to_string(),
                    )),
                );
                ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "PlatformRequestTimeout",
                )
            }
        }
    }
    pub(crate) async fn materialize_browser_artifacts(
        &self,
        session_id: &str,
        tool_call_id: &str,
        content: &str,
    ) -> HamburResult<String> {
        let Ok(mut value) = serde_json::from_str::<Value>(content) else {
            return Ok(content.to_string());
        };
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
            })
            .await?;
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
    pub(crate) async fn resolve_web_tool_result(
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
            "web_fetch" => {
                let backend = self
                    .database
                    .settings_snapshot()
                    .await
                    .ok()
                    .map(|snapshot| setting_value(&snapshot, "webFetchBackend", "local"))
                    .unwrap_or_else(|| "local".to_string());
                run_web_fetch(invocation, arguments, &backend)
            }
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

        self.tools.normalize_raw(raw).unwrap_or_else(|error| {
            ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            )
        })
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
                .resolve_submit_delegate_result(session_id, invocation, arguments)
                .await;
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
    pub(crate) async fn resolve_submit_delegate_result(
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
            .update_trace_span_status(&state.trace_id, "completed", &summary, true)
            .await;
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
            .await
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
            .await
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
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = self
                    .database
                    .update_trace_span_status(&trace.id, "failed", &error.to_string(), true)
                    .await;
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
                    .update_trace_span_status(&trace.id, "failed", message, true)
                    .await;
                ToolResult::failed(&invocation.tool_call_id, &invocation.name, message)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prepare_delegate_child_turn(
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
            .create_turn_with_route(delegate_session_id, "StreamingAssistant", &route)
            .await?;
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
            )
            .await?;
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
            )
            .await?;
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
            )
            .await?;
        self.insert_initial_pending_markdown_block(
            delegate_session_id,
            &turn.id,
            &assistant_message.id,
        )
        .await?;
        let snapshot = self.database.session_snapshot(delegate_session_id).await?;
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
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStarted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            "DelegateTask".to_string(),
            None,
        );
        let _ = self.emit_session_event(
            RuntimeEventKind::MessageUpserted,
            delegate_session_id.to_string(),
            turn.id.clone(),
            snapshot.clone(),
            child_content.clone(),
            None,
        );
        let _ = self.emit_session_event(
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
        let skills_index_prompt = self.build_skills_index_prompt_async().await;
        let memory_system_prompt = self.build_memory_system_prompt_async().await;
        let stream_sources_by_route = route_candidates
            .iter()
            .map(|candidate| {
                stream_source_for_command(
                    &stream_command,
                    &turn.id,
                    &child_content,
                    vec![ModelMessage {
                        role: "user".to_string(),
                        content: child_content.clone(),
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
    pub(crate) async fn resolve_view_image_result(
        &self,
        session_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let detail = arguments
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let detail = if detail.is_empty() {
            match self.database.settings_snapshot().await {
                Ok(snapshot) => {
                    match setting_value(&snapshot, "viewImageScaleMode", "resize_fit").as_str() {
                        "original" => "original",
                        _ => "high",
                    }
                }
                Err(_) => "high",
            }
        } else {
            detail
        };
        if path.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image path must not be empty",
            );
        }
        if !route.supports_image_input && vision_handoff_target(route_candidates, route).is_none() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image unavailable: no vision-capable handoff target",
            );
        }

        let file = match self
            .database
            .resolve_file_by_sandbox_path(session_id, path)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                );
            }
        };
        if !file.mime_type.starts_with("image/") {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image requires an image file",
            );
        }
        let image_attached_to_next_request =
            route.supports_image_input || vision_handoff_target(route_candidates, route).is_some();
        let content = json!({
            "path": path,
            "resolvedPath": file.sandbox_path,
            "detail": normalize_image_detail(detail),
            "width": 0,
            "height": 0,
            "mimeType": file.mime_type,
            "fileId": file.id,
            "imageAttachedToNextRequest": image_attached_to_next_request
        });
        let context_stub = format!(
            "Image returned by view_image for tool_call_id={}: ImagePart(fileId={}, path={}, mimeType={}, detail={})",
            invocation.tool_call_id,
            content
                .get("fileId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("resolvedPath")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            content
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Image prepared for vision continuation".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub,
        }
    }
    pub(crate) async fn finish_cancelled_turn(
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
            )
            .await;
        let _ = self
            .persist_reasoning_block_for_timeline(
                session_id,
                turn_id,
                assistant_message_id,
                reasoning,
            )
            .await;
        let _ = self
            .database
            .fail_turn(turn_id, "Cancelled", "Cancelled", "turn cancelled")
            .await;
        self.maybe_fail_delegate_session(session_id, "delegate task was cancelled")
            .await;
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnCancelled,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            "turn cancelled".to_string(),
            Some(&HamburError::Cancelled),
        );
    }
    pub(crate) async fn finish_failed_turn(
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
            )
            .await;
        let _ = self
            .persist_reasoning_block_for_timeline(
                session_id,
                turn_id,
                assistant_message_id,
                reasoning,
            )
            .await;
        let _ = self
            .database
            .fail_turn(turn_id, "Failed", error.code().as_str(), &error.to_string())
            .await;
        self.maybe_fail_delegate_session(session_id, &error.to_string())
            .await;
        self.clear_active_turn(session_id, turn_id);
        let snapshot = self
            .database
            .session_snapshot(session_id)
            .await
            .unwrap_or_default();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnFailed,
            session_id.to_string(),
            turn_id.to_string(),
            snapshot,
            error.to_string(),
            Some(&error),
        );
    }
    pub(crate) async fn maybe_fail_delegate_session(&self, session_id: &str, message: &str) {
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
            .update_trace_span_status(&state.trace_id, "failed", &summary, true)
            .await;
        let _ = state.sender.send(DelegateCompletionPayload {
            is_error: true,
            content,
            summary,
        });
    }
    pub(crate) fn append_stream_markdown(
        &self,
        session_id: &str,
        message_id: &str,
        chunk: &str,
        finalize: bool,
    ) -> Option<MarkdownRenderUpdate> {
        let stream_key = format!("{session_id}:{message_id}");
        let mut streams = self.markdown_streams.lock().ok()?;
        let pipeline = streams
            .entry(stream_key.clone())
            .or_insert_with(|| MarkdownPipeline::new(message_id.to_string()));
        let update = if finalize {
            pipeline.finalize()
        } else {
            pipeline.append(chunk)
        };
        if finalize {
            streams.remove(&stream_key);
        }
        if update.committed_nodes.is_empty()
            && update.pending_node.is_none()
            && !update.reset
            && update.invalidated_block_ids.is_empty()
        {
            None
        } else {
            Some(update)
        }
    }
    pub(crate) async fn insert_initial_pending_markdown_block(
        &self,
        session_id: &str,
        turn_id: &str,
        message_id: &str,
    ) -> HamburResult<()> {
        let stable_key = hambur_db::pending_markdown_stable_key(message_id);
        let node = MarkdownBlockNode {
            message_id: message_id.to_string(),
            block_id: 0,
            stable_key: stable_key.clone(),
            source_kind: "assistant".to_string(),
            node_kind: "root".to_string(),
            committed: false,
            level: 0,
            inlines: Vec::new(),
            language: String::new(),
            text: String::new(),
            raw: String::new(),
            children_json: String::new(),
            items_json: String::new(),
            table_header: Vec::new(),
            table_rows: Vec::new(),
            table_alignments: Vec::new(),
            path: String::new(),
            file_kind: String::new(),
        };
        let payload_json = serde_json::to_string(&node).map_err(|error| {
            HamburError::Internal(format!(
                "serialize initial pending markdown block payload: {error}"
            ))
        })?;
        self.database
            .upsert_message_block_payload(
                session_id,
                turn_id,
                NewMessageBlockPayload {
                    id: String::new(),
                    message_id: message_id.to_string(),
                    block_id: 0,
                    block_type: "content".to_string(),
                    stable_key,
                    committed: false,
                    payload_json,
                    raw: String::new(),
                    small_summary: String::new(),
                },
            )
            .await?;
        Ok(())
    }
    pub(crate) async fn persist_markdown_update_for_timeline(
        &self,
        session_id: &str,
        turn_id: &str,
        update: &MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if update.message_id.trim().is_empty() {
            return Ok(());
        }
        for node in &update.committed_nodes {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize markdown block payload: {error}"))
            })?;
            self.database
                .upsert_message_block_payload(
                    session_id,
                    turn_id,
                    NewMessageBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        block_type: "content".to_string(),
                        stable_key: node.stable_key.clone(),
                        committed: true,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )
                .await?;
        }
        if let Some(node) = &update.pending_node {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize pending markdown block payload: {error}"))
            })?;
            self.database
                .upsert_message_block_payload(
                    session_id,
                    turn_id,
                    NewMessageBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        block_type: "content".to_string(),
                        stable_key: hambur_db::pending_markdown_stable_key(&update.message_id),
                        committed: false,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )
                .await?;
        } else if !update.committed_nodes.is_empty() || update.reset {
            self.database
                .remove_pending_markdown_block(session_id, &update.message_id)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn persist_reasoning_block_for_timeline(
        &self,
        session_id: &str,
        turn_id: &str,
        message_id: &str,
        reasoning: &str,
    ) -> HamburResult<()> {
        if message_id.trim().is_empty() || reasoning.trim().is_empty() {
            return Ok(());
        }
        let stable_key = hambur_db::reasoning_block_stable_key(message_id);
        let node = MarkdownBlockNode {
            message_id: message_id.to_string(),
            block_id: 0,
            stable_key: stable_key.clone(),
            source_kind: "assistant_reasoning".to_string(),
            node_kind: "reasoning".to_string(),
            committed: true,
            level: 0,
            inlines: Vec::new(),
            language: String::new(),
            text: reasoning.to_string(),
            raw: reasoning.to_string(),
            children_json: String::new(),
            items_json: String::new(),
            table_header: Vec::new(),
            table_rows: Vec::new(),
            table_alignments: Vec::new(),
            path: String::new(),
            file_kind: String::new(),
        };
        let payload_json = serde_json::to_string(&node).map_err(|error| {
            HamburError::Internal(format!("serialize reasoning block payload: {error}"))
        })?;
        self.database
            .upsert_message_block_payload(
                session_id,
                turn_id,
                NewMessageBlockPayload {
                    id: String::new(),
                    message_id: message_id.to_string(),
                    block_id: 0,
                    block_type: "reasoning".to_string(),
                    stable_key,
                    committed: true,
                    payload_json,
                    raw: reasoning.to_string(),
                    small_summary: reasoning.chars().take(160).collect(),
                },
            )
            .await?;
        Ok(())
    }
    pub fn delete_skill(&self, identifier: String) -> RuntimeCommandAck {
        let command = RuntimeCommand {
            kind: "DeleteSkill".to_string(),
            message_id: identifier.clone(),
            idempotency_key: format!("skill:{identifier}:delete:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        };
        self.dispatch(command)
    }

    pub fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Ok(active_turns) = self.active_turns.lock() {
            for active in active_turns.values() {
                active.cancel.store(true, Ordering::SeqCst);
            }
        }

        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        let _ = self.emit(RuntimeEventKind::RuntimeClosed, snapshot, None);
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn app_files_dir(&self) -> &str {
        &self.bootstrap.app_files_dir
    }

    pub(crate) fn snapshot_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }
    pub(crate) fn active_turn_for_session(&self, session_id: &str) -> Option<ActiveTurn> {
        self.active_turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(session_id).cloned())
    }
    pub(crate) fn clear_active_turn(&self, session_id: &str, turn_id: &str) {
        if let Ok(mut turns) = self.active_turns.lock()
            && turns
                .get(session_id)
                .is_some_and(|active| active.turn_id == turn_id)
        {
            turns.remove(session_id);
        }
    }
    pub(crate) fn emit(
        &self,
        kind: RuntimeEventKind,
        snapshot: AppSnapshot,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) && !matches!(kind, RuntimeEventKind::RuntimeClosed)
        {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let (error_code, message) = match error {
            Some(error) => (error.code().as_str().to_string(), error.to_string()),
            None => (String::new(), String::new()),
        };
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id: snapshot.selected_session_id.clone(),
            turn_id: String::new(),
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: PlatformRequest::default(),
            error_code,
            message,
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
    pub(crate) fn emit_session_event(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        snapshot: AppSnapshot,
        message: String,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) && !matches!(kind, RuntimeEventKind::RuntimeClosed)
        {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let (error_code, error_message) = match error {
            Some(error) => (error.code().as_str().to_string(), error.to_string()),
            None => (String::new(), message),
        };
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id,
            turn_id,
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: PlatformRequest::default(),
            error_code,
            message: error_message,
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
    pub(crate) fn emit_error(&self, error: HamburError) -> HamburResult<()> {
        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        self.emit(RuntimeEventKind::RuntimeError, snapshot, Some(&error))
    }
    pub(crate) fn emit_markdown(
        &self,
        session_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        self.emit_markdown_for_turn(session_id, String::new(), markdown_render_update)
    }
    pub(crate) fn emit_markdown_for_turn(
        &self,
        session_id: String,
        turn_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let snapshot = self
            .tokio
            .block_on(async {
                if session_id.is_empty() {
                    self.database.bootstrap_snapshot().await
                } else {
                    self.database.session_snapshot(&session_id).await
                }
            })
            .unwrap_or_default();
        self.emit_markdown_event_with_snapshot(
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
        )
    }
    pub(crate) async fn emit_markdown_event_async(
        &self,
        session_id: String,
        turn_id: String,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }
        self.persist_markdown_update_for_timeline(&session_id, &turn_id, &markdown_render_update)
            .await?;
        let snapshot = if session_id.is_empty() {
            self.database.bootstrap_snapshot().await?
        } else {
            self.database.session_snapshot(&session_id).await?
        };
        self.emit_markdown_event_with_snapshot(
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
        )
    }
    pub(crate) fn emit_markdown_event_with_snapshot(
        &self,
        session_id: String,
        turn_id: String,
        snapshot: AppSnapshot,
        markdown_render_update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind: RuntimeEventKind::MarkdownRenderUpdate,
            session_id,
            turn_id,
            snapshot,
            markdown_render_update,
            platform_request: PlatformRequest::default(),
            error_code: String::new(),
            message: String::new(),
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
    pub(crate) fn emit_platform_request_event(
        &self,
        request: PlatformRequest,
        snapshot: AppSnapshot,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind: RuntimeEventKind::PlatformRequest,
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            snapshot,
            markdown_render_update: MarkdownRenderUpdate::default(),
            platform_request: request,
            error_code: String::new(),
            message: String::new(),
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }
    pub(crate) fn skills_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("skills")
    }
    pub(crate) fn memory_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("memory")
    }
    pub(crate) fn ensure_seeded_skills(&self) -> HamburResult<()> {
        seed_bundled_skills(&self.skills_root())
    }
    pub(crate) fn delete_skill_internal(&self, identifier: &str) -> HamburResult<String> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let disabled = self.disabled_skill_paths();
        let skill_file = self.resolve_skill_file(&root, identifier, &disabled)?;
        let skill_dir = skill_file.parent().ok_or_else(|| {
            HamburError::InvalidCommand(format!("skill directory not found: {identifier}"))
        })?;
        if !skill_dir.join("SKILL.md").is_file() {
            return Err(HamburError::InvalidCommand(format!(
                "skill not found: {identifier}"
            )));
        }
        let relative = relative_path(&root, &skill_file)?.replace('\\', "/");
        fs::remove_dir_all(skill_dir)
            .map_err(|error| HamburError::Internal(format!("delete skill directory: {error}")))?;
        Ok(relative)
    }
    pub(crate) fn resolve_skill_file(
        &self,
        root: &PathBuf,
        identifier: &str,
        disabled: &HashSet<String>,
    ) -> HamburResult<PathBuf> {
        let raw = identifier.trim();
        let normalized = raw
            .strip_prefix(SANDBOX_SKILLS_PATH)
            .unwrap_or(raw)
            .trim_start_matches('/')
            .strip_prefix("skills/")
            .unwrap_or_else(|| {
                raw.strip_prefix(SANDBOX_SKILLS_PATH)
                    .unwrap_or(raw)
                    .trim_start_matches('/')
            });
        if normalized.is_empty()
            || normalized.contains("..")
            || normalized.contains('\\')
            || normalized.starts_with('/')
        {
            return Err(HamburError::InvalidCommand(
                "invalid skill identifier".to_string(),
            ));
        }
        let candidates = [
            normalized.to_string(),
            format!("{normalized}/SKILL.md"),
            format!("{normalized}.md"),
        ];
        for candidate in candidates {
            let path = safe_join(root, &candidate)?;
            if path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            {
                return Ok(path);
            }
        }
        let lowered = normalized.to_ascii_lowercase();
        for skill in self.list_skills_with_disabled(disabled)? {
            let path_without_file = skill.path.trim_end_matches("/SKILL.md");
            if skill.name.eq_ignore_ascii_case(&lowered)
                || skill.name.eq_ignore_ascii_case(normalized)
                || path_without_file.eq_ignore_ascii_case(normalized)
                || path_without_file
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(normalized))
            {
                return safe_join(root, &skill.path);
            }
        }
        Err(HamburError::InvalidCommand(format!(
            "skill not found: {identifier}"
        )))
    }
    pub(crate) fn load_skill_from_file(
        &self,
        root: &PathBuf,
        skill_file: &PathBuf,
        disabled: &HashSet<String>,
    ) -> HamburResult<RuntimeSkillDetail> {
        let raw = fs::read_to_string(skill_file).map_err(|error| {
            HamburError::Internal(format!("read skill {}: {error}", skill_file.display()))
        })?;
        let path = relative_path(root, skill_file)?.replace('\\', "/");
        let skill_dir_path = path.trim_end_matches("/SKILL.md").to_string();
        let skill_dir = root.join(&skill_dir_path);
        let frontmatter = parse_frontmatter(&raw);
        let body = strip_frontmatter(&raw);
        let name = frontmatter
            .get("name")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_else(|| {
                skill_dir_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("skill")
                    .to_string()
            });
        let description = frontmatter
            .get("description")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default();
        let tags = frontmatter.get("tags").cloned().unwrap_or_default();
        let category = skill_dir_path
            .rsplit_once('/')
            .map(|(category, _)| category.to_string())
            .unwrap_or_default();
        let files = list_relative_files(&skill_dir)?;
        let modified_at_ms = files
            .iter()
            .filter_map(|file| fs::metadata(skill_dir.join(file)).ok())
            .filter_map(|metadata| metadata.modified().ok())
            .filter_map(system_time_to_ms)
            .max()
            .unwrap_or_default();
        let created_at_ms = fs::metadata(&skill_dir)
            .ok()
            .and_then(|metadata| metadata.created().ok())
            .and_then(system_time_to_ms)
            .unwrap_or(modified_at_ms);
        let linked_files_json = linked_skill_files_json(&skill_dir);
        Ok(RuntimeSkillDetail {
            summary: RuntimeSkillSummary {
                name,
                description: description
                    .chars()
                    .take(SKILL_MAX_DESCRIPTION_CHARS)
                    .collect(),
                path: path.clone(),
                category,
                tags,
                built_in: is_bundled_skill_path(&path),
                enabled: !disabled.contains(&path),
                created_at_ms,
                modified_at_ms,
                files,
            },
            content: body,
            raw_content: raw,
            skill_dir_path,
            linked_files_json,
            selected_file_path: String::new(),
            selected_file_content: String::new(),
        })
    }
    pub(crate) fn disabled_skill_paths(&self) -> HashSet<String> {
        self.tokio
            .block_on(self.database.settings_snapshot())
            .map(disabled_skill_paths_from_snapshot)
            .unwrap_or_default()
    }
    pub(crate) async fn disabled_skill_paths_async(&self) -> HashSet<String> {
        self.database
            .settings_snapshot()
            .await
            .map(disabled_skill_paths_from_snapshot)
            .unwrap_or_default()
    }
    pub(crate) fn disabled_tool_names(&self) -> HashSet<String> {
        self.safe_block_on(self.database.settings_snapshot())
            .map(|snapshot| disabled_tool_names_from_snapshot(&snapshot))
            .unwrap_or_default()
    }
    pub(crate) async fn disabled_tool_names_async(&self) -> HashSet<String> {
        self.database
            .settings_snapshot()
            .await
            .map(|snapshot| disabled_tool_names_from_snapshot(&snapshot))
            .unwrap_or_default()
    }
    pub(crate) fn compile_enabled_main_tools_json(&self) -> String {
        let disabled = self.disabled_tool_names();
        self.tools
            .schemas()
            .compile_openai_tools_json_excluding(&disabled)
    }
    pub(crate) fn compile_enabled_delegate_tools_json(&self) -> String {
        let disabled = self.disabled_tool_names();
        self.tools
            .schemas()
            .compile_delegate_openai_tools_json_excluding(&disabled)
    }
    pub(crate) fn build_skills_index_prompt(&self) -> String {
        let skills = self
            .list_skills_internal()
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();
        format_skills_index_prompt(skills)
    }
    pub(crate) async fn build_skills_index_prompt_async(&self) -> String {
        let disabled = self.disabled_skill_paths_async().await;
        let skills = self
            .list_skills_with_disabled(&disabled)
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();
        format_skills_index_prompt(skills)
    }
    pub(crate) fn build_memory_system_prompt(&self) -> String {
        self.memory_snapshot()
            .map(|snapshot| format_memory_system_prompt(&snapshot))
            .unwrap_or_default()
    }
    pub(crate) async fn build_memory_system_prompt_async(&self) -> String {
        self.memory_snapshot_async()
            .await
            .map(|snapshot| format_memory_system_prompt(&snapshot))
            .unwrap_or_default()
    }
    pub(crate) fn memory_snapshot(&self) -> HamburResult<MemorySnapshot> {
        let root = self.memory_root();
        memory_snapshot_from_root(&root)
    }
    pub(crate) async fn memory_snapshot_async(&self) -> HamburResult<MemorySnapshot> {
        let root = self.memory_root();
        tokio::task::spawn_blocking(move || memory_snapshot_from_root(&root))
            .await
            .map_err(|error| HamburError::Internal(format!("memory snapshot task: {error}")))?
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
            .clamp(1, 2000);
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
    pub(crate) fn memory_tool_result(&self, arguments: &Value) -> HamburResult<Value> {
        let target = arguments
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("memory");
        let target = if target.eq_ignore_ascii_case("user") {
            "user"
        } else {
            "memory"
        };
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let path = root.join(if target == "user" {
            USER_FILE_NAME
        } else {
            MEMORY_FILE_NAME
        });
        let mut entries = parse_memory_entries(&read_memory_file_content(&path)?);
        let result = match action {
            "read" => memory_response(true, target, &entries, "Entries loaded.", ""),
            "add" => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    memory_response(false, target, &entries, "", "Content cannot be empty.")
                } else if is_review_status_entry(&content) {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "Memory review status is not durable memory and must not be stored.",
                    )
                } else if entries.contains(&content) {
                    memory_response(
                        true,
                        target,
                        &entries,
                        "Entry already exists (no duplicate added).",
                        "",
                    )
                } else {
                    entries.push(content);
                    write_memory_entries(&path, &entries)?;
                    memory_response(true, target, &entries, "Entry added.", "")
                }
            }
            "replace" => {
                let old = arguments
                    .get("old_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if old.is_empty() || content.is_empty() {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "old_text and content are required.",
                    )
                } else {
                    let matches = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| entry.contains(old))
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    if matches.len() != 1 {
                        memory_response(
                            false,
                            target,
                            &entries,
                            "",
                            "Expected exactly one matching entry.",
                        )
                    } else {
                        entries[matches[0]] = content.to_string();
                        write_memory_entries(&path, &entries)?;
                        memory_response(true, target, &entries, "Entry replaced.", "")
                    }
                }
            }
            "remove" => {
                let old = arguments
                    .get("old_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let matches = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| entry.contains(old))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if old.is_empty() || matches.len() != 1 {
                    memory_response(
                        false,
                        target,
                        &entries,
                        "",
                        "Expected exactly one matching entry.",
                    )
                } else {
                    entries.remove(matches[0]);
                    write_memory_entries(&path, &entries)?;
                    memory_response(true, target, &entries, "Entry removed.", "")
                }
            }
            _ => memory_response(false, target, &entries, "", "Unknown memory action."),
        };
        Ok(result)
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
