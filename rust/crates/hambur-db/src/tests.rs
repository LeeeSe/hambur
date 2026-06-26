use super::*;

use std::{fs, path::PathBuf};

    use hambur_core::new_id;
    use tokio::runtime::Runtime;

    use super::{
        HamburDatabase, ModelRouteSnapshot, NewMarkdownBlockPayload, NewTimelineItem, NewToolCall,
        NewToolResult, NewTraceSpan, pending_markdown_stable_key,
    };

    #[test]
    fn sessions_survive_restart_and_delete_from_snapshot() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("First")
                .await
                .expect("create session");
            assert_eq!(created.sessions.len(), 1);
            assert_eq!(created.sessions[0].title, "First");
            let session_id = created.selected_session_id.clone();
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let bootstrap = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(bootstrap.sessions.len(), 1);
            assert_eq!(bootstrap.selected_session_id, session_id);

            let deleted = restarted
                .delete_session(&session_id)
                .await
                .expect("delete session");
            assert!(deleted.sessions.is_empty());
            assert!(deleted.selected_session_id.is_empty());
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn active_session_survives_restart_after_opening_older_session() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let first = database
                .create_session("First")
                .await
                .expect("create first");
            let first_id = first.selected_session_id;
            let second = database
                .create_session("Second")
                .await
                .expect("create second");
            assert_eq!(second.selected_session_id, second.sessions[0].id);

            let opened_first = database.open_session(&first_id).await.expect("open first");
            assert_eq!(opened_first.selected_session_id, first_id);
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let snapshot = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(snapshot.selected_session_id, first_id);
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn message_timeline_and_turn_repositories_write_basic_records() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello")
                .await
                .expect("insert message");
            assert_eq!(message.role, "user");
            assert_eq!(message.content_text, "hello");

            let item = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: message.id.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: message.id.clone(),
                        small_summary: "hello".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("upsert timeline item");
            assert_eq!(item.stable_key, message.id);
            assert_eq!(item.version_sequence, 1);

            let updated = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: item.stable_key.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: item.payload_ref.clone(),
                        small_summary: "hello again".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("update timeline item");
            assert_eq!(updated.version_sequence, 2);
            assert_eq!(updated.small_summary, "hello again");

            let turn = database
                .create_turn(&session_id, "Preparing")
                .await
                .expect("create turn");
            let finished = database
                .update_turn_status(&turn.id, "Finished", true)
                .await
                .expect("finish turn");
            assert_eq!(finished.status, "Finished");
            assert!(finished.finished_at_ms > 0);

            let snapshot = database
                .session_snapshot(&session_id)
                .await
                .expect("session snapshot");
            assert_eq!(snapshot.timeline_items.len(), 1);
            assert_eq!(snapshot.timeline_items[0].small_summary, "hello again");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn pending_markdown_block_keeps_message_block_order() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Markdown")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;
            let message = database
                .insert_message(&session_id, "assistant", "first\n\nsecond")
                .await
                .expect("insert assistant message");

            database
                .upsert_markdown_block_payload(
                    &session_id,
                    "turn-1",
                    NewMarkdownBlockPayload {
                        id: String::new(),
                        message_id: message.id.clone(),
                        block_id: 1,
                        stable_key: format!("{}:1", message.id),
                        committed: true,
                        payload_json: "{}".to_string(),
                        raw: "first".to_string(),
                        small_summary: "first".to_string(),
                    },
                )
                .await
                .expect("upsert committed block");
            database
                .upsert_markdown_block_payload(
                    &session_id,
                    "turn-1",
                    NewMarkdownBlockPayload {
                        id: String::new(),
                        message_id: message.id.clone(),
                        block_id: 2,
                        stable_key: pending_markdown_stable_key(&message.id),
                        committed: false,
                        payload_json: "{}".to_string(),
                        raw: "second".to_string(),
                        small_summary: "second".to_string(),
                    },
                )
                .await
                .expect("upsert pending block");

            let snapshot = database
                .session_snapshot(&session_id)
                .await
                .expect("session snapshot");
            let markdown_items: Vec<_> = snapshot
                .timeline_items
                .iter()
                .filter(|item| {
                    item.content_type == "assistant_markdown_block"
                        || item.content_type == "assistant_pending_block"
                })
                .collect();

            assert_eq!(markdown_items.len(), 2);
            assert_eq!(markdown_items[0].content_type, "assistant_markdown_block");
            assert_eq!(markdown_items[0].small_summary, "first");
            assert_eq!(markdown_items[1].content_type, "assistant_pending_block");
            assert_eq!(markdown_items[1].small_summary, "second");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn snapshot_query_methods_are_paginated_and_side_effect_free() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Searchable Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello snapshot")
                .await
                .expect("insert message");

            for index in 0..3u64 {
                database
                    .upsert_timeline_item(
                        &session_id,
                        NewTimelineItem {
                            stable_key: format!("item-{index}"),
                            content_type: "message".to_string(),
                            display_sequence: message.created_at_ms + index,
                            payload_ref: message.id.clone(),
                            small_summary: format!("summary {index}"),
                            kind: "UserMessage".to_string(),
                        },
                    )
                    .await
                    .expect("upsert timeline item");
            }

            let sessions = database.session_list(10, 0).await.expect("session list");
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].title, "Searchable Chat");

            let search = database
                .search_sessions("Searchable", 10)
                .await
                .expect("search sessions");
            assert_eq!(search.len(), 1);

            let page = database
                .timeline_page(&session_id, 0, 2)
                .await
                .expect("timeline page");
            assert_eq!(page.items.len(), 2);
            assert!(page.has_more);
            assert!(page.next_before_cursor > 0);

            let snapshot = database
                .message_snapshot(&message.id)
                .await
                .expect("message snapshot");
            assert_eq!(snapshot.expect("message").content_text, "hello snapshot");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn trace_spans_and_tool_results_render_through_timeline() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Tools")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;
            let route = ModelRouteSnapshot {
                provider_id: "provider".to_string(),
                provider_name: "Provider".to_string(),
                provider_protocol: "OpenAiCompatible".to_string(),
                model_id: "model".to_string(),
                model_display_name: "Model".to_string(),
                model_group_id: "grp".to_string(),
                ..Default::default()
            };
            let turn = database
                .create_turn_with_route(&session_id, "ExecutingTools", &route)
                .await
                .expect("turn");
            let assistant = database
                .insert_message_with_route(
                    &session_id,
                    "assistant",
                    "",
                    "",
                    "streaming",
                    &turn.id,
                    &route,
                )
                .await
                .expect("assistant");
            let tool_call = database
                .insert_tool_call(NewToolCall {
                    id: "call_1".to_string(),
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    assistant_message_id: assistant.id.clone(),
                    name: "echo".to_string(),
                    arguments_json: r#"{"text":"hello"}"#.to_string(),
                    display_title: "Echo".to_string(),
                    status: "running".to_string(),
                    requires_approval: false,
                    call_index: 0,
                })
                .await
                .expect("tool call");
            let trace = database
                .insert_trace_span(NewTraceSpan {
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    kind: "tool".to_string(),
                    title: "Echo".to_string(),
                    content: "running".to_string(),
                    status: "running".to_string(),
                    tool_call_id: tool_call.id.clone(),
                    visible: true,
                    ..Default::default()
                })
                .await
                .expect("trace");
            database
                .update_trace_span_status(&trace.id, "completed", "hello", true)
                .await
                .expect("complete trace");
            let tool_message = database
                .insert_tool_result_message(
                    &session_id,
                    &turn.id,
                    &tool_call.id,
                    "echo",
                    "hello",
                    &route,
                )
                .await
                .expect("tool message");
            let result = database
                .insert_tool_result(NewToolResult {
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    tool_call_id: tool_call.id.clone(),
                    message_id: tool_message.id,
                    is_error: false,
                    content_json: r#"{"text":"hello"}"#.to_string(),
                    summary: "hello".to_string(),
                    artifacts_json: "[]".to_string(),
                    trust_level: "trusted".to_string(),
                    context_stub: "hello".to_string(),
                    ..Default::default()
                })
                .await
                .expect("tool result");
            database
                .update_tool_call_status(&tool_call.id, "completed", &result.id, "", "", true)
                .await
                .expect("complete tool call");

            let page = database
                .timeline_page(&session_id, 0, 20)
                .await
                .expect("timeline");
            let trace_item = page
                .items
                .iter()
                .find(|item| item.kind == "ToolTrace")
                .expect("trace item");
            assert_eq!(trace_item.trace_title, "Echo");
            assert_eq!(trace_item.trace_status, "completed");
            assert_eq!(trace_item.tool_call_id, "call_1");
            assert_eq!(trace_item.tool_name, "echo");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn hiding_assistant_message_hides_same_turn_tool_traces() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Tools")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;
            let route = ModelRouteSnapshot {
                provider_id: "provider".to_string(),
                provider_name: "Provider".to_string(),
                provider_protocol: "OpenAiCompatible".to_string(),
                model_id: "model".to_string(),
                model_display_name: "Model".to_string(),
                model_group_id: "grp".to_string(),
                ..Default::default()
            };
            let turn = database
                .create_turn_with_route(&session_id, "ExecutingTools", &route)
                .await
                .expect("turn");
            let assistant = database
                .insert_message_with_route(
                    &session_id,
                    "assistant",
                    "old assistant",
                    "",
                    "completed",
                    &turn.id,
                    &route,
                )
                .await
                .expect("assistant");
            database
                .upsert_markdown_block_payload(
                    &session_id,
                    &turn.id,
                    NewMarkdownBlockPayload {
                        id: String::new(),
                        message_id: assistant.id.clone(),
                        block_id: 1,
                        stable_key: format!("{}:1", assistant.id),
                        committed: true,
                        payload_json: "{}".to_string(),
                        raw: "old assistant".to_string(),
                        small_summary: "old assistant".to_string(),
                    },
                )
                .await
                .expect("assistant markdown");
            database
                .insert_trace_span(NewTraceSpan {
                    session_id: session_id.clone(),
                    turn_id: turn.id.clone(),
                    kind: "tool".to_string(),
                    title: "Tool without persisted call".to_string(),
                    content: "running".to_string(),
                    status: "running".to_string(),
                    visible: true,
                    ..Default::default()
                })
                .await
                .expect("trace");

            let before = database
                .timeline_page(&session_id, 0, 20)
                .await
                .expect("timeline before");
            assert!(
                before.items.iter().any(|item| item.kind == "ToolTrace"),
                "missing trace before hide"
            );

            database
                .hide_visible_timeline_after_message(&session_id, &assistant.id, true)
                .await
                .expect("hide assistant branch");

            let after = database
                .timeline_page(&session_id, 0, 20)
                .await
                .expect("timeline after");
            assert!(
                !after.items.iter().any(|item| item.kind == "ToolTrace"),
                "tool trace remained visible after assistant hide"
            );
            assert!(
                !after
                    .items
                    .iter()
                    .any(|item| item.content_type == "assistant_markdown_block"),
                "assistant markdown remained visible after assistant hide"
            );
        });

        let _ = fs::remove_file(path);
    }

    fn temp_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("{}.db", new_id("hambur_db_test")))
    }
