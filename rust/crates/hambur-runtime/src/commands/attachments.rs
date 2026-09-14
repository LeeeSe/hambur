use crate::*;

impl RuntimeEngine {
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
        let result = (|| {
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
                })?;
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
                })?;
            let attachments = self
                .database
                .pending_attachments_for_session(&command.session_id)?;
            Ok::<_, HamburError>((attachment, attachments))
        })();

        match result {
            Ok((attachment, attachments)) => {
                let _ = self.emit_attachments(
                    RuntimeEventKind::AttachmentImported,
                    command.session_id.clone(),
                    attachments,
                    attachment.display_name,
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

        let result = (|| {
            let (_attachment, cleanup) = self
                .database
                .remove_pending_attachment(&command.session_id, &attachment_id)?;
            if let Some(job) = cleanup {
                let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                let _ = self.database.mark_file_cleanup_done(&job.id);
            }
            self.database
            .pending_attachments_for_session(&command.session_id)
        })();

        match result {
            Ok(attachments) => {
                let _ = self.emit_attachments(
                    RuntimeEventKind::PendingAttachmentRemoved,
                    command.session_id.clone(),
                    attachments,
                    attachment_id,
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
        let result = (|| {
            let pending = self
                .database
                .pending_attachments_for_session(&command.session_id)?;
            for attachment in pending {
                let (_removed, cleanup) = self
                    .database
                    .remove_pending_attachment(&command.session_id, &attachment.id)?;
                if let Some(job) = cleanup {
                    let _ = self.filestore.delete_relative_if_exists(&job.relative_path);
                    let _ = self.database.mark_file_cleanup_done(&job.id);
                }
            }
            self.database
            .pending_attachments_for_session(&command.session_id)
        })();

        match result {
            Ok(attachments) => {
                let _ = self.emit_attachments(
                    RuntimeEventKind::PendingAttachmentsCleaned,
                    command.session_id.clone(),
                    attachments,
                    "Pending attachments cleared".to_string(),
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }
}
