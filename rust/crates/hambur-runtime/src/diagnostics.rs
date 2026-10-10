//! 结构化诊断日志：Rust tracing → 内存环形缓冲区 → Kotlin 日志页。
//!
//! 约束（global.md "Diagnostics And Logs" / "Logging"）：
//! - 只保留内存环形缓冲区，不落 SQLite；
//! - 可见缓冲区与后端事件共享 `sequence`：每条记录带上写入时刻的最新事件序号；
//! - 统一脱敏：API key、Authorization、请求体、附件内容、超大工具输出；
//! - `loggingEnabled` 关闭时直接丢弃，避免无谓开销。
//!
//! 缓冲区被两处写入：
//! 1. 全局 tracing subscriber（本文件），捕获各 crate 的 `tracing::*` 宏；
//! 2. `RuntimeEngine::log_diagnostic` 这类显式调用。
//!
//! Kotlin 通过 `get_diagnostics_snapshot(since_ordinal, limit)` 增量拉取。

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use regex::Regex;
use tracing::field::{Field, Visit};
use tracing::span;
use tracing::subscriber::Subscriber;
use tracing::{Event, Level, Metadata};

/// 单条消息上限；超出截断，避免把大块工具输出拖进内存。
pub const DIAGNOSTIC_MESSAGE_MAX_CHARS: usize = 2000;
/// 环形缓冲区容量。
pub const DIAGNOSTIC_BUFFER_CAPACITY: usize = 400;

const TRACE_TARGET_MAX_CHARS: usize = 64;
/// 脱敏后的占位符，测试用它断言替换结果。
#[cfg(test)]
const REDACTED: &str = "***";

/// 一条诊断记录。不可变，可直接映射成 DTO。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    /// 缓冲区内的单调序号，供 Kotlin 做增量拉取游标。
    pub ordinal: u64,
    /// 写入时刻的最新后端事件序号（与 `BackendEvent.sequence` 同一空间）。
    pub backend_sequence: u64,
    pub created_at_ms: u64,
    /// "debug" | "info" | "warn" | "error"
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 线程安全的内存环形缓冲区。
pub struct DiagnosticsBuffer {
    inner: Mutex<VecDeque<DiagnosticRecord>>,
    enabled: AtomicBool,
    next_ordinal: AtomicU64,
    backend_sequence: AtomicU64,
    capacity: usize,
}

impl DiagnosticsBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            enabled: AtomicBool::new(true),
            next_ordinal: AtomicU64::new(0),
            backend_sequence: AtomicU64::new(0),
            capacity: capacity.max(1),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 由事件管线在分配序号后调用，让诊断条目共享后端事件序号。
    pub fn set_backend_sequence(&self, sequence: u64) {
        self.backend_sequence.store(sequence, Ordering::Relaxed);
    }

    pub fn backend_sequence(&self) -> u64 {
        self.backend_sequence.load(Ordering::Relaxed)
    }

    /// 记录一条日志。关闭状态或锁中毒时静默丢弃——诊断日志不能影响业务。
    pub fn push(&self, level: &str, target: &str, message: impl Into<String>) {
        if !self.is_enabled() {
            return;
        }
        let message = message.into();
        let record = DiagnosticRecord {
            ordinal: self.next_ordinal.fetch_add(1, Ordering::Relaxed) + 1,
            backend_sequence: self.backend_sequence(),
            created_at_ms: now_ms(),
            level: level.to_string(),
            target: sanitize_target(target),
            message: redact(&message),
        };
        if let Ok(mut buffer) = self.inner.lock() {
            buffer.push_back(record);
            while buffer.len() > self.capacity {
                buffer.pop_front();
            }
        }
    }

    /// 返回 ordinal 严格大于 `since_ordinal` 的最多 `limit` 条记录（由旧到新）。
    pub fn snapshot(&self, since_ordinal: u64, limit: usize) -> Vec<DiagnosticRecord> {
        let Ok(buffer) = self.inner.lock() else {
            return Vec::new();
        };
        buffer
            .iter()
            .filter(|record| record.ordinal > since_ordinal)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|buffer| buffer.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 把 tracing 事件写进 [`DiagnosticsBuffer`] 的全局 subscriber。
pub struct DiagnosticsSubscriber {
    buffer: Arc<DiagnosticsBuffer>,
}

impl DiagnosticsSubscriber {
    pub fn new(buffer: Arc<DiagnosticsBuffer>) -> Self {
        Self { buffer }
    }
}

/// 安装全局 subscriber。进程内只会成功一次；已被占用或重复调用都会被忽略。
pub fn install_diagnostics_subscriber(buffer: Arc<DiagnosticsBuffer>) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(move || {
        let _ = tracing::subscriber::set_global_default(DiagnosticsSubscriber::new(buffer));
    });
}

/// 采集策略：WARN/ERROR 全收；INFO 只收 Hambur 自己的 crate，挡掉依赖库的噪声。
fn should_capture(level: &Level, target: &str) -> bool {
    if *level <= Level::WARN {
        return true;
    }
    *level == Level::INFO && target.starts_with("hambur")
}

fn level_label(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

impl Subscriber for DiagnosticsSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.buffer.is_enabled() && should_capture(metadata.level(), metadata.target())
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if !self.buffer.is_enabled() {
            return;
        }
        let metadata = event.metadata();
        if !should_capture(metadata.level(), metadata.target()) {
            return;
        }
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let message = visitor.finish();
        if message.is_empty() {
            return;
        }
        // 保留原来 eprintln! 在 logcat 里的可见性，同时进入内存缓冲区。
        eprintln!("[{}] {} {}", metadata.level(), metadata.target(), message);
        self.buffer
            .push(level_label(metadata.level()), metadata.target(), message);
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

/// 把 tracing 事件的字段拼成一行文本。
#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: Vec<String>,
}

impl FieldVisitor {
    fn finish(self) -> String {
        match (self.message.is_empty(), self.fields.is_empty()) {
            (true, _) => self.fields.join(" "),
            (false, true) => self.message,
            (false, false) => format!("{} {}", self.message, self.fields.join(" ")),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" && self.message.is_empty() {
            self.message = format!("{value:?}");
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" && self.message.is_empty() {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }
}

struct RedactionRule {
    pattern: Regex,
    replacement: &'static str,
}

fn redaction_rules() -> &'static [RedactionRule] {
    static RULES: OnceLock<Vec<RedactionRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            RedactionRule {
                // 授权头整行吞掉：`Authorization: Bearer <token>` 的 scheme 与 token 之间
                // 有空格，只匹配 `\S+` 会吃掉 "Bearer" 却把真正的 token 留在行里。
                pattern: Regex::new(r"(?i)(authorization\s*[:=]\s*)([^\r\n]+)").expect("regex"),
                replacement: "${1}***",
            },
            RedactionRule {
                pattern: Regex::new(r"(?i)\b(bearer\s+)([A-Za-z0-9._\-]{6,})").expect("regex"),
                replacement: "${1}***",
            },
            RedactionRule {
                pattern: Regex::new(
                    r#"(?i)((?:api[_\-]?key|apikey|access[_\-]?token|refresh[_\-]?token|secret|password|passwd|pwd)\s*["']?\s*[:=]\s*["']?)([^\s"',}\]]{4,})"#,
                )
                .expect("regex"),
                replacement: "${1}***",
            },
            RedactionRule {
                pattern: Regex::new(r"\bsk-[A-Za-z0-9._\-]{8,}").expect("regex"),
                replacement: "***",
            },
        ]
    })
}

/// 脱敏 + 截断。日志绝不带出密钥、授权头或超大正文。
pub fn redact(message: &str) -> String {
    let mut text = message.to_string();
    for rule in redaction_rules() {
        text = rule
            .pattern
            .replace_all(&text, rule.replacement)
            .into_owned();
    }
    truncate(&text)
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= DIAGNOSTIC_MESSAGE_MAX_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(DIAGNOSTIC_MESSAGE_MAX_CHARS).collect();
    let dropped = text.chars().count() - DIAGNOSTIC_MESSAGE_MAX_CHARS;
    format!("{kept}…(+{dropped})")
}

fn sanitize_target(target: &str) -> String {
    target.chars().take(TRACE_TARGET_MAX_CHARS).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_authorization_and_bearer_tokens() {
        let value = redact("Authorization: Bearer abcdefghijklmnop");
        assert!(!value.contains("abcdefghijklmnop"), "leaked token: {value}");
        assert!(value.contains(REDACTED));
    }

    #[test]
    fn redacts_api_key_assignments() {
        let value = redact("api_key=supersecret123");
        assert!(!value.contains("supersecret123"), "leaked key: {value}");
        assert!(value.contains("api_key=***"));
    }

    #[test]
    fn redacts_bare_sk_keys() {
        let value = redact("using sk-abcdefghijklmnopqrstuvwxyz now");
        assert!(!value.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn truncates_oversized_messages() {
        let huge = "x".repeat(DIAGNOSTIC_MESSAGE_MAX_CHARS + 10);
        let value = redact(&huge);
        assert!(value.chars().count() < huge.chars().count());
        assert!(value.ends_with(')'));
    }

    #[test]
    fn buffer_keeps_newest_and_bounds_capacity() {
        let buffer = DiagnosticsBuffer::new(3);
        for index in 0..5 {
            buffer.push("info", "hambur_test", format!("entry-{index}"));
        }
        let records = buffer.snapshot(0, 10);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].message, "entry-2");
        assert_eq!(records[2].message, "entry-4");
    }

    #[test]
    fn buffer_respects_since_ordinal_cursor() {
        let buffer = DiagnosticsBuffer::new(10);
        for index in 0..4 {
            buffer.push("info", "hambur_test", format!("entry-{index}"));
        }
        let records = buffer.snapshot(2, 10);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].ordinal, 3);
        assert_eq!(records[1].ordinal, 4);
    }

    #[test]
    fn disabled_buffer_drops_entries() {
        let buffer = DiagnosticsBuffer::new(10);
        buffer.set_enabled(false);
        buffer.push("error", "hambur_test", "boom");
        assert!(buffer.is_empty());
    }

    #[test]
    fn records_share_backend_sequence() {
        let buffer = DiagnosticsBuffer::new(10);
        buffer.set_backend_sequence(42);
        buffer.push("warn", "hambur_test", "message");
        assert_eq!(buffer.snapshot(0, 1)[0].backend_sequence, 42);
    }

    #[test]
    fn trims_long_targets() {
        let buffer = DiagnosticsBuffer::new(4);
        let long_target = "hambur::".repeat(40);
        buffer.push("info", &long_target, "message");
        assert_eq!(
            buffer.snapshot(0, 1)[0].target.chars().count(),
            TRACE_TARGET_MAX_CHARS
        );
    }
}
