use crate::*;

impl RuntimeEngine {
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
        let result = (|| {
            let setting = if command.kind == "DeleteStartupTask" {
                self.database.delete_app_setting(&setting_key)?;
                hambur_db::AppSettingRecord {
                    key: setting_key.clone(),
                    value: String::new(),
                    updated_at_ms: now_ms(),
                }
            } else {
                self.database
                    .upsert_app_setting(&setting_key, &setting_value)?
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
                )?;
            Ok::<(), HamburError>(())
        })();
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
                (|| {
                    self.database
                        .delete_app_setting(&format!("skill_enabled:{deleted_path}"))
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
                        )?;
                    Ok(())
                })()
            });
        self.finish_settings_command(command, result, "Skill deleted")
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
        let settings_snap = self.database.settings_snapshot()
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
        let message = json!({
            "available": status.available,
            "backend": status.backend,
            "abi": status.abi,
            "reason": status.reason,
            "action": command.kind,
            "sessionIdProvided": !command.session_id.trim().is_empty()
        })
        .to_string();
        let _ = self.emit_plain(
            RuntimeEventKind::TurnStateChanged,
            command.session_id,
            command.turn_id,
            message,
            );
        accepted_ack(command.command_id, command.idempotency_key)
    }
}
