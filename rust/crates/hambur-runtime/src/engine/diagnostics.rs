use crate::*;

impl RuntimeEngine {
    /// 显式写一条诊断日志。tracing 宏走全局 subscriber，这里是给拿不到宏的
    /// 内部路径（例如需要拼好文本的地方）用的同一缓冲区入口。
    pub(crate) fn log_diagnostic(&self, level: &str, target: &str, message: impl Into<String>) {
        self.diagnostics.push(level, target, message);
    }

    /// 由 app_settings.loggingEnabled 同步采集开关。
    pub(crate) fn set_logging_enabled(&self, enabled: bool) {
        if enabled {
            self.diagnostics.set_enabled(true);
            self.log_diagnostic("info", "hambur_runtime::diagnostics", "loggingEnabled=true");
        } else {
            // 关闭前先落一条，让日志页能看出记录到哪里为止。
            self.log_diagnostic(
                "info",
                "hambur_runtime::diagnostics",
                "loggingEnabled=false",
            );
            self.diagnostics.set_enabled(false);
        }
    }

    /// Settings → 日志 的增量拉取入口。条目按 ordinal 由旧到新，调用方用最后一条的
    /// ordinal 作为下次的 `since_ordinal`。
    pub fn get_diagnostics_snapshot(
        &self,
        since_ordinal: u64,
        limit: u32,
    ) -> RuntimeDiagnosticsSnapshot {
        let limit = (limit as usize).clamp(1, DIAGNOSTIC_BUFFER_CAPACITY);
        let entries = self
            .diagnostics
            .snapshot(since_ordinal, limit)
            .into_iter()
            .map(|record| RuntimeDiagnosticEntry {
                ordinal: record.ordinal,
                backend_sequence: record.backend_sequence,
                created_at_ms: record.created_at_ms,
                level: record.level,
                target: record.target,
                message: record.message,
            })
            .collect();
        RuntimeDiagnosticsSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            entries,
        }
    }
}
