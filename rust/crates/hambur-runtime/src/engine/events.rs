use crate::*;

impl RuntimeEngine {
    /// Single choke point for pushing an event onto the queue.
    fn push_event(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        payload: RuntimeEventPayload,
        message: String,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) && kind != RuntimeEventKind::RuntimeClosed {
            return Err(HamburError::RuntimeClosed);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        // 诊断日志与事件共享序号：记录这条事件之后写入的日志都能对上是哪个事件之后的。
        self.diagnostics.set_backend_sequence(sequence);
        let (error_code, message) = match error {
            Some(error) => (error.code().as_str().to_string(), error.to_string()),
            None => (String::new(), message),
        };
        let event = RuntimeEvent {
            event_id: new_id("evt"),
            schema_version: DTO_SCHEMA_VERSION,
            sequence,
            created_at_ms: now_ms(),
            kind,
            session_id,
            turn_id,
            payload,
            error_code,
            message,
        };

        self.sender
            .try_send(event)
            .map_err(|error| HamburError::Internal(format!("event queue: {error}")))
    }

    /// Structural change scoped to a session: carries the session snapshot.
    pub(crate) fn emit_with_snapshot(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        snapshot: AppSnapshot,
        message: String,
        error: Option<&HamburError>,
    ) -> HamburResult<()> {
        self.push_event(
            kind,
            session_id,
            turn_id,
            RuntimeEventPayload::Snapshot { snapshot },
            message,
            error,
        )
    }

    /// Structural change that switches or refreshes the visible session.
    pub(crate) fn emit_snapshot(
        &self,
        kind: RuntimeEventKind,
        snapshot: AppSnapshot,
    ) -> HamburResult<()> {
        let session_id = snapshot.selected_session_id.clone();
        self.emit_with_snapshot(
            kind,
            session_id,
            String::new(),
            snapshot,
            String::new(),
            None,
        )
    }

    /// Notification without data; the UI re-queries whatever it needs.
    pub(crate) fn emit_plain(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        message: impl Into<String>,
    ) -> HamburResult<()> {
        self.push_event(
            kind,
            session_id,
            turn_id,
            RuntimeEventPayload::None,
            message.into(),
            None,
        )
    }

    pub(crate) fn emit_error(&self, error: HamburError) -> HamburResult<()> {
        self.push_event(
            RuntimeEventKind::RuntimeError,
            String::new(),
            String::new(),
            RuntimeEventPayload::None,
            String::new(),
            Some(&error),
        )
    }

    pub(crate) fn emit_delta(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        message_id: String,
        delta: String,
    ) -> HamburResult<()> {
        self.push_event(
            kind,
            session_id,
            turn_id,
            RuntimeEventPayload::Delta { message_id, delta },
            String::new(),
            None,
        )
    }

    pub(crate) fn emit_markdown(
        &self,
        session_id: String,
        turn_id: String,
        update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        self.push_event(
            RuntimeEventKind::MarkdownRenderUpdate,
            session_id,
            turn_id,
            RuntimeEventPayload::Markdown { update },
            String::new(),
            None,
        )
    }

    /// Persist the markdown block state for the timeline, then emit the render update.
    ///
    /// 提交块（整段正文）只写进 message_blocks，时间线行不会自己跑到 UI 上：
    /// 客户端只在 snapshot 事件里重建 timelineItems，而流式期间没有 snapshot，
    /// 于是 UI 只能看到开局那条 pending 占位块——表现就是"流式时只有一段正文，
    /// 结束后所有正文一起出现"。这里在提交块落库后补一条带 snapshot 的
    /// TurnStateChanged，让新提交的段落立刻出现在时间线里。
    pub(crate) fn emit_markdown_persisted(
        &self,
        session_id: String,
        turn_id: String,
        update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }
        self.persist_markdown_update_for_timeline(&session_id, &turn_id, &update)?;
        // 先发渲染更新，再发快照：客户端拿到快照时 messageBlocksByPayloadRef
        // 里已经有这些块，新时间线项可以同帧渲染出来。
        self.emit_markdown(session_id.clone(), turn_id.clone(), update)?;
        let snapshot = self
            .database
            .session_snapshot(&session_id)
            .unwrap_or_default();
        self.emit_with_snapshot(
            RuntimeEventKind::TurnStateChanged,
            session_id,
            turn_id,
            snapshot,
            "MarkdownCommitted".to_string(),
            None,
        )
    }

    pub(crate) fn emit_attachments(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        attachments: Vec<AttachmentRecord>,
        message: impl Into<String>,
    ) -> HamburResult<()> {
        self.push_event(
            kind,
            session_id,
            String::new(),
            RuntimeEventPayload::Attachments { attachments },
            message.into(),
            None,
        )
    }

    pub(crate) fn emit_platform_request(&self, request: PlatformRequest) -> HamburResult<()> {
        let session_id = request.session_id.clone();
        let turn_id = request.turn_id.clone();
        self.push_event(
            RuntimeEventKind::PlatformRequest,
            session_id,
            turn_id,
            RuntimeEventPayload::PlatformRequest { request },
            String::new(),
            None,
        )
    }

    pub(crate) fn emit_failure(
        &self,
        kind: RuntimeEventKind,
        session_id: String,
        turn_id: String,
        error: &HamburError,
    ) -> HamburResult<()> {
        self.push_event(
            kind,
            session_id,
            turn_id,
            RuntimeEventPayload::None,
            String::new(),
            Some(error),
        )
    }
}
