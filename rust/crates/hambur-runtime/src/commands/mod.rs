use crate::*;

mod attachments;
mod chat;
mod providers;
mod sessions;
mod settings;
mod tools;

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
            "DeleteProviderModel" | "DeleteModel" => {
                self.execute_delete_provider_model(command)
            }
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
}
