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
        self.emit_with_snapshot(kind, session_id, String::new(), snapshot, String::new(), None)
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
        self.emit_markdown(session_id, turn_id, update)
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
