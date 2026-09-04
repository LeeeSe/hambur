use crate::*;

impl HamburDatabase {
    pub async fn create_session(&self, title: &str) -> HamburResult<AppSnapshot> {
        let id = self.insert_session(title, "chat").await?;
        self.set_active_session(Some(&id)).await?;
        self.snapshot_for_selected(Some(&id)).await
    }

    pub async fn create_internal_session(
        &self,
        title: &str,
        purpose: &str,
    ) -> HamburResult<String> {
        self.insert_session(title, purpose).await
    }

    async fn insert_session(&self, title: &str, purpose: &str) -> HamburResult<String> {
        let id = new_id("ses");
        let now = now_ms();
        let title = normalize_title(title);
        let purpose = normalize_session_purpose(purpose)?;

        self.connection
            .execute(
                "INSERT INTO sessions (id, title, purpose, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id.clone(), title, purpose, now as i64, now as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(id)
    }

    pub async fn rename_session(&self, session_id: &str, title: &str) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        let now = now_ms();
        let title = normalize_title(title);
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET title = ?1, updated_at_ms = ?2
                 WHERE id = ?3 AND deleted_at_ms IS NULL",
                params![title, now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn set_session_pinned(
        &self,
        session_id: &str,
        pinned: bool,
    ) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        let now = now_ms();
        let pinned_at_ms = if pinned { now } else { 0 };
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET pinned_at_ms = ?1, updated_at_ms = ?2
                 WHERE id = ?3 AND deleted_at_ms IS NULL",
                params![pinned_at_ms as i64, now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn delete_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET deleted_at_ms = ?1, updated_at_ms = ?1
                 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        if self.active_session_id().await?.as_deref() == Some(session_id) {
            self.set_active_session(None).await?;
        }
        self.snapshot_for_selected(None).await
    }

    pub async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        let role = normalize_role(role)?;
        self.connection
            .execute(
                "INSERT INTO messages
                    (id, session_id, role, content_text, created_at_ms, version_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![
                    id.clone(),
                    session_id,
                    role.clone(),
                    content_text,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(MessageRecord {
            id,
            session_id: session_id.to_string(),
            role,
            content_text: content_text.to_string(),
            reasoning_content: String::new(),
            status: "completed".to_string(),
            turn_id: String::new(),
            created_at_ms: now,
            version_sequence: 1,
            provider_id_snapshot: String::new(),
            provider_name_snapshot: String::new(),
            provider_protocol: String::new(),
            model_id_snapshot: String::new(),
            model_name_snapshot: String::new(),
            model_group_id: String::new(),
            finish_reason: String::new(),
            native_finish_reason: String::new(),
            tool_call_id: String::new(),
            tool_name: String::new(),
            tool_title: String::new(),
            prompt_prefix: String::new(),
            attachments: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_message_with_route(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
        reasoning_content: &str,
        status: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        self.insert_message_with_route_and_prefix(
            session_id,
            role,
            content_text,
            reasoning_content,
            status,
            turn_id,
            route,
            "",
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_message_with_route_and_prefix(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
        reasoning_content: &str,
        status: &str,
        turn_id: &str,
        route: &ModelRouteSnapshot,
        prompt_prefix: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        let role = normalize_role(role)?;
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO messages
                    (
                        id,
                        session_id,
                        turn_id,
                        role,
                        status,
                        content_text,
                        reasoning_content,
                        created_at_ms,
                        version_sequence,
                        provider_id_snapshot,
                        provider_name_snapshot,
                        provider_protocol,
                        model_id_snapshot,
                        model_name_snapshot,
                        model_group_id,
                        finish_reason,
                        native_finish_reason,
                        prompt_prefix
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11, ?12, ?13, ?14, '', '', ?15)",
                params![
                    id.clone(),
                    session_id,
                    turn_id,
                    role.clone(),
                    status.clone(),
                    content_text,
                    reasoning_content,
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    prompt_prefix
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(MessageRecord {
            id,
            session_id: session_id.to_string(),
            role,
            content_text: content_text.to_string(),
            reasoning_content: reasoning_content.to_string(),
            status,
            turn_id: turn_id.to_string(),
            created_at_ms: now,
            version_sequence: 1,
            provider_id_snapshot: route.provider_id.clone(),
            provider_name_snapshot: route.provider_name.clone(),
            provider_protocol: route.provider_protocol.clone(),
            model_id_snapshot: route.model_id.clone(),
            model_name_snapshot: route.model_display_name.clone(),
            model_group_id: route.model_group_id.clone(),
            finish_reason: String::new(),
            native_finish_reason: String::new(),
            tool_call_id: String::new(),
            tool_name: String::new(),
            tool_title: String::new(),
            prompt_prefix: prompt_prefix.to_string(),
            attachments: Vec::new(),
        })
    }

    pub async fn update_message_stream_result(
        &self,
        message_id: &str,
        content_text: &str,
        reasoning_content: &str,
        status: &str,
        finish_reason: &str,
        native_finish_reason: &str,
    ) -> HamburResult<MessageRecord> {
        let status = normalize_status(status)?;
        let changed = self
            .connection
            .execute(
                "UPDATE messages
                 SET content_text = ?1,
                     reasoning_content = ?2,
                     status = ?3,
                     finish_reason = ?4,
                     native_finish_reason = ?5,
                     version_sequence = version_sequence + 1
                 WHERE id = ?6",
                params![
                    content_text,
                    reasoning_content,
                    status,
                    finish_reason,
                    native_finish_reason,
                    message_id
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "message not found: {message_id}"
            )));
        }
        self.message_by_id(message_id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("message disappeared: {message_id}")))
    }

    pub async fn update_message_route_snapshot(
        &self,
        message_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        let changed = self
            .connection
            .execute(
                "UPDATE messages
                 SET provider_id_snapshot = ?1,
                     provider_name_snapshot = ?2,
                     provider_protocol = ?3,
                     model_id_snapshot = ?4,
                     model_name_snapshot = ?5,
                     model_group_id = ?6,
                     version_sequence = version_sequence + 1
                 WHERE id = ?7",
                params![
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    message_id,
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "message not found: {message_id}"
            )));
        }
        self.message_by_id(message_id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("message disappeared: {message_id}")))
    }

    pub async fn insert_tool_result_message(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        content_text: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO messages
                    (
                        id,
                        session_id,
                        turn_id,
                        role,
                        status,
                        content_text,
                        reasoning_content,
                        created_at_ms,
                        version_sequence,
                        provider_id_snapshot,
                        provider_name_snapshot,
                        provider_protocol,
                        model_id_snapshot,
                        model_name_snapshot,
                        model_group_id,
                        finish_reason,
                        native_finish_reason,
                        tool_call_id,
                        tool_name,
                        tool_title
                    )
                 VALUES (?1, ?2, ?3, 'tool', 'completed', ?4, '', ?5, 1, ?6, ?7, ?8, ?9, ?10, ?11, 'tool_result', 'tool_result', ?12, ?13, ?13)",
                params![
                    id.clone(),
                    session_id,
                    turn_id,
                    content_text,
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    tool_call_id,
                    tool_name,
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        self.message_by_id(&id)
            .await?
            .ok_or_else(|| HamburError::Internal(format!("tool message disappeared: {id}")))
    }

    pub async fn insert_trace_span(&self, input: NewTraceSpan) -> HamburResult<TraceSpanRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = new_id("trace");
        let now = now_ms();
        let status = normalize_status(&input.status)?;
        self.connection
            .execute(
                "INSERT INTO trace_spans
                    (
                        id,
                        session_id,
                        turn_id,
                        parent_span_id,
                        kind,
                        title,
                        content,
                        status,
                        started_at_ms,
                        ended_at_ms,
                        tool_call_id,
                        payload_json,
                        visible
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12)",
                params![
                    id.clone(),
                    input.session_id.clone(),
                    input.turn_id,
                    input.parent_span_id,
                    input.kind,
                    input.title,
                    input.content,
                    status,
                    now as i64,
                    input.tool_call_id,
                    input.payload_json,
                    input.visible,
                ],
            )
            .await
            .map_err(database_error)?;

        if input.visible {
            let trace = self.trace_span_by_id(&id).await?;
            self.upsert_timeline_item(
                &input.session_id,
                NewTimelineItem {
                    stable_key: trace.id.clone(),
                    content_type: "trace".to_string(),
                    display_sequence: trace.started_at_ms,
                    payload_ref: trace.id.clone(),
                    small_summary: trace.title.clone(),
                    kind: match trace.kind.as_str() {
                        "tool" => "ToolTrace".to_string(),
                        "error" => "ErrorTrace".to_string(),
                        _ => "TraceSpan".to_string(),
                    },
                },
            )
            .await?;
        }

        self.trace_span_by_id(&id).await
    }

    pub async fn update_trace_span_status(
        &self,
        trace_id: &str,
        status: &str,
        content: &str,
        ended: bool,
    ) -> HamburResult<TraceSpanRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE trace_spans
                 SET status = ?1,
                     content = ?2,
                     ended_at_ms = CASE WHEN ?3 THEN ?4 ELSE ended_at_ms END
                 WHERE id = ?5",
                params![status, content, ended, now as i64, trace_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "trace span not found: {trace_id}"
            )));
        }
        let trace = self.trace_span_by_id(trace_id).await?;
        self.upsert_timeline_item(
            &trace.session_id,
            NewTimelineItem {
                stable_key: trace.id.clone(),
                content_type: "trace".to_string(),
                display_sequence: trace.started_at_ms,
                payload_ref: trace.id.clone(),
                small_summary: trace.title.clone(),
                kind: match trace.kind.as_str() {
                    "tool" => "ToolTrace".to_string(),
                    "error" => "ErrorTrace".to_string(),
                    _ => "TraceSpan".to_string(),
                },
            },
        )
        .await?;
        Ok(trace)
    }

    pub async fn insert_tool_call(&self, input: NewToolCall) -> HamburResult<ToolCallRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = if input.id.trim().is_empty() {
            new_id("tool_call")
        } else {
            input.id
        };
        let now = now_ms();
        let status = normalize_status(&input.status)?;
        self.connection
            .execute(
                "INSERT INTO tool_calls
                    (
                        id,
                        session_id,
                        turn_id,
                        assistant_message_id,
                        name,
                        arguments_json,
                        display_title,
                        status,
                        requires_approval,
                        approval_status,
                        started_at_ms,
                        ended_at_ms,
                        result_id,
                        error_code,
                        error_message,
                        call_index
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'not_required', ?10, NULL, '', '', '', ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    status = excluded.status,
                    display_title = excluded.display_title,
                    arguments_json = excluded.arguments_json,
                    requires_approval = excluded.requires_approval,
                    call_index = excluded.call_index,
                    started_at_ms = CASE
                        WHEN tool_calls.started_at_ms = 0 THEN excluded.started_at_ms
                        ELSE tool_calls.started_at_ms
                    END",
                params![
                    id.clone(),
                    input.session_id,
                    input.turn_id,
                    input.assistant_message_id,
                    input.name,
                    input.arguments_json,
                    input.display_title,
                    status,
                    input.requires_approval,
                    now as i64,
                    input.call_index as i64,
                ],
            )
            .await
            .map_err(database_error)?;

        self.tool_call_by_id(&id).await
    }

    pub async fn update_tool_call_status(
        &self,
        tool_call_id: &str,
        status: &str,
        result_id: &str,
        error_code: &str,
        error_message: &str,
        ended: bool,
    ) -> HamburResult<ToolCallRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE tool_calls
                 SET status = ?1,
                     result_id = ?2,
                     error_code = ?3,
                     error_message = ?4,
                     ended_at_ms = CASE WHEN ?5 THEN ?6 ELSE ended_at_ms END
                 WHERE id = ?7",
                params![
                    status,
                    result_id,
                    error_code,
                    error_message,
                    ended,
                    now as i64,
                    tool_call_id,
                ],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "tool call not found: {tool_call_id}"
            )));
        }
        self.tool_call_by_id(tool_call_id).await
    }

    pub async fn insert_tool_result(&self, input: NewToolResult) -> HamburResult<ToolResultRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        let id = new_id("tool_result");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO tool_results
                    (
                        id,
                        session_id,
                        turn_id,
                        tool_call_id,
                        message_id,
                        is_error,
                        content_json,
                        summary,
                        artifacts_json,
                        trust_level,
                        truncated,
                        offloaded_file_id,
                        offloaded_path,
                        context_stub,
                        created_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    id.clone(),
                    input.session_id,
                    input.turn_id,
                    input.tool_call_id,
                    input.message_id,
                    input.is_error,
                    input.content_json,
                    input.summary,
                    input.artifacts_json,
                    input.trust_level,
                    input.truncated,
                    input.offloaded_file_id,
                    input.offloaded_path,
                    input.context_stub,
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;

        self.tool_result_by_id(&id).await
    }

    pub async fn upsert_file_record(&self, input: NewFileRecord) -> HamburResult<FileRecord> {
        let id = if input.id.trim().is_empty() {
            new_id("file")
        } else {
            input.id.trim().chars().take(160).collect()
        };
        let scope = normalize_file_scope(&input.scope)?;
        let session_id = input.session_id.trim().to_string();
        if scope == "session" {
            self.ensure_session_exists(&session_id).await?;
        }
        let relative_path = normalize_db_path(&input.relative_path, "relative_path")?;
        let sandbox_path = normalize_db_path(&input.sandbox_path, "sandbox_path")?;
        let mime_type = normalize_mime_type(&input.mime_type);
        let retention_policy = normalize_retention_policy(&input.retention_policy)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO files
                    (
                        id,
                        scope,
                        session_id,
                        relative_path,
                        sandbox_path,
                        mime_type,
                        byte_size,
                        sha256,
                        created_at_ms,
                        updated_at_ms,
                        retention_policy
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    scope = excluded.scope,
                    session_id = excluded.session_id,
                    relative_path = excluded.relative_path,
                    sandbox_path = excluded.sandbox_path,
                    mime_type = excluded.mime_type,
                    byte_size = excluded.byte_size,
                    sha256 = excluded.sha256,
                    updated_at_ms = excluded.updated_at_ms,
                    retention_policy = excluded.retention_policy",
                params![
                    id.clone(),
                    scope,
                    session_id,
                    relative_path,
                    sandbox_path,
                    mime_type,
                    input.byte_size as i64,
                    input.sha256.trim().chars().take(128).collect::<String>(),
                    now as i64,
                    retention_policy,
                ],
            )
            .await
            .map_err(database_error)?;

        self.file_by_id(&id).await
    }

    pub async fn create_pending_attachment(
        &self,
        input: NewAttachment,
    ) -> HamburResult<AttachmentRecord> {
        self.ensure_session_exists(&input.session_id).await?;
        self.file_by_id(&input.file_id).await?;
        let id = if input.id.trim().is_empty() {
            new_id("att")
        } else {
            input.id.trim().chars().take(160).collect()
        };
        let kind = normalize_attachment_kind(&input.kind, &input.mime_type);
        let status = normalize_attachment_status(&input.status)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO attachments
                    (
                        id,
                        session_id,
                        message_id,
                        kind,
                        display_name,
                        mime_type,
                        byte_size,
                        origin_type,
                        original_uri,
                        file_id,
                        sandbox_path,
                        width,
                        height,
                        sha256,
                        status,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                    display_name = excluded.display_name,
                    mime_type = excluded.mime_type,
                    byte_size = excluded.byte_size,
                    original_uri = excluded.original_uri,
                    sandbox_path = excluded.sandbox_path,
                    width = excluded.width,
                    height = excluded.height,
                    sha256 = excluded.sha256,
                    status = excluded.status,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id.clone(),
                    input.session_id,
                    input.message_id,
                    kind,
                    normalize_display_name(&input.display_name),
                    normalize_mime_type(&input.mime_type),
                    input.byte_size as i64,
                    normalize_origin_type(&input.origin_type),
                    input
                        .original_uri
                        .trim()
                        .chars()
                        .take(1024)
                        .collect::<String>(),
                    input.file_id,
                    normalize_db_path(&input.sandbox_path, "sandbox_path")?,
                    input.width as i64,
                    input.height as i64,
                    input.sha256.trim().chars().take(128).collect::<String>(),
                    status,
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;
        self.attachment_by_id(&id).await
    }

    pub async fn attach_pending_to_message(
        &self,
        session_id: &str,
        message_id: &str,
        attachment_ids: &[String],
    ) -> HamburResult<Vec<AttachmentRecord>> {
        self.ensure_session_exists(session_id).await?;
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let now = now_ms();
        let mut attached = Vec::new();
        for attachment_id in attachment_ids {
            let attachment = self.attachment_by_id(attachment_id).await?;
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
            self.connection
                .execute(
                    "UPDATE attachments
                     SET message_id = ?1,
                         status = 'attached',
                         updated_at_ms = ?2
                     WHERE id = ?3",
                    params![message_id, now as i64, attachment_id.clone()],
                )
                .await
                .map_err(database_error)?;
            attached.push(self.attachment_by_id(attachment_id).await?);
        }
        Ok(attached)
    }

    pub async fn remove_pending_attachment(
        &self,
        session_id: &str,
        attachment_id: &str,
    ) -> HamburResult<(AttachmentRecord, Option<FileCleanupJobRecord>)> {
        self.ensure_session_exists(session_id).await?;
        let attachment = self.attachment_by_id(attachment_id).await?;
        if attachment.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "attachment does not belong to session: {attachment_id}"
            )));
        }
        if attachment.status != "pending" || !attachment.message_id.is_empty() {
            return Err(HamburError::InvalidCommand(format!(
                "attachment is not removable pending state: {attachment_id}"
            )));
        }
        let now = now_ms();
        self.connection
            .execute(
                "UPDATE attachments
                 SET status = 'removed',
                     updated_at_ms = ?1
                 WHERE id = ?2",
                params![now as i64, attachment_id],
            )
            .await
            .map_err(database_error)?;
        let removed = self.attachment_by_id(attachment_id).await?;
        let cleanup = self
            .schedule_file_cleanup(&removed.file_id, "pending_attachment_removed")
            .await
            .ok();
        Ok((removed, cleanup))
    }

    pub async fn cleanup_pending_attachments(
        &self,
        older_than_ms: u64,
    ) -> HamburResult<Vec<FileCleanupJobRecord>> {
        let cutoff = now_ms().saturating_sub(older_than_ms);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    kind,
                    display_name,
                    mime_type,
                    byte_size,
                    origin_type,
                    original_uri,
                    file_id,
                    sandbox_path,
                    width,
                    height,
                    sha256,
                    status,
                    created_at_ms,
                    updated_at_ms
                FROM attachments
                WHERE status = 'pending'
                  AND message_id = ''
                  AND created_at_ms < ?1
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![cutoff as i64],
            )
            .await
            .map_err(database_error)?;

        let mut expired = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            expired.push(attachment_from_row(&row)?);
        }

        let now = now_ms();
        let mut jobs = Vec::new();
        for attachment in expired {
            self.connection
                .execute(
                    "UPDATE attachments
                     SET status = 'expired',
                         updated_at_ms = ?1
                     WHERE id = ?2 AND status = 'pending'",
                    params![now as i64, attachment.id],
                )
                .await
                .map_err(database_error)?;
            if let Ok(job) = self
                .schedule_file_cleanup(&attachment.file_id, "pending_attachment_expired")
                .await
            {
                jobs.push(job);
            }
        }
        Ok(jobs)
    }

    pub async fn schedule_file_cleanup(
        &self,
        file_id: &str,
        reason: &str,
    ) -> HamburResult<FileCleanupJobRecord> {
        let file = self.file_by_id(file_id).await?;
        let id = new_id("cleanup");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO file_cleanup_jobs
                    (
                        id,
                        file_id,
                        relative_path,
                        reason,
                        status,
                        attempts,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5)",
                params![
                    id.clone(),
                    file.id,
                    file.relative_path,
                    reason.trim().chars().take(120).collect::<String>(),
                    now as i64,
                ],
            )
            .await
            .map_err(database_error)?;
        self.file_cleanup_job_by_id(&id).await
    }

    pub async fn mark_file_cleanup_done(&self, job_id: &str) -> HamburResult<FileCleanupJobRecord> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE file_cleanup_jobs
                 SET status = 'completed',
                     updated_at_ms = ?1
                 WHERE id = ?2",
                params![now as i64, job_id],
            )
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "file cleanup job not found: {job_id}"
            )));
        }
        self.file_cleanup_job_by_id(job_id).await
    }

    pub async fn upsert_timeline_item(
        &self,
        session_id: &str,
        item: NewTimelineItem,
    ) -> HamburResult<TimelineItemSnapshot> {
        self.ensure_session_exists(session_id).await?;

        let stable_key = item.stable_key;
        if stable_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline stable_key must not be empty".to_string(),
            ));
        }
        if item.content_type.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline content_type must not be empty".to_string(),
            ));
        }

        let id = new_id("tl");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO timeline_items
                    (
                        id,
                        session_id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind,
                        visible,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, 1, ?9, ?9)
                 ON CONFLICT(session_id, stable_key) DO UPDATE SET
                    content_type = excluded.content_type,
                    display_sequence = excluded.display_sequence,
                    version_sequence = timeline_items.version_sequence + 1,
                    payload_ref = excluded.payload_ref,
                    small_summary = excluded.small_summary,
                    kind = excluded.kind,
                    visible = 1,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id,
                    session_id,
                    stable_key.clone(),
                    item.content_type,
                    item.display_sequence as i64,
                    item.payload_ref,
                    item.small_summary,
                    item.kind,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        self.timeline_item_by_stable_key(session_id, &stable_key)
            .await
    }

    pub async fn upsert_message_block_payload(
        &self,
        session_id: &str,
        _turn_id: &str,
        input: NewMessageBlockPayload,
    ) -> HamburResult<MessageBlockPayloadRecord> {
        self.ensure_session_exists(session_id).await?;
        if input.message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "message block message_id must not be empty".to_string(),
            ));
        }
        if input.stable_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "message block stable_key must not be empty".to_string(),
            ));
        }
        let block_type = input.block_type.trim();
        if block_type.is_empty() {
            return Err(HamburError::InvalidCommand(
                "message block block_type must not be empty".to_string(),
            ));
        }

        let id = if input.id.trim().is_empty() {
            new_id("mdb")
        } else {
            input.id.clone()
        };
        let now = now_ms();
        let stable_key = input.stable_key.clone();
        self.connection
            .execute(
                "INSERT INTO message_blocks
                    (
                        id,
                        session_id,
                        message_id,
                        block_id,
                        block_type,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?11)
                 ON CONFLICT(session_id, stable_key) DO UPDATE SET
                    message_id = excluded.message_id,
                    block_id = excluded.block_id,
                    block_type = excluded.block_type,
                    committed = excluded.committed,
                    payload_json = excluded.payload_json,
                    raw = excluded.raw,
                    small_summary = excluded.small_summary,
                    version_sequence = message_blocks.version_sequence + 1,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id.clone(),
                    session_id,
                    input.message_id,
                    input.block_id as i64,
                    block_type,
                    stable_key.clone(),
                    if input.committed { 1_i64 } else { 0_i64 },
                    input.payload_json,
                    input.raw,
                    input.small_summary,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        let record = self
            .message_block_by_stable_key(session_id, &stable_key)
            .await?;
        let display_base = self
            .message_by_id(&record.message_id)
            .await?
            .map(|message| message.created_at_ms)
            .unwrap_or(record.created_at_ms);
        let (content_type, display_sequence, kind) = if record.block_type == "reasoning" {
            (
                "assistant_reasoning_block".to_string(),
                display_base.saturating_sub(1),
                "AssistantReasoningBlock".to_string(),
            )
        } else if record.committed {
            (
                "assistant_markdown_block".to_string(),
                display_base.saturating_add(record.block_id),
                "AssistantMarkdownBlock".to_string(),
            )
        } else {
            (
                "assistant_pending_block".to_string(),
                display_base.saturating_add(record.block_id),
                "AssistantPendingBlock".to_string(),
            )
        };
        self.upsert_timeline_item(
            session_id,
            NewTimelineItem {
                stable_key: record.stable_key.clone(),
                content_type,
                display_sequence,
                payload_ref: record.id.clone(),
                small_summary: record.small_summary.clone(),
                kind,
            },
        )
        .await?;
        if record.block_type == "content" && record.committed {
            self.hide_timeline_item(session_id, &pending_markdown_stable_key(&record.message_id))
                .await?;
        }
        Ok(record)
    }

    pub async fn remove_pending_markdown_block(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> HamburResult<()> {
        self.hide_timeline_item(session_id, &pending_markdown_stable_key(message_id))
            .await
    }

    pub(crate) async fn hide_timeline_item(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<()> {
        if stable_key.trim().is_empty() {
            return Ok(());
        }
        let now = now_ms();
        self.connection
            .execute(
                "UPDATE timeline_items
                 SET visible = 0,
                     version_sequence = version_sequence + 1,
                     updated_at_ms = ?3
                 WHERE session_id = ?1 AND stable_key = ?2",
                params![session_id, stable_key, now as i64],
            )
            .await
            .map(|_| ())
            .map_err(database_error)
    }
    pub async fn create_turn(&self, session_id: &str, status: &str) -> HamburResult<TurnRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("turn");
        let now = now_ms();
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO turns
                    (id, session_id, status, created_at_ms, updated_at_ms, finished_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?4, NULL)",
                params![id.clone(), session_id, status.clone(), now as i64],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            status,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: 0,
            selected_provider_id: String::new(),
            selected_provider_name: String::new(),
            provider_protocol: String::new(),
            selected_model_id: String::new(),
            selected_model_name: String::new(),
            model_group_id: String::new(),
            error_code: String::new(),
            error_message: String::new(),
        })
    }

    pub async fn create_turn_with_route(
        &self,
        session_id: &str,
        status: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<TurnRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("turn");
        let now = now_ms();
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO turns
                    (
                        id,
                        session_id,
                        status,
                        created_at_ms,
                        updated_at_ms,
                        finished_at_ms,
                        selected_provider_id,
                        selected_provider_name,
                        provider_protocol,
                        selected_model_id,
                        selected_model_name,
                        model_group_id,
                        error_code,
                        error_message
                    )
                 VALUES (?1, ?2, ?3, ?4, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, '', '')",
                params![
                    id.clone(),
                    session_id,
                    status.clone(),
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone()
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            status,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: 0,
            selected_provider_id: route.provider_id.clone(),
            selected_provider_name: route.provider_name.clone(),
            provider_protocol: route.provider_protocol.clone(),
            selected_model_id: route.model_id.clone(),
            selected_model_name: route.model_display_name.clone(),
            model_group_id: route.model_group_id.clone(),
            error_code: String::new(),
            error_message: String::new(),
        })
    }

    pub async fn update_turn_status(
        &self,
        turn_id: &str,
        status: &str,
        finished: bool,
    ) -> HamburResult<TurnRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET status = ?1,
                     updated_at_ms = ?2,
                     finished_at_ms = CASE WHEN ?3 THEN ?2 ELSE finished_at_ms END
                 WHERE id = ?4",
                params![status, now as i64, finished, turn_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }

        self.turn_by_id(turn_id).await
    }

    pub async fn update_turn_route_snapshot(
        &self,
        turn_id: &str,
        route: &ModelRouteSnapshot,
    ) -> HamburResult<TurnRecord> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET updated_at_ms = ?1,
                     selected_provider_id = ?2,
                     selected_provider_name = ?3,
                     provider_protocol = ?4,
                     selected_model_id = ?5,
                     selected_model_name = ?6,
                     model_group_id = ?7
                 WHERE id = ?8",
                params![
                    now as i64,
                    route.provider_id.clone(),
                    route.provider_name.clone(),
                    route.provider_protocol.clone(),
                    route.model_id.clone(),
                    route.model_display_name.clone(),
                    route.model_group_id.clone(),
                    turn_id,
                ],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }
        self.turn_by_id(turn_id).await
    }

    pub async fn fail_turn(
        &self,
        turn_id: &str,
        status: &str,
        error_code: &str,
        error_message: &str,
    ) -> HamburResult<TurnRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET status = ?1,
                     updated_at_ms = ?2,
                     finished_at_ms = ?2,
                     error_code = ?3,
                     error_message = ?4
                 WHERE id = ?5",
                params![status, now as i64, error_code, error_message, turn_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }

        self.turn_by_id(turn_id).await
    }

    pub async fn mark_session_memory_reviewed(
        &self,
        session_id: &str,
        reviewed: bool,
    ) -> HamburResult<()> {
        self.ensure_session_exists(session_id).await?;
        self.connection
            .execute(
                "UPDATE sessions SET memory_reviewed = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![reviewed, session_id],
            )
            .await
            .map(|_| ())
            .map_err(database_error)
    }

    pub async fn mark_session_memory_dirty(&self, session_id: &str) -> HamburResult<()> {
        self.ensure_session_exists(session_id).await?;
        self.connection
            .execute(
                "UPDATE sessions SET memory_reviewed = 0 WHERE id = ?1 AND deleted_at_ms IS NULL",
                params![session_id],
            )
            .await
            .map(|_| ())
            .map_err(database_error)
    }

    pub async fn hide_visible_timeline_after_message(
        &self,
        session_id: &str,
        message_id: &str,
        include_message: bool,
    ) -> HamburResult<()> {
        self.ensure_session_exists(session_id).await?;
        let message = self.message_snapshot(message_id).await?.ok_or_else(|| {
            HamburError::InvalidCommand(format!("message not found: {message_id}"))
        })?;
        if message.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "message does not belong to session: {message_id}"
            )));
        }
        let comparator = if include_message { ">=" } else { ">" };
        let mut rows = self
            .connection
            .query(
                format!(
                    "SELECT id FROM messages
                     WHERE session_id = ?1
                       AND created_at_ms {comparator} ?2"
                )
                .as_str(),
                params![session_id, message.created_at_ms as i64],
            )
            .await
            .map_err(database_error)?;
        let mut message_ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            message_ids.push(row.get::<String>(0).map_err(database_error)?);
        }
        if message_ids.is_empty() {
            return Ok(());
        }
        let now = now_ms();
        for hidden_message_id in message_ids {
            self.hide_timeline_for_message_id(session_id, &hidden_message_id, now)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn hide_timeline_for_message_id(
        &self,
        session_id: &str,
        message_id: &str,
        now: u64,
    ) -> HamburResult<()> {
        let message = self.message_snapshot(message_id).await?.ok_or_else(|| {
            HamburError::InvalidCommand(format!("message not found: {message_id}"))
        })?;
        self.connection
            .execute(
                "UPDATE timeline_items
                 SET visible = 0,
                     version_sequence = version_sequence + 1,
                     updated_at_ms = ?1
                 WHERE session_id = ?2
                   AND visible = 1
                   AND payload_ref = ?3",
                params![now as i64, session_id, message_id],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "UPDATE timeline_items
                 SET visible = 0,
                     version_sequence = version_sequence + 1,
                     updated_at_ms = ?1
                 WHERE session_id = ?2
                   AND visible = 1
                   AND payload_ref IN (
                       SELECT id FROM message_blocks
                       WHERE session_id = ?2 AND message_id = ?3
                   )",
                params![now as i64, session_id, message_id],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "UPDATE timeline_items
                 SET visible = 0,
                     version_sequence = version_sequence + 1,
                     updated_at_ms = ?1
                 WHERE session_id = ?2
                   AND visible = 1
                   AND payload_ref IN (
                       SELECT ts.id
                       FROM trace_spans ts
                       LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                       LEFT JOIN messages tm ON tm.tool_call_id = tc.id
                       WHERE ts.session_id = ?2
                         AND (
                             tc.assistant_message_id = ?3
                             OR tm.id = ?3
                             OR (
                                 ?4 = 'assistant'
                                 AND ts.turn_id = ?5
                                 AND ts.started_at_ms >= ?6
                             )
                         )
                   )",
                params![
                    now as i64,
                    session_id,
                    message_id,
                    message.role.as_str(),
                    message.turn_id.as_str(),
                    message.created_at_ms as i64
                ],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }
    pub async fn upsert_provider(&self, input: ProviderUpsert) -> HamburResult<ProviderRecord> {
        let provider_id = normalize_provider_id(&input.id);
        let name = normalize_title(&input.name);
        let icon_name = input.icon_name.trim().chars().take(80).collect::<String>();
        let api_type = input.api_type.trim();
        if api_type != "OpenAiCompatible" {
            return Err(HamburError::InvalidCommand(format!(
                "unsupported provider api_type: {api_type}"
            )));
        }
        let base_url = normalize_base_url(&input.base_url)?;
        let secret_ref = normalize_secret_ref(&input.secret_ref)?;
        let now = now_ms();

        self.connection
            .execute(
                "INSERT INTO providers
                    (id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    icon_name = excluded.icon_name,
                    api_type = excluded.api_type,
                    base_url = excluded.base_url,
                    secret_ref = excluded.secret_ref,
                    enabled = excluded.enabled,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    provider_id.clone(),
                    name,
                    icon_name,
                    api_type,
                    base_url,
                    secret_ref,
                    input.enabled,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;

        self.provider_by_id(&provider_id).await
    }

    pub async fn delete_provider(&self, provider_id: &str) -> HamburResult<()> {
        let provider_id = provider_id.trim();
        if provider_id.is_empty() {
            return Err(HamburError::InvalidCommand(
                "provider_id must not be empty".to_string(),
            ));
        }
        let changed = self
            .connection
            .execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .await
            .map_err(database_error)?;
        if changed == 0 {
            return Err(HamburError::ProviderUnavailable(format!(
                "provider not found: {provider_id}"
            )));
        }
        Ok(())
    }

    pub async fn replace_provider_models(
        &self,
        provider_id: &str,
        models: Vec<ProviderModelUpsert>,
    ) -> HamburResult<Vec<ProviderModelRecord>> {
        self.provider_by_id(provider_id).await?;
        if models.is_empty() {
            return Err(HamburError::InvalidCommand(
                "model refresh returned no usable models".to_string(),
            ));
        }

        self.connection
            .execute(
                "DELETE FROM provider_models WHERE provider_id = ?1",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let now = now_ms();
        for model in models {
            let model_id = model.model_id.trim();
            if model_id.is_empty() {
                continue;
            }
            let display_name = if model.display_name.trim().is_empty() {
                model_id.to_string()
            } else {
                model.display_name.trim().chars().take(160).collect()
            };
            let row_id = new_id("pmod");
            self.connection
                .execute(
                    "INSERT INTO provider_models
                        (
                            id,
                            provider_id,
                            model_id,
                            display_name,
                            supports_tool_call,
                            supports_reasoning,
                            supports_image_input,
                            supports_structured_output,
                            supports_temperature,
                            context_limit,
                            output_limit,
                            reasoning_field,
                            metadata_json,
                            synced_at_ms
                        )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        row_id,
                        provider_id,
                        model_id,
                        display_name,
                        model.supports_tool_call,
                        model.supports_reasoning,
                        model.supports_image_input,
                        model.supports_structured_output,
                        model.supports_temperature,
                        model.context_limit.max(1) as i64,
                        model.output_limit.max(1) as i64,
                        model.reasoning_field,
                        model.metadata_json,
                        now as i64
                    ],
                )
                .await
                .map_err(database_error)?;
        }

        let saved = self.provider_models(provider_id).await?;
        if let Some(first) = saved.first() {
            self.ensure_default_model_groups(provider_id, &first.model_id)
                .await?;
        }
        Ok(saved)
    }

    pub async fn upsert_provider_model_override(
        &self,
        input: ProviderModelOverride,
    ) -> HamburResult<ProviderModelRecord> {
        let provider_id = input.provider_id.trim().to_string();
        let model_id = input.model_id.trim().to_string();
        if provider_id.is_empty() || model_id.is_empty() {
            return Err(HamburError::InvalidCommand(
                "provider_id and model_id must not be empty".to_string(),
            ));
        }
        self.provider_by_id(&provider_id).await?;
        let display_name = if input.display_name.trim().is_empty() {
            model_id.clone()
        } else {
            input.display_name.trim().chars().take(160).collect()
        };
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO provider_models
                    (
                        id,
                        provider_id,
                        model_id,
                        display_name,
                        supports_tool_call,
                        supports_reasoning,
                        supports_image_input,
                        supports_structured_output,
                        supports_temperature,
                        context_limit,
                        output_limit,
                        reasoning_field,
                        metadata_json,
                        synced_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(provider_id, model_id) DO UPDATE SET
                    display_name = excluded.display_name,
                    supports_tool_call = excluded.supports_tool_call,
                    supports_reasoning = excluded.supports_reasoning,
                    supports_image_input = excluded.supports_image_input,
                    supports_structured_output = excluded.supports_structured_output,
                    supports_temperature = excluded.supports_temperature,
                    context_limit = excluded.context_limit,
                    output_limit = excluded.output_limit,
                    reasoning_field = excluded.reasoning_field,
                    metadata_json = excluded.metadata_json,
                    synced_at_ms = excluded.synced_at_ms",
                params![
                    new_id("pmod"),
                    provider_id.clone(),
                    model_id.clone(),
                    display_name,
                    input.supports_tool_call,
                    input.supports_reasoning,
                    input.supports_image_input,
                    input.supports_structured_output,
                    input.supports_temperature,
                    input.context_limit.max(1) as i64,
                    input.output_limit.max(1) as i64,
                    input.reasoning_field,
                    input.metadata_json,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;

        self.provider_model_by_key(&provider_id, &model_id).await
    }

    pub async fn ensure_default_model_groups(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<()> {
        let now = now_ms();
        for (group_id, name, default_key) in [
            ("grp_primary_chat", "Primary Chat", "primary"),
            (
                "grp_secondary_background",
                "Secondary Background",
                "secondary",
            ),
        ] {
            self.connection
                .execute(
                    "INSERT INTO model_groups
                        (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, 'fallback', 'default', ?3, ?3)
                     ON CONFLICT(id) DO UPDATE SET updated_at_ms = excluded.updated_at_ms",
                    params![group_id, name, now as i64],
                )
                .await
                .map_err(database_error)?;
            self.connection
                .execute(
                    "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET
                        group_id = excluded.group_id,
                        updated_at_ms = excluded.updated_at_ms",
                    params![default_key, group_id, now as i64],
                )
                .await
                .map_err(database_error)?;
        }

        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, 'grp_primary_chat', ?2, ?3, 0, 1)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET enabled = 1",
                params![new_id("mgm"), provider_id, model_id],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn upsert_primary_chat_member(
        &self,
        provider_id: &str,
        model_id: &str,
        position: u32,
    ) -> HamburResult<()> {
        self.provider_by_id(provider_id).await?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO model_groups
                    (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                 VALUES ('grp_primary_chat', 'Primary Chat', 'fallback', 'default', ?1, ?1)
                 ON CONFLICT(id) DO UPDATE SET updated_at_ms = excluded.updated_at_ms",
                params![now as i64],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                 VALUES ('primary', 'grp_primary_chat', ?1)
                 ON CONFLICT(key) DO UPDATE SET
                    group_id = excluded.group_id,
                    updated_at_ms = excluded.updated_at_ms",
                params![now as i64],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, 'grp_primary_chat', ?2, ?3, ?4, 1)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET
                    position = excluded.position,
                    enabled = 1",
                params![new_id("mgm"), provider_id, model_id, position as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn upsert_model_group(
        &self,
        group_id: &str,
        name: &str,
        routing_strategy: &str,
        fallback_policy: &str,
    ) -> HamburResult<ModelGroupRecord> {
        let group_id = normalize_setting_id(group_id, "grp");
        let name = normalize_title(name);
        let routing_strategy = normalize_routing_strategy(routing_strategy)?;
        let fallback_policy = normalize_fallback_policy(fallback_policy)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO model_groups
                    (id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    routing_strategy = excluded.routing_strategy,
                    fallback_policy = excluded.fallback_policy,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    group_id.clone(),
                    name,
                    routing_strategy,
                    fallback_policy,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.model_group_by_id(&group_id).await
    }

    pub async fn upsert_model_group_member(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
        position: u32,
        enabled: bool,
    ) -> HamburResult<ModelGroupMemberRecord> {
        self.model_group_by_id(group_id).await?;
        self.provider_model_by_key(provider_id, model_id).await?;
        self.connection
            .execute(
                "INSERT INTO model_group_members
                    (id, group_id, provider_id, model_id, position, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(group_id, provider_id, model_id) DO UPDATE SET
                    position = excluded.position,
                    enabled = excluded.enabled",
                params![
                    new_id("mgm"),
                    group_id,
                    provider_id,
                    model_id,
                    position as i64,
                    enabled
                ],
            )
            .await
            .map_err(database_error)?;
        self.model_group_member_by_key(group_id, provider_id, model_id)
            .await
    }

    pub async fn delete_model_group_member(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<()> {
        let group_id = normalize_setting_id(group_id, "grp");
        let provider_id = normalize_provider_id(provider_id);
        self.connection
            .execute(
                "DELETE FROM model_group_members WHERE group_id = ?1 AND provider_id = ?2 AND model_id = ?3",
                params![group_id, provider_id, model_id.trim()],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn set_default_model_group(
        &self,
        key: &str,
        group_id: &str,
    ) -> HamburResult<DefaultModelGroupRecord> {
        let key = normalize_default_group_key(key)?;
        self.model_group_by_id(group_id).await?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO default_model_groups (key, group_id, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    group_id = excluded.group_id,
                    updated_at_ms = excluded.updated_at_ms",
                params![key.clone(), group_id, now as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(DefaultModelGroupRecord {
            key,
            group_id: group_id.to_string(),
            updated_at_ms: now,
        })
    }

    pub async fn upsert_app_setting(
        &self,
        key: &str,
        value: &str,
    ) -> HamburResult<AppSettingRecord> {
        let key = normalize_app_setting_key(key)?;
        let value = normalize_app_setting_value(&key, value)?;
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO app_settings (key, value, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at_ms = excluded.updated_at_ms",
                params![key.clone(), value.clone(), now as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(AppSettingRecord {
            key,
            value,
            updated_at_ms: now,
        })
    }

    pub async fn upsert_model_catalog_cache(
        &self,
        key: &str,
        catalog_json: &str,
        synced_at_ms: u64,
    ) -> HamburResult<ModelCatalogCacheRecord> {
        let key = normalize_setting_id(key, "model-catalog");
        self.connection
            .execute(
                "INSERT INTO model_catalog_cache (key, catalog_json, synced_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    catalog_json = excluded.catalog_json,
                    synced_at_ms = excluded.synced_at_ms",
                params![key.clone(), catalog_json, synced_at_ms as i64],
            )
            .await
            .map_err(database_error)?;
        Ok(ModelCatalogCacheRecord {
            key,
            catalog_json: catalog_json.to_string(),
            synced_at_ms,
        })
    }

    pub async fn delete_model_group(&self, group_id: &str) -> HamburResult<()> {
        let group_id = normalize_setting_id(group_id, "grp");
        self.connection
            .execute(
                "DELETE FROM default_model_groups WHERE group_id = ?1",
                params![group_id.clone()],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute("DELETE FROM model_groups WHERE id = ?1", params![group_id])
            .await
            .map_err(database_error)?;
        Ok(())
    }

    pub async fn delete_app_setting(&self, key: &str) -> HamburResult<()> {
        let key = normalize_app_setting_key(key)?;
        self.connection
            .execute("DELETE FROM app_settings WHERE key = ?1", params![key])
            .await
            .map_err(database_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_config_audit(
        &self,
        command_id: &str,
        actor: &str,
        action: &str,
        target_kind: &str,
        target_id: &str,
        redacted_summary: &str,
        approval_required: bool,
        approval_token: &str,
    ) -> HamburResult<ConfigAuditRecord> {
        let id = new_id("audit");
        let now = now_ms();
        let record = ConfigAuditRecord {
            id: id.clone(),
            command_id: command_id.trim().chars().take(120).collect(),
            actor: actor.trim().chars().take(80).collect(),
            action: action.trim().chars().take(80).collect(),
            target_kind: target_kind.trim().chars().take(80).collect(),
            target_id: target_id.trim().chars().take(180).collect(),
            redacted_summary: redacted_summary.trim().chars().take(500).collect(),
            approval_required,
            approval_token: approval_token.trim().chars().take(160).collect(),
            created_at_ms: now,
        };
        self.connection
            .execute(
                "INSERT INTO config_audit
                    (id, command_id, actor, action, target_kind, target_id, redacted_summary, approval_required, approval_token, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    record.id.clone(),
                    record.command_id.clone(),
                    record.actor.clone(),
                    record.action.clone(),
                    record.target_kind.clone(),
                    record.target_id.clone(),
                    record.redacted_summary.clone(),
                    record.approval_required,
                    record.approval_token.clone(),
                    record.created_at_ms as i64
                ],
            )
            .await
            .map_err(database_error)?;
        Ok(record)
    }

    pub(crate) async fn migrate(&self) -> HamburResult<()> {
        self.connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    purpose TEXT NOT NULL DEFAULT 'chat',
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    pinned_at_ms INTEGER NOT NULL DEFAULT 0,
                    memory_reviewed INTEGER NOT NULL DEFAULT 0,
                    deleted_at_ms INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_active_updated
                    ON sessions(deleted_at_ms, pinned_at_ms DESC, created_at_ms DESC);

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    turn_id TEXT NOT NULL DEFAULT '',
                    role TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'completed',
                    content_text TEXT NOT NULL,
                    reasoning_content TEXT NOT NULL DEFAULT '',
                    created_at_ms INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL DEFAULT 1,
                    provider_id_snapshot TEXT NOT NULL DEFAULT '',
                    provider_name_snapshot TEXT NOT NULL DEFAULT '',
                    provider_protocol TEXT NOT NULL DEFAULT '',
                    model_id_snapshot TEXT NOT NULL DEFAULT '',
                    model_name_snapshot TEXT NOT NULL DEFAULT '',
                    model_group_id TEXT NOT NULL DEFAULT '',
                    finish_reason TEXT NOT NULL DEFAULT '',
                    native_finish_reason TEXT NOT NULL DEFAULT '',
                    tool_call_id TEXT NOT NULL DEFAULT '',
                    tool_name TEXT NOT NULL DEFAULT '',
                    tool_title TEXT NOT NULL DEFAULT '',
                    prompt_prefix TEXT NOT NULL DEFAULT ''
                );

                CREATE INDEX IF NOT EXISTS idx_messages_session_order
                    ON messages(session_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS timeline_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    stable_key TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    display_sequence INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    payload_ref TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    visible INTEGER NOT NULL DEFAULT 1,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_timeline_items_session_order
                    ON timeline_items(session_id, display_sequence, id);

                CREATE TABLE IF NOT EXISTS markdown_blocks (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL,
                    block_id INTEGER NOT NULL,
                    stable_key TEXT NOT NULL,
                    committed INTEGER NOT NULL,
                    payload_json TEXT NOT NULL,
                    raw TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_markdown_blocks_session_message
                    ON markdown_blocks(session_id, message_id, block_id);

                CREATE TABLE IF NOT EXISTS message_blocks (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL,
                    block_id INTEGER NOT NULL,
                    block_type TEXT NOT NULL,
                    stable_key TEXT NOT NULL,
                    committed INTEGER NOT NULL,
                    payload_json TEXT NOT NULL,
                    raw TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_message_blocks_session_message
                    ON message_blocks(session_id, message_id, block_type, block_id);

                INSERT OR IGNORE INTO message_blocks
                    (
                        id,
                        session_id,
                        message_id,
                        block_id,
                        block_type,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    )
                SELECT
                    id,
                    session_id,
                    message_id,
                    block_id,
                    'content',
                    stable_key,
                    committed,
                    payload_json,
                    raw,
                    small_summary,
                    version_sequence,
                    created_at_ms,
                    updated_at_ms
                FROM markdown_blocks;

                INSERT OR IGNORE INTO message_blocks
                    (
                        id,
                        session_id,
                        message_id,
                        block_id,
                        block_type,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    )
                SELECT
                    m.id || ':reasoning:block',
                    m.session_id,
                    m.id,
                    0,
                    'reasoning',
                    m.id || ':reasoning',
                    1,
                    '',
                    m.reasoning_content,
                    substr(m.reasoning_content, 1, 160),
                    m.version_sequence,
                    m.created_at_ms,
                    m.created_at_ms
                FROM messages m
                WHERE m.role = 'assistant'
                  AND trim(m.reasoning_content) != '';

                INSERT OR IGNORE INTO timeline_items
                    (
                        id,
                        session_id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind,
                        visible,
                        created_at_ms,
                        updated_at_ms
                    )
                SELECT
                    mb.id || ':timeline',
                    mb.session_id,
                    mb.stable_key,
                    'assistant_reasoning_block',
                    max(m.created_at_ms - 1, 0),
                    mb.version_sequence,
                    mb.id,
                    mb.small_summary,
                    'AssistantReasoningBlock',
                    1,
                    mb.created_at_ms,
                    mb.updated_at_ms
                FROM message_blocks mb
                JOIN messages m ON m.id = mb.message_id
                WHERE mb.block_type = 'reasoning';

                CREATE TABLE IF NOT EXISTS turns (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    status TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    finished_at_ms INTEGER,
                    selected_provider_id TEXT NOT NULL DEFAULT '',
                    selected_provider_name TEXT NOT NULL DEFAULT '',
                    provider_protocol TEXT NOT NULL DEFAULT '',
                    selected_model_id TEXT NOT NULL DEFAULT '',
                    selected_model_name TEXT NOT NULL DEFAULT '',
                    model_group_id TEXT NOT NULL DEFAULT '',
                    error_code TEXT NOT NULL DEFAULT '',
                    error_message TEXT NOT NULL DEFAULT ''
                );

                CREATE INDEX IF NOT EXISTS idx_turns_session_updated
                    ON turns(session_id, updated_at_ms DESC, id);

                CREATE TABLE IF NOT EXISTS trace_spans (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    parent_span_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER,
                    tool_call_id TEXT NOT NULL DEFAULT '',
                    payload_json TEXT NOT NULL DEFAULT '',
                    visible INTEGER NOT NULL DEFAULT 1
                );

                CREATE INDEX IF NOT EXISTS idx_trace_spans_session_started
                    ON trace_spans(session_id, started_at_ms, id);

                CREATE TABLE IF NOT EXISTS tool_calls (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    assistant_message_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    arguments_json TEXT NOT NULL,
                    display_title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    requires_approval INTEGER NOT NULL,
                    approval_status TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER,
                    result_id TEXT NOT NULL DEFAULT '',
                    error_code TEXT NOT NULL DEFAULT '',
                    error_message TEXT NOT NULL DEFAULT '',
                    call_index INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_tool_calls_turn_started
                    ON tool_calls(turn_id, started_at_ms, id);

                CREATE TABLE IF NOT EXISTS tool_results (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    turn_id TEXT NOT NULL,
                    tool_call_id TEXT NOT NULL REFERENCES tool_calls(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL,
                    is_error INTEGER NOT NULL,
                    content_json TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    artifacts_json TEXT NOT NULL,
                    trust_level TEXT NOT NULL,
                    truncated INTEGER NOT NULL,
                    offloaded_file_id TEXT NOT NULL DEFAULT '',
                    offloaded_path TEXT NOT NULL DEFAULT '',
                    context_stub TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_tool_results_tool_call
                    ON tool_results(tool_call_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS files (
                    id TEXT PRIMARY KEY NOT NULL,
                    scope TEXT NOT NULL,
                    session_id TEXT NOT NULL DEFAULT '',
                    relative_path TEXT NOT NULL,
                    sandbox_path TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    sha256 TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    retention_policy TEXT NOT NULL
                );

                CREATE UNIQUE INDEX IF NOT EXISTS idx_files_sandbox_path
                    ON files(sandbox_path);

                CREATE INDEX IF NOT EXISTS idx_files_session_scope
                    ON files(session_id, scope, created_at_ms);

                CREATE TABLE IF NOT EXISTS attachments (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    message_id TEXT NOT NULL DEFAULT '',
                    kind TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    origin_type TEXT NOT NULL,
                    original_uri TEXT NOT NULL,
                    file_id TEXT NOT NULL REFERENCES files(id),
                    sandbox_path TEXT NOT NULL,
                    width INTEGER NOT NULL DEFAULT 0,
                    height INTEGER NOT NULL DEFAULT 0,
                    sha256 TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_attachments_session_pending
                    ON attachments(session_id, message_id, status, created_at_ms);

                CREATE INDEX IF NOT EXISTS idx_attachments_message
                    ON attachments(message_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS file_cleanup_jobs (
                    id TEXT PRIMARY KEY NOT NULL,
                    file_id TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    status TEXT NOT NULL,
                    attempts INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_file_cleanup_jobs_status
                    ON file_cleanup_jobs(status, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS providers (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    icon_name TEXT NOT NULL,
                    api_type TEXT NOT NULL,
                    base_url TEXT NOT NULL,
                    secret_ref TEXT NOT NULL,
                    enabled INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS provider_models (
                    id TEXT PRIMARY KEY NOT NULL,
                    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                    model_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    supports_tool_call INTEGER NOT NULL,
                    supports_reasoning INTEGER NOT NULL,
                    supports_image_input INTEGER NOT NULL,
                    supports_structured_output INTEGER NOT NULL,
                    supports_temperature INTEGER NOT NULL,
                    context_limit INTEGER NOT NULL,
                    output_limit INTEGER NOT NULL,
                    reasoning_field TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    synced_at_ms INTEGER NOT NULL,
                    UNIQUE(provider_id, model_id)
                );

                CREATE TABLE IF NOT EXISTS model_groups (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    routing_strategy TEXT NOT NULL,
                    fallback_policy TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS model_group_members (
                    id TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL REFERENCES model_groups(id) ON DELETE CASCADE,
                    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
                    model_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    enabled INTEGER NOT NULL,
                    UNIQUE(group_id, provider_id, model_id)
                );

                CREATE TABLE IF NOT EXISTS default_model_groups (
                    key TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS app_settings (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS model_catalog_cache (
                    key TEXT PRIMARY KEY NOT NULL,
                    catalog_json TEXT NOT NULL,
                    synced_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS config_audit (
                    id TEXT PRIMARY KEY NOT NULL,
                    command_id TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    action TEXT NOT NULL,
                    target_kind TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    redacted_summary TEXT NOT NULL,
                    approval_required INTEGER NOT NULL,
                    approval_token TEXT NOT NULL DEFAULT '',
                    created_at_ms INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_config_audit_created
                    ON config_audit(created_at_ms DESC, id DESC);
                ",
            )
            .await
            .map_err(database_error)?;

        self.add_column_if_missing("sessions", "pinned_at_ms", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.add_column_if_missing("messages", "turn_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "status", "TEXT NOT NULL DEFAULT 'completed'")
            .await?;
        self.add_column_if_missing("messages", "reasoning_content", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "provider_id_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing(
            "messages",
            "provider_name_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "provider_protocol", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "model_id_snapshot", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "model_name_snapshot",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "model_group_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "finish_reason", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "messages",
            "native_finish_reason",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("messages", "tool_call_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "tool_name", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "tool_title", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("messages", "prompt_prefix", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("timeline_items", "visible", "INTEGER NOT NULL DEFAULT 1")
            .await?;
        self.add_column_if_missing("turns", "selected_provider_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing(
            "turns",
            "selected_provider_name",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing("turns", "provider_protocol", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "selected_model_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "selected_model_name", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "model_group_id", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "error_code", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("turns", "error_message", "TEXT NOT NULL DEFAULT ''")
            .await?;
        self.add_column_if_missing("sessions", "memory_reviewed", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.add_column_if_missing("sessions", "purpose", "TEXT NOT NULL DEFAULT 'chat'")
            .await?;
        self.connection
            .execute(
                "UPDATE sessions
                 SET purpose = 'delegate'
                 WHERE purpose = 'chat' AND title LIKE 'Delegate:%'",
                params![],
            )
            .await
            .map_err(database_error)?;
        self.connection
            .execute(
                "DELETE FROM app_state
                 WHERE key = 'active_session_id'
                   AND value IN (
                     SELECT id FROM sessions WHERE purpose != 'chat'
                   )",
                params![],
            )
            .await
            .map_err(database_error)?;
        self.add_column_if_missing("tool_calls", "call_index", "INTEGER NOT NULL DEFAULT 0")
            .await?;

        Ok(())
    }
    pub(crate) async fn add_column_if_missing(
        &self,
        table: &str,
        column: &str,
        definition: &str,
    ) -> HamburResult<()> {
        let pragma = format!("PRAGMA table_info({table})");
        let mut rows = self
            .connection
            .query(pragma.as_str(), params![])
            .await
            .map_err(database_error)?;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let name = row.get::<String>(1).map_err(database_error)?;
            if name == column {
                return Ok(());
            }
        }

        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
        self.connection
            .execute(sql.as_str(), params![])
            .await
            .map(|_| ())
            .map_err(database_error)
    }
    pub(crate) async fn ensure_session_exists(&self, session_id: &str) -> HamburResult<()> {
        if self.session_exists(session_id).await? {
            Ok(())
        } else {
            Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )))
        }
    }
    pub(crate) async fn touch_session(&self, session_id: &str, now: u64) -> HamburResult<()> {
        self.connection
            .execute(
                "UPDATE sessions
                 SET updated_at_ms = ?1,
                     memory_reviewed = 0
                 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }
    pub(crate) async fn set_active_session(&self, session_id: Option<&str>) -> HamburResult<()> {
        let now = now_ms();
        match session_id {
            Some(session_id) => self
                .connection
                .execute(
                    "INSERT INTO app_state (key, value, updated_at_ms)
                     VALUES ('active_session_id', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET
                        value = excluded.value,
                        updated_at_ms = excluded.updated_at_ms",
                    params![session_id, now as i64],
                )
                .await
                .map(|_| ())
                .map_err(database_error),
            None => self
                .connection
                .execute(
                    "DELETE FROM app_state WHERE key = 'active_session_id'",
                    params![],
                )
                .await
                .map(|_| ())
                .map_err(database_error),
        }
    }
}
