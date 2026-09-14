use crate::*;

impl RuntimeEngine {
    pub(crate) fn execute_create_session(&self, command: RuntimeCommand) -> RuntimeCommandAck {
        if self.shutdown.load(Ordering::SeqCst) {
            return rejected_ack(
                command.command_id,
                command.idempotency_key,
                HamburError::RuntimeClosed,
            );
        }

        let previous_session_id = self.database.bootstrap_snapshot()
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self.database.create_session(&command.title)
        {
            Ok(snapshot) => {
                if !snapshot.selected_session_id.trim().is_empty()
                    && let Err(error) = self.sandbox.prepare_session(&snapshot.selected_session_id)
                {
                    let _ = self.emit_error(error.clone());
                    return rejected_ack(command.command_id, command.idempotency_key, error);
                }
                let new_session_id = snapshot.selected_session_id.clone();
                let _ = self.emit_snapshot(RuntimeEventKind::SessionCreated, snapshot);
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

        let previous_session_id = self.database.bootstrap_snapshot()
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        match self.database.open_session(&command.session_id)
        {
            Ok(snapshot) => {
                let _ = self.emit_snapshot(RuntimeEventKind::SessionOpened, snapshot);
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
        match self.database.rename_session(&command.session_id, &title)
        {
            Ok(snapshot) => {
                let _ = self.emit_with_snapshot(
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
        match self.database
                .set_session_pinned(&command.session_id, pinned) {
            Ok(snapshot) => {
                let _ = self.emit_with_snapshot(
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

        match self.database.delete_session(&command.session_id)
        {
            Ok(snapshot) => {
                self.kill_processes_for_session(&command.session_id);
                let _ = self.emit_snapshot(RuntimeEventKind::SessionDeleted, snapshot);
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
}
