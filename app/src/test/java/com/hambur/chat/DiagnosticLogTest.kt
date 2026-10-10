package com.hambur.chat

import com.hambur.chat.diagnostics.DiagLevel
import com.hambur.chat.diagnostics.DiagSource
import com.hambur.chat.diagnostics.DiagnosticLogs
import com.hambur.chat.diagnostics.DiagnosticRedactor
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * 锁定 Settings → 日志 的诊断缓冲区契约：
 * - 环形缓冲区最新在前、容量有上限；
 * - 清空只影响内存；
 * - loggingEnabled 关闭时不再记录；
 * - 记录时带上后端序号；
 * - 落盘前脱敏，密钥/授权头不会出现在日志里。
 */
class DiagnosticLogTest {
    @Before
    fun setUp() {
        DiagnosticLogs.resetForTest()
    }

    @After
    fun tearDown() {
        DiagnosticLogs.resetForTest()
    }

    @Test
    fun newestEntryComesFirst() {
        DiagnosticLogs.record(DiagLevel.INFO, DiagSource.APP, "tag", "first")
        DiagnosticLogs.record(DiagLevel.INFO, DiagSource.APP, "tag", "second")

        val entries = DiagnosticLogs.snapshot()
        assertEquals(2, entries.size)
        assertEquals("second", entries[0].message)
        assertEquals("first", entries[1].message)
    }

    @Test
    fun ringBufferDropsOldestEntriesBeyondCapacity() {
        repeat(DiagnosticLogs.Capacity + 25) { index ->
            DiagnosticLogs.record(DiagLevel.INFO, DiagSource.APP, "tag", "entry-$index")
        }

        val entries = DiagnosticLogs.snapshot()
        assertEquals(DiagnosticLogs.Capacity, entries.size)
        assertEquals("entry-${DiagnosticLogs.Capacity + 24}", entries.first().message)
        assertEquals("entry-25", entries.last().message)
    }

    @Test
    fun clearEmptiesTheVisibleBuffer() {
        DiagnosticLogs.record(DiagLevel.WARN, DiagSource.JANK, "ChatJank", "frame_gap")
        assertEquals(1, DiagnosticLogs.snapshot().size)

        DiagnosticLogs.clear()

        assertTrue(DiagnosticLogs.snapshot().isEmpty())
        assertTrue(DiagnosticLogs.entries.value.isEmpty())
    }

    @Test
    fun disabledRecorderDropsNewEntries() {
        DiagnosticLogs.enabled = false

        DiagnosticLogs.record(DiagLevel.ERROR, DiagSource.BACKEND, "runtime", "boom")

        assertTrue(DiagnosticLogs.snapshot().isEmpty())
    }

    @Test
    fun entriesCarrySequenceFromProvider() {
        DiagnosticLogs.sequenceProvider = { 4242L }

        DiagnosticLogs.record(DiagLevel.INFO, DiagSource.APP, "tag", "message")

        assertEquals(4242L, DiagnosticLogs.snapshot().first().sequence)
    }

    @Test
    fun explicitSequenceOverridesProvider() {
        DiagnosticLogs.sequenceProvider = { 1L }

        DiagnosticLogs.record(
            level = DiagLevel.INFO,
            source = DiagSource.BACKEND,
            tag = "rust",
            message = "message",
            sequence = 99L,
        )

        assertEquals(99L, DiagnosticLogs.snapshot().first().sequence)
    }

    @Test
    fun levelsParseFromBackendLabels() {
        assertEquals(DiagLevel.WARN, DiagLevel.fromLabel("warn"))
        assertEquals(DiagLevel.ERROR, DiagLevel.fromLabel("ERROR"))
        assertEquals(DiagLevel.DEBUG, DiagLevel.fromLabel("debug"))
        assertEquals(DiagLevel.INFO, DiagLevel.fromLabel("something-unknown"))
    }

    @Test
    fun redactsAuthorizationHeader() {
        val redacted = DiagnosticRedactor.redact("Authorization: Bearer abcdefghijklmnopqrstuvwxyz")

        assertFalse(redacted.contains("abcdefghijklmnopqrstuvwxyz"))
        assertTrue(redacted.contains("***"))
    }

    @Test
    fun redactsApiKeyAssignments() {
        val redacted = DiagnosticRedactor.redact("""api_key=supersecret123""")

        assertFalse(redacted.contains("supersecret123"))
        assertTrue(redacted.contains("api_key=***"))
    }

    @Test
    fun redactsBareSkKeys() {
        val redacted = DiagnosticRedactor.redact("using sk-abcdefghijklmnopqrstuvwxyz now")

        assertFalse(redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"))
    }

    @Test
    fun truncatesOversizedMessages() {
        val huge = "x".repeat(DiagnosticLogs.MaxMessageChars + 500)

        DiagnosticLogs.record(DiagLevel.INFO, DiagSource.APP, "tag", huge)

        val message = DiagnosticLogs.snapshot().first().message
        assertTrue(message.length < huge.length)
        assertTrue(message.endsWith(")"))
    }
}
