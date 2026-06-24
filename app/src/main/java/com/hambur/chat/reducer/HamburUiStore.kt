package com.hambur.chat.reducer

import android.util.Log
import com.hambur.chat.uniffi.AppBootstrapConfig
import com.hambur.chat.uniffi.AttachmentDto
import com.hambur.chat.uniffi.BackendCommand
import com.hambur.chat.uniffi.BackendEvent
import com.hambur.chat.uniffi.CommandAck
import com.hambur.chat.uniffi.ConfigAuditDto
import com.hambur.chat.uniffi.DefaultModelGroupDto
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.MessageDto
import com.hambur.chat.uniffi.ModelGroupDto
import com.hambur.chat.uniffi.ModelGroupMemberDto
import com.hambur.chat.uniffi.ProviderModelDto
import com.hambur.chat.uniffi.PublicProviderDto
import com.hambur.chat.uniffi.SessionListSnapshotDto
import com.hambur.chat.uniffi.SettingsSnapshotDto
import com.hambur.chat.uniffi.TimelineItemDto
import com.hambur.chat.uniffi.createRuntime
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.platform.PlatformResult
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
import org.json.JSONObject

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
    val payloadRef: String,
    val smallSummary: String,
    val kind: String,
    val traceTitle: String = "",
    val traceContent: String = "",
    val traceStatus: String = "",
    val toolCallId: String = "",
    val toolName: String = "",
)

data class UiMessageSnapshot(
    val id: String,
    val sessionId: String,
    val role: String,
    val contentText: String,
    val reasoningContent: String,
    val status: String,
    val turnId: String,
    val providerName: String,
    val modelName: String,
    val finishReason: String,
    val nativeFinishReason: String,
    val versionSequence: ULong,
)

data class UiPendingAttachment(
    val id: String,
    val kind: String,
    val displayName: String,
    val mimeType: String,
    val byteSize: ULong,
    val sandboxPath: String,
)

data class UiProviderSettings(
    val id: String,
    val name: String,
    val baseUrl: String,
    val secretLabel: String,
    val enabled: Boolean,
)

data class UiProviderModelSettings(
    val providerId: String,
    val modelId: String,
    val displayName: String,
    val supportsToolCall: Boolean,
    val supportsReasoning: Boolean,
    val supportsImageInput: Boolean,
    val supportsTemperature: Boolean,
    val contextLimit: UInt,
    val outputLimit: UInt,
)

data class UiModelGroupSettings(
    val id: String,
    val name: String,
    val routingStrategy: String,
    val fallbackPolicy: String,
)

data class UiModelGroupMemberSettings(
    val groupId: String,
    val providerId: String,
    val providerName: String,
    val modelId: String,
    val modelDisplayName: String,
    val position: UInt,
    val enabled: Boolean,
)

data class UiDefaultModelGroupSettings(
    val key: String,
    val groupId: String,
)

data class UiAppSetting(
    val key: String,
    val value: String,
)

data class UiConfigAudit(
    val id: String,
    val action: String,
    val targetKind: String,
    val targetId: String,
    val redactedSummary: String,
    val approvalRequired: Boolean,
    val createdAtMs: ULong,
)

data class UiSharedBrowserState(
    val active: Boolean = false,
    val requestId: String = "",
    val action: String = "",
    val url: String = "",
    val status: String = "Idle",
    val lastText: String = "",
)

data class HamburUiState(
    val runtimeStatus: String = "Starting",
    val latestEventKind: String = "Waiting",
    val footer: String = "Rust runtime owns backend state",
    val sessions: List<UiSessionSummary> = emptyList(),
    val selectedSessionId: String = "",
    val timelineItems: List<UiTimelineItem> = emptyList(),
    val messagesById: Map<String, UiMessageSnapshot> = emptyMap(),
    val pendingAttachments: List<UiPendingAttachment> = emptyList(),
    val providers: List<UiProviderSettings> = emptyList(),
    val providerModels: List<UiProviderModelSettings> = emptyList(),
    val modelGroups: List<UiModelGroupSettings> = emptyList(),
    val modelGroupMembers: List<UiModelGroupMemberSettings> = emptyList(),
    val defaultModelGroups: List<UiDefaultModelGroupSettings> = emptyList(),
    val appSettings: List<UiAppSetting> = emptyList(),
    val configAudits: List<UiConfigAudit> = emptyList(),
    val sharedBrowser: UiSharedBrowserState = UiSharedBrowserState(),
    val markdownMessageId: String = "",
    val markdownBlocks: List<MarkdownBlockNodeDto> = emptyList(),
    val pendingMarkdownBlock: MarkdownBlockNodeDto? = null,
    val markdownBlocksByMessageId: Map<String, List<MarkdownBlockNodeDto>> = emptyMap(),
    val pendingMarkdownByMessageId: Map<String, MarkdownBlockNodeDto> = emptyMap(),
    val activePreviewPath: String = "",
    val snapshotSequence: ULong = 0UL,
    val lastAppliedSequence: ULong = 0UL,
    val appliedEventIds: Set<String> = emptySet(),
    val activeTurnIds: Map<String, String> = emptyMap(),
)

class HamburUiStore(
    appFilesDir: String,
    private val platformAdapter: AndroidPlatformAdapter,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val runtime = createRuntime(AppBootstrapConfig(appFilesDir = appFilesDir))
    private val _state = MutableStateFlow(HamburUiState())
    private val startupLock = Any()
    private val startupBuffer = mutableListOf<BackendEvent>()
    private val markdownCoalesceLock = Any()
    private val pendingMarkdownEvents = mutableListOf<BackendEvent>()
    private val commandCounter = AtomicLong()
    private var markdownFlushScheduled = false
    private var baselineApplied = false
    private var defaultProviderConfigured = false

    val state: StateFlow<HamburUiState> = _state.asStateFlow()

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

    fun saveProvider(
        providerId: String,
        name: String,
        baseUrl: String,
        secretRef: String,
        apiKey: String,
        enabled: Boolean,
    ) {
        if (baseUrl.isBlank() || secretRef.isBlank()) return
        if (apiKey.isNotBlank() && secretRef.startsWith("android-secret://")) {
            runCatching {
                platformAdapter.saveSecret(secretRef, apiKey)
            }.onFailure { error ->
                _state.update {
                    it.copy(
                        latestEventKind = "SecretStoreFailed",
                        footer = error.message ?: "Secret store failed",
                    )
                }
                return
            }
        }
        val payload = """
            {"secretRef":"${secretRef.jsonEscaped()}","enabled":$enabled,"iconName":"sparkles"}
        """.trimIndent()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateProvider",
                    idempotencyKey = "provider:${providerId.ifBlank { "new" }}:update:${nextCommandOrdinal()}",
                    title = name.ifBlank { "OpenAI Compatible" },
                    providerId = providerId,
                    chunk = baseUrl,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun deleteProvider(providerId: String, approved: Boolean) {
        if (providerId.isBlank() || !approved) return
        val payload = """{"approvalToken":"approve:delete-provider"}"""
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "DeleteProvider",
                    idempotencyKey = "provider:$providerId:delete:${nextCommandOrdinal()}",
                    providerId = providerId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun refreshProviderModels(providerId: String, modelId: String) {
        if (providerId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "RefreshProviderModels",
                    idempotencyKey = "provider:$providerId:models:${nextCommandOrdinal()}",
                    providerId = providerId,
                    modelId = modelId.ifBlank { "hambur-openai-compatible-text" },
                ),
            )
        }
    }

    fun saveModelOverride(
        providerId: String,
        modelId: String,
        displayName: String,
        supportsToolCall: Boolean,
        supportsReasoning: Boolean,
        supportsImageInput: Boolean,
        contextLimit: UInt,
        outputLimit: UInt,
    ) {
        if (providerId.isBlank() || modelId.isBlank()) return
        val payload = """
            {"providerId":"${providerId.jsonEscaped()}","modelId":"${modelId.jsonEscaped()}","displayName":"${displayName.jsonEscaped()}","supportsToolCall":$supportsToolCall,"supportsReasoning":$supportsReasoning,"supportsImageInput":$supportsImageInput,"supportsStructuredOutput":false,"supportsTemperature":true,"contextLimit":$contextLimit,"outputLimit":$outputLimit}
        """.trimIndent()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateModelOverride",
                    idempotencyKey = "model:$providerId:$modelId:override:${nextCommandOrdinal()}",
                    providerId = providerId,
                    modelId = modelId,
                    title = displayName,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveModelGroup(
        groupId: String,
        name: String,
        routingStrategy: String,
        fallbackPolicy: String,
    ) {
        val payload = """
            {"groupId":"${groupId.jsonEscaped()}","name":"${name.jsonEscaped()}","routingStrategy":"${routingStrategy.jsonEscaped()}","fallbackPolicy":"${fallbackPolicy.jsonEscaped()}"}
        """.trimIndent()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateModelGroup",
                    idempotencyKey = "model-group:${groupId.ifBlank { "new" }}:update:${nextCommandOrdinal()}",
                    messageId = groupId,
                    title = name,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun addModelGroupMember(
        groupId: String,
        providerId: String,
        modelId: String,
        position: UInt,
        enabled: Boolean,
    ) {
        if (groupId.isBlank() || providerId.isBlank() || modelId.isBlank()) return
        val payload = """
            {"groupId":"${groupId.jsonEscaped()}","providerId":"${providerId.jsonEscaped()}","modelId":"${modelId.jsonEscaped()}","position":$position,"enabled":$enabled}
        """.trimIndent()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateModelGroupMember",
                    idempotencyKey = "model-group-member:$groupId:$providerId:$modelId:${nextCommandOrdinal()}",
                    messageId = groupId,
                    providerId = providerId,
                    modelId = modelId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun setDefaultModelGroup(key: String, groupId: String) {
        if (key.isBlank() || groupId.isBlank()) return
        val payload = """{"key":"${key.jsonEscaped()}","groupId":"${groupId.jsonEscaped()}"}"""
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "SetDefaultModelGroup",
                    idempotencyKey = "default-model-group:$key:$groupId:${nextCommandOrdinal()}",
                    chunk = key,
                    messageId = groupId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveAppSetting(key: String, value: String, approved: Boolean = false) {
        val commandKind = when (key) {
            "tool_settings" -> "UpdateToolSettings"
            "skills" -> "UpdateSkills"
            "memory_projections" -> "UpdateMemoryProjections"
            "startup_tasks" -> "UpdateStartupTasks"
            "rootfs_settings" -> "UpdateRootfsSettings"
            else -> return
        }
        val payload = if (key == "startup_tasks" || key == "rootfs_settings") {
            value.withApprovalToken("approve:$key", approved)
        } else {
            value
        }
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = commandKind,
                    idempotencyKey = "app-setting:$key:${nextCommandOrdinal()}",
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveRawAppSetting(key: String, value: String, approved: Boolean = false) {
        if (key.isBlank() || value.isBlank()) return
        val payload = """{"settingKey":"${key.jsonEscaped()}","value":${value.jsonValueOrString()}}"""
            .withApprovalToken("approve:$key", approved)
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateAppSetting",
                    idempotencyKey = "app-setting-raw:$key:${nextCommandOrdinal()}",
                    chunk = key,
                    content = value,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveBrowserToolSettings(value: String) {
        if (value.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateBrowserToolSettings",
                    idempotencyKey = "browser-tool-settings:${nextCommandOrdinal()}",
                    payloadJson = value,
                ),
            )
        }
    }

    fun setSkillEnabled(skillId: String, enabled: Boolean) {
        if (skillId.isBlank()) return
        val payload = """{"skillId":"${skillId.jsonEscaped()}","enabled":$enabled}"""
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateSkillEnabled",
                    idempotencyKey = "skill:$skillId:enabled:$enabled:${nextCommandOrdinal()}",
                    messageId = skillId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveStartupTask(taskId: String, payloadJson: String, approved: Boolean) {
        if (taskId.isBlank() || payloadJson.isBlank()) return
        val payload = payloadJson.withApprovalToken("approve:startup_tasks", approved)
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateStartupTask",
                    idempotencyKey = "startup-task:$taskId:update:${nextCommandOrdinal()}",
                    messageId = taskId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun deleteStartupTask(taskId: String, approved: Boolean) {
        if (taskId.isBlank()) return
        val payload = """{"taskId":"${taskId.jsonEscaped()}"}"""
            .withApprovalToken("approve:startup_tasks", approved)
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "DeleteStartupTask",
                    idempotencyKey = "startup-task:$taskId:delete:${nextCommandOrdinal()}",
                    messageId = taskId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun saveRootfsSetting(key: String, value: String, approved: Boolean) {
        if (key.isBlank() || value.isBlank()) return
        val payload = """{"settingKey":"${key.jsonEscaped()}","value":${value.jsonValueOrString()}}"""
            .withApprovalToken("approve:rootfs_setting:$key", approved)
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "UpdateRootfsSetting",
                    idempotencyKey = "rootfs-setting:$key:update:${nextCommandOrdinal()}",
                    chunk = key,
                    content = value,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun runRootfsWarmup(sessionId: String) {
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "RunRootfsWarmup",
                    idempotencyKey = "rootfs:warmup:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                ),
            )
        }
    }

    fun resetRootfs(approved: Boolean) {
        val payload = """{"approvalToken":"approve:rootfs_reset"}""".takeIf { approved }.orEmpty()
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "ResetRootfs",
                    idempotencyKey = "rootfs:reset:${nextCommandOrdinal()}",
                    payloadJson = payload,
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

    fun renderMarkdownText(sessionId: String, messageId: String, markdown: String) {
        if (sessionId.isBlank() || messageId.isBlank() || markdown.isBlank()) return
        scope.launch {
            val chunks = markdown.chunked(4096)
            chunks.forEachIndexed { index, chunk ->
                val ack = runtime.dispatch(
                    backendCommand(
                        kind = "AppendMarkdownDelta",
                        idempotencyKey = "markdown-render:$messageId:$index:${markdown.hashCode()}",
                        sessionId = sessionId,
                        messageId = messageId,
                        chunk = chunk,
                    ),
                )
                applyRejectedAck(ack)
                if (!ack.accepted) return@launch
            }
            val finalAck = runtime.dispatch(
                backendCommand(
                    kind = "AppendMarkdownDelta",
                    idempotencyKey = "markdown-render:$messageId:final:${markdown.hashCode()}",
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

    fun regenerateMessage(sessionId: String, sourceMessageId: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "RegenerateMessage",
                    idempotencyKey = "message:$sourceMessageId:regenerate:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                ),
            )
            applyRejectedAck(ack)
        }
    }

    fun retryMessage(sessionId: String, sourceMessageId: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "RetryTurn",
                    idempotencyKey = "message:$sourceMessageId:retry:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                ),
            )
            applyRejectedAck(ack)
        }
    }

    fun editMessage(sessionId: String, sourceMessageId: String, content: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank() || content.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "EditMessage",
                    idempotencyKey = "message:$sourceMessageId:edit:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                    content = content,
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

    fun clearPendingAttachments(sessionId: String) {
        if (sessionId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "ClearPendingAttachments",
                    idempotencyKey = "$sessionId:clear-pending-attachments:${nextCommandOrdinal()}",
                    sessionId = sessionId,
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
        val settingsSnapshot = runCatching {
            runtime.getSettingsSnapshot()
        }.getOrNull()

        val bufferedEvents = synchronized(startupLock) {
            _state.update {
                it.applyBaseline(
                    snapshot = sessionSnapshot,
                    selectedSessionId = selectedSessionId,
                    timelineItems = timelineItems,
                    settingsSnapshot = settingsSnapshot,
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
        refreshVisibleMessageSnapshots()
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
        if (event.kind == "PlatformRequest") {
            handlePlatformRequest(event)
        }
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
        if (event.kind == "SettingsChanged" || event.kind == "ModelsUpdated") {
            refreshSettingsSnapshot()
        }
        when (event.kind) {
            "SessionCreated",
            "SessionOpened",
            "MessageUpserted",
            "AssistantMessageStarted",
            "AssistantContentDelta",
            "AssistantMessageFinished",
            "TurnFinished",
            "TurnFailed",
            "TurnCancelled" -> refreshVisibleMessageSnapshots()
        }
    }

    private fun handlePlatformRequest(event: BackendEvent) {
        val request = event.platformRequest
        if (request.requestId.isBlank()) return
        val action = request.payloadJson.jsonStringAt("action", "action")
        val url = request.payloadJson.jsonStringAt("action", "url")
        _state.update {
            it.copy(
                sharedBrowser = UiSharedBrowserState(
                    active = true,
                    requestId = request.requestId,
                    action = action.ifBlank { request.kind },
                    url = url,
                    status = "Running",
                ),
            )
        }

        scope.launch {
            val result = runCatching {
                platformAdapter.handle(request)
            }.getOrElse { error ->
                PlatformResult(
                    requestId = request.requestId,
                    isError = true,
                    errorCode = "PlatformAdapterFailed",
                    message = error.message ?: "Platform adapter failed",
                )
            }
            submitPlatformResult(result)
            _state.update {
                it.copy(
                    sharedBrowser = it.sharedBrowser.copy(
                        active = false,
                        status = if (result.isError) {
                            result.errorCode.ifBlank { "Failed" }
                        } else {
                            "Completed"
                        },
                        lastText = result.payloadJson.take(240).ifBlank { result.message },
                    ),
                )
            }
        }
    }

    private fun submitPlatformResult(result: PlatformResult) {
        val payload = """
            {"requestId":"${result.requestId.jsonEscaped()}","isError":${result.isError},"payloadJson":${result.payloadJson.jsonValueOrString()},"errorCode":"${result.errorCode.jsonEscaped()}","message":"${result.message.jsonEscaped()}"}
        """.trimIndent()
        val ack = runtime.dispatch(
            backendCommand(
                kind = "SubmitPlatformResult",
                idempotencyKey = "platform:${result.requestId}:result",
                messageId = result.requestId,
                payloadJson = payload,
            ),
        )
        applyRejectedAck(ack)
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

    private fun refreshSettingsSnapshot() {
        val snapshot = runCatching {
            runtime.getSettingsSnapshot()
        }.getOrNull() ?: return
        _state.update { state ->
            state.applySettingsSnapshot(snapshot)
        }
    }

    private fun refreshVisibleMessageSnapshots() {
        val current = _state.value
        val messageItems = current.timelineItems
            .asSequence()
            .filter { it.contentType == "message" && it.payloadRef.isNotBlank() }
            .filter { item ->
                val cached = current.messagesById[item.payloadRef]
                cached == null || cached.versionSequence < item.versionSequence
            }
            .map { it.payloadRef }
            .distinct()
            .toList()
        if (messageItems.isEmpty()) return

        scope.launch {
            val loaded = messageItems.mapNotNull { messageId ->
                runCatching {
                    runtime.getMessageSnapshot(messageId).message?.toUiMessageSnapshot()
                }.getOrNull()
            }
            if (loaded.isEmpty()) return@launch
            _state.update { state ->
                state.copy(
                    messagesById = state.messagesById + loaded.associateBy { it.id },
                )
            }
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

private fun HamburUiState.applyBaseline(
    snapshot: SessionListSnapshotDto,
    selectedSessionId: String,
    timelineItems: List<TimelineItemDto>,
    settingsSnapshot: SettingsSnapshotDto?,
    baselineSequence: ULong,
): HamburUiState {
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
        messagesById = emptyMap(),
        providers = settingsSnapshot?.providers?.toUiProviders().orEmpty(),
        providerModels = settingsSnapshot?.providerModels?.toUiProviderModels().orEmpty(),
        modelGroups = settingsSnapshot?.modelGroups?.toUiModelGroups().orEmpty(),
        modelGroupMembers = settingsSnapshot?.modelGroupMembers?.toUiModelGroupMembers().orEmpty(),
        defaultModelGroups = settingsSnapshot?.defaultModelGroups?.toUiDefaultModelGroups().orEmpty(),
        appSettings = settingsSnapshot?.settings?.toUiAppSettings().orEmpty(),
        configAudits = settingsSnapshot?.configAudits?.toUiConfigAudits().orEmpty(),
        markdownMessageId = "",
        markdownBlocks = emptyList(),
        pendingMarkdownBlock = null,
        markdownBlocksByMessageId = emptyMap(),
        pendingMarkdownByMessageId = emptyMap(),
        activePreviewPath = "",
        pendingAttachments = emptyList(),
        snapshotSequence = baselineSequence,
        lastAppliedSequence = baselineSequence,
        appliedEventIds = emptySet(),
        activeTurnIds = emptyMap(),
    )
}

private fun HamburUiState.applySettingsSnapshot(snapshot: SettingsSnapshotDto): HamburUiState {
    return copy(
        providers = snapshot.providers.toUiProviders(),
        providerModels = snapshot.providerModels.toUiProviderModels(),
        modelGroups = snapshot.modelGroups.toUiModelGroups(),
        modelGroupMembers = snapshot.modelGroupMembers.toUiModelGroupMembers(),
        defaultModelGroups = snapshot.defaultModelGroups.toUiDefaultModelGroups(),
        appSettings = snapshot.settings.toUiAppSettings(),
        configAudits = snapshot.configAudits.toUiConfigAudits(),
    )
}

private fun HamburUiState.reduce(event: BackendEvent): HamburUiState {
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
        "SettingsChanged",
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
        event.kind == "SettingsChanged" -> event.message.ifBlank { "Settings updated" }
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
    val currentMessageBlocks = markdownBlocksByMessageId[markdownUpdate.messageId].orEmpty()
    val baseMarkdownBlocks = if (shouldApplyMarkdown && markdownUpdate.reset) {
        emptyList()
    } else {
        currentMessageBlocks
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
        currentMessageBlocks
    }
    val nextMarkdownBlocksByMessageId = if (shouldApplyMarkdown) {
        markdownBlocksByMessageId + (markdownUpdate.messageId to nextMarkdownBlocks)
    } else if (sessionChanged) {
        emptyMap()
    } else {
        markdownBlocksByMessageId
    }
    val nextPendingMarkdownByMessageId = if (shouldApplyMarkdown) {
        val pendingNode = markdownUpdate.pendingNode
        if (pendingNode == null) {
            pendingMarkdownByMessageId - markdownUpdate.messageId
        } else {
            pendingMarkdownByMessageId + (markdownUpdate.messageId to pendingNode)
        }
    } else if (sessionChanged) {
        emptyMap()
    } else {
        pendingMarkdownByMessageId
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
                payloadRef = it.payloadRef,
                smallSummary = it.smallSummary,
                kind = it.kind,
                traceTitle = it.traceTitle,
                traceContent = it.traceContent,
                traceStatus = it.traceStatus,
                toolCallId = it.toolCallId,
                toolName = it.toolName,
            )
        },
        messagesById = if (sessionChanged) emptyMap() else messagesById,
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
        markdownBlocksByMessageId = nextMarkdownBlocksByMessageId,
        pendingMarkdownByMessageId = nextPendingMarkdownByMessageId,
        activePreviewPath = if (sessionChanged) "" else activePreviewPath,
        sharedBrowser = sharedBrowser,
        lastAppliedSequence = event.sequence,
        appliedEventIds = nextAppliedEventIds,
        activeTurnIds = updateActiveTurnIds(event),
    )
}

private fun BackendEvent.belongsToSelectedSession(selectedSessionId: String): Boolean {
    return sessionId.isBlank() || selectedSessionId.isBlank() || sessionId == selectedSessionId
}

private fun HamburUiState.isStaleTurnEvent(event: BackendEvent): Boolean {
    if (event.sessionId.isBlank() || event.turnId.isBlank() || event.kind == "TurnStarted") {
        return false
    }
    val activeTurnId = activeTurnIds[event.sessionId] ?: return false
    return activeTurnId != event.turnId
}

private fun HamburUiState.updateActiveTurnIds(event: BackendEvent): Map<String, String> {
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

private fun HamburUiState.rememberEventId(eventId: String): Set<String> {
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
            payloadRef = it.payloadRef,
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

private fun MessageDto.toUiMessageSnapshot(): UiMessageSnapshot {
    return UiMessageSnapshot(
        id = id,
        sessionId = sessionId,
        role = role,
        contentText = contentText,
        reasoningContent = reasoningContent,
        status = status,
        turnId = turnId,
        providerName = providerNameSnapshot,
        modelName = modelNameSnapshot,
        finishReason = finishReason,
        nativeFinishReason = nativeFinishReason,
        versionSequence = versionSequence,
    )
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

private fun List<PublicProviderDto>.toUiProviders(): List<UiProviderSettings> {
    return map {
        UiProviderSettings(
            id = it.id,
            name = it.name,
            baseUrl = it.baseUrl,
            secretLabel = it.secretLabel,
            enabled = it.enabled,
        )
    }
}

private fun List<ProviderModelDto>.toUiProviderModels(): List<UiProviderModelSettings> {
    return map {
        UiProviderModelSettings(
            providerId = it.providerId,
            modelId = it.modelId,
            displayName = it.displayName,
            supportsToolCall = it.supportsToolCall,
            supportsReasoning = it.supportsReasoning,
            supportsImageInput = it.supportsImageInput,
            supportsTemperature = it.supportsTemperature,
            contextLimit = it.contextLimit,
            outputLimit = it.outputLimit,
        )
    }
}

private fun List<ModelGroupDto>.toUiModelGroups(): List<UiModelGroupSettings> {
    return map {
        UiModelGroupSettings(
            id = it.id,
            name = it.name,
            routingStrategy = it.routingStrategy,
            fallbackPolicy = it.fallbackPolicy,
        )
    }
}

private fun List<ModelGroupMemberDto>.toUiModelGroupMembers(): List<UiModelGroupMemberSettings> {
    return map {
        UiModelGroupMemberSettings(
            groupId = it.groupId,
            providerId = it.providerId,
            providerName = it.providerName,
            modelId = it.modelId,
            modelDisplayName = it.modelDisplayName,
            position = it.position,
            enabled = it.enabled,
        )
    }
}

private fun List<DefaultModelGroupDto>.toUiDefaultModelGroups(): List<UiDefaultModelGroupSettings> {
    return map {
        UiDefaultModelGroupSettings(
            key = it.key,
            groupId = it.groupId,
        )
    }
}

private fun List<com.hambur.chat.uniffi.AppSettingDto>.toUiAppSettings(): List<UiAppSetting> {
    return map {
        UiAppSetting(
            key = it.key,
            value = it.value,
        )
    }
}

private fun List<ConfigAuditDto>.toUiConfigAudits(): List<UiConfigAudit> {
    return map {
        UiConfigAudit(
            id = it.id,
            action = it.action,
            targetKind = it.targetKind,
            targetId = it.targetId,
            redactedSummary = it.redactedSummary,
            approvalRequired = it.approvalRequired,
            createdAtMs = it.createdAtMs,
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

private fun String.withApprovalToken(token: String, approved: Boolean): String {
    if (!approved) return this
    val trimmed = trim()
    if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
        val body = trimmed.drop(1).dropLast(1).trim()
        val suffix = "\"approvalToken\":\"${token.jsonEscaped()}\""
        return if (body.isEmpty()) {
            "{$suffix}"
        } else {
            "{$body,$suffix}"
        }
    }
    return """{"value":"${trimmed.jsonEscaped()}","approvalToken":"${token.jsonEscaped()}"}"""
}

private fun String.jsonValueOrString(): String {
    val trimmed = trim()
    return if (
        trimmed.startsWith("{") ||
        trimmed.startsWith("[") ||
        trimmed == "true" ||
        trimmed == "false" ||
        trimmed == "null"
    ) {
        trimmed
    } else {
        "\"${trimmed.jsonEscaped()}\""
    }
}

private fun String.jsonStringAt(parent: String, key: String): String {
    return runCatching {
        JSONObject(this)
            .optJSONObject(parent)
            ?.optString(key)
            .orEmpty()
    }.getOrDefault("")
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
