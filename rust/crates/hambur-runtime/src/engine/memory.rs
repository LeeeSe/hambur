use crate::*;

impl RuntimeEngine {
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
                .unwrap_or_default();
            for session in sessions {
                engine
                    .clone()
                    .maybe_spawn_memory_review_for_session(session.id, "app_startup");
            }
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
        let review = match self.database.session_review_record(&session_id) {
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
                .mark_session_memory_reviewed(&session_id, true);
            return;
        }
        if review.memory_reviewed {
            return;
        }
        if review.messages.is_empty() && review.trace_spans.is_empty() {
            let _ = self
                .database
                .mark_session_memory_reviewed(&session_id, true);
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
                .mark_session_memory_reviewed(&session_id, true);
                        let _ = self.emit_plain(
                RuntimeEventKind::SettingsChanged,
                session_id,
                String::new(),
                "Memory reviewed".to_string(),
            );
        }
    }

    pub(crate) async fn run_automatic_memory_review(
        self: Arc<Self>,
        review: SessionReviewRecord,
        reason: &str,
    ) -> HamburResult<bool> {
        let routes = self.database.memory_review_route()?;
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
        let memory_snapshot = self.memory_snapshot().unwrap_or_default();
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
                ..Default::default()
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
                let result = self.execute_memory_review_tool(invocation);
                messages.push(ModelMessage {
                    role: "tool".to_string(),
                    content: result.context_stub,
                    reasoning_content: String::new(),
                    tool_calls_json: String::new(),
                    tool_call_id: result.tool_call_id,
                    ..Default::default()
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

    pub(crate) fn memory_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("memory")
    }

    pub(crate) fn build_memory_system_prompt(&self) -> String {
        self.memory_snapshot()
            .map(|snapshot| format_memory_system_prompt(&snapshot))
            .unwrap_or_default()
    }

    pub(crate) fn memory_snapshot(&self) -> HamburResult<MemorySnapshot> {
        memory_snapshot_from_root(&self.memory_root())
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
