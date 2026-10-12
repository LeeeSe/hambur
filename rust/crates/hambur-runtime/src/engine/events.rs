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

    /// Persist the markdown block state for the timeline, then emit the render update
    /// plus the timeline delta it produced.
    ///
    /// 提交块（整段正文）只写进 message_blocks，时间线行不会自己跑到 UI 上：
    /// 客户端只在结构性事件里更新 timelineItems。早期实现为此在每个提交块上重发一份
    /// **整会话快照**，长会话下一个回合要发几十份、每份都是 O(整段会话) 的查询与拷贝。
    /// 现在只发这次写库真正新增/更新/隐藏的时间线项与块载荷，回合结束时才做一次全量对账。
    pub(crate) fn emit_markdown_persisted(
        &self,
        session_id: String,
        turn_id: String,
        update: MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(HamburError::RuntimeClosed);
        }
        let delta = self.persist_markdown_update_for_timeline(&session_id, &turn_id, &update)?;
        // 先发渲染更新，再发时间线增量：客户端拿到增量时块载荷已经在同一批数据里。
        self.emit_markdown(session_id.clone(), turn_id.clone(), update)?;
        self.emit_timeline_delta(session_id, turn_id, delta, "MarkdownCommitted")
    }

    /// Emit a session-scoped incremental timeline change (no full snapshot).
    pub(crate) fn emit_timeline_delta(
        &self,
        session_id: String,
        turn_id: String,
        delta: TimelineDelta,
        message: impl Into<String>,
    ) -> HamburResult<()> {
        if delta.is_empty() {
            return Ok(());
        }
        self.push_event(
            RuntimeEventKind::TurnStateChanged,
            session_id,
            turn_id,
            RuntimeEventPayload::TimelineDelta {
                items: delta.items,
                removed_stable_keys: delta.removed_stable_keys,
                message_blocks: delta.message_blocks,
            },
            message.into(),
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
