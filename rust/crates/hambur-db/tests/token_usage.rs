//! Token 用量读写与聚合回归测试。
//!
//! Token 用量页依赖 `token_usage_summary()` 的 SELECT 列顺序和聚合语义：
//! 三个分组的字段顺序、COALESCE 兜底、以及"会话删除后用量行被清掉"。
//! 这个用例把写入 → 汇总 → 清理这条链路锁死。

use hambur_db::{HamburDatabase, NewTokenUsage};

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

/// 与 `timeline_items.rs` 相同的临时库：断言 panic 时也会关闭句柄并清理文件与 WAL 边车。
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

fn usage(
    session_id: &str,
    provider_id: &str,
    model_id: &str,
    prompt: u64,
    completion: u64,
) -> NewTokenUsage {
    NewTokenUsage {
        session_id: session_id.to_string(),
        turn_id: "turn".to_string(),
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        ..Default::default()
    }
}

#[test]
fn empty_database_reports_zero_usage() {
    let temp = TempDatabase::open("token_usage_empty");
    let summary = temp
        .database()
        .token_usage_summary()
        .expect("summarize empty table");
    assert_eq!(summary.request_count, 0);
    assert_eq!(summary.total_tokens, 0);
    assert!(summary.by_provider.is_empty());
    assert!(summary.by_model.is_empty());
}

#[test]
fn aggregates_totals_by_provider_and_by_model() {
    let temp = TempDatabase::open("token_usage_aggregate");
    let database = temp.database();
    let snapshot = database.create_session("usage").expect("create session");
    let session_id = snapshot.selected_session_id;

    database
        .insert_token_usage(usage(&session_id, "deepseek", "deepseek-chat", 100, 20))
        .expect("insert first");
    database
        .insert_token_usage(usage(&session_id, "deepseek", "deepseek-chat", 50, 10))
        .expect("insert second");
    database
        .insert_token_usage(usage(&session_id, "openai", "gpt-5.1", 200, 40))
        .expect("insert third");

    let summary = database.token_usage_summary().expect("summarize");
    assert_eq!(summary.request_count, 3);
    assert_eq!(summary.prompt_tokens, 350);
    assert_eq!(summary.completion_tokens, 70);
    assert_eq!(summary.total_tokens, 420);

    // 按总量倒序：openai(240) 第一，deepseek(180) 第二。
    assert_eq!(summary.by_provider.len(), 2);
    assert_eq!(summary.by_provider[0].provider_id, "openai");
    assert_eq!(summary.by_provider[0].total_tokens, 240);
    assert_eq!(summary.by_provider[1].provider_id, "deepseek");
    assert_eq!(summary.by_provider[1].request_count, 2);
    assert_eq!(summary.by_provider[1].total_tokens, 180);
    // 按 provider 分组时 model_id 为空。
    assert!(
        summary
            .by_provider
            .iter()
            .all(|group| group.model_id.is_empty())
    );

    assert_eq!(summary.by_model.len(), 2);
    assert_eq!(summary.by_model[0].provider_id, "openai");
    assert_eq!(summary.by_model[0].model_id, "gpt-5.1");
    assert_eq!(summary.by_model[1].provider_id, "deepseek");
    assert_eq!(summary.by_model[1].model_id, "deepseek-chat");
    assert_eq!(summary.by_model[1].request_count, 2);
}

#[test]
fn keeps_cached_and_reasoning_token_details() {
    let temp = TempDatabase::open("token_usage_details");
    let database = temp.database();
    let snapshot = database.create_session("usage").expect("create session");

    let mut record = usage(
        &snapshot.selected_session_id,
        "deepseek",
        "deepseek-reasoner",
        10,
        5,
    );
    record.cached_tokens = 8;
    record.reasoning_tokens = 3;
    database.insert_token_usage(record).expect("insert");

    let summary = database.token_usage_summary().expect("summarize");
    assert_eq!(summary.cached_tokens, 8);
    assert_eq!(summary.reasoning_tokens, 3);
}

#[test]
fn deleting_a_session_usage_removes_only_that_session() {
    let temp = TempDatabase::open("token_usage_delete");
    let database = temp.database();
    let first = database.create_session("first").expect("session one");
    let second = database.create_session("second").expect("session two");

    database
        .insert_token_usage(usage(&first.selected_session_id, "p", "m", 10, 1))
        .expect("insert first");
    database
        .insert_token_usage(usage(&second.selected_session_id, "p", "m", 20, 2))
        .expect("insert second");

    database
        .delete_token_usage_for_session(&first.selected_session_id)
        .expect("delete first session usage");

    let summary = database.token_usage_summary().expect("summarize");
    assert_eq!(summary.request_count, 1);
    assert_eq!(summary.total_tokens, 22);
}
