//! 时间线行读写往返回归测试。
//!
//! 背景（修复前的 bug）：`TimelineItemSnapshot` 增加了 `trace_arguments` 字段后，共享的
//! `timeline_item_from_row` 会读第 14 列，但 `timeline_item_by_stable_key` 的 SELECT
//! 仍然只查了 13 列。于是每次 `upsert_timeline_item`（每个 Markdown 块提交、每个工具
//! trace 更新都会走）在回读时都报 "Invalid column index"，日志里刷数据库错误。
//!
//! 这个用例锁死"写入后回读"这条路径，任何 SELECT 列数与行映射器不一致都会在这里失败。

use hambur_db::{HamburDatabase, NewTimelineItem, NewToolCall, NewTraceSpan};

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

/// A test database that closes its handle and removes the file plus SQLite sidecars even
/// when an assertion panics; an end-of-test `remove_file` leaks on failure and cannot
/// unlink while the handle is open on Windows.
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
        // Close the handle before unlinking so Windows can remove the file.
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
fn upserting_a_trace_item_reads_back_with_its_tool_arguments() {
    let temp = TempDatabase::open("timeline_roundtrip");
    let database = temp.database();
    let snapshot = database
        .create_session("roundtrip")
        .expect("create session");
    let session_id = snapshot.selected_session_id.clone();
    assert!(!session_id.is_empty());

    let trace = database
        .insert_trace_span(NewTraceSpan {
            session_id: session_id.clone(),
            turn_id: "turn-1".to_string(),
            kind: "tool".to_string(),
            title: "安装依赖".to_string(),
            content: r#"{"command":"pip install numpy"}"#.to_string(),
            status: "running".to_string(),
            tool_call_id: "call-1".to_string(),
            visible: true,
            ..Default::default()
        })
        .expect("insert trace span");

    database
        .insert_tool_call(NewToolCall {
            id: "call-1".to_string(),
            session_id: session_id.clone(),
            turn_id: "turn-1".to_string(),
            assistant_message_id: "msg-1".to_string(),
            name: "terminal".to_string(),
            arguments_json: r#"{"command":"pip install numpy","title":"安装依赖"}"#.to_string(),
            display_title: "安装依赖".to_string(),
            status: "running".to_string(),
            requires_approval: false,
            call_index: 0,
        })
        .expect("insert tool call");

    // 这一步在修复前会失败：回读的 SELECT 少一列，行映射器读越界。
    let item = database
        .upsert_timeline_item(
            &session_id,
            NewTimelineItem {
                stable_key: trace.id.clone(),
                content_type: "trace".to_string(),
                display_sequence: trace.started_at_ms,
                payload_ref: trace.id.clone(),
                small_summary: trace.title.clone(),
                kind: "ToolTrace".to_string(),
            },
        )
        .expect("upsert timeline item must read back cleanly");
    assert_eq!(item.tool_name, "terminal");
    assert_eq!(
        item.trace_arguments, r#"{"command":"pip install numpy","title":"安装依赖"}"#,
        "回读必须带上工具参数原文"
    );

    // 会话快照里也应能看到同一行（走的是另一条 SELECT，列数必须同样对齐）。
    let snapshot = database
        .session_snapshot(&session_id)
        .expect("session snapshot");
    let listed = snapshot
        .timeline_items
        .iter()
        .find(|row| row.stable_key == trace.id)
        .expect("timeline item is listed");
    assert_eq!(listed.tool_name, "terminal");
    assert_eq!(listed.trace_arguments, item.trace_arguments);
}
