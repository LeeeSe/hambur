//! 中断遗留状态收敛回归测试。
//!
//! 进程被杀 / 崩溃 / 覆盖安装会把正在执行的行留在中间态：turn 停在
//! `ExecutingTools`、tool_call 和 trace span 停在 `running`、消息停在 `streaming`。
//! 这些行不会自愈，而且带 `tool_calls` 的 assistant 消息会永远等不到 tool 结果，
//! 下一次请求就是「assistant 带 tool_calls 却没有紧跟 tool 响应」——OpenAI 兼容接口
//! 直接 400，用户看到的现象是消息发出去了但完全不回复。
//!
//! `reconcile_interrupted_state()` 必须在启动时把它们落成终止态，并且幂等。

use hambur_db::{HamburDatabase, ModelRouteSnapshot, NewToolCall, NewTraceSpan};

fn temp_database_path(tag: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    path.push(format!(
        "hambur_interrupted_test_{tag}_{}_{unique}.sqlite",
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

#[test]
fn reconcile_closes_interrupted_rows_and_is_idempotent() {
    let temp = TempDatabase::open("interrupted");
    let database = temp.database();
    let session_id = database
        .create_session("汉化")
        .expect("create session")
        .selected_session_id;
    let route = ModelRouteSnapshot::default();

    let turn = database
        .create_turn(&session_id, "ExecutingTools")
        .expect("create interrupted turn");
    let assistant = database
        .insert_message_with_route_and_prefix(
            &session_id, "assistant", "", "", "streaming", &turn.id, &route, "",
        )
        .expect("insert streaming assistant message");
    let call = database
        .insert_tool_call(NewToolCall {
            id: String::new(),
            session_id: session_id.clone(),
            turn_id: turn.id.clone(),
            assistant_message_id: assistant.id.clone(),
            name: "terminal".to_string(),
            arguments_json: "{}".to_string(),
            display_title: "等待 apk 锁".to_string(),
            status: "running".to_string(),
            requires_approval: false,
            call_index: 0,
        })
        .expect("insert running tool call");
    let span = database
        .insert_trace_span(NewTraceSpan {
            session_id: session_id.clone(),
            turn_id: turn.id.clone(),
            parent_span_id: String::new(),
            kind: "tool".to_string(),
            title: "等待 apk 锁".to_string(),
            content: String::new(),
            status: "running".to_string(),
            tool_call_id: call.id.clone(),
            payload_json: String::new(),
            visible: true,
        })
        .expect("insert running trace span");

    // 已经正常结束的行不能被误伤。
    let finished_turn = database
        .create_turn(&session_id, "Finished")
        .expect("finished turn");
    database
        .insert_message_with_route_and_prefix(
            &session_id,
            "assistant",
            "done",
            "",
            "completed",
            &finished_turn.id,
            &route,
            "",
        )
        .expect("completed assistant message");

    let summary = database
        .reconcile_interrupted_state()
        .expect("reconcile interrupted state");
    assert_eq!(summary.turns, 1, "只有 ExecutingTools 的 turn 需要收敛");
    assert_eq!(summary.tool_calls, 1);
    assert_eq!(summary.trace_spans, 1);
    assert_eq!(summary.messages, 1);
    assert!(!summary.is_empty());

    // 幂等：第二次已经没有可收敛的行。
    let second = database
        .reconcile_interrupted_state()
        .expect("second reconcile");
    assert!(
        second.is_empty(),
        "中断状态必须一次收敛干净，第二次不应再改动任何行: {second:?}"
    );

    let messages = database
        .messages_for_session(&session_id)
        .expect("messages for session");
    let assistant_record = messages
        .iter()
        .find(|message| message.id == assistant.id)
        .expect("assistant message");
    assert_eq!(assistant_record.status, "failed_partial");
    assert_eq!(assistant_record.finish_reason, "interrupted");
    assert_eq!(assistant_record.native_finish_reason, "interrupted");
    let completed_record = messages
        .iter()
        .find(|message| message.turn_id == finished_turn.id)
        .expect("completed assistant message");
    assert_eq!(completed_record.status, "completed");
    assert_eq!(completed_record.finish_reason, "");

    // 时间线里的工具步进也必须是终止态，否则 UI 会一直停在「执行中」。
    let page = database
        .timeline_page(&session_id, 0, 50)
        .expect("timeline page");
    let trace_item = page
        .items
        .iter()
        .find(|item| item.payload_ref == span.id)
        .expect("trace timeline item");
    assert_eq!(trace_item.trace_status, "failed");
}

#[test]
fn empty_database_reconciles_nothing() {
    let temp = TempDatabase::open("interrupted_empty");
    let summary = temp
        .database()
        .reconcile_interrupted_state()
        .expect("reconcile empty database");
    assert!(summary.is_empty());
}
