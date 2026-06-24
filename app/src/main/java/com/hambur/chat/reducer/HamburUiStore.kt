package com.hambur.chat.reducer

import android.util.Log
import com.hambur.chat.uniffi.AppBootstrapConfig
import com.hambur.chat.uniffi.AttachmentDto
import com.hambur.chat.uniffi.BackendCommand
import com.hambur.chat.uniffi.BackendEvent
import com.hambur.chat.uniffi.CommandAck
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.SessionListSnapshotDto
import com.hambur.chat.uniffi.TimelineItemDto
import com.hambur.chat.uniffi.createRuntime
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class UiSessionSummary(
    val id: String,
    val title: String,
    val messageCount: UInt,
    val latestPreview: String,
)

data class UiTimelineItem(
    val id: String,
    val stableKey: String,
    val contentType: String,
    val versionSequence: ULong,
    val smallSummary: String,
    val kind: String,
    val traceTitle: String = "",
    val traceContent: String = "",
    val traceStatus: String = "",
    val toolCallId: String = "",
    val toolName: String = "",
)

data class UiPendingAttachment(
    val id: String,
    val kind: String,
    val displayName: String,
    val mimeType: String,
    val byteSize: ULong,
    val sandboxPath: String,
)

data class AppShellState(
    val runtimeStatus: String = "Starting",
    val latestEventKind: String = "Waiting",
    val footer: String = "Rust runtime owns backend state",
    val sessions: List<UiSessionSummary> = emptyList(),
    val selectedSessionId: String = "",
    val timelineItems: List<UiTimelineItem> = emptyList(),
    val pendingAttachments: List<UiPendingAttachment> = emptyList(),
    val markdownMessageId: String = "",
    val markdownBlocks: List<MarkdownBlockNodeDto> = emptyList(),
    val pendingMarkdownBlock: MarkdownBlockNodeDto? = null,
    val activePreviewPath: String = "",
    val snapshotSequence: ULong = 0UL,
    val lastAppliedSequence: ULong = 0UL,
    val appliedEventIds: Set<String> = emptySet(),
    val activeTurnIds: Map<String, String> = emptyMap(),
)

class HamburUiStore(appFilesDir: String) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val runtime = createRuntime(AppBootstrapConfig(appFilesDir = appFilesDir))
    private val _state = MutableStateFlow(AppShellState())
    private val startupLock = Any()
    private val startupBuffer = mutableListOf<BackendEvent>()
    private val markdownCoalesceLock = Any()
    private val pendingMarkdownEvents = mutableListOf<BackendEvent>()
    private val commandCounter = AtomicLong()
    private var markdownFlushScheduled = false
    private var baselineApplied = false
    private var defaultProviderConfigured = false

    val state: StateFlow<AppShellState> = _state.asStateFlow()

    init {
        scope.launch { collectBackendEvents() }
        scope.launch { applyInitialSnapshotBaseline() }
    }

    fun createSession(title: String) {
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "CreateSession",
                    idempotencyKey = "session:create:${nextCommandOrdinal()}",
                    title = title,
                ),
            )
        }
    }

    fun openSession(sessionId: String) {
        if (sessionId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "OpenSession",
                    idempotencyKey = "$sessionId:open",
                    sessionId = sessionId,
                ),
            )
        }
    }

    fun deleteSession(sessionId: String) {
        if (sessionId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "SoftDeleteSession",
                    idempotencyKey = "$sessionId:soft-delete:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                ),
            )
        }
    }

    fun streamMarkdownPreview(sessionId: String) {
        if (sessionId.isBlank()) return

        scope.launch {
            val messageId = "preview-${System.currentTimeMillis()}"
            val chunks = listOf(
                "# Markdown preview\n\n",
                "A paragraph with **strong text**, `inline code`, and [a file](hambur://file/report.md).\n\n",
                "- First item\n- Second item\n\n",
                "```kotlin\nfun greet() = \"hello\"\n```\n\n",
                "| Kind | Status |\n| --- | --- |\n| table | ready |\n",
            )

            chunks.forEachIndexed { index, chunk ->
                val ack = runtime.dispatch(
                    backendCommand(
                        kind = "AppendMarkdownDelta",
                        idempotencyKey = "markdown:$messageId:$index",
                        sessionId = sessionId,
                        messageId = messageId,
                        chunk = chunk,
                    ),
                )
                applyRejectedAck(ack)
                delay(32)
            }

            val finalAck = runtime.dispatch(
                backendCommand(
                    kind = "AppendMarkdownDelta",
                    idempotencyKey = "markdown:$messageId:final",
                    sessionId = sessionId,
                    messageId = messageId,
                    finalize = true,
                ),
            )
            applyRejectedAck(finalAck)
        }
    }

    fun sendMessage(sessionId: String, content: String) {
        if (sessionId.isBlank()) return
        val attachmentIds = _state.value.pendingAttachments.map { it.id }
        if (content.isBlank() && attachmentIds.isEmpty()) return

        scope.launch {
            ensureDefaultTextProvider()
            val payload = if (attachmentIds.isEmpty()) {
                ""
            } else {
                attachmentPayloadJson(attachmentIds)
            }
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "SendMessage",
                    idempotencyKey = "message:${System.currentTimeMillis()}:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    content = content,
                    reasoning = "Routing through the configured OpenAI-compatible text provider.",
                    payloadJson = payload,
                ),
            )
            applyRejectedAck(ack)
        }
    }

    fun importAttachmentMetadata(
        sessionId: String,
        displayName: String,
        mimeType: String,
        byteSize: ULong = 0UL,
        originalUri: String = "",
        sourcePath: String = "",
    ) {
        if (sessionId.isBlank()) return
        val escapedName = displayName.jsonEscaped()
        val escapedMime = mimeType.jsonEscaped()
        val escapedUri = originalUri.jsonEscaped()
        val escapedPath = sourcePath.jsonEscaped()
        val payload = """
            {"displayName":"$escapedName","mimeType":"$escapedMime","byteSize":$byteSize,"originalUri":"$escapedUri","sourcePath":"$escapedPath","originType":"content_uri"}
        """.trimIndent()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "ImportAttachmentFromUri",
                    idempotencyKey = "$sessionId:import-attachment:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun removePendingAttachment(sessionId: String, attachmentId: String) {
        if (sessionId.isBlank() || attachmentId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "RemovePendingAttachment",
                    idempotencyKey = "$attachmentId:remove",
                    sessionId = sessionId,
                    messageId = attachmentId,
                ),
            )
        }
    }

    fun cancelActiveTurn(sessionId: String) {
        if (sessionId.isBlank()) return
        val turnId = _state.value.activeTurnIds[sessionId].orEmpty()
        if (turnId.isBlank()) return

        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "CancelTurn",
                    idempotencyKey = "$turnId:cancel",
                    sessionId = sessionId,
                    turnId = turnId,
                ),
            )
        }
    }

    fun openMarkdownDestination(destination: String) {
        if (destination.isBlank()) return
        _state.update {
            it.copy(
                latestEventKind = "FilePreviewRoute",
                footer = destination,
                activePreviewPath = destination,
            )
        }
    }

    fun shutdown() {
        runCatching {
            runtime.dispatch(
                backendCommand(
                    kind = "Shutdown",
                    idempotencyKey = "runtime:shutdown",
                ),
            )
        }
        runtime.shutdown()
        scope.cancel()
    }

    private fun collectBackendEvents() {
        while (scope.isActive) {
            val event = runtime.nextEvent() ?: break
            Log.i(
                "HamburBackend",
                "Collected backend event kind=${event.kind} sequence=${event.sequence}",
            )

            val shouldApplyNow = synchronized(startupLock) {
                if (baselineApplied) {
                    true
                } else {
                    startupBuffer.add(event)
                    false
                }
            }
            if (shouldApplyNow) {
                applyEvent(event)
            }
            if (event.kind == "RuntimeClosed") break
        }

        _state.update {
            if (it.runtimeStatus == "Closed") {
                it
            } else {
                it.copy(runtimeStatus = "Closed")
            }
        }
    }

    private fun applyInitialSnapshotBaseline() {
        val sessionSnapshot = runCatching {
            runtime.getSessionListSnapshot(limit = 100u, offset = 0u)
        }.getOrElse { error ->
            _state.update {
                it.copy(
                    runtimeStatus = "Error",
                    footer = error.message ?: "Backend snapshot query failed",
                )
            }
            return
        }

        val selectedSessionId = sessionSnapshot.selectedSessionId.ifBlank {
            sessionSnapshot.sessions.firstOrNull()?.id.orEmpty()
        }
        val timelinePage = if (selectedSessionId.isBlank()) {
            null
        } else {
            runCatching {
                runtime.getTimelinePage(
                    sessionId = selectedSessionId,
                    beforeCursor = 0UL,
                    limit = 50u,
                )
            }.getOrNull()
        }
        val baselineSequence = maxSequence(
            sessionSnapshot.snapshotSequence,
            timelinePage?.snapshotSequence ?: 0UL,
        )
        val timelineItems = timelinePage?.items.orEmpty()

        val bufferedEvents = synchronized(startupLock) {
            _state.update {
                it.applyBaseline(
                    snapshot = sessionSnapshot,
                    selectedSessionId = selectedSessionId,
                    timelineItems = timelineItems,
                    baselineSequence = baselineSequence,
                )
            }
            baselineApplied = true
            startupBuffer
                .asSequence()
                .filter { it.sequence > baselineSequence }
                .sortedBy { it.sequence }
                .toList()
                .also { startupBuffer.clear() }
        }

        bufferedEvents.forEach(::applyEvent)
    }

    private fun runCommand(block: () -> CommandAck) {
        scope.launch {
            val ack = runCatching(block).getOrElse { error ->
                _state.update {
                    it.copy(
                        runtimeStatus = "Error",
                        footer = error.message ?: "Backend command failed",
                    )
                }
                return@launch
            }

            applyRejectedAck(ack)
        }
    }

    private fun applyEvent(event: BackendEvent) {
        if (event.kind == "MarkdownRenderUpdate") {
            enqueueMarkdownEvent(event)
            return
        }
        val markdownEvents = drainMarkdownEvents()
        _state.update { state ->
            markdownEvents.fold(state) { nextState, markdownEvent ->
                nextState.reduce(markdownEvent)
            }.reduce(event)
        }
    }

    private fun enqueueMarkdownEvent(event: BackendEvent) {
        val shouldSchedule = synchronized(markdownCoalesceLock) {
            pendingMarkdownEvents.add(event)
            if (markdownFlushScheduled) {
                false
            } else {
                markdownFlushScheduled = true
                true
            }
        }

        if (shouldSchedule) {
            scope.launch {
                delay(16)
                flushMarkdownEvents()
            }
        }
    }

    private fun flushMarkdownEvents() {
        val events = drainMarkdownEvents()
        if (events.isEmpty()) return
        _state.update { state ->
            events.fold(state) { nextState, event ->
                nextState.reduce(event)
            }
        }
    }

    private fun drainMarkdownEvents(): List<BackendEvent> {
        return synchronized(markdownCoalesceLock) {
            markdownFlushScheduled = false
            pendingMarkdownEvents
                .sortedBy { it.sequence }
                .also { pendingMarkdownEvents.clear() }
        }
    }

    private fun applyRejectedAck(ack: CommandAck) {
        if (ack.accepted) return

        _state.update {
            it.copy(
                runtimeStatus = "Error",
                footer = ack.message.ifBlank { ack.rejectionCode },
            )
        }
    }

    private suspend fun ensureDefaultTextProvider() {
        if (defaultProviderConfigured) return

        val providerId = "provider-openai-compatible-default"
        val providerAck = runtime.dispatch(
            backendCommand(
                kind = "UpdateProvider",
                idempotencyKey = "$providerId:update:default",
                providerId = providerId,
                title = "OpenAI Compatible",
                chunk = "https://api.openai.com/v1",
                payloadJson = "android-secret://providers/default-openai-compatible",
            ),
        )
        applyRejectedAck(providerAck)
        if (!providerAck.accepted) return

        val modelsJson = """
            {"data":[{"id":"hambur-openai-compatible-text","display_name":"OpenAI Compatible Text","supports_reasoning":true,"supports_tool_call":true,"supports_image_input":false,"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}]}
        """.trimIndent()
        val modelsAck = runtime.dispatch(
            backendCommand(
                kind = "RefreshProviderModels",
                idempotencyKey = "$providerId:refresh:default",
                providerId = providerId,
                modelId = "hambur-openai-compatible-text",
                payloadJson = modelsJson,
            ),
        )
        applyRejectedAck(modelsAck)
        defaultProviderConfigured = modelsAck.accepted
    }

    private fun backendCommand(
        kind: String,
        idempotencyKey: String,
        sessionId: String = "",
        turnId: String = "",
        title: String = "",
        messageId: String = "",
        chunk: String = "",
        content: String = "",
        reasoning: String = "",
        providerId: String = "",
        modelId: String = "",
        sourceMessageId: String = "",
        payloadJson: String = "",
        finalize: Boolean = false,
    ): BackendCommand {
        return BackendCommand(
            commandId = "cmd-${System.currentTimeMillis()}-${nextCommandOrdinal()}",
            idempotencyKey = idempotencyKey,
            createdAtMs = System.currentTimeMillis().toULong(),
            kind = kind,
            sessionId = sessionId,
            turnId = turnId,
            title = title,
            messageId = messageId,
            chunk = chunk,
            content = content,
            reasoning = reasoning,
            providerId = providerId,
            modelId = modelId,
            sourceMessageId = sourceMessageId,
            payloadJson = payloadJson,
            finalize = finalize,
        )
    }

    private fun nextCommandOrdinal(): Long = commandCounter.incrementAndGet()
}

private fun AppShellState.applyBaseline(
    snapshot: SessionListSnapshotDto,
    selectedSessionId: String,
    timelineItems: List<TimelineItemDto>,
    baselineSequence: ULong,
): AppShellState {
    return copy(
        runtimeStatus = "Ready",
        latestEventKind = "SnapshotBaseline",
        footer = "Snapshot loaded",
        sessions = snapshot.sessions.map {
            UiSessionSummary(
                id = it.id,
                title = it.title,
                messageCount = it.messageCount,
                latestPreview = it.latestPreview,
            )
        },
        selectedSessionId = selectedSessionId,
        timelineItems = timelineItems.toUiTimelineItems(),
        markdownMessageId = "",
        markdownBlocks = emptyList(),
        pendingMarkdownBlock = null,
        activePreviewPath = "",
        pendingAttachments = emptyList(),
        snapshotSequence = baselineSequence,
        lastAppliedSequence = baselineSequence,
        appliedEventIds = emptySet(),
        activeTurnIds = emptyMap(),
    )
}

private fun AppShellState.reduce(event: BackendEvent): AppShellState {
    if (event.eventId in appliedEventIds || event.sequence <= lastAppliedSequence) {
        return this
    }

    val nextAppliedEventIds = rememberEventId(event.eventId)
    if (isStaleTurnEvent(event)) {
        return copy(
            latestEventKind = event.kind,
            lastAppliedSequence = event.sequence,
            appliedEventIds = nextAppliedEventIds,
        )
    }

    val snapshot = event.snapshot
    val status = when (event.kind) {
        "RuntimeReady",
        "SessionCreated",
        "SessionOpened",
        "SessionDeleted",
        "ModelsUpdated",
        "AttachmentImported",
        "PendingAttachmentRemoved",
        "PendingAttachmentsCleaned",
        "MessageUpserted",
        "AssistantMessageFinished",
        "ToolCallFinished",
        "TurnFinished",
        "TurnCancelled" -> "Ready"
        "TurnStarted",
        "TurnStateChanged",
        "AssistantMessageStarted",
        "AssistantContentDelta",
        "AssistantReasoningDelta",
        "ToolCallStarted",
        "ToolCallDelta",
        "MarkdownRenderUpdate" -> "Streaming"
        "ToolCallFailed",
        "TurnFailed" -> "Error"
        "RuntimeClosed" -> "Closed"
        "RuntimeError" -> "Error"
        else -> runtimeStatus
    }
    val footer = when {
        event.errorCode.isNotBlank() -> event.message.ifBlank { event.errorCode }
        event.kind == "RuntimeReady" -> "Snapshot loaded"
        event.kind == "SessionCreated" -> "Session created"
        event.kind == "SessionOpened" -> "Session opened"
        event.kind == "SessionDeleted" -> "Session deleted"
        event.kind == "ModelsUpdated" -> event.message.ifBlank { "Models updated" }
        event.kind == "AttachmentImported" -> event.message.ifBlank { "Attachment imported" }
        event.kind == "PendingAttachmentRemoved" -> "Attachment removed"
        event.kind == "PendingAttachmentsCleaned" -> "Pending attachments cleared"
        event.kind == "TurnStarted" -> "Turn started"
        event.kind == "AssistantMessageStarted" -> "Assistant streaming"
        event.kind == "AssistantReasoningDelta" -> "Reasoning streamed"
        event.kind == "AssistantContentDelta" -> "Content streamed"
        event.kind == "ToolCallStarted" -> event.message.ifBlank { "Tool started" }
        event.kind == "ToolCallDelta" -> "Tool call streamed"
        event.kind == "ToolCallFinished" -> event.message.ifBlank { "Tool finished" }
        event.kind == "ToolCallFailed" -> event.message.ifBlank { "Tool failed" }
        event.kind == "AssistantMessageFinished" -> "Assistant finished"
        event.kind == "TurnFinished" -> "Turn finished"
        event.kind == "TurnCancelled" -> "Turn cancelled"
        event.kind == "RuntimeClosed" -> "Runtime closed"
        else -> footer
    }

    val nextSelectedSessionId = snapshot.selectedSessionId
    val sessionChanged = nextSelectedSessionId != selectedSessionId
    val markdownUpdate = event.markdownRenderUpdate
    val shouldApplyMarkdown = event.kind == "MarkdownRenderUpdate" &&
        markdownUpdate.messageId.isNotBlank() &&
        event.belongsToSelectedSession(nextSelectedSessionId)
    val baseMarkdownBlocks = if (shouldApplyMarkdown &&
        (markdownUpdate.reset || markdownMessageId != markdownUpdate.messageId)
    ) {
        emptyList()
    } else {
        markdownBlocks
    }
    val nextMarkdownBlocks = if (shouldApplyMarkdown) {
        val invalidatedIds = markdownUpdate.invalidatedBlockIds.toSet()
        val committedKeys = markdownUpdate.committedNodes.map { it.stableKey }.toSet()
        val retained = baseMarkdownBlocks
            .filterNot { block ->
                block.blockId in invalidatedIds || block.stableKey in committedKeys
            }
            .toMutableList()
        retained.addAll(markdownUpdate.committedNodes)
        retained.sortedBy { it.blockId }
    } else {
        markdownBlocks
    }

    return copy(
        runtimeStatus = status,
        latestEventKind = event.kind,
        footer = footer,
        sessions = snapshot.sessions.map {
            UiSessionSummary(
                id = it.id,
                title = it.title,
                messageCount = it.messageCount,
                latestPreview = it.latestPreview,
            )
        },
        selectedSessionId = nextSelectedSessionId,
        timelineItems = snapshot.timelineItems.map {
            UiTimelineItem(
                id = it.id,
                stableKey = it.stableKey,
                contentType = it.contentType,
                versionSequence = it.versionSequence,
                smallSummary = it.smallSummary,
                kind = it.kind,
                traceTitle = it.traceTitle,
                traceContent = it.traceContent,
                traceStatus = it.traceStatus,
                toolCallId = it.toolCallId,
                toolName = it.toolName,
            )
        },
        pendingAttachments = snapshot.pendingAttachments.toUiPendingAttachments(),
        markdownMessageId = when {
            shouldApplyMarkdown -> markdownUpdate.messageId
            sessionChanged -> ""
            else -> markdownMessageId
        },
        markdownBlocks = when {
            shouldApplyMarkdown -> nextMarkdownBlocks
            sessionChanged -> emptyList()
            else -> markdownBlocks
        },
        pendingMarkdownBlock = when {
            shouldApplyMarkdown -> markdownUpdate.pendingNode
            sessionChanged -> null
            else -> pendingMarkdownBlock
        },
        activePreviewPath = if (sessionChanged) "" else activePreviewPath,
        lastAppliedSequence = event.sequence,
        appliedEventIds = nextAppliedEventIds,
        activeTurnIds = updateActiveTurnIds(event),
    )
}

private fun BackendEvent.belongsToSelectedSession(selectedSessionId: String): Boolean {
    return sessionId.isBlank() || selectedSessionId.isBlank() || sessionId == selectedSessionId
}

private fun AppShellState.isStaleTurnEvent(event: BackendEvent): Boolean {
    if (event.sessionId.isBlank() || event.turnId.isBlank() || event.kind == "TurnStarted") {
        return false
    }
    val activeTurnId = activeTurnIds[event.sessionId] ?: return false
    return activeTurnId != event.turnId
}

private fun AppShellState.updateActiveTurnIds(event: BackendEvent): Map<String, String> {
    if (event.sessionId.isBlank() || event.turnId.isBlank()) return activeTurnIds
    return when (event.kind) {
        "TurnStarted" -> activeTurnIds + (event.sessionId to event.turnId)
        "TurnFinished", "TurnFailed", "TurnCancelled" -> {
            if (activeTurnIds[event.sessionId] == event.turnId) {
                activeTurnIds - event.sessionId
            } else {
                activeTurnIds
            }
        }
        else -> activeTurnIds
    }
}

private fun AppShellState.rememberEventId(eventId: String): Set<String> {
    if (eventId.isBlank()) return appliedEventIds
    val next = appliedEventIds + eventId
    return if (next.size > 512) {
        next.toList().takeLast(256).toSet()
    } else {
        next
    }
}

private fun List<TimelineItemDto>.toUiTimelineItems(): List<UiTimelineItem> {
    return map {
        UiTimelineItem(
            id = it.id,
            stableKey = it.stableKey,
            contentType = it.contentType,
            versionSequence = it.versionSequence,
            smallSummary = it.smallSummary,
            kind = it.kind,
            traceTitle = it.traceTitle,
            traceContent = it.traceContent,
            traceStatus = it.traceStatus,
            toolCallId = it.toolCallId,
            toolName = it.toolName,
        )
    }
}

private fun List<AttachmentDto>.toUiPendingAttachments(): List<UiPendingAttachment> {
    return map {
        UiPendingAttachment(
            id = it.id,
            kind = it.kind,
            displayName = it.displayName,
            mimeType = it.mimeType,
            byteSize = it.byteSize,
            sandboxPath = it.sandboxPath,
        )
    }
}

private fun String.jsonEscaped(): String {
    return buildString {
        this@jsonEscaped.forEach { ch ->
            when (ch) {
                '\\' -> append("\\\\")
                '"' -> append("\\\"")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> append(ch)
            }
        }
    }
}

private fun attachmentPayloadJson(attachmentIds: List<String>): String {
    return attachmentIds.joinToString(
        prefix = "{\"attachmentIds\":[\"",
        separator = "\",\"",
        postfix = "\"]}",
    ) {
        it.jsonEscaped()
    }
}

private fun maxSequence(first: ULong, second: ULong): ULong {
    return if (first >= second) first else second
}
