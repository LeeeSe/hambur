use crate::*;

impl HamburDatabase {
    pub async fn bootstrap_snapshot(&self) -> HamburResult<AppSnapshot> {
        self.snapshot_for_selected(None).await
    }

    pub async fn open_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        if !self.session_exists(session_id).await? {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        let now = now_ms();
        self.connection
            .execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        self.set_active_session(Some(session_id)).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn pending_attachments_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<AttachmentRecord>> {
        self.ensure_session_exists(session_id).await?;
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
                WHERE session_id = ?1
                  AND message_id = ''
                  AND status = 'pending'
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            attachments.push(attachment_from_row(&row)?);
        }
        Ok(attachments)
    }

    pub async fn pending_file_cleanup_jobs(
        &self,
        limit: u32,
    ) -> HamburResult<Vec<FileCleanupJobRecord>> {
        let limit = clamp_limit(limit, 1, 100);
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, file_id, relative_path, reason, status, attempts, created_at_ms, updated_at_ms
                FROM file_cleanup_jobs
                WHERE status = 'pending'
                ORDER BY created_at_ms ASC, id ASC
                LIMIT ?1
                ",
                params![limit as i64],
            )
            .await
            .map_err(database_error)?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            jobs.push(file_cleanup_job_from_row(&row)?);
        }
        Ok(jobs)
    }

    pub async fn resolve_file_by_sandbox_path(
        &self,
        session_id: &str,
        sandbox_path: &str,
    ) -> HamburResult<FileRecord> {
        self.ensure_session_exists(session_id).await?;
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM files
                WHERE sandbox_path = ?1
                  AND (scope != 'session' OR session_id = ?2)
                LIMIT 1
                ",
                params![sandbox_path, session_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file not found for sandbox path: {sandbox_path}"
            )));
        };
        file_from_row(&row)
    }

    pub(crate) async fn markdown_block_by_stable_key(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<MarkdownBlockPayloadRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    message_id,
                    block_id,
                    stable_key,
                    committed,
                    payload_json,
                    raw,
                    small_summary,
                    version_sequence,
                    created_at_ms,
                    updated_at_ms
                FROM markdown_blocks
                WHERE session_id = ?1 AND stable_key = ?2
                LIMIT 1
                ",
                params![session_id, stable_key],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::Internal(format!(
                "markdown block not found after upsert: {stable_key}"
            )));
        };
        markdown_block_from_row(&row)
    }
    pub(crate) async fn markdown_blocks_for_payload_refs(
        &self,
        session_id: &str,
        payload_refs: &[String],
    ) -> HamburResult<Vec<MarkdownBlockPayloadRecord>> {
        let mut blocks = Vec::new();
        for payload_ref in payload_refs {
            if payload_ref.trim().is_empty() {
                continue;
            }
            let mut rows = self
                .connection
                .query(
                    "
                    SELECT
                        id,
                        session_id,
                        message_id,
                        block_id,
                        stable_key,
                        committed,
                        payload_json,
                        raw,
                        small_summary,
                        version_sequence,
                        created_at_ms,
                        updated_at_ms
                    FROM markdown_blocks
                    WHERE session_id = ?1 AND id = ?2
                    LIMIT 1
                    ",
                    params![session_id, payload_ref],
                )
                .await
                .map_err(database_error)?;
            if let Some(row) = rows.next().await.map_err(database_error)? {
                blocks.push(markdown_block_from_row(&row)?);
            }
        }
        Ok(blocks)
    }
    pub async fn session_snapshot(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn session_list(&self, limit: u32, offset: u32) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(limit, offset).await
    }

    pub async fn session_summary(&self, session_id: &str) -> HamburResult<SessionSummary> {
        self.ensure_session_exists(session_id).await?;
        self.session_summary_by_id(session_id).await
    }

    pub async fn unreviewed_sessions(&self, limit: u32) -> HamburResult<Vec<SessionSummary>> {
        let limit = clamp_limit(limit, 1, 100);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    s.memory_reviewed,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id AND ti.visible = 1
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                  AND s.memory_reviewed = 0
                ORDER BY s.updated_at_ms ASC, s.created_at_ms ASC, s.id ASC
                LIMIT ?1
                ",
                params![limit as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }
        Ok(sessions)
    }

    pub async fn session_review_record(
        &self,
        session_id: &str,
    ) -> HamburResult<SessionReviewRecord> {
        let summary = self.session_summary(session_id).await?;
        Ok(SessionReviewRecord {
            id: summary.id.clone(),
            title: summary.title,
            created_at_ms: summary.created_at_ms,
            updated_at_ms: summary.updated_at_ms,
            memory_reviewed: summary.memory_reviewed,
            messages: self.messages_for_session(session_id).await?,
            trace_spans: self.trace_spans_for_session(session_id).await?,
        })
    }

    pub async fn timeline_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        self.ensure_session_exists(session_id).await?;
        let mut page = self
            .timeline_items_page(session_id, before_cursor, limit)
            .await?;
        let payload_refs = markdown_payload_refs(&page.items);
        page.markdown_block_payloads = self
            .markdown_blocks_for_payload_refs(session_id, &payload_refs)
            .await?;
        Ok(page)
    }

    pub async fn visible_chat_transcript_before_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> HamburResult<Vec<ChatTranscriptEntry>> {
        self.ensure_session_exists(session_id).await?;
        let boundary = self.message_snapshot(message_id).await?.ok_or_else(|| {
            HamburError::InvalidCommand(format!("message not found: {message_id}"))
        })?;
        if boundary.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "message does not belong to session: {message_id}"
            )));
        }

        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    m.id,
                    m.session_id,
                    m.role,
                    m.content_text,
                    m.reasoning_content,
                    m.status,
                    m.turn_id,
                    m.created_at_ms,
                    m.version_sequence,
                    m.provider_id_snapshot,
                    m.provider_name_snapshot,
                    m.provider_protocol,
                    m.model_id_snapshot,
                    m.model_name_snapshot,
                    m.model_group_id,
                    m.finish_reason,
                    m.native_finish_reason,
                    m.tool_call_id,
                    m.tool_name,
                    m.tool_title
                FROM messages m
                WHERE m.session_id = ?1
                  AND (
                      m.created_at_ms < ?2
                      OR (m.created_at_ms = ?2 AND m.id < ?3)
                  )
                  AND (
                      (
                          m.role = 'user'
                          AND EXISTS (
                              SELECT 1
                              FROM timeline_items ti
                              WHERE ti.session_id = m.session_id
                                AND ti.payload_ref = m.id
                                AND ti.visible = 1
                                AND ti.content_type = 'user_message'
                          )
                      )
                      OR (
                          m.role = 'assistant'
                          AND (
                              EXISTS (
                                  SELECT 1
                                  FROM markdown_blocks mb
                                  JOIN timeline_items ti
                                    ON ti.session_id = mb.session_id
                                   AND ti.payload_ref = mb.id
                                   AND ti.visible = 1
                                   AND ti.content_type = 'assistant_markdown_block'
                                  WHERE mb.session_id = m.session_id
                                    AND mb.message_id = m.id
                              )
                              OR EXISTS (
                                  SELECT 1
                                  FROM tool_calls tc
                                  JOIN trace_spans ts
                                    ON ts.tool_call_id = tc.id
                                  JOIN timeline_items ti
                                    ON ti.session_id = ts.session_id
                                   AND ti.payload_ref = ts.id
                                   AND ti.visible = 1
                                   AND ti.content_type = 'trace'
                                  WHERE tc.assistant_message_id = m.id
                              )
                          )
                      )
                      OR (
                          m.role = 'tool'
                          AND EXISTS (
                              SELECT 1
                              FROM tool_calls tc
                              JOIN trace_spans ts
                                ON ts.tool_call_id = tc.id
                              JOIN timeline_items ti
                                ON ti.session_id = ts.session_id
                               AND ti.payload_ref = ts.id
                               AND ti.visible = 1
                               AND ti.content_type = 'trace'
                              WHERE tc.id = m.tool_call_id
                          )
                      )
                  )
                ORDER BY m.created_at_ms ASC, m.id ASC
                ",
                params![
                    session_id,
                    boundary.created_at_ms as i64,
                    boundary.id.clone()
                ],
            )
            .await
            .map_err(database_error)?;

        let mut entries = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let mut message = message_from_row(&row)?;
            message.attachments = self.attachments_for_message(&message.id).await?;
            let tool_calls = if message.role == "assistant" {
                self.tool_calls_for_assistant_message(&message.id).await?
            } else {
                Vec::new()
            };
            entries.push(ChatTranscriptEntry {
                message,
                tool_calls,
            });
        }
        Ok(entries)
    }

    pub async fn message_snapshot(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        if message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "message_id must not be empty".to_string(),
            ));
        }
        let Some(mut message) = self.message_by_id(message_id).await? else {
            return Ok(None);
        };
        message.attachments = self.attachments_for_message(message_id).await?;
        Ok(Some(message))
    }

    pub async fn source_user_message_for(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;
        let source = self.message_snapshot(message_id).await?.ok_or_else(|| {
            HamburError::InvalidCommand(format!("message not found: {message_id}"))
        })?;

        if source.session_id != session_id {
            return Err(HamburError::InvalidCommand(format!(
                "message does not belong to session: {message_id}"
            )));
        }
        if source.role == "user" {
            return Ok(source);
        }

        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    role,
                    content_text,
                    reasoning_content,
                    status,
                    turn_id,
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
                FROM messages
                WHERE session_id = ?1
                  AND role = 'user'
                  AND created_at_ms <= ?2
                ORDER BY created_at_ms DESC, id DESC
                LIMIT 1
                ",
                params![session_id, source.created_at_ms as i64],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "source user message not found for: {message_id}"
            )));
        };

        message_from_row(&row)
    }

    pub async fn search_sessions(
        &self,
        query: &str,
        limit: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let query = query.trim();
        if query.is_empty() {
            return self.list_sessions_page(limit, 0).await;
        }

        let limit = clamp_limit(limit, 1, 100);
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    s.memory_reviewed,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                  AND s.title LIKE ?1 ESCAPE '\\'
                ORDER BY s.pinned_at_ms DESC, s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?2
                ",
                params![pattern, limit as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }

    pub async fn provider_by_id(&self, provider_id: &str) -> HamburResult<ProviderRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms
                FROM providers
                WHERE id = ?1
                LIMIT 1
                ",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ProviderUnavailable(format!(
                "provider not found: {provider_id}"
            )));
        };
        provider_record_from_row(&row)
    }

    pub async fn provider_model_by_key(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<ProviderModelRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM provider_models
                WHERE provider_id = ?1 AND model_id = ?2
                LIMIT 1
                ",
                params![provider_id, model_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "provider model not found: {provider_id}/{model_id}"
            )));
        };
        provider_model_from_row(&row)
    }

    pub async fn provider_models(
        &self,
        provider_id: &str,
    ) -> HamburResult<Vec<ProviderModelRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM provider_models
                WHERE provider_id = ?1
                ORDER BY model_id ASC
                ",
                params![provider_id],
            )
            .await
            .map_err(database_error)?;

        let mut models = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            models.push(provider_model_from_row(&row)?);
        }
        Ok(models)
    }

    pub async fn settings_snapshot(&self) -> HamburResult<SettingsSnapshot> {
        Ok(SettingsSnapshot {
            providers: self.public_providers().await?,
            provider_models: self.all_provider_models().await?,
            model_groups: self.model_groups().await?,
            model_group_members: self.model_group_members().await?,
            default_model_groups: self.default_model_groups().await?,
            settings: self.app_settings().await?,
            config_audits: self.config_audits(40).await?,
        })
    }

    pub async fn primary_chat_route(&self) -> HamburResult<Vec<ModelRouteSnapshot>> {
        self.default_model_group_route("primary").await
    }

    pub async fn memory_review_route(&self) -> HamburResult<Vec<ModelRouteSnapshot>> {
        self.default_model_group_route("secondary").await
    }

    pub(crate) async fn default_model_group_route(
        &self,
        default_key: &str,
    ) -> HamburResult<Vec<ModelRouteSnapshot>> {
        let default_key = normalize_default_group_key(default_key)?;
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    p.id,
                    p.name,
                    p.api_type,
                    p.base_url,
                    p.secret_ref,
                    pm.model_id,
                    pm.display_name,
                    mg.id,
                    mg.name,
                    mg.routing_strategy,
                    mg.fallback_policy,
                    mgm.position,
                    pm.supports_tool_call,
                    pm.supports_reasoning,
                    pm.supports_image_input,
                    pm.supports_structured_output,
                    pm.supports_temperature,
                    pm.context_limit,
                    pm.output_limit,
                    pm.reasoning_field
                FROM default_model_groups d
                JOIN model_groups mg ON mg.id = d.group_id
                JOIN model_group_members mgm ON mgm.group_id = mg.id
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = p.id AND pm.model_id = mgm.model_id
                WHERE d.key = ?1
                  AND p.enabled = 1
                  AND mgm.enabled = 1
                ORDER BY mgm.position ASC, p.id ASC, pm.model_id ASC
                ",
                params![default_key],
            )
            .await
            .map_err(database_error)?;

        let mut targets = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            targets.push(model_route_from_row(&row)?);
        }

        if targets.is_empty() {
            let fallback = self.first_enabled_model_route().await?;
            targets.extend(fallback);
        }
        Ok(targets)
    }
    pub(crate) async fn first_enabled_model_route(&self) -> HamburResult<Vec<ModelRouteSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    p.id,
                    p.name,
                    p.api_type,
                    p.base_url,
                    p.secret_ref,
                    pm.model_id,
                    pm.display_name,
                    'grp_primary_chat',
                    'Primary Chat',
                    'fallback',
                    'default',
                    0,
                    pm.supports_tool_call,
                    pm.supports_reasoning,
                    pm.supports_image_input,
                    pm.supports_structured_output,
                    pm.supports_temperature,
                    pm.context_limit,
                    pm.output_limit,
                    pm.reasoning_field
                FROM providers p
                JOIN provider_models pm ON pm.provider_id = p.id
                WHERE p.enabled = 1
                ORDER BY p.updated_at_ms DESC, pm.model_id ASC
                LIMIT 1
                ",
                params![],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ProviderUnavailable(
                "no enabled provider model is configured".to_string(),
            ));
        };
        Ok(vec![model_route_from_row(&row)?])
    }
    pub(crate) async fn snapshot_for_selected(
        &self,
        requested_session_id: Option<&str>,
    ) -> HamburResult<AppSnapshot> {
        let sessions = self.list_sessions().await?;
        let active_session_id = self.active_session_id().await?;
        let selected_session_id = match requested_session_id {
            Some(session_id) if sessions.iter().any(|session| session.id == session_id) => {
                session_id.to_string()
            }
            _ if active_session_id.as_ref().is_some_and(|session_id| {
                sessions.iter().any(|session| session.id == *session_id)
            }) =>
            {
                active_session_id.unwrap_or_default()
            }
            _ => sessions
                .first()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
        };
        let timeline_items = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            self.timeline_items_for_session(&selected_session_id)
                .await?
        };
        let markdown_block_payloads = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            let payload_refs = markdown_payload_refs(&timeline_items);
            self.markdown_blocks_for_payload_refs(&selected_session_id, &payload_refs)
                .await?
        };
        let pending_attachments = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            self.pending_attachments_for_session(&selected_session_id)
                .await?
        };

        Ok(AppSnapshot {
            sessions,
            selected_session_id,
            timeline_items,
            markdown_block_payloads,
            pending_attachments,
        })
    }
    pub(crate) async fn list_sessions(&self) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(100, 0).await
    }
    pub(crate) async fn list_sessions_page(
        &self,
        limit: u32,
        offset: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let limit = clamp_limit(limit, 1, 200);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    s.memory_reviewed,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id AND ti.visible = 1
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                ORDER BY s.pinned_at_ms DESC, s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?1 OFFSET ?2
                ",
                params![limit as i64, offset as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }
    pub(crate) async fn timeline_items_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<TimelineItemSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    ti.id,
                    ti.stable_key,
                    ti.content_type,
                    ti.display_sequence,
                    ti.version_sequence,
                    ti.payload_ref,
                    ti.small_summary,
                    ti.kind,
                    COALESCE(ts.title, ''),
                    COALESCE(ts.content, ''),
                    COALESCE(ts.status, ''),
                    COALESCE(ts.tool_call_id, ''),
                    COALESCE(tc.name, '')
                FROM timeline_items ti
                LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                WHERE ti.session_id = ?1 AND ti.visible = 1
                ORDER BY ti.display_sequence ASC, ti.id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(TimelineItemSnapshot {
                id: row.get::<String>(0).map_err(database_error)?,
                stable_key: row.get::<String>(1).map_err(database_error)?,
                content_type: row.get::<String>(2).map_err(database_error)?,
                display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
                version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
                payload_ref: row.get::<String>(5).map_err(database_error)?,
                small_summary: row.get::<String>(6).map_err(database_error)?,
                kind: row.get::<String>(7).map_err(database_error)?,
                trace_title: row.get::<String>(8).map_err(database_error)?,
                trace_content: row.get::<String>(9).map_err(database_error)?,
                trace_status: row.get::<String>(10).map_err(database_error)?,
                tool_call_id: row.get::<String>(11).map_err(database_error)?,
                tool_name: row.get::<String>(12).map_err(database_error)?,
            });
        }

        Ok(items)
    }
    pub(crate) async fn timeline_items_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        let limit = clamp_limit(limit, 1, 100);
        let fetch_limit = limit.saturating_add(1);
        let mut rows = if before_cursor == 0 {
            self.connection
                .query(
                    "
                    SELECT
                        ti.id,
                        ti.stable_key,
                        ti.content_type,
                        ti.display_sequence,
                        ti.version_sequence,
                        ti.payload_ref,
                        ti.small_summary,
                        ti.kind,
                        COALESCE(ts.title, ''),
                        COALESCE(ts.content, ''),
                        COALESCE(ts.status, ''),
                        COALESCE(ts.tool_call_id, ''),
                        COALESCE(tc.name, '')
                    FROM timeline_items ti
                    LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                    LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                    WHERE ti.session_id = ?1 AND ti.visible = 1
                    ORDER BY ti.display_sequence DESC, ti.id DESC
                    LIMIT ?2
                    ",
                    params![session_id, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        } else {
            self.connection
                .query(
                    "
                    SELECT
                        ti.id,
                        ti.stable_key,
                        ti.content_type,
                        ti.display_sequence,
                        ti.version_sequence,
                        ti.payload_ref,
                        ti.small_summary,
                        ti.kind,
                        COALESCE(ts.title, ''),
                        COALESCE(ts.content, ''),
                        COALESCE(ts.status, ''),
                        COALESCE(ts.tool_call_id, ''),
                        COALESCE(tc.name, '')
                    FROM timeline_items ti
                    LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                    LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                    WHERE ti.session_id = ?1
                      AND ti.visible = 1
                      AND ti.display_sequence < ?2
                    ORDER BY ti.display_sequence DESC, ti.id DESC
                    LIMIT ?3
                    ",
                    params![session_id, before_cursor as i64, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        };

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(timeline_item_from_row(&row)?);
        }

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        items.reverse();
        let next_before_cursor = if has_more {
            items
                .first()
                .map(|item| item.display_sequence)
                .unwrap_or_default()
        } else {
            0
        };

        Ok(TimelinePageData {
            items,
            markdown_block_payloads: Vec::new(),
            next_before_cursor,
            has_more,
        })
    }
    pub(crate) async fn session_exists(&self, session_id: &str) -> HamburResult<bool> {
        let mut rows = self
            .connection
            .query(
                "SELECT id FROM sessions WHERE id = ?1 AND deleted_at_ms IS NULL LIMIT 1",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        Ok(rows.next().await.map_err(database_error)?.is_some())
    }
    pub(crate) async fn session_summary_by_id(&self, session_id: &str) -> HamburResult<SessionSummary> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    s.pinned_at_ms,
                    s.memory_reviewed,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.id = ?1 AND s.deleted_at_ms IS NULL
                LIMIT 1
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        };

        session_summary_from_row(&row)
    }
    pub(crate) async fn active_session_id(&self) -> HamburResult<Option<String>> {
        let mut rows = self
            .connection
            .query(
                "SELECT value FROM app_state WHERE key = 'active_session_id' LIMIT 1",
                params![],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };
        let value = row.get::<String>(0).map_err(database_error)?;
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }
    pub(crate) async fn timeline_item_by_stable_key(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<TimelineItemSnapshot> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    ti.id,
                    ti.stable_key,
                    ti.content_type,
                    ti.display_sequence,
                    ti.version_sequence,
                    ti.payload_ref,
                    ti.small_summary,
                    ti.kind,
                    COALESCE(ts.title, ''),
                    COALESCE(ts.content, ''),
                    COALESCE(ts.status, ''),
                    COALESCE(ts.tool_call_id, ''),
                    COALESCE(tc.name, '')
                FROM timeline_items ti
                LEFT JOIN trace_spans ts ON ti.content_type = 'trace' AND ts.id = ti.payload_ref
                LEFT JOIN tool_calls tc ON tc.id = ts.tool_call_id
                WHERE ti.session_id = ?1 AND ti.stable_key = ?2 AND ti.visible = 1
                LIMIT 1
                ",
                params![session_id, stable_key],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::Internal(format!(
                "timeline item not found after upsert: {stable_key}"
            )));
        };

        timeline_item_from_row(&row)
    }
    pub(crate) async fn message_by_id(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    role,
                    content_text,
                    reasoning_content,
                    status,
                    turn_id,
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
                FROM messages
                WHERE id = ?1
                LIMIT 1
                ",
                params![message_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };

        Ok(Some(message_from_row(&row)?))
    }
    pub async fn messages_for_session(&self, session_id: &str) -> HamburResult<Vec<MessageRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    session_id,
                    role,
                    content_text,
                    reasoning_content,
                    status,
                    turn_id,
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
                FROM messages
                WHERE session_id = ?1
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut messages = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let mut message = message_from_row(&row)?;
            message.attachments = self.attachments_for_message(&message.id).await?;
            messages.push(message);
        }
        Ok(messages)
    }
    pub(crate) async fn turn_by_id(&self, turn_id: &str) -> HamburResult<TurnRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM turns
                WHERE id = ?1
                LIMIT 1
                ",
                params![turn_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        };

        Ok(TurnRecord {
            id: row.get::<String>(0).map_err(database_error)?,
            session_id: row.get::<String>(1).map_err(database_error)?,
            status: row.get::<String>(2).map_err(database_error)?,
            created_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
            updated_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
            finished_at_ms: row
                .get::<Option<i64>>(5)
                .map_err(database_error)?
                .map(unsigned_ms)
                .unwrap_or_default(),
            selected_provider_id: row.get::<String>(6).map_err(database_error)?,
            selected_provider_name: row.get::<String>(7).map_err(database_error)?,
            provider_protocol: row.get::<String>(8).map_err(database_error)?,
            selected_model_id: row.get::<String>(9).map_err(database_error)?,
            selected_model_name: row.get::<String>(10).map_err(database_error)?,
            model_group_id: row.get::<String>(11).map_err(database_error)?,
            error_code: row.get::<String>(12).map_err(database_error)?,
            error_message: row.get::<String>(13).map_err(database_error)?,
        })
    }
    pub(crate) async fn trace_span_by_id(&self, trace_id: &str) -> HamburResult<TraceSpanRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                    payload_json
                FROM trace_spans
                WHERE id = ?1
                LIMIT 1
                ",
                params![trace_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "trace span not found: {trace_id}"
            )));
        };
        trace_span_from_row(&row)
    }
    pub(crate) async fn trace_spans_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<TraceSpanRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                    payload_json
                FROM trace_spans
                WHERE session_id = ?1
                ORDER BY started_at_ms ASC, id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut traces = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            traces.push(trace_span_from_row(&row)?);
        }
        Ok(traces)
    }
    pub(crate) async fn tool_call_by_id(&self, tool_call_id: &str) -> HamburResult<ToolCallRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM tool_calls
                WHERE id = ?1
                LIMIT 1
                ",
                params![tool_call_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "tool call not found: {tool_call_id}"
            )));
        };
        tool_call_from_row(&row)
    }
    pub(crate) async fn tool_calls_for_assistant_message(
        &self,
        assistant_message_id: &str,
    ) -> HamburResult<Vec<ToolCallRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM tool_calls
                WHERE assistant_message_id = ?1
                ORDER BY call_index ASC, started_at_ms ASC, id ASC
                ",
                params![assistant_message_id],
            )
            .await
            .map_err(database_error)?;

        let mut calls = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            calls.push(tool_call_from_row(&row)?);
        }
        Ok(calls)
    }
    pub(crate) async fn tool_result_by_id(&self, result_id: &str) -> HamburResult<ToolResultRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM tool_results
                WHERE id = ?1
                LIMIT 1
                ",
                params![result_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "tool result not found: {result_id}"
            )));
        };
        tool_result_from_row(&row)
    }
    pub async fn file_by_id(&self, file_id: &str) -> HamburResult<FileRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM files
                WHERE id = ?1
                LIMIT 1
                ",
                params![file_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file not found: {file_id}"
            )));
        };
        file_from_row(&row)
    }

    pub async fn attachment_by_id(&self, attachment_id: &str) -> HamburResult<AttachmentRecord> {
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
                WHERE id = ?1
                LIMIT 1
                ",
                params![attachment_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "attachment not found: {attachment_id}"
            )));
        };
        attachment_from_row(&row)
    }

    pub async fn attachments_for_message(
        &self,
        message_id: &str,
    ) -> HamburResult<Vec<AttachmentRecord>> {
        if message_id.trim().is_empty() {
            return Ok(Vec::new());
        }
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
                WHERE message_id = ?1
                  AND status = 'attached'
                ORDER BY created_at_ms ASC, id ASC
                ",
                params![message_id],
            )
            .await
            .map_err(database_error)?;
        let mut attachments = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            attachments.push(attachment_from_row(&row)?);
        }
        Ok(attachments)
    }

    pub(crate) async fn file_cleanup_job_by_id(&self, job_id: &str) -> HamburResult<FileCleanupJobRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, file_id, relative_path, reason, status, attempts, created_at_ms, updated_at_ms
                FROM file_cleanup_jobs
                WHERE id = ?1
                LIMIT 1
                ",
                params![job_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "file cleanup job not found: {job_id}"
            )));
        };
        file_cleanup_job_from_row(&row)
    }
    pub(crate) async fn public_providers(&self) -> HamburResult<Vec<PublicProviderRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, icon_name, api_type, base_url, secret_ref, enabled, created_at_ms, updated_at_ms
                FROM providers
                ORDER BY updated_at_ms DESC, id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut providers = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            providers.push(public_provider_from_row(&row)?);
        }
        Ok(providers)
    }
    pub(crate) async fn all_provider_models(&self) -> HamburResult<Vec<ProviderModelRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
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
                FROM provider_models
                ORDER BY provider_id ASC, model_id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut models = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            models.push(provider_model_from_row(&row)?);
        }
        Ok(models)
    }
    pub(crate) async fn model_groups(&self) -> HamburResult<Vec<ModelGroupRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms
                FROM model_groups
                ORDER BY id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut groups = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            groups.push(model_group_from_row(&row)?);
        }
        Ok(groups)
    }
    pub(crate) async fn model_group_by_id(&self, group_id: &str) -> HamburResult<ModelGroupRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, name, routing_strategy, fallback_policy, created_at_ms, updated_at_ms
                FROM model_groups
                WHERE id = ?1
                LIMIT 1
                ",
                params![group_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "model group not found: {group_id}"
            )));
        };
        model_group_from_row(&row)
    }
    pub(crate) async fn model_group_members(&self) -> HamburResult<Vec<ModelGroupMemberRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    mgm.id,
                    mgm.group_id,
                    mgm.provider_id,
                    p.name,
                    mgm.model_id,
                    pm.display_name,
                    mgm.position,
                    mgm.enabled
                FROM model_group_members mgm
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = mgm.provider_id AND pm.model_id = mgm.model_id
                ORDER BY mgm.group_id ASC, mgm.position ASC, mgm.provider_id ASC, mgm.model_id ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut members = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            members.push(model_group_member_from_row(&row)?);
        }
        Ok(members)
    }
    pub(crate) async fn model_group_member_by_key(
        &self,
        group_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> HamburResult<ModelGroupMemberRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    mgm.id,
                    mgm.group_id,
                    mgm.provider_id,
                    p.name,
                    mgm.model_id,
                    pm.display_name,
                    mgm.position,
                    mgm.enabled
                FROM model_group_members mgm
                JOIN providers p ON p.id = mgm.provider_id
                JOIN provider_models pm ON pm.provider_id = mgm.provider_id AND pm.model_id = mgm.model_id
                WHERE mgm.group_id = ?1 AND mgm.provider_id = ?2 AND mgm.model_id = ?3
                LIMIT 1
                ",
                params![group_id, provider_id, model_id],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::ModelUnavailable(format!(
                "model group member not found: {group_id}/{provider_id}/{model_id}"
            )));
        };
        model_group_member_from_row(&row)
    }
    pub(crate) async fn default_model_groups(&self) -> HamburResult<Vec<DefaultModelGroupRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT key, group_id, updated_at_ms
                FROM default_model_groups
                ORDER BY key ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut defaults = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            defaults.push(DefaultModelGroupRecord {
                key: row.get::<String>(0).map_err(database_error)?,
                group_id: row.get::<String>(1).map_err(database_error)?,
                updated_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
            });
        }
        Ok(defaults)
    }
    pub(crate) async fn app_settings(&self) -> HamburResult<Vec<AppSettingRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT key, value, updated_at_ms
                FROM app_settings
                ORDER BY key ASC
                ",
                params![],
            )
            .await
            .map_err(database_error)?;
        let mut settings = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            settings.push(AppSettingRecord {
                key: row.get::<String>(0).map_err(database_error)?,
                value: row.get::<String>(1).map_err(database_error)?,
                updated_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
            });
        }
        Ok(settings)
    }
    pub(crate) async fn config_audits(&self, limit: u32) -> HamburResult<Vec<ConfigAuditRecord>> {
        let limit = clamp_limit(limit, 1, 100);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    command_id,
                    actor,
                    action,
                    target_kind,
                    target_id,
                    redacted_summary,
                    approval_required,
                    approval_token,
                    created_at_ms
                FROM config_audit
                ORDER BY created_at_ms DESC, id DESC
                LIMIT ?1
                ",
                params![limit as i64],
            )
            .await
            .map_err(database_error)?;
        let mut audits = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            audits.push(config_audit_from_row(&row)?);
        }
        Ok(audits)
    }
}
