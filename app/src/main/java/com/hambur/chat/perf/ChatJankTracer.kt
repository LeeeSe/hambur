package com.hambur.chat.perf

import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.max

object ChatJankTracer {
    private const val Tag = "ChatJank"
    private const val ActiveTraceWindowNs = 6_000_000_000L
    private const val FrameWindowNs = 1_800_000_000L
    private const val SlowFrameMs = 24.0
    private const val JankFrameMs = 32.0
    private const val FrameBudgetNs = 16_666_667L

    private val traceIds = AtomicLong()
    private val lock = Any()

    @Volatile
    private var activeTrace: SwitchTrace? = null
    private var frameWindowId = 0L

    fun beginSessionSwitch(
        source: String,
        currentSessionId: String,
        targetSessionId: String,
        targetMessageCount: String = "",
        drawerProgress: Float = 0f,
        extra: String = "",
    ): Long {
        val nowNs = nowNs()
        val trace = SwitchTrace(
            id = traceIds.incrementAndGet(),
            startNs = nowNs,
            currentSessionId = currentSessionId,
            targetSessionId = targetSessionId,
        )
        synchronized(lock) {
            activeTrace = trace
        }
        log(
            trace = trace,
            level = Log.INFO,
            phase = "switch_begin",
            nowNs = nowNs,
            extra = joinFields(
                "source=$source",
                "from=${shortId(currentSessionId)}",
                "to=${shortId(targetSessionId)}",
                "targetMessages=$targetMessageCount",
                "drawerProgress=${formatFloat(drawerProgress)}",
                extra,
            ),
        )
        startFrameWindow(trace.id)
        return trace.id
    }

    fun markSessionSwitch(
        phase: String,
        targetSessionId: String = "",
        extra: String = "",
    ) {
        val nowNs = nowNs()
        val trace = activeTraceFor(targetSessionId = targetSessionId, nowNs = nowNs) ?: return
        log(
            trace = trace,
            level = Log.INFO,
            phase = phase,
            nowNs = nowNs,
            extra = extra,
        )
    }

    fun markSessionSwitchOnce(
        phase: String,
        key: String = phase,
        targetSessionId: String = "",
        extra: String = "",
    ) {
        val nowNs = nowNs()
        val trace = synchronized(lock) {
            val candidate = activeTraceForLocked(targetSessionId = targetSessionId, nowNs = nowNs)
                ?: return
            if (!candidate.onceKeys.add(key)) return
            candidate
        }
        log(
            trace = trace,
            level = Log.INFO,
            phase = phase,
            nowNs = nowNs,
            extra = extra,
        )
    }

    fun markDuration(
        phase: String,
        startNs: Long,
        targetSessionId: String = "",
        warnAtMs: Double = 8.0,
        extra: String = "",
        always: Boolean = false,
    ) {
        val nowNs = nowNs()
        val trace = activeTraceFor(targetSessionId = targetSessionId, nowNs = nowNs) ?: return
        val durationMs = (nowNs - startNs).nsToMs()
        if (!always && durationMs < warnAtMs) return
        log(
            trace = trace,
            level = if (durationMs >= warnAtMs) Log.WARN else Log.INFO,
            phase = phase,
            nowNs = nowNs,
            extra = joinFields(
                "durationMs=${formatMs(durationMs)}",
                extra,
            ),
        )
    }

    fun <T> timeSessionSwitch(
        phase: String,
        targetSessionId: String = "",
        warnAtMs: Double = 8.0,
        extra: String = "",
        always: Boolean = false,
        block: () -> T,
    ): T {
        val startNs = nowNs()
        return try {
            block()
        } finally {
            markDuration(
                phase = phase,
                startNs = startNs,
                targetSessionId = targetSessionId,
                warnAtMs = warnAtMs,
                extra = extra,
                always = always,
            )
        }
    }

    fun nowNs(): Long = SystemClock.elapsedRealtimeNanos()

    private fun startFrameWindow(traceId: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) return

        val choreographer = Choreographer.getInstance()
        val windowStartNs = nowNs()
        val windowId = synchronized(lock) {
            frameWindowId += 1
            frameWindowId
        }

        var lastFrameNs = 0L
        var frameCount = 0
        var slowFrames = 0
        var jankFrames = 0
        var worstFrameNs = 0L

        val callback = object : Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                val trace = synchronized(lock) {
                    val trace = activeTrace ?: return
                    if (trace.id != traceId || frameWindowId != windowId) return
                    trace
                }
                val nowNs = nowNs()
                if (lastFrameNs == 0L) {
                    log(
                        trace = trace,
                        level = Log.INFO,
                        phase = "frame_window_start",
                        nowNs = nowNs,
                    )
                } else {
                    val frameNs = frameTimeNanos - lastFrameNs
                    frameCount += 1
                    worstFrameNs = max(worstFrameNs, frameNs)
                    val frameMs = frameNs.nsToMs()
                    if (frameMs >= SlowFrameMs) {
                        slowFrames += 1
                        if (frameMs >= JankFrameMs) {
                            jankFrames += 1
                        }
                        log(
                            trace = trace,
                            level = Log.WARN,
                            phase = "frame_gap",
                            nowNs = nowNs,
                            extra = joinFields(
                                "gapMs=${formatMs(frameMs)}",
                                "missedFrames=${missedFrames(frameNs)}",
                            ),
                        )
                    }
                }

                lastFrameNs = frameTimeNanos
                if (nowNs - windowStartNs < FrameWindowNs) {
                    choreographer.postFrameCallback(this)
                } else {
                    log(
                        trace = trace,
                        level = if (jankFrames > 0) Log.WARN else Log.INFO,
                        phase = "frame_window_end",
                        nowNs = nowNs,
                        extra = joinFields(
                            "frames=$frameCount",
                            "slowFrames=$slowFrames",
                            "jankFrames=$jankFrames",
                            "worstMs=${formatMs(worstFrameNs.nsToMs())}",
                        ),
                    )
                }
            }
        }

        choreographer.postFrameCallback(callback)
    }

    private fun activeTraceFor(targetSessionId: String, nowNs: Long): SwitchTrace? {
        return synchronized(lock) {
            activeTraceForLocked(targetSessionId = targetSessionId, nowNs = nowNs)
        }
    }

    private fun activeTraceForLocked(targetSessionId: String, nowNs: Long): SwitchTrace? {
        val trace = activeTrace ?: return null
        if (nowNs - trace.startNs > ActiveTraceWindowNs) {
            activeTrace = null
            return null
        }
        if (targetSessionId.isNotBlank() && targetSessionId != trace.targetSessionId) {
            return null
        }
        return trace
    }

    private fun log(
        trace: SwitchTrace,
        level: Int,
        phase: String,
        nowNs: Long,
        extra: String = "",
    ) {
        val message = joinFields(
            "trace=${trace.id}",
            "phase=$phase",
            "tMs=${formatMs((nowNs - trace.startNs).nsToMs())}",
            "target=${shortId(trace.targetSessionId)}",
            "thread=${Thread.currentThread().name}",
            extra,
        )
        when (level) {
            Log.WARN -> Log.w(Tag, message)
            Log.ERROR -> Log.e(Tag, message)
            else -> Log.i(Tag, message)
        }
    }

    private data class SwitchTrace(
        val id: Long,
        val startNs: Long,
        val currentSessionId: String,
        val targetSessionId: String,
        val onceKeys: MutableSet<String> = mutableSetOf(),
    )

    private fun missedFrames(frameNs: Long): Int {
        return (frameNs / FrameBudgetNs).toInt().coerceAtLeast(1) - 1
    }

    private fun Long.nsToMs(): Double = this / 1_000_000.0

    private fun formatMs(value: Double): String {
        return String.format(Locale.US, "%.1f", value)
    }

    private fun formatFloat(value: Float): String {
        return String.format(Locale.US, "%.2f", value)
    }

    private fun shortId(sessionId: String): String {
        if (sessionId.isBlank()) return "-"
        return if (sessionId.length <= 10) {
            sessionId
        } else {
            sessionId.take(4) + ".." + sessionId.takeLast(6)
        }
    }

    private fun joinFields(vararg fields: String): String {
        return fields
            .filter { it.isNotBlank() }
            .joinToString(separator = " ")
    }
}
