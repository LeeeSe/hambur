use crate::*;

impl RuntimeEngine {
    pub(crate) fn execute_send_message(
        &self,
        command: RuntimeCommand,
        command_kind: &'static str,
    ) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let content = match self.resolve_turn_content(&command, command_kind) {
            Ok(content) => content,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let send_options = SendOptions::parse(&command.payload_json);
        let attachment_ids = send_options.attachment_ids.clone();
        if content.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("message content must not be empty".to_string()),
            );
        }

        let pending_attachments =
            match self.load_pending_attachments(&command.session_id, &attachment_ids) {
                Ok(attachments) => attachments,
                Err(error) => {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
        if self.active_turn_for_session(&command.session_id).is_some() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::SessionBusy(command.session_id.clone()),
            );
        }

        let routes = match self.database.primary_chat_route() {
            Ok(routes) => routes,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let mut plan = route_plan_from_records(routes);
        let requirements = RouteRequirements {
            requires_tool_protocol: send_options.search_enabled,
            requires_image_input: false,
            requires_structured_output: false,
        };
        plan = match self
            .router
            .lock()
            .map_err(|_| HamburError::Internal("router registry poisoned".to_string()))
            .and_then(|mut router| router.resolve(plan, requirements))
        {
            Ok(plan) => plan,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let route_snapshots = plan
            .targets
            .iter()
            .map(route_snapshot_from_target)
            .collect::<Vec<_>>();
        let Some(route) = route_snapshots.first().cloned() else {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::ModelUnavailable("route has no usable target".to_string()),
            );
        };

        let visible_user_content = content.clone();
        let can_reuse_source_user = matches!(command_kind, "RetryTurn" | "RegenerateMessage")
            && command.content.trim().is_empty()
            && command.chunk.trim().is_empty()
            && pending_attachments.is_empty();
        let setup = self.tokio.block_on(async {
            let reusable_user_message = self
                .prepare_visible_branch_for_command(&command, command_kind)?;
            let turn = self
                .database
                .create_turn_with_route(&command.session_id, "StreamingAssistant", &route)?;
            let res = async {
                let reuse_existing_user = can_reuse_source_user && reusable_user_message.is_some();
                let user_message = if reuse_existing_user {
                    reusable_user_message.expect("checked reusable user message")
                } else {
                    let prompt_prefix = format!(
                        "[Current Time: {}]",
                        format_beijing_timestamp_with_weekday(now_ms())
                    );
                    let user_message = self
                        .database
                        .insert_message_with_route_and_prefix(
                            &command.session_id,
                            "user",
                            &visible_user_content,
                            "",
                            "completed",
                            &turn.id,
                            &route,
                            &prompt_prefix,
                        )?;
                    let attachment_ids = pending_attachments
                        .iter()
                        .map(|attachment| attachment.id.clone())
                        .collect::<Vec<_>>();
                    self.database
                        .attach_pending_to_message(
                            &command.session_id,
                            &user_message.id,
                            &attachment_ids,
                        )?;
                    self.database
                        .upsert_timeline_item(
                            &command.session_id,
                            NewTimelineItem {
                                stable_key: user_message.id.clone(),
                                content_type: "user_message".to_string(),
                                display_sequence: user_message.created_at_ms,
                                payload_ref: user_message.id.clone(),
                                small_summary: visible_user_content.chars().take(160).collect(),
                                kind: if command_kind == "EditMessage" {
                                    "EditedUserMessage".to_string()
                                } else {
                                    "UserMessage".to_string()
                                },
                            },
                        )?;
                    self.maybe_title_session_from_first_user_message(
                        &command.session_id,
                        &visible_user_content,
                    )?;
                    user_message
                };
                let assistant_message = self
                    .database
                    .insert_message_with_route(
                        &command.session_id,
                        "assistant",
                        "",
                        "",
                        "streaming",
                        &turn.id,
                        &route,
                    )?;
                let engine = self.self_ref.lock().ok().and_then(|value| value.upgrade()).ok_or_else(|| {
                    HamburError::Internal("runtime self reference unavailable".to_string())
                })?;
                engine.insert_initial_pending_markdown_block(
                    &command.session_id,
                    &turn.id,
                    &assistant_message.id,
                )?;
                let model_user_content = format_user_content_with_prefix(
                    &user_message.prompt_prefix,
                    &content,
                    &pending_attachments,
                );
                let chat_context = self
                    .build_chat_context_messages(
                        &command.session_id,
                        &user_message,
                        &pending_attachments,
                        &route,
                        true,
                        &model_user_content,
                    )?;
                let snapshot = self.database.session_snapshot(&command.session_id)?;
                let disabled_tools = self.disabled_tool_names();
                let tools_json = self.tools.schemas().compile_openai_tools_json_excluding(&disabled_tools);
                let skills_index_prompt = self.build_skills_index_prompt();
                let memory_system_prompt = self.build_memory_system_prompt();
                Ok::<_, HamburError>((
                    turn.clone(),
                    user_message,
                    assistant_message,
                    chat_context,
                    snapshot,
                    !reuse_existing_user,
                    tools_json,
                    skills_index_prompt,
                    memory_system_prompt,
                ))
            }.await;

            if let Err(ref e) = res {
                let _ = self.database.fail_turn(&turn.id, "Failed", "SetupError", &e.to_string());
            }
            res
        });

        let (
            turn,
            _user_message,
            assistant_message,
            chat_context,
            snapshot,
            user_was_inserted,
            tools_json,
            skills_index_prompt,
            memory_system_prompt,
        ) = match setup {
            Ok(value) => value,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };

        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut active_turns) = self.active_turns.lock() {
            if let Some(prev) = active_turns.insert(
                command.session_id.clone(),
                ActiveTurn {
                    turn_id: turn.id.clone(),
                    cancel: cancel.clone(),
                },
            ) {
                prev.cancel.store(true, Ordering::SeqCst);
            }
        }

        let _ = self.emit_with_snapshot(
            RuntimeEventKind::TurnStarted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot.clone(),
            command_kind.to_string(),
            None,
        );
        if user_was_inserted {
            let _ = self.emit_with_snapshot(
                RuntimeEventKind::MessageUpserted,
                command.session_id.clone(),
                turn.id.clone(),
                snapshot.clone(),
                visible_user_content.clone(),
                None,
            );
        }
        let _ = self.emit_with_snapshot(
            RuntimeEventKind::AssistantMessageStarted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot,
            route.model_display_name.clone(),
            None,
        );

        let Some(engine) = self.self_ref.lock().ok().and_then(|value| value.upgrade()) else {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::Internal("runtime self reference unavailable".to_string()),
            );
        };
        let fallback_policy = plan.fallback_policy;
        let stream_sources_by_route = route_snapshots
            .iter()
            .map(|route| {
                stream_source_for_command(
                    &command,
                    &turn.id,
                    &content,
                    chat_context.clone(),
                    route,
                    &tools_json,
                    &skills_index_prompt,
                    &memory_system_prompt,
                    send_options.deep_thinking_enabled,
                    send_options.search_enabled,
                )
            })
            .collect::<Vec<_>>();
        let handle = self.tokio.handle().clone();
        handle.spawn(async move {
            engine
                .run_chat_turn(
                    command.session_id,
                    turn.id,
                    assistant_message.id,
                    route_snapshots,
                    fallback_policy,
                    cancel,
                    stream_sources_by_route,
                    0,
                )
                .await;
        });

        accepted_ack(command.command_id, command.idempotency_key)
    }

    pub(crate) fn execute_cancel_turn(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let active = self.active_turns.lock().ok().and_then(|turns| {
            turns.iter().find_map(|(session_id, active)| {
                let session_matches =
                    command.session_id.is_empty() || command.session_id == *session_id;
                let turn_matches = command.turn_id.is_empty() || command.turn_id == active.turn_id;
                if session_matches && turn_matches {
                    Some(active.clone())
                } else {
                    None
                }
            })
        });

        if let Some(active) = active {
            active.cancel.store(true, Ordering::SeqCst);
        }

        accepted_ack(command.command_id, command.idempotency_key)
    }

    pub(crate) fn execute_append_markdown_delta(
        &self,
        command: RuntimeCommand,
    ) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if command.message_id.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("message_id must not be empty".to_string()),
            );
        }

        let stream_key = format!("{}:{}", command.session_id, command.message_id);
        let result = {
            let mut streams = match self.markdown_streams.lock() {
                Ok(streams) => streams,
                Err(_) => {
                    return rejected_ack(
                        command.command_id,
                        command.idempotency_key,
                        HamburError::Internal("markdown stream registry poisoned".to_string()),
                    );
                }
            };
            let pipeline = streams
                .entry(stream_key.clone())
                .or_insert_with(|| MarkdownPipeline::new(command.message_id.clone()));
            let update = if command.chunk.is_empty() {
                MarkdownRenderUpdate {
                    message_id: command.message_id.clone(),
                    ..Default::default()
                }
            } else {
                pipeline.append(&command.chunk)
            };
            let final_update = if command.finalize {
                Some(pipeline.finalize())
            } else {
                None
            };
            if command.finalize {
                streams.remove(&stream_key);
            }
            (update, final_update)
        };

        if !result.0.committed_nodes.is_empty()
            || result.0.pending_node.is_some()
            || result.0.reset
            || !result.0.invalidated_block_ids.is_empty()
        {
            let _ = self.emit_markdown(command.session_id.clone(), String::new(), result.0);
        }
        if let Some(update) = result.1
            && (!update.committed_nodes.is_empty()
                || update.pending_node.is_some()
                || update.reset
                || !update.invalidated_block_ids.is_empty())
        {
            let _ = self.emit_markdown(command.session_id.clone(), String::new(), update);
        }

        accepted_ack(command.command_id, command.idempotency_key)
    }

    pub(crate) fn execute_submit_platform_result(
        &self,
        command: RuntimeCommand,
    ) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let payload = config_payload_value(&command.payload_json);
        let request_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "requestId"))
            .if_blank(config_string(&payload, "request_id"));
        let Some(sender) = self
            .platform_requests
            .lock()
            .ok()
            .and_then(|mut requests| requests.remove(&request_id))
        else {
            return accepted_ack(command.command_id, command.idempotency_key);
        };

        let result = PlatformResultPayload {
            is_error: config_bool(&payload, "isError", false),
            payload_json: config_value_string(&payload, "payloadJson"),
            error_code: config_string(&payload, "errorCode"),
            message: config_string(&payload, "message"),
        };
        let _ = sender.send(result);
        accepted_ack(command.command_id, command.idempotency_key)
    }
}
