//! 长会话发送路径上的批量查询与"写库增量"回归测试。
//!
//! 背景（修复前）：
//! - `message_blocks_for_payload_refs` 对每个 payload_ref 单独发一条 SELECT，
//!   250 条消息的会话一次快照要跑 ~450 条语句；
//! - `visible_chat_transcript_before_message` 对每条消息查一次 attachments、
//!   每条 assistant 查一次 tool_calls（且当时没有 assistant_message_id 索引）；
//! - 每个提交块都重新拉一份整会话快照。
//!
//! 这些用例锁死批量路径的正确性（含跨分片边界）与增量写库的返回内容。

use hambur_db::{
    HamburDatabase, ModelRouteSnapshot, NewMessageBlockPayload, NewTimelineItem, NewToolCall,
    NewTraceSpan, pending_markdown_stable_key,
};

fn temp_database_path(tag: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    path.push(format!(
        "hambur_db_test_{tag}_{}_{unique}.sqlite",
        std::process::id()
    ));
    path
}

struct TempDatabase {
    path: std::path::PathBuf,
    database: Option<HamburDatabase>,
}

impl TempDatabase {
    fn open(tag: &str) -> Self {
        let path = temp_database_path(tag);
        let database = HamburDatabase::open(&path).expect("open database");
        Self {
            path,
            database: Some(database),
        }
    }

    fn database(&self) -> &HamburDatabase {
        self.database.as_ref().expect("database is open")
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        self.database.take();
        let _ = std::fs::remove_file(&self.path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = self.path.clone().into_os_string();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(sidecar));
        }
    }
}

fn insert_message(
    database: &HamburDatabase,
    session_id: &str,
    role: &str,
    content: &str,
) -> hambur_db::MessageRecord {
    database
        .insert_message_with_route(
            session_id,
            role,
            content,
            "",
            "completed",
            "turn-1",
            &ModelRouteSnapshot::default(),
        )
        .expect("insert message")
}

fn upsert_block(
    database: &HamburDatabase,
    session_id: &str,
    message_id: &str,
    block_id: u64,
    stable_key: &str,
    committed: bool,
) -> hambur_db::MessageBlockUpsert {
    database
        .upsert_message_block_payload(
            session_id,
            "turn-1",
            NewMessageBlockPayload {
                id: String::new(),
                message_id: message_id.to_string(),
                block_id,
                block_type: "content".to_string(),
                stable_key: stable_key.to_string(),
                committed,
                payload_json: "{}".to_string(),
                raw: format!("block {block_id}"),
                small_summary: format!("block {block_id}"),
            },
        )
        .expect("upsert block")
}

/// 批量取块必须跨分片边界完整返回（分片粒度是 400，这里塞 420 个）。
#[test]
fn session_snapshot_returns_every_block_across_query_chunks() {
    let temp = TempDatabase::open("block_batching");
    let database = temp.database();
    let session_id = database
        .create_session("batching")
        .expect("create session")
        .selected_session_id;

    let message = insert_message(database, &session_id, "assistant", "");
    let total = 420u64;
    for block_id in 0..total {
        upsert_block(
            database,
            &session_id,
            &message.id,
            block_id,
            &format!("{}:{block_id}", message.id),
            true,
        );
    }

    let snapshot = database.session_snapshot(&session_id).expect("snapshot");
    assert_eq!(
        snapshot.message_block_payloads.len() as u64,
        total,
        "跨分片的块一个都不能少"
    );
    let listed = snapshot
        .message_block_payloads
        .iter()
        .filter(|block| block.message_id == message.id)
        .count();
    assert_eq!(listed as u64, total);
    // 时间线项同样要齐（快照里的 payload_ref 指向块行 id）。
    let timeline_blocks = snapshot
        .timeline_items
        .iter()
        .filter(|item| item.content_type == "assistant_markdown_block")
        .count();
    assert_eq!(timeline_blocks as u64, total);
}

/// transcript 批量补 attachments / tool_calls：顺序、归属、内容都要和逐条查一致。
#[test]
fn transcript_batches_tool_calls_per_assistant_message() {
    let temp = TempDatabase::open("transcript_batching");
    let database = temp.database();
    let session_id = database
        .create_session("transcript")
        .expect("create session")
        .selected_session_id;

    let user = insert_message(database, &session_id, "user", "第一条");
    database
        .upsert_timeline_item(
            &session_id,
            NewTimelineItem {
                stable_key: user.id.clone(),
                content_type: "user_message".to_string(),
                display_sequence: user.created_at_ms,
                payload_ref: user.id.clone(),
                small_summary: "第一条".to_string(),
                kind: "UserMessage".to_string(),
            },
        )
        .expect("user timeline item");

    // 时间戳决定 transcript 的边界，用 sleep 保证严格递增。
    std::thread::sleep(std::time::Duration::from_millis(3));
    let assistant = insert_message(database, &session_id, "assistant", "回答");
    upsert_block(
        database,
        &session_id,
        &assistant.id,
        0,
        &format!("{}:0", assistant.id),
        true,
    );
    for call_index in 0..3u32 {
        let call_id = format!("call-{call_index}");
        database
            .insert_trace_span(NewTraceSpan {
                session_id: session_id.clone(),
                turn_id: "turn-1".to_string(),
                kind: "tool".to_string(),
                title: format!("工具 {call_index}"),
                content: "{}".to_string(),
                status: "completed".to_string(),
                tool_call_id: call_id.clone(),
                visible: true,
                ..Default::default()
            })
            .expect("trace span");
        database
            .insert_tool_call(NewToolCall {
                id: call_id,
                session_id: session_id.clone(),
                turn_id: "turn-1".to_string(),
                assistant_message_id: assistant.id.clone(),
                name: format!("tool_{call_index}"),
                arguments_json: "{}".to_string(),
                display_title: format!("工具 {call_index}"),
                status: "completed".to_string(),
                requires_approval: false,
                call_index,
            })
            .expect("tool call");
    }

    std::thread::sleep(std::time::Duration::from_millis(3));
    let boundary = insert_message(database, &session_id, "user", "第二条");
    database
        .upsert_timeline_item(
            &session_id,
            NewTimelineItem {
                stable_key: boundary.id.clone(),
                content_type: "user_message".to_string(),
                display_sequence: boundary.created_at_ms,
                payload_ref: boundary.id.clone(),
                small_summary: "第二条".to_string(),
                kind: "UserMessage".to_string(),
            },
        )
        .expect("boundary timeline item");

    let entries = database
        .visible_chat_transcript_before_message(&session_id, &boundary.id)
        .expect("transcript");

    let user_entry = entries
        .iter()
        .find(|entry| entry.message.id == user.id)
        .expect("user message is in the transcript");
    assert!(user_entry.tool_calls.is_empty());
    let assistant_entry = entries
        .iter()
        .find(|entry| entry.message.id == assistant.id)
        .expect("assistant message is in the transcript");
    let call_indexes = assistant_entry
        .tool_calls
        .iter()
        .map(|call| call.call_index)
        .collect::<Vec<_>>();
    assert_eq!(
        call_indexes,
        vec![0, 1, 2],
        "批量取回的工具调用必须按 call_index 排序且不串消息"
    );
    assert!(
        entries.iter().all(|entry| entry.message.id != boundary.id),
        "边界消息本身不在 transcript 里"
    );
}

/// 提交正文块必须报告"它 upsert 了哪条时间线项"与"顺带隐藏了哪个占位块"。
#[test]
fn committing_a_block_reports_the_timeline_delta() {
    let temp = TempDatabase::open("block_delta");
    let database = temp.database();
    let session_id = database
        .create_session("delta")
        .expect("create session")
        .selected_session_id;

    let message = insert_message(database, &session_id, "assistant", "");
    let pending_key = pending_markdown_stable_key(&message.id);
    let pending = upsert_block(database, &session_id, &message.id, 0, &pending_key, false);
    assert_eq!(
        pending.timeline_item.content_type,
        "assistant_pending_block"
    );
    assert!(pending.hidden_stable_keys.is_empty());

    let committed_key = format!("{}:1", message.id);
    let committed = upsert_block(database, &session_id, &message.id, 1, &committed_key, true);

    assert_eq!(
        committed.timeline_item.content_type, "assistant_markdown_block",
        "提交块的时间线项必须随写库结果一起返回"
    );
    assert_eq!(committed.timeline_item.stable_key, committed_key);
    assert_eq!(
        committed.timeline_item.payload_ref, committed.block.id,
        "时间线项指向块行 id"
    );
    assert_eq!(
        committed.hidden_stable_keys,
        vec![pending_key],
        "提交正文块会隐藏尾部占位块，必须报告给客户端"
    );

    // 占位块确实不可见了。
    let snapshot = database.session_snapshot(&session_id).expect("snapshot");
    assert!(
        snapshot
            .timeline_items
            .iter()
            .all(|item| item.stable_key != pending_markdown_stable_key(&message.id)),
        "隐藏的占位块不应出现在快照里"
    );
}
