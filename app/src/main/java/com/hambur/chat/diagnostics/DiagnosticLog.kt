package com.hambur.chat.diagnostics

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicLong

/**
 * 应用内诊断日志（Settings → 日志）的内存环形缓冲区。
 *
 * 设计约束（来自 global.md 的 "Diagnostics And Logs"）：
 * - 最近日志是纯内存环形缓冲区，[clear] 只清可见缓冲区，绝不写 SQLite／不删领域数据。
 * - Rust 结构化 tracing 与 Kotlin 日志在可用时共享后端 sequence：Kotlin 侧每条记录带上
 *   记录时刻"已应用的最新后端事件序号"（[sequenceProvider]），Rust 侧条目自带序号。
 * - 落盘前统一脱敏：API key、Authorization、请求体、附件内容、超大工具输出。
 *
 * 这是一个进程级单例：`ChatJankTracer` 这样的 object 没有依赖注入入口，直接写入这里最省事。
 * 每个进程只有一个 `HamburUiStore`，它把 [entries] 收集进 UI state。
 */
object DiagnosticLogs {
    /** 环形缓冲区容量。够排查问题，又不会在长会话里无限增长。 */
    const val Capacity: Int = 500

    /** 单条消息上限；超出部分截断，避免把大块工具输出拖进内存。 */
    const val MaxMessageChars: Int = 2000

    private val lock = Any()
    private val buffer = ArrayDeque<DiagLogEntry>(Capacity)
    private val nextId = AtomicLong(0L)

    private val _entries = MutableStateFlow<List<DiagLogEntry>>(emptyList())

    /** 最新在前的日志快照。UI 只读，不要在外部修改。 */
    val entries: StateFlow<List<DiagLogEntry>> = _entries.asStateFlow()

    /**
     * 是否记录日志，对应 app_settings 的 `loggingEnabled`。
     * 关闭时 [record] 直接返回，避免无谓的字符串拼接与脱敏开销。
     */
    @Volatile
    var enabled: Boolean = true

    /** 返回记录时刻已应用的最新后端事件序号；没有可用序号时返回 0。 */
    @Volatile
    var sequenceProvider: () -> Long = { 0L }

    /** 记录一条日志。[throwable] 会折叠进消息文本；[sequence] 显式指定后端序号（Rust 条目用）。 */
    fun record(
        level: DiagLevel,
        source: DiagSource,
        tag: String,
        message: String,
        throwable: Throwable? = null,
        sequence: Long? = null,
    ) {
        if (!enabled) return
        val raw = buildString {
            append(message)
            if (throwable != null) {
                if (isNotEmpty()) append(' ')
                append(throwable.javaClass.simpleName)
                throwable.message?.let { append(": ").append(it) }
            }
        }
        val safeMessage = DiagnosticRedactor.redact(raw)
        val entrySequence = sequence
            ?: runCatching { sequenceProvider() }.getOrDefault(0L)
        val entry = DiagLogEntry(
            id = nextId.incrementAndGet(),
            sequence = entrySequence,
            timestampMs = System.currentTimeMillis(),
            level = level,
            source = source,
            tag = tag.take(64),
            message = safeMessage,
        )
        val snapshot = synchronized(lock) {
            buffer.addFirst(entry)
            while (buffer.size > Capacity) {
                buffer.removeLast()
            }
            buffer.toList()
        }
        _entries.value = snapshot
    }

    /** 便捷方法：应用级日志，同时写入 logcat。 */
    fun app(level: DiagLevel, tag: String, message: String, throwable: Throwable? = null) {
        when (level) {
            DiagLevel.DEBUG -> Log.d(tag, message, throwable)
            DiagLevel.INFO -> Log.i(tag, message, throwable)
            DiagLevel.WARN -> Log.w(tag, message, throwable)
            DiagLevel.ERROR -> Log.e(tag, message, throwable)
        }
        record(level, DiagSource.APP, tag, message, throwable)
    }

    /** 清空可见缓冲区。只影响内存，不碰数据库。 */
    fun clear() {
        synchronized(lock) { buffer.clear() }
        _entries.value = emptyList()
    }

    /** 当前缓冲区快照（最新在前）。 */
    fun snapshot(): List<DiagLogEntry> = synchronized(lock) { buffer.toList() }

    /** 用于测试：重置开关/序号并清空。 */
    internal fun resetForTest() {
        enabled = true
        sequenceProvider = { 0L }
        clear()
    }
}

/** 日志级别，数值越大越严重。 */
enum class DiagLevel(val label: String, val rank: Int) {
    DEBUG("DEBUG", 0),
    INFO("INFO", 1),
    WARN("WARN", 2),
    ERROR("ERROR", 3),
    ;

    companion object {
        fun fromLabel(label: String): DiagLevel {
            return entries.firstOrNull { it.label.equals(label, ignoreCase = true) } ?: INFO
        }
    }
}

/** 日志来源，用于 Settings → 日志 的分类页签。 */
enum class DiagSource(val label: String) {
    APP("app"),
    JANK("jank"),
    BACKEND("backend"),
}

/** 一条诊断日志。不可变，可直接用于 Compose 列表 key 与 diff。 */
data class DiagLogEntry(
    val id: Long,
    /** 记录时刻已应用的最新后端事件序号；0 表示当时没有可用序号。 */
    val sequence: Long,
    val timestampMs: Long,
    val level: DiagLevel,
    val source: DiagSource,
    val tag: String,
    val message: String,
)

/**
 * 日志脱敏。规则保守但可测：
 * - Authorization / Bearer / api_key / token / secret / password 的值替换为 `***`。
 * - `sk-` 前缀的密钥串整体替换。
 * - 超长文本按 [DiagnosticLogs.MaxMessageChars] 截断。
 */
object DiagnosticRedactor {
    private const val Redacted = "***"

    private val patterns: List<Regex> = listOf(
        // Authorization: Bearer xxx / authorization=xxx
        // 整行吞掉：scheme 与 token 之间有空格，只匹配 \S+ 会只吃掉 "Bearer" 而漏出 token。
        Regex("""(?i)(authorization\s*[:=]\s*)([^\r\n]+)"""),
        // Bearer xxx
        Regex("""(?i)\b(bearer\s+)([A-Za-z0-9._\-]{6,})"""),
        // api_key=xxx / "apiKey": "xxx" / access_token: xxx / password=xxx
        Regex(
            """(?i)((?:api[_\-]?key|apikey|access[_\-]?token|refresh[_\-]?token|secret|password|passwd|pwd)\s*["']?\s*[:=]\s*["']?)([^\s"',}\]]{4,})""",
        ),
        // 裸的 sk- 密钥
        Regex("""\bsk-[A-Za-z0-9._\-]{8,}"""),
    )

    fun redact(input: String): String {
        var text = input
        for (pattern in patterns) {
            text = pattern.replace(text) { match ->
                if (match.groupValues.size >= 3) {
                    match.groupValues[1] + Redacted
                } else {
                    Redacted
                }
            }
        }
        return truncate(text)
    }

    private fun truncate(text: String): String {
        if (text.length <= DiagnosticLogs.MaxMessageChars) return text
        return text.take(DiagnosticLogs.MaxMessageChars) + "…(+${text.length - DiagnosticLogs.MaxMessageChars})"
    }
}
