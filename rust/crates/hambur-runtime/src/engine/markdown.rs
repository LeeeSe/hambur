use crate::*;

impl RuntimeEngine {
    pub(crate) fn append_stream_markdown(
        &self,
        session_id: &str,
        message_id: &str,
        chunk: &str,
        finalize: bool,
    ) -> Option<MarkdownRenderUpdate> {
        let stream_key = format!("{session_id}:{message_id}");
        let mut streams = self.markdown_streams.lock().ok()?;
        let pipeline = streams
            .entry(stream_key.clone())
            .or_insert_with(|| MarkdownPipeline::new(message_id.to_string()));
        let update = if finalize {
            pipeline.finalize()
        } else {
            pipeline.append(chunk)
        };
        if finalize {
            streams.remove(&stream_key);
        }
        if update.committed_nodes.is_empty()
            && update.pending_node.is_none()
            && !update.reset
            && update.invalidated_block_ids.is_empty()
        {
            None
        } else {
            Some(update)
        }
    }

    pub(crate) fn insert_initial_pending_markdown_block(
        &self,
        session_id: &str,
        turn_id: &str,
        message_id: &str,
    ) -> HamburResult<()> {
        let stable_key = hambur_db::pending_markdown_stable_key(message_id);
        let node = MarkdownBlockNode {
            message_id: message_id.to_string(),
            block_id: 0,
            stable_key: stable_key.clone(),
            source_kind: "assistant".to_string(),
            node_kind: "root".to_string(),
            committed: false,
            level: 0,
            inlines: Vec::new(),
            language: String::new(),
            text: String::new(),
            raw: String::new(),
            children_json: String::new(),
            items_json: String::new(),
            table_header: Vec::new(),
            table_rows: Vec::new(),
            table_alignments: Vec::new(),
            path: String::new(),
            file_kind: String::new(),
        };
        let payload_json = serde_json::to_string(&node).map_err(|error| {
            HamburError::Internal(format!(
                "serialize initial pending markdown block payload: {error}"
            ))
        })?;
        self.database
            .upsert_message_block_payload(
                session_id,
                turn_id,
                NewMessageBlockPayload {
                    id: String::new(),
                    message_id: message_id.to_string(),
                    block_id: 0,
                    block_type: "content".to_string(),
                    stable_key,
                    committed: false,
                    payload_json,
                    raw: String::new(),
                    small_summary: String::new(),
                },
            )?;
        Ok(())
    }

    pub(crate) fn persist_markdown_update_for_timeline(
        &self,
        session_id: &str,
        turn_id: &str,
        update: &MarkdownRenderUpdate,
    ) -> HamburResult<()> {
        if update.message_id.trim().is_empty() {
            return Ok(());
        }
        for node in &update.committed_nodes {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize markdown block payload: {error}"))
            })?;
            self.database
                .upsert_message_block_payload(
                    session_id,
                    turn_id,
                    NewMessageBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        block_type: "content".to_string(),
                        stable_key: node.stable_key.clone(),
                        committed: true,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )?;
        }
        if let Some(node) = &update.pending_node {
            let payload_json = serde_json::to_string(node).map_err(|error| {
                HamburError::Internal(format!("serialize pending markdown block payload: {error}"))
            })?;
            self.database
                .upsert_message_block_payload(
                    session_id,
                    turn_id,
                    NewMessageBlockPayload {
                        id: String::new(),
                        message_id: update.message_id.clone(),
                        block_id: node.block_id,
                        block_type: "content".to_string(),
                        stable_key: hambur_db::pending_markdown_stable_key(&update.message_id),
                        committed: false,
                        payload_json,
                        raw: node.raw.clone(),
                        small_summary: markdown_block_summary(node),
                    },
                )?;
        } else if !update.committed_nodes.is_empty() || update.reset {
            self.database
                .remove_pending_markdown_block(session_id, &update.message_id)?;
        }
        Ok(())
    }

    pub(crate) fn persist_reasoning_block_for_timeline(
        &self,
        session_id: &str,
        turn_id: &str,
        message_id: &str,
        reasoning: &str,
    ) -> HamburResult<()> {
        if message_id.trim().is_empty() || reasoning.is_empty() {
            return Ok(());
        }
        let stable_key = hambur_db::reasoning_block_stable_key(message_id);
        let node = MarkdownBlockNode {
            message_id: message_id.to_string(),
            block_id: 0,
            stable_key: stable_key.clone(),
            source_kind: "assistant_reasoning".to_string(),
            node_kind: "reasoning".to_string(),
            committed: true,
            level: 0,
            inlines: Vec::new(),
            language: String::new(),
            text: reasoning.to_string(),
            raw: reasoning.to_string(),
            children_json: String::new(),
            items_json: String::new(),
            table_header: Vec::new(),
            table_rows: Vec::new(),
            table_alignments: Vec::new(),
            path: String::new(),
            file_kind: String::new(),
        };
        let payload_json = serde_json::to_string(&node).map_err(|error| {
            HamburError::Internal(format!("serialize reasoning block payload: {error}"))
        })?;
        self.database
            .upsert_message_block_payload(
                session_id,
                turn_id,
                NewMessageBlockPayload {
                    id: String::new(),
                    message_id: message_id.to_string(),
                    block_id: 0,
                    block_type: "reasoning".to_string(),
                    stable_key,
                    committed: true,
                    payload_json,
                    raw: reasoning.to_string(),
                    small_summary: reasoning.chars().take(160).collect(),
                },
            )?;
        Ok(())
    }
}
