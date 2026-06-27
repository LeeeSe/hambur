use crate::*;

impl RuntimeEngine {
    pub fn dispatch(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        let command = normalize_command(command);
        let validation = validate_command(&command);
        if let Err(error) = validation {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let key = command.idempotency_key.clone();
        if let Some(previous) = self
            .idempotency
            .lock()
            .ok()
            .and_then(|registry| registry.get(&key).cloned())
        {
            return RuntimeCommandAck {
                duplicate: true,
                ..previous
            };
        }

        let ack = self.handle_command(command);
        if let Ok(mut registry) = self.idempotency.lock() {
            registry.insert(key, ack.clone());
        }
        ack
    }

    pub fn create_session(&self, title: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "CreateSession".to_string(),
            title,
            idempotency_key: new_id("idem_session_create"),
            ..RuntimeCommand::default()
        })
    }

    pub fn open_session(&self, session_id: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "OpenSession".to_string(),
            session_id: session_id.clone(),
            idempotency_key: format!("{session_id}:open:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        })
    }

    pub fn delete_session(&self, session_id: String) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "SoftDeleteSession".to_string(),
            session_id: session_id.clone(),
            idempotency_key: format!("{session_id}:soft-delete:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        })
    }

    pub fn append_markdown_delta(
        &self,
        session_id: String,
        message_id: String,
        chunk: String,
        finalize: bool,
    ) -> RuntimeCommandAck {
        self.dispatch(RuntimeCommand {
            kind: "AppendMarkdownDelta".to_string(),
            session_id,
            message_id: message_id.clone(),
            chunk,
            finalize,
            idempotency_key: format!("markdown:{message_id}:{}", new_id("delta")),
            ..RuntimeCommand::default()
        })
    }

    pub(crate) fn handle_command(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        match command.kind.as_str() {
            "Initialize" => accepted_ack(command.command_id, command.idempotency_key),
            "Shutdown" => {
                self.shutdown();
                accepted_ack(command.command_id, command.idempotency_key)
            }
            "CreateSession" => self.execute_create_session(command),
            "OpenSession" => self.execute_open_session(command),
            "RenameSession" | "UpdateSessionTitle" => self.execute_rename_session(command),
            "SetSessionPinned" | "PinSession" | "UnpinSession" => {
                self.execute_set_session_pinned(command)
            }
            "DeleteSession" | "SoftDeleteSession" | "HardPurgeSession" => {
                self.execute_delete_session(command)
            }
            "UpdateProvider" => self.execute_update_provider(command),
            "DeleteProvider" => self.execute_delete_provider(command),
            "RefreshProviderModels" => self.execute_refresh_provider_models(command),
            "UpdateModelOverride" | "UpdateModelDetail" => {
                self.execute_update_model_override(command)
            }
            "UpdateModelGroup" => self.execute_update_model_group(command),
            "UpdateModelGroupMember" => self.execute_update_model_group_member(command),
            "SetDefaultModelGroup" => self.execute_set_default_model_group(command),
            "DeleteModelGroup" => self.execute_delete_model_group(command),
            "DeleteModelGroupMember" => self.execute_delete_model_group_member(command),
            "UpdateDefaultModelGroups" => self.execute_update_default_model_groups(command),
            "UpdateToolSettings"
            | "UpdateSkills"
            | "UpdateMemoryProjections"
            | "UpdateStartupTasks"
            | "UpdateRootfsSettings"
            | "UpdateAppearance"
            | "UpdateLogs"
            | "UpdateTokenUsage"
            | "UpdatePersona"
            | "UpdateEnvironmentVariables"
            | "UpdateAppSetting"
            | "UpdateBrowserToolSettings"
            | "UpdateSkillEnabled"
            | "UpdateStartupTask"
            | "DeleteStartupTask"
            | "UpdateRootfsSetting" => self.execute_update_app_setting(command),
            "DeleteSkill" => self.execute_delete_skill(command),
            "ImportAttachmentFromUri" => self.execute_import_attachment(command),
            "RemovePendingAttachment" => self.execute_remove_pending_attachment(command),
            "ClearPendingAttachments" => self.execute_clear_pending_attachments(command),
            "SendMessage" => self.execute_send_message(command, "SendMessage"),
            "RetryTurn" => self.execute_send_message(command, "RetryTurn"),
            "RegenerateMessage" => self.execute_send_message(command, "RegenerateMessage"),
            "EditMessage" => self.execute_send_message(command, "EditMessage"),
            "CancelTurn" => self.execute_cancel_turn(command),
            "SubmitPlatformResult" => self.execute_submit_platform_result(command),
            "RunRootfsWarmup" | "ResetRootfs" => self.execute_rootfs_lifecycle(command),
            "AppendMarkdownDelta" | "MarkdownRenderUpdate" => {
                self.execute_append_markdown_delta(command)
            }
            _ => rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand(format!("unsupported command kind: {}", command.kind)),
            ),
        }
    }
    pub(crate) fn execute_create_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let previous_session_id = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self
            .tokio
            .block_on(self.database.create_session(&command.title))
        {
            Ok(snapshot) => {
                if !snapshot.selected_session_id.trim().is_empty()
                    && let Err(error) = self.sandbox.prepare_session(&snapshot.selected_session_id)
                {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
                let new_session_id = snapshot.selected_session_id.clone();
                let _ = self.emit(RuntimeEventKind::SessionCreated, snapshot, None);
                self.spawn_memory_review_if_session_changed(
                    previous_session_id,
                    new_session_id,
                    "session_switch",
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
    pub(crate) fn execute_open_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let previous_session_id = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self
            .tokio
            .block_on(self.database.open_session(&command.session_id))
        {
            Ok(snapshot) => {
                let _ = self.emit(RuntimeEventKind::SessionOpened, snapshot, None);
                self.spawn_memory_review_if_session_changed(
                    previous_session_id,
                    command.session_id.clone(),
                    "session_switch",
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
    pub(crate) fn execute_rename_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let title = command
            .title
            .clone()
            .if_blank(config_payload_string(&command.payload_json, "title"))
            .if_blank(command.content.clone())
            .if_blank(command.chunk.clone());
        match self
            .tokio
            .block_on(self.database.rename_session(&command.session_id, &title))
        {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SessionRenamed,
                    command.session_id,
                    String::new(),
                    snapshot,
                    "Session renamed".to_string(),
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
    pub(crate) fn execute_set_session_pinned(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let pinned = if command.kind == "PinSession" {
            true
        } else if command.kind == "UnpinSession" {
            false
        } else {
            config_bool(&payload, "pinned", false)
        };
        match self.tokio.block_on(
            self.database
                .set_session_pinned(&command.session_id, pinned),
        ) {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SessionPinnedChanged,
                    command.session_id,
                    String::new(),
                    snapshot,
                    pinned.to_string(),
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
    pub(crate) fn execute_delete_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        match self
            .tokio
            .block_on(self.database.delete_session(&command.session_id))
        {
            Ok(snapshot) => {
                self.kill_processes_for_session(&command.session_id);
                let _ = self.emit(RuntimeEventKind::SessionDeleted, snapshot, None);
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
    pub(crate) fn execute_update_provider(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let result = self.tokio.block_on(async {
            let provider = self
                .database
                .upsert_provider(ProviderUpsert {
                    id: provider_id.clone(),
                    name: command.title.clone(),
                    icon_name: config_payload_string(&command.payload_json, "iconName")
                        .if_blank("sparkles".to_string()),
                    api_type: OPENAI_COMPATIBLE_PROTOCOL.to_string(),
                    base_url: command.chunk.clone(),
                    secret_ref: provider_secret_ref_from_payload(&command.payload_json),
                    enabled: config_payload_bool(&command.payload_json, "enabled", true),
                })
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateProvider",
                    "provider",
                    &provider.id,
                    &format!(
                        "Provider '{}' saved with redacted secret ({})",
                        provider.name,
                        redacted_secret_label(&provider.secret_ref)
                    ),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    "Provider updated".to_string(),
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
    pub(crate) fn execute_delete_provider(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if let Err(error) = require_approval(&command, "delete-provider") {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let result = self.tokio.block_on(async {
            self.database.delete_provider(&provider_id).await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteProvider",
                    "provider",
                    &provider_id,
                    "Provider deleted after explicit approval",
                    true,
                    &approval_token_from_payload(&command.payload_json),
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    snapshot,
                    "Provider deleted".to_string(),
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
    pub(crate) fn execute_refresh_provider_models(
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

        let provider_id = command
            .provider_id
            .clone()
            .if_blank(command.message_id.clone());
        let payload = config_payload_value(&command.payload_json);
        let models_response =
            self.tokio
                .block_on(self.provider_models_response(&provider_id, &command, &payload));
        let models_response = match models_response {
            Ok(response) => response,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let mut models =
            match OpenAiCompatibleAdapter::parse_models_response(&provider_id, &models_response) {
                Ok(models) => models,
                Err(error) => {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
        self.tokio
            .block_on(self.enrich_provider_models_from_catalog(&mut models));
        let upserts = models
            .into_iter()
            .map(|model| ProviderModelUpsert {
                model_id: model.model_id,
                display_name: model.display_name,
                supports_tool_call: model.capabilities.supports_tool_call,
                supports_reasoning: model.capabilities.supports_reasoning,
                supports_image_input: model.capabilities.supports_image_input,
                supports_structured_output: model.capabilities.supports_structured_output,
                supports_temperature: model.capabilities.supports_temperature,
                context_limit: model.capabilities.context_limit,
                output_limit: model.capabilities.output_limit,
                reasoning_field: model.capabilities.reasoning_field,
                metadata_json: model.metadata_json,
            })
            .collect::<Vec<_>>();

        match self
            .tokio
            .block_on(self.database.replace_provider_models(&provider_id, upserts))
        {
            Ok(models) => {
                for (index, model) in models.iter().enumerate() {
                    let _ = self
                        .tokio
                        .block_on(self.database.upsert_primary_chat_member(
                            &provider_id,
                            &model.model_id,
                            index as u32,
                        ));
                }
                let _ = self.tokio.block_on(self.database.insert_config_audit(
                    &command.command_id,
                    "user",
                    "RefreshProviderModels",
                    "provider",
                    &provider_id,
                    &format!("{} provider models refreshed", models.len()),
                    false,
                    "",
                ));
                let snapshot = self
                    .tokio
                    .block_on(self.database.bootstrap_snapshot())
                    .unwrap_or_default();
                let _ = self.emit_session_event(
                    RuntimeEventKind::ModelsUpdated,
                    String::new(),
                    String::new(),
                    snapshot,
                    format!("{} models refreshed", models.len()),
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
    pub(crate) fn execute_update_model_override(
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
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(config_string(&payload, "providerId").if_blank(command.message_id.clone()));
        let model_id = command
            .model_id
            .clone()
            .if_blank(config_string(&payload, "modelId"));
        let result = self.tokio.block_on(async {
            let model = self
                .database
                .upsert_provider_model_override(ProviderModelOverride {
                    provider_id: provider_id.clone(),
                    model_id: model_id.clone(),
                    display_name: command
                        .title
                        .clone()
                        .if_blank(config_string(&payload, "displayName")),
                    supports_tool_call: config_bool(&payload, "supportsToolCall", true),
                    supports_reasoning: config_bool(&payload, "supportsReasoning", true),
                    supports_image_input: config_bool(&payload, "supportsImageInput", false),
                    supports_structured_output: config_bool(
                        &payload,
                        "supportsStructuredOutput",
                        false,
                    ),
                    supports_temperature: config_bool(&payload, "supportsTemperature", true),
                    context_limit: config_u32(&payload, "contextLimit", 32000),
                    output_limit: config_u32(&payload, "outputLimit", 4096),
                    reasoning_field: config_string(&payload, "reasoningField"),
                    metadata_json: config_object_string(&payload, "metadataJson"),
                })
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelOverride",
                    "provider_model",
                    &format!("{}/{}", model.provider_id, model.model_id),
                    &format!("Model '{}' overrides saved", model.display_name),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model overrides updated")
    }
    pub(crate) fn execute_update_model_group(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let payload = config_payload_value(&command.payload_json);
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            let group = self
                .database
                .upsert_model_group(
                    &group_id,
                    &command
                        .title
                        .clone()
                        .if_blank(config_string(&payload, "name")),
                    &config_string(&payload, "routingStrategy"),
                    &config_string(&payload, "fallbackPolicy"),
                )
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelGroup",
                    "model_group",
                    &group.id,
                    &format!("Model group '{}' saved", group.name),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group updated")
    }
    pub(crate) fn execute_update_model_group_member(
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
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"))
            .if_blank("grp_primary_chat".to_string());
        let provider_id = command
            .provider_id
            .clone()
            .if_blank(config_string(&payload, "providerId"));
        let model_id = command
            .model_id
            .clone()
            .if_blank(config_string(&payload, "modelId"));
        let position = config_u32(&payload, "position", 0);
        let enabled = config_bool(&payload, "enabled", true);
        let result = self.tokio.block_on(async {
            let member = self
                .database
                .upsert_model_group_member(&group_id, &provider_id, &model_id, position, enabled)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateModelGroupMember",
                    "model_group_member",
                    &format!(
                        "{}/{}/{}",
                        member.group_id, member.provider_id, member.model_id
                    ),
                    &format!("Model group member enabled={}", member.enabled),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group member updated")
    }
    pub(crate) fn execute_set_default_model_group(
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
        let key = command
            .chunk
            .clone()
            .if_blank(config_string(&payload, "key"));
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            let default = self
                .database
                .set_default_model_group(&key, &group_id)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "SetDefaultModelGroup",
                    "default_model_group",
                    &default.key,
                    &format!("Default group '{}' -> '{}'", default.key, default.group_id),
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Default model group updated")
    }
    pub(crate) fn execute_update_default_model_groups(
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
        let primary_group_id = config_string(&payload, "primaryGroupId")
            .if_blank(config_string(&payload, "primary_group_id"))
            .if_blank(config_string(&payload, "primary"))
            .if_blank(command.message_id.clone());
        let secondary_group_id = config_string(&payload, "secondaryGroupId")
            .if_blank(config_string(&payload, "secondary_group_id"))
            .if_blank(config_string(&payload, "secondary"));
        let result = self.tokio.block_on(async {
            if !primary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("primary", &primary_group_id)
                    .await?;
            }
            if !secondary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("secondary", &secondary_group_id)
                    .await?;
            }
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "UpdateDefaultModelGroups",
                    "default_model_groups",
                    "default_model_groups",
                    "Default model groups updated",
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Default model groups updated")
    }
    pub(crate) fn execute_delete_model_group(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if let Err(error) = require_approval(&command, "delete-model-group") {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let payload = config_payload_value(&command.payload_json);
        let group_id = command
            .message_id
            .clone()
            .if_blank(config_string(&payload, "groupId"));
        let result = self.tokio.block_on(async {
            self.database.delete_model_group(&group_id).await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteModelGroup",
                    "model_group",
                    &group_id,
                    "Model group deleted after explicit approval",
                    true,
                    &approval_token_from_payload(&command.payload_json),
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group deleted")
    }
    pub(crate) fn execute_delete_model_group_member(
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
        let group_id = command.message_id.clone();
        let provider_id = command.provider_id.clone();
        let model_id = command.model_id.clone();
        let result = self.tokio.block_on(async {
            self.database
                .delete_model_group_member(&group_id, &provider_id, &model_id)
                .await?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteModelGroupMember",
                    "model_group_member",
                    &format!("{}/{}/{}", group_id, provider_id, model_id),
                    "Model group member deleted",
                    false,
                    "",
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Model group member deleted")
    }
    pub(crate) fn execute_update_app_setting(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let setting_key = match setting_key_for_command(&command) {
            Ok(setting_key) => setting_key,
            Err(error) => {
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        if setting_requires_approval(&setting_key)
            && let Err(error) = require_approval(&command, &setting_key)
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }
        let setting_value = setting_value_for_command(&command);
        let result = self.tokio.block_on(async {
            let setting = if command.kind == "DeleteStartupTask" {
                self.database.delete_app_setting(&setting_key).await?;
                hambur_db::AppSettingRecord {
                    key: setting_key.clone(),
                    value: String::new(),
                    updated_at_ms: now_ms(),
                }
            } else {
                self.database
                    .upsert_app_setting(&setting_key, &setting_value)
                    .await?
            };
            let approval_token = approval_token_from_payload(&command.payload_json);
            let approval_required = setting_requires_approval(&setting.key);
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    command.kind.as_str(),
                    "app_setting",
                    &setting.key,
                    &setting_audit_summary(&command.kind, &setting.key),
                    approval_required,
                    &approval_token,
                )
                .await?;
            self.database.bootstrap_snapshot().await
        });
        self.finish_settings_command(command, result, "Setting updated")
    }
    pub(crate) fn execute_delete_skill(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        let identifier = command
            .message_id
            .clone()
            .if_blank(config_payload_string(&command.payload_json, "skillId"))
            .if_blank(config_payload_string(&command.payload_json, "skillPath"));
        let result = self
            .delete_skill_internal(&identifier)
            .and_then(|deleted_path| {
                self.tokio.block_on(async {
                    self.database
                        .delete_app_setting(&format!("skill_enabled:{deleted_path}"))
                        .await
                        .ok();
                    self.database
                        .insert_config_audit(
                            &command.command_id,
                            "user",
                            "DeleteSkill",
                            "skill",
                            &deleted_path,
                            "Skill directory deleted",
                            false,
                            "",
                        )
                        .await?;
                    self.database.bootstrap_snapshot().await
                })
            });
        self.finish_settings_command(command, result, "Skill deleted")
    }
    pub(crate) fn execute_import_attachment(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let metadata = match AttachmentImportPayload::parse(&command.payload_json) {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let display_name = metadata.display_name.if_blank("attachment".to_string());
        let reserved = match self
            .filestore
            .reserve_session_attachment(&command.session_id, &display_name)
        {
            Ok(reserved) => reserved,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        let attachment_path = match self.sandbox.resolve(
            &command.session_id,
            &reserved.sandbox_path,
            SandboxAccess::Read,
        ) {
            Ok(resolved) => resolved.host_path,
            Err(error) => {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        };
        if let Some(parent) = attachment_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            let error = HamburError::Internal(format!("create attachment parent: {error}"));
            let _ = self.emit_error(error.clone());
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        if !metadata.bytes_base64.trim().is_empty() {
            let bytes = match BASE64_STANDARD.decode(metadata.bytes_base64.as_bytes()) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let error =
                        HamburError::InvalidCommand(format!("attachment bytes base64: {error}"));
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };
            if let Err(error) = fs::write(&attachment_path, bytes) {
                let error = HamburError::Internal(format!("write attachment bytes: {error}"));
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        } else if !metadata.source_path.trim().is_empty() {
            if let Err(error) = fs::copy(&metadata.source_path, &attachment_path)
                .map(|_| ())
                .map_err(|error| HamburError::Internal(format!("copy attachment source: {error}")))
            {
                let _ = self.emit_error(error.clone());
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        } else if !attachment_path.exists()
            && let Err(error) = fs::write(&attachment_path, [])
        {
            let error = HamburError::Internal(format!("create attachment placeholder: {error}"));
            let _ = self.emit_error(error.clone());
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let _ = fs::copy(&attachment_path, &reserved.host_path);
        let byte_size = if attachment_path.exists() {
            fs::metadata(&attachment_path)
                .map(|metadata| metadata.len())
                .unwrap_or(metadata.byte_size)
        } else {
            metadata.byte_size
        };
        let result = self.tokio.block_on(async {
            self.database
                .upsert_file_record(NewFileRecord {
                    id: reserved.file_id.clone(),
                    scope: "session".to_string(),
                    session_id: command.session_id.clone(),
                    relative_path: reserved.relative_path.clone(),
                    sandbox_path: reserved.sandbox_path.clone(),
                    mime_type: metadata.mime_type.clone(),
                    byte_size,
                    sha256: metadata.sha256.clone(),
                    retention_policy: "delete_with_session".to_string(),
                })
                .await?;
            let attachment = self
                .database
                .create_pending_attachment(NewAttachment {
                    id: String::new(),
                    session_id: command.session_id.clone(),
                    message_id: String::new(),
                    kind: metadata.kind.clone(),
                    display_name,
                    mime_type: metadata.mime_type.clone(),
                    byte_size,
                    origin_type: metadata.origin_type.clone(),
                    original_uri: metadata.original_uri.clone(),
                    file_id: reserved.file_id,
                    sandbox_path: reserved.sandbox_path,
                    width: metadata.width,
                    height: metadata.height,
                    sha256: metadata.sha256,
                    status: "pending".to_string(),
                })
                .await?;
            let snapshot = self.database.session_snapshot(&command.session_id).await?;
            Ok::<_, HamburError>((attachment, snapshot))
        });

        match result {
            Ok((attachment, snapshot)) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::AttachmentImported,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    attachment.display_name,
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
    pub(crate) fn execute_remove_pending_attachment(
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
        let attachment_id = command.message_id.clone().if_blank(command.chunk.clone());
        if attachment_id.trim().is_empty() {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::InvalidCommand("attachment_id must not be empty".to_string()),
            );
        }

        let result = self.tokio.block_on(async {
            let (_attachment, cleanup) = self
                .database
                .remove_pending_attachment(&command.session_id, &attachment_id)
                .await?;
            if let Some(job) = cleanup {
                let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                let _ = self.database.mark_file_cleanup_done(&job.id).await;
            }
            self.database.session_snapshot(&command.session_id).await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::PendingAttachmentRemoved,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    attachment_id,
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
    pub(crate) fn execute_clear_pending_attachments(
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
        let result = self.tokio.block_on(async {
            let pending = self
                .database
                .pending_attachments_for_session(&command.session_id)
                .await?;
            for attachment in pending {
                let (_removed, cleanup) = self
                    .database
                    .remove_pending_attachment(&command.session_id, &attachment.id)
                    .await?;
                if let Some(job) = cleanup {
                    let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                    let _ = self.database.mark_file_cleanup_done(&job.id).await;
                }
            }
            self.database.session_snapshot(&command.session_id).await
        });

        match result {
            Ok(snapshot) => {
                let _ = self.emit_session_event(
                    RuntimeEventKind::PendingAttachmentsCleaned,
                    command.session_id.clone(),
                    String::new(),
                    snapshot,
                    "Pending attachments cleared".to_string(),
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

        let mut content = match self.resolve_turn_content(&command, command_kind) {
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

        let routes = match self.tokio.block_on(self.database.primary_chat_route()) {
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
        let model_user_content = format_user_content_with_attachments(&content, &pending_attachments);
        let can_reuse_source_user = matches!(command_kind, "RetryTurn" | "RegenerateMessage")
            && command.content.trim().is_empty()
            && command.chunk.trim().is_empty()
            && pending_attachments.is_empty();
        let setup = self.tokio.block_on(async {
            let reusable_user_message = self
                .prepare_visible_branch_for_command(&command, command_kind)
                .await?;
            let turn = self
                .database
                .create_turn_with_route(&command.session_id, "StreamingAssistant", &route)
                .await?;
            let reuse_existing_user = can_reuse_source_user && reusable_user_message.is_some();
            let user_message = if reuse_existing_user {
                reusable_user_message.expect("checked reusable user message")
            } else {
                let user_message = self
                    .database
                    .insert_message_with_route(
                        &command.session_id,
                        "user",
                        &visible_user_content,
                        "",
                        "completed",
                        &turn.id,
                        &route,
                    )
                    .await?;
                let attachment_ids = pending_attachments
                    .iter()
                    .map(|attachment| attachment.id.clone())
                    .collect::<Vec<_>>();
                self.database
                    .attach_pending_to_message(
                        &command.session_id,
                        &user_message.id,
                        &attachment_ids,
                    )
                    .await?;
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
                    )
                    .await?;
                self.maybe_title_session_from_first_user_message(
                    &command.session_id,
                    &visible_user_content,
                )
                .await?;
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
                )
                .await?;
            let chat_context = self
                .build_chat_context_messages(
                    &command.session_id,
                    &user_message,
                    &pending_attachments,
                    &route,
                    true,
                    &model_user_content,
                )
                .await?;
            let snapshot = self.database.session_snapshot(&command.session_id).await?;
            Ok::<_, HamburError>((
                turn,
                user_message,
                assistant_message,
                chat_context,
                snapshot,
                !reuse_existing_user,
            ))
        });

        let (turn, _user_message, assistant_message, chat_context, snapshot, user_was_inserted) =
            match setup {
                Ok(value) => value,
                Err(error) => {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
            };

        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut active_turns) = self.active_turns.lock() {
            active_turns.insert(
                command.session_id.clone(),
                ActiveTurn {
                    turn_id: turn.id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStarted,
            command.session_id.clone(),
            turn.id.clone(),
            snapshot.clone(),
            command_kind.to_string(),
            None,
        );
        if user_was_inserted {
            let _ = self.emit_session_event(
                RuntimeEventKind::MessageUpserted,
                command.session_id.clone(),
                turn.id.clone(),
                snapshot.clone(),
                visible_user_content.clone(),
                None,
            );
        }
        let _ = self.emit_session_event(
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
        let tools_json = self.tools.schemas().compile_openai_tools_json();
        let skills_index_prompt = self.build_skills_index_prompt();
        let memory_system_prompt = self.build_memory_system_prompt();
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
            let _ = self.emit_markdown(command.session_id.clone(), result.0);
        }
        if let Some(update) = result.1
            && (!update.committed_nodes.is_empty()
                || update.pending_node.is_some()
                || update.reset
                || !update.invalidated_block_ids.is_empty())
        {
            let _ = self.emit_markdown(command.session_id.clone(), update);
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
    pub(crate) fn execute_rootfs_lifecycle(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }
        if command.kind == "ResetRootfs"
            && let Err(error) = require_approval(&command, "rootfs_reset")
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .tokio
            .block_on(self.database.settings_snapshot())
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);

        if command.kind == "ResetRootfs" {
            let payload = config_payload_value(&command.payload_json);
            let preserve_root = config_bool(&payload, "preserveRoot", true);
            if let Err(error) = self.sandbox.reset_rootfs(preserve_root) {
                return rejected_ack(command.command_id, command.idempotency_key, error);
            }
        }

        if let Err(error) = self
            .sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)
        {
            return rejected_ack(command.command_id, command.idempotency_key, error);
        }

        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let status = self.sandbox.rootfs_status();
        let snapshot = self
            .tokio
            .block_on(self.database.bootstrap_snapshot())
            .unwrap_or_default();
        let message = json!({
            "available": status.available,
            "backend": status.backend,
            "abi": status.abi,
            "reason": status.reason,
            "action": command.kind,
            "sessionIdProvided": !command.session_id.trim().is_empty()
        })
        .to_string();
        let _ = self.emit_session_event(
            RuntimeEventKind::TurnStateChanged,
            command.session_id,
            command.turn_id,
            snapshot,
            message,
            None,
        );
        accepted_ack(command.command_id, command.idempotency_key)
    }

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
                .emit_markdown_event_async(session_id.to_string(), turn_id.to_string(), update)
                .await;
        }

        self.database
            .update_message_stream_result(
                assistant_message_id,
                content,
                reasoning,
                "requires_tool",
                &finish_reason.if_blank("tool_calls".to_string()),
                &native_finish_reason.if_blank("tool_calls".to_string()),
            )
            .await?;

        self.database
            .update_turn_status(turn_id, "ExecutingTools", false)
            .await?;
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
                })
                .await?;
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
                })
                .await?;
            trace_ids.insert(invocation.tool_call_id.clone(), trace.id);
            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
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
            )
            .await;
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
        for record in records {
            let tool_message = self
                .database
                .insert_tool_result_message(
                    session_id,
                    turn_id,
                    &record.invocation.tool_call_id,
                    &record.invocation.name,
                    &record.result.context_stub,
                    route,
                )
                .await?;
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
                })
                .await?;
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
                )
                .await?;
            self.update_latest_tool_trace(
                trace_ids
                    .get(&record.invocation.tool_call_id)
                    .map(String::as_str),
                &record.invocation.tool_call_id,
                status,
                &record.result.summary,
            )
            .await?;

            let snapshot = self
                .database
                .session_snapshot(session_id)
                .await
                .unwrap_or_default();
            let _ = self.emit_session_event(
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
                content: record.result.context_stub,
                reasoning_content: String::new(),
                tool_calls_json: String::new(),
                tool_call_id: record.invocation.tool_call_id,
            });
        }

        if cancel.load(Ordering::SeqCst) || self.shutdown.load(Ordering::SeqCst) {
            self.finish_cancelled_turn(
                session_id,
                turn_id,
                assistant_message_id,
                content,
                reasoning,
            )
            .await;
            return Ok(None);
        }

        self.database
            .update_turn_status(turn_id, "ContinuingAfterTools", false)
            .await?;
        let continuation_route = view_image_handoff.unwrap_or_else(|| route.clone());
        if continuation_route.provider_id != route.provider_id
            || continuation_route.model_id != route.model_id
        {
            self.database
                .update_turn_route_snapshot(turn_id, &continuation_route)
                .await?;
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
                "RouteHandoff(ImageInspectionRequired)".to_string(),
                None,
            );
        }
        if context_stubs
            .iter()
            .any(|stub| stub.contains("ImagePart(fileId="))
        {
            let synthetic = self
                .database
                .insert_message_with_route(
                    session_id,
                    "user",
                    &format_synthetic_view_image_message(&context_stubs),
                    "",
                    "completed",
                    turn_id,
                    &continuation_route,
                )
                .await?;
            self.database
                .upsert_timeline_item(
                    session_id,
                    NewTimelineItem {
                        stable_key: synthetic.id.clone(),
                        content_type: "user_message".to_string(),
                        display_sequence: synthetic.created_at_ms,
                        payload_ref: synthetic.id.clone(),
                        small_summary: synthetic.content_text.chars().take(160).collect(),
                        kind: "SyntheticUserMessage".to_string(),
                    },
                )
                .await?;
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
            )
            .await?;

        let continuation_source = tool_continuation_stream_source(
            current_request,
            &continuation_route,
            &self.tools.schemas().compile_openai_tools_json(),
            content,
            reasoning,
            assistant_tool_calls,
            tool_result_messages,
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
    pub(crate) async fn execute_memory_review_tool(
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
        self.execute_knowledge_tool(invocation).await.result
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
        let mut regular = Vec::new();
        let mut records = Vec::new();
        let delegate_task_count = invocations
            .iter()
            .filter(|invocation| invocation.name == "delegate_task")
            .count();
        for invocation in invocations {
            match invocation.name.as_str() {
                "view_image" => {
                    records.push(
                        self.execute_view_image_tool(
                            session_id,
                            route,
                            route_candidates,
                            invocation,
                        )
                        .await,
                    );
                }
                "session_search" => {
                    records.push(self.execute_session_search_tool(invocation).await);
                }
                "terminal" | "process" => {
                    records.push(self.execute_sandbox_tool(invocation).await);
                }
                "read_file" | "write_file" | "patch" | "search_files" => {
                    records.push(self.execute_file_tool(invocation).await);
                }
                "memory" | "skill_list" | "skills_list" | "skill_view" => {
                    records.push(self.execute_knowledge_tool(invocation).await);
                }
                "hambur_config" => {
                    records.push(self.execute_hambur_config_tool(invocation).await);
                }
                "browser_use" => {
                    records.push(
                        self.execute_browser_tool(session_id, turn_id, invocation)
                            .await,
                    );
                }
                "web_fetch" | "web_search" => {
                    records.push(self.execute_web_tool(invocation).await);
                }
                "delegate_task" | "submit_delegate_result" => {
                    if invocation.name == "delegate_task" && delegate_task_count > 3 {
                        let started_at_ms = now_ms();
                        records.push(ToolExecutionRecord {
                            result: ToolResult::failed(
                                &invocation.tool_call_id,
                                &invocation.name,
                                "delegate batch limit exceeded",
                            ),
                            invocation,
                            started_at_ms,
                            ended_at_ms: now_ms(),
                        });
                    } else {
                        records.push(
                            self.execute_delegate_tool(
                                session_id,
                                turn_id,
                                route,
                                route_candidates,
                                invocation,
                            )
                            .await,
                        );
                    }
                }
                _ => {
                    regular.push(invocation);
                }
            }
        }

        if !regular.is_empty() {
            let batch = ToolCallBatch::new(
                turn_id.to_string(),
                assistant_message_id.to_string(),
                regular,
            );
            records.extend(self.tools.execute_batch(batch).await?);
        }
        records.sort_by_key(|record| record.invocation.index);
        Ok(records)
    }
    pub(crate) async fn execute_session_search_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_session_search_result(&invocation, &arguments)
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
    pub(crate) fn execute_sandbox_command(
        &self,
        session_id: &str,
        command: &str,
        cwd_sandbox: &str,
        timeout_ms: u64,
    ) -> HamburResult<hambur_sandbox::SandboxExecResult> {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self
            .safe_block_on(self.database.settings_snapshot())
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
    pub(crate) async fn execute_sandbox_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_sandbox_tool_result(&invocation, &arguments),
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
    pub(crate) async fn execute_file_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_file_tool_result(&invocation, &arguments),
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
    pub(crate) async fn execute_knowledge_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_knowledge_tool_result(&invocation, &arguments)
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
    pub(crate) async fn execute_hambur_config_tool(
        &self,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_hambur_config_tool_result(&invocation, &arguments)
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
    pub(crate) async fn execute_browser_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_browser_tool_result(session_id, turn_id, &invocation, &arguments)
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
    pub(crate) async fn execute_web_tool(&self, invocation: ToolInvocation) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => self.resolve_web_tool_result(&invocation, &arguments).await,
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
    pub(crate) async fn execute_delegate_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_delegate_result(
                    session_id,
                    turn_id,
                    route,
                    route_candidates,
                    &invocation,
                    &arguments,
                )
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
    pub(crate) async fn execute_view_image_tool(
        &self,
        session_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: ToolInvocation,
    ) -> ToolExecutionRecord {
        let started_at_ms = now_ms();
        let result = match invocation.arguments_value() {
            Ok(arguments) => {
                self.resolve_view_image_result(
                    session_id,
                    route,
                    route_candidates,
                    &invocation,
                    &arguments,
                )
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
}

impl RuntimeEngine {
    pub(crate) async fn provider_models_response(
        &self,
        provider_id: &str,
        command: &RuntimeCommand,
        payload: &Value,
    ) -> HamburResult<String> {
        if payload
            .get("data")
            .and_then(Value::as_array)
            .is_some_and(|data| !data.is_empty())
        {
            return Ok(command.payload_json.clone());
        }

        let provider = self.database.provider_by_id(provider_id).await.ok();
        let base_url = command.chunk.trim().to_string().if_blank(
            provider
                .as_ref()
                .map(|value| value.base_url.clone())
                .unwrap_or_default(),
        );
        let api_key = payload
            .get("apiKey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
            .if_blank(provider_api_key_from_payload(payload))
            .if_blank(
                provider
                    .as_ref()
                    .and_then(|value| secret_ref_to_api_key(&value.secret_ref))
                    .unwrap_or_default(),
            );
        let api_key = if api_key.trim().is_empty() {
            if let Some(provider) = provider.as_ref() {
                self.resolve_provider_api_key(
                    "",
                    "",
                    &ModelRouteSnapshot {
                        provider_id: provider.id.clone(),
                        provider_name: provider.name.clone(),
                        provider_protocol: provider.api_type.clone(),
                        base_url: provider.base_url.clone(),
                        secret_ref: provider.secret_ref.clone(),
                        model_id: command.model_id.clone(),
                        model_display_name: command.model_id.clone(),
                        ..ModelRouteSnapshot::default()
                    },
                    &Arc::new(AtomicBool::new(false)),
                )
                .await
                .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            api_key
        };

        if !base_url.trim().is_empty() {
            if let Ok(response) = fetch_openai_compatible_models(&base_url, &api_key).await
                && response.trim().starts_with('{')
            {
                return Ok(response);
            }
        }

        Ok(default_models_response(&command.model_id))
    }

    pub(crate) async fn models_dev_catalog_json(&self, force: bool) -> HamburResult<String> {
        let cached = self
            .database
            .model_catalog_cache(MODEL_CATALOG_CACHE_KEY)
            .await?;
        if !force
            && let Some(cache) = cached.as_ref()
            && cache.synced_at_ms > 0
            && now_ms().saturating_sub(cache.synced_at_ms) < MODEL_CATALOG_CACHE_MAX_AGE_MS
            && !cache.catalog_json.trim().is_empty()
        {
            return Ok(cache.catalog_json.clone());
        }

        let fetched = reqwest_text_url(MODELS_DEV_API_URL, 20).await?;
        let _: Value = serde_json::from_str(&fetched).map_err(|error| {
            HamburError::ProviderUnavailable(format!("parse models.dev catalog: {error}"))
        })?;
        self.database
            .upsert_model_catalog_cache(MODEL_CATALOG_CACHE_KEY, &fetched, now_ms())
            .await?;
        Ok(fetched)
    }

    pub(crate) async fn enrich_provider_models_from_catalog(&self, models: &mut [ProviderModel]) {
        let Ok(catalog_json) = self.models_dev_catalog_json(false).await else {
            return;
        };
        let Ok(catalog) = serde_json::from_str::<Value>(&catalog_json) else {
            return;
        };
        for model in models {
            let Some(detail) = match_catalog_model(&catalog, &model.model_id) else {
                continue;
            };
            merge_catalog_detail_into_provider_model(model, detail);
        }
    }
}

fn merge_catalog_detail_into_provider_model(model: &mut ProviderModel, detail: &Value) {
    if let Some(name) = detail
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        model.display_name = name.to_string();
    }
    model.capabilities.supports_tool_call =
        detail_bool(detail, "tool_call", model.capabilities.supports_tool_call);
    model.capabilities.supports_reasoning =
        detail_bool(detail, "reasoning", model.capabilities.supports_reasoning);
    model.capabilities.supports_image_input =
        detail_bool(
            detail,
            "attachment",
            model.capabilities.supports_image_input,
        ) || detail_modalities_include(detail, "input", "image")
            || detail_modalities_include(detail, "input", "vision");
    model.capabilities.supports_structured_output = detail_bool(
        detail,
        "structured_output",
        model.capabilities.supports_structured_output,
    );
    model.capabilities.supports_temperature = detail_bool(
        detail,
        "temperature",
        model.capabilities.supports_temperature,
    );
    if let Some(context_limit) = detail
        .get("limit")
        .and_then(|limit| limit.get("context").or_else(|| limit.get("input")))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        model.capabilities.context_limit = context_limit;
    }
    if let Some(output_limit) = detail
        .get("limit")
        .and_then(|limit| limit.get("output"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        model.capabilities.output_limit = output_limit;
    }
    if let Some(reasoning_field) = detail
        .get("interleaved")
        .and_then(|value| {
            value
                .get("field")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        })
        .filter(|value| !value.trim().is_empty())
    {
        model.capabilities.reasoning_field = reasoning_field.to_string();
    }
    model.metadata_json = detail.to_string();
}

fn match_catalog_model<'a>(catalog: &'a Value, model_id: &str) -> Option<&'a Value> {
    let normalized = normalize_model_id(model_id);
    let providers = catalog
        .get("providers")
        .and_then(Value::as_object)
        .or_else(|| catalog.as_object());
    let global_models = catalog.get("models").and_then(Value::as_object);

    if let Some(models) = global_models {
        if let Some(detail) = models.get(model_id) {
            return Some(detail);
        }
        if let Some((_, detail)) = models
            .iter()
            .find(|(id, _)| normalize_model_id(id) == normalized)
        {
            return Some(detail);
        }
        if let Some((_, detail)) = models
            .iter()
            .find(|(id, _)| normalize_model_id(id).ends_with(&format!("/{normalized}")))
        {
            return Some(detail);
        }
    }

    let providers = providers?;
    for provider in providers.values() {
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        if let Some(detail) = models.get(model_id) {
            return Some(detail);
        }
        if let Some((_, detail)) = models.iter().find(|(id, detail)| {
            normalize_model_id(id) == normalized
                || detail
                    .get("id")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .as_deref()
                    == Some(normalized.as_str())
                || detail
                    .get("name")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .as_deref()
                    == Some(normalized.as_str())
        }) {
            return Some(detail);
        }
        if let Some((_, detail)) = models.iter().find(|(id, detail)| {
            normalize_model_id(id).ends_with(&format!("/{normalized}"))
                || detail
                    .get("id")
                    .and_then(Value::as_str)
                    .map(normalize_model_id)
                    .is_some_and(|value| value.ends_with(&format!("/{normalized}")))
        }) {
            return Some(detail);
        }
    }
    None
}

fn normalize_model_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn detail_bool(detail: &Value, key: &str, fallback: bool) -> bool {
    detail.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

fn detail_modalities_include(detail: &Value, direction: &str, expected: &str) -> bool {
    detail
        .get("modalities")
        .and_then(|modalities| modalities.get(direction))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value.eq_ignore_ascii_case(expected))
        })
        .unwrap_or(false)
}

async fn fetch_openai_compatible_models(base_url: &str, api_key: &str) -> HamburResult<String> {
    let url = openai_models_url(base_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let mut request = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "Hambur/0.1");
    if !api_key.trim().is_empty() {
        request = request.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key.trim()),
        );
    }
    let response = request.send().await.map_err(|error| {
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

fn openai_models_url(base_url: &str) -> String {
    let base_url = base_url.trim();
    if base_url.ends_with("/models") || base_url.ends_with("/models/") {
        base_url.to_string()
    } else if base_url.ends_with('/') {
        format!("{base_url}models")
    } else {
        format!("{base_url}/models")
    }
}

fn provider_api_key_from_payload(payload: &Value) -> String {
    payload
        .get("secretRef")
        .and_then(Value::as_str)
        .and_then(secret_ref_to_api_key)
        .unwrap_or_default()
}

fn secret_ref_to_api_key(secret_ref: &str) -> Option<String> {
    let env_name = secret_ref.strip_prefix("env://")?;
    std::env::var(env_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
