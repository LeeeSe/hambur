use crate::*;

impl RuntimeEngine {
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
        let result = (|| {
            let provider = self
                .database
                .upsert_provider(ProviderUpsert {
                    id: provider_id.clone(),
                    name: command.title.clone(),
                    icon_name: config_payload_string(&command.payload_json, "iconName")
                        .if_blank("sparkles".to_string()),
                    api_type: config_payload_string(&command.payload_json, "apiType")
                        .if_blank(OPENAI_COMPATIBLE_PROTOCOL.to_string()),
                    base_url: command.chunk.clone(),
                    secret_ref: provider_secret_ref_from_payload(&command.payload_json),
                    enabled: config_payload_bool(&command.payload_json, "enabled", true),
                })?;
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
                )?;
            Ok::<(), HamburError>(())
        })();

        match result {
            Ok(()) => {
                let _ = self.emit_plain(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    "Provider updated".to_string(),
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
        let result = (|| {
            self.database.delete_provider(&provider_id)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();

        match result {
            Ok(()) => {
                let _ = self.emit_plain(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    "Provider deleted".to_string(),
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

        match self.database.replace_provider_models(&provider_id, upserts)
        {
            Ok(models) => {
                for (index, model) in models.iter().enumerate() {
                    if model.metadata_json.contains("\"custom\":true")
                        || model.metadata_json.contains("\"custom\": true")
                    {
                        continue;
                    }
                    let _ = self.database.upsert_primary_chat_member(
                            &provider_id,
                            &model.model_id,
                            index as u32,
                        );
                }
                let _ = self.database.insert_config_audit(
                    &command.command_id,
                    "user",
                    "RefreshProviderModels",
                    "provider",
                    &provider_id,
                    &format!("{} provider models refreshed", models.len()),
                    false,
                    "",
                );
                                let _ = self.emit_plain(
                    RuntimeEventKind::ModelsUpdated,
                    String::new(),
                    String::new(),
                    format!("{} models refreshed", models.len()),
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    pub(crate) fn execute_delete_provider_model(
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
        let result = (|| {
            self.database
                .delete_provider_model(&provider_id, &model_id)?;
            self.database
                .insert_config_audit(
                    &command.command_id,
                    "user",
                    "DeleteProviderModel",
                    "provider_model",
                    &format!("{}/{}", provider_id, model_id),
                    "Provider model deleted",
                    false,
                    "",
                )?;
            Ok::<(), HamburError>(())
        })();
        self.finish_settings_command(command, result, "Model deleted")
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
        let result = (|| {
            let is_custom = config_bool(&payload, "custom", false);
            let mut metadata_value: Value = serde_json::from_str(&config_object_string(
                &payload,
                "metadataJson",
            ))
            .unwrap_or_else(|_| serde_json::json!({}));
            if is_custom && metadata_value.get("custom").is_none() {
                if let Some(obj) = metadata_value.as_object_mut() {
                    obj.insert("custom".to_string(), Value::Bool(true));
                }
            }
            let metadata_json = metadata_value.to_string();

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
                    metadata_json,
                })?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
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
                )?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
            let member = self
                .database
                .upsert_model_group_member(&group_id, &provider_id, &model_id, position, enabled)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
            let default = self
                .database
                .set_default_model_group(&key, &group_id)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
            if !primary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("primary", &primary_group_id)?;
            }
            if !secondary_group_id.trim().is_empty() {
                self.database
                    .set_default_model_group("secondary", &secondary_group_id)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
            self.database.delete_model_group(&group_id)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
        let result = (|| {
            self.database
                .delete_model_group_member(&group_id, &provider_id, &model_id)?;
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
                )?;
            Ok::<(), HamburError>(())
        })();
        self.finish_settings_command(command, result, "Model group member deleted")
    }
}
