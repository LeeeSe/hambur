package com.hambur.chat.reducer

import android.util.Log
import com.hambur.chat.perf.ChatJankTracer
import com.hambur.chat.uniffi.AppBootstrapConfig
import com.hambur.chat.uniffi.AttachmentDto
import com.hambur.chat.uniffi.BackendCommand
import com.hambur.chat.uniffi.BackendEvent
import com.hambur.chat.uniffi.CommandAck
import com.hambur.chat.uniffi.ConfigAuditDto
import com.hambur.chat.uniffi.DefaultModelGroupDto
import com.hambur.chat.uniffi.MarkdownBlockPayloadDto
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.MessageDto
import com.hambur.chat.uniffi.MemoryFileDetailDto
import com.hambur.chat.uniffi.MemoryFileSummaryDto
import com.hambur.chat.uniffi.ModelGroupDto
import com.hambur.chat.uniffi.ModelGroupMemberDto
import com.hambur.chat.uniffi.ProviderModelDto
import com.hambur.chat.uniffi.PublicProviderDto
import com.hambur.chat.uniffi.SessionListSnapshotDto
import com.hambur.chat.uniffi.RootfsStatusDto
import com.hambur.chat.uniffi.SettingsSnapshotDto
import com.hambur.chat.uniffi.SkillDetailDto
import com.hambur.chat.uniffi.SkillSummaryDto
import com.hambur.chat.uniffi.TimelineItemDto
import com.hambur.chat.uniffi.createRuntime
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.platform.PlatformResult
import java.io.File
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
    val purpose: String,
    val createdAtMs: ULong,
    val updatedAtMs: ULong,
    val pinnedAtMs: ULong,
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
    val attachments: List<UiPendingAttachment> = emptyList(),
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
    val attachments: List<UiPendingAttachment> = emptyList(),
)

data class UiPendingAttachment(
    val id: String,
    val kind: String,
    val displayName: String,
    val mimeType: String,
    val byteSize: ULong,
    val sandboxPath: String,
    val originalUri: String,
)

data class UiProviderSettings(
    val id: String,
    val name: String,
    val baseUrl: String,
    val secretLabel: String,
    val enabled: Boolean,
    val iconName: String,
    val apiType: String,
)

data class UiProviderModelSettings(
    val providerId: String,
    val modelId: String,
    val displayName: String,
    val supportsToolCall: Boolean,
    val supportsReasoning: Boolean,
    val supportsImageInput: Boolean,
    val supportsStructuredOutput: Boolean,
    val supportsTemperature: Boolean,
    val contextLimit: UInt,
    val outputLimit: UInt,
    val reasoningField: String,
    val metadataJson: String,
    val syncedAtMs: ULong,
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

data class UiSkillSummary(
    val name: String,
    val description: String,
    val path: String,
    val category: String,
    val tags: List<String>,
    val builtIn: Boolean,
    val enabled: Boolean,
    val createdAtMs: ULong,
    val modifiedAtMs: ULong,
    val files: List<String>,
)

data class UiSkillDetail(
    val summary: UiSkillSummary = UiSkillSummary("", "", "", "", emptyList(), false, true, 0u, 0u, emptyList()),
    val content: String = "",
    val linkedFilesJson: String = "",
    val selectedFilePath: String = "",
    val selectedFileContent: String = "",
)

data class UiMemoryFileSummary(
    val name: String,
    val sizeBytes: ULong,
    val modifiedAtMs: ULong,
    val entryCount: UInt,
    val preview: String,
)

data class UiMemoryFileDetail(
    val name: String = "",
    val sizeBytes: ULong = 0UL,
    val modifiedAtMs: ULong = 0UL,
    val entryCount: UInt = 0u,
    val content: String = "",
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
    val reasoningByMessageId: Map<String, String> = emptyMap(),
    val pendingAttachments: List<UiPendingAttachment> = emptyList(),
    val providers: List<UiProviderSettings> = emptyList(),
    val providerModels: List<UiProviderModelSettings> = emptyList(),
    val modelGroups: List<UiModelGroupSettings> = emptyList(),
    val modelGroupMembers: List<UiModelGroupMemberSettings> = emptyList(),
    val defaultModelGroups: List<UiDefaultModelGroupSettings> = emptyList(),
    val appSettings: List<UiAppSetting> = emptyList(),
    val configAudits: List<UiConfigAudit> = emptyList(),
    val skills: List<UiSkillSummary> = emptyList(),
    val skillDetails: Map<String, UiSkillDetail> = emptyMap(),
    val memoryFiles: List<UiMemoryFileSummary> = emptyList(),
    val memoryFileDetails: Map<String, UiMemoryFileDetail> = emptyMap(),
    val sharedBrowser: UiSharedBrowserState = UiSharedBrowserState(),
    val markdownBlocksByPayloadRef: Map<String, MarkdownBlockNodeDto> = emptyMap(),
    val activePreviewPath: String = "",
    val snapshotSequence: ULong = 0UL,
    val lastAppliedSequence: ULong = 0UL,
    val appliedEventIds: Set<String> = emptySet(),
    val activeTurnIds: Map<String, String> = emptyMap(),
    val rootfsStatus: RootfsStatusDto? = null,
    val thinkingEnabledBySession: Map<String, Boolean> = emptyMap(),
    val defaultThinkingEnabled: Boolean = false,
)

data class UiSessionScrollPosition(
    val firstVisibleItemIndex: Int = 0,
    val firstVisibleItemScrollOffset: Int = 0,
)

private data class UiSessionCacheEntry(
    val timelineItems: List<UiTimelineItem>,
    val markdownBlocksByPayloadRef: Map<String, MarkdownBlockNodeDto>,
    val messagesById: Map<String, UiMessageSnapshot>,
    val reasoningByMessageId: Map<String, String>,
    val pendingAttachments: List<UiPendingAttachment>,
    val snapshotSequence: ULong,
)

private data class PersistedSessionUiState(
    val thinkingEnabledBySession: Map<String, Boolean> = emptyMap(),
    val scrollPositions: Map<String, UiSessionScrollPosition> = emptyMap(),
)

class HamburUiStore(
    appFilesDir: String,
    nativeLibraryDir: String,
    private val platformAdapter: AndroidPlatformAdapter,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val sessionUiStateFile = File(appFilesDir, "session-ui-state.json")
    init {
        Log.i(
            "RootfsDebug",
            "HamburUiStore init appFilesDir=$appFilesDir nativeLibraryDir=$nativeLibraryDir",
        )
    }
    private val runtime = createRuntime(
        AppBootstrapConfig(
            appFilesDir = appFilesDir,
            nativeLibraryDir = nativeLibraryDir,
        )
    )
    private val _state = MutableStateFlow(HamburUiState())
    private val startupLock = Any()
    private val startupBuffer = mutableListOf<BackendEvent>()
    private val markdownCoalesceLock = Any()
    private val pendingMarkdownEvents = mutableListOf<BackendEvent>()
    private val sessionCacheLock = Any()
    private val sessionCache = linkedMapOf<String, UiSessionCacheEntry>()
    private val sessionScrollPositions = mutableMapOf<String, UiSessionScrollPosition>()
    private val persistedSessionUiState = loadPersistedSessionUiState(sessionUiStateFile)
    private val commandCounter = AtomicLong()
    private var markdownFlushScheduled = false
    private var baselineApplied = false
    private var defaultProviderConfigured = false
    private var sessionUiStatePersistScheduled = false
    private var creatingSession = false

    val state: StateFlow<HamburUiState> = _state.asStateFlow()

    init {
        sessionScrollPositions.putAll(persistedSessionUiState.scrollPositions)
        _state.update {
            it.copy(thinkingEnabledBySession = persistedSessionUiState.thinkingEnabledBySession)
        }
        scope.launch { collectBackendEvents() }
        scope.launch { applyInitialSnapshotBaseline() }
    }

    fun createSession(title: String) {
        val current = _state.value
        if (current.isNewSessionBlank()) return
        synchronized(sessionCacheLock) {
            if (creatingSession) return
            creatingSession = true
        }
        rememberSessionCache(current)
        val newSessionThinking = current.thinkingEnabledForSession()
        runCommand(commandKind = "CreateSession") {
            val (ack, sessionId) = createRealSession(title)
            if (!ack.accepted) {
                synchronized(sessionCacheLock) {
                    creatingSession = false
                }
            }
            if (ack.accepted && sessionId.isNotBlank()) {
                _state.update {
                    it.copy(
                        thinkingEnabledBySession = it.thinkingEnabledBySession + (sessionId to newSessionThinking),
                    )
                }
                persistSessionUiState()
            }
            ack
        }
    }

    fun openSession(sessionId: String) {
        if (sessionId.isBlank()) return
        val target = _state.value.sessions.firstOrNull { it.id == sessionId }
        ChatJankTracer.markSessionSwitch(
            phase = "store_openSession_enqueue",
            targetSessionId = sessionId,
            extra = "targetMessages=${target?.messageCount ?: 0u}",
        )
        applyCachedSession(sessionId)
        runCommand(commandKind = "OpenSession", targetSessionId = sessionId) {
            runtime.dispatch(
                backendCommand(
                    kind = "OpenSession",
                    idempotencyKey = "$sessionId:open:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                ),
            )
        }
    }

    fun setSessionThinkingEnabled(sessionId: String, enabled: Boolean) {
        val targetSessionId = sessionId
            .ifBlank { _state.value.selectedSessionId }
            .ifBlank { NEW_SESSION_THINKING_KEY }
        _state.update {
            it.copy(
                thinkingEnabledBySession = it.thinkingEnabledBySession + (targetSessionId to enabled),
            )
        }
        persistSessionUiState()
    }

    fun updateSessionScrollPosition(
        sessionId: String,
        firstVisibleItemIndex: Int,
        firstVisibleItemScrollOffset: Int,
    ) {
        if (sessionId.isBlank()) return
        synchronized(sessionCacheLock) {
            sessionScrollPositions[sessionId] = UiSessionScrollPosition(
                firstVisibleItemIndex = firstVisibleItemIndex.coerceAtLeast(0),
                firstVisibleItemScrollOffset = firstVisibleItemScrollOffset.coerceAtLeast(0),
            )
        }
        persistSessionUiState()
    }

    fun sessionScrollPosition(sessionId: String): UiSessionScrollPosition {
        if (sessionId.isBlank()) return UiSessionScrollPosition()
        return synchronized(sessionCacheLock) {
            sessionScrollPositions[sessionId] ?: UiSessionScrollPosition()
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

    fun renameSession(sessionId: String, title: String) {
        if (sessionId.isBlank() || title.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "RenameSession",
                    idempotencyKey = "$sessionId:rename:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    title = title,
                ),
            )
        }
    }

    fun setSessionPinned(sessionId: String, pinned: Boolean) {
        if (sessionId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "SetSessionPinned",
                    idempotencyKey = "$sessionId:pinned:$pinned:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    payloadJson = """{"pinned":$pinned}""",
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
        iconName: String,
        apiType: String,
    ) {
        if (baseUrl.isBlank()) return
        val effectiveSecretRef = if (secretRef.startsWith("android-secret://")) {
            secretRef
        } else {
            "android-secret://providers/${providerId.ifBlank { "prv_" + java.util.UUID.randomUUID().toString().replace("-", "") }}"
        }
        if (apiKey.isNotBlank()) {
            runCatching {
                platformAdapter.saveSecret(effectiveSecretRef, apiKey)
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
            {"secretRef":"${effectiveSecretRef.jsonEscaped()}","enabled":$enabled,"iconName":"${iconName.jsonEscaped()}","apiType":"${apiType.jsonEscaped()}"}
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

    fun refreshProviderModels(
        providerId: String,
        baseUrl: String,
        apiKey: String,
        secretRef: String,
        modelId: String
    ) {
        if (providerId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "RefreshProviderModels",
                    idempotencyKey = "provider:$providerId:models:${nextCommandOrdinal()}",
                    providerId = providerId,
                    modelId = modelId.ifBlank { "hambur-openai-compatible-text" },
                    chunk = baseUrl,
                    payloadJson = providerRefreshPayload(secretRef),
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

    fun deleteModelGroup(groupId: String, approved: Boolean = true) {
        if (groupId.isBlank() || !approved) return
        val payload = """{"approvalToken":"approve:delete-model-group","groupId":"${groupId.jsonEscaped()}"}"""
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "DeleteModelGroup",
                    idempotencyKey = "model-group:$groupId:delete:${nextCommandOrdinal()}",
                    messageId = groupId,
                    payloadJson = payload,
                ),
            )
        }
    }

    fun deleteModelGroupMember(groupId: String, providerId: String, modelId: String) {
        if (groupId.isBlank() || providerId.isBlank() || modelId.isBlank()) return
        runCommand {
            runtime.dispatch(
                backendCommand(
                    kind = "DeleteModelGroupMember",
                    idempotencyKey = "model-group-member:$groupId:$providerId:$modelId:delete:${nextCommandOrdinal()}",
                    messageId = groupId,
                    providerId = providerId,
                    modelId = modelId,
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

    fun resetRootfs(preserveRoot: Boolean, approved: Boolean) {
        val payload = if (approved) {
            """{"approvalToken":"approve:rootfs_reset","preserveRoot":$preserveRoot}"""
        } else {
            ""
        }
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

    fun refreshRootfsStatus() {
        scope.launch {
            try {
                Log.i("RootfsDebug", "refreshRootfsStatus start")
                val status = runtime.getRootfsStatus()
                Log.i(
                    "RootfsDebug",
                    "refreshRootfsStatus result installed=${status.rootfsInstalled} backend=${status.backend} root=${status.rootAvailable} chroot=${status.chrootAvailable} proot=${status.prootAvailable} version=${status.version} size=${status.rootfsSizeBytes} path=${status.rootfsPath}",
                )
                _state.update { it.copy(rootfsStatus = status) }
            } catch (e: Exception) {
                Log.e("HamburUiStore", "Failed to refresh rootfs status: ${e.message}", e)
                Log.e("RootfsDebug", "refreshRootfsStatus failed: ${e.message}", e)
            }
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

    fun renderMarkdownDocument(messageId: String, markdown: String): List<MarkdownBlockNodeDto> {
        if (messageId.isBlank() || markdown.isBlank()) return emptyList()
        return runCatching {
            runtime.renderMarkdownDocument(messageId, markdown)
        }.getOrDefault(emptyList())
    }

    fun sendMessage(
        sessionId: String,
        content: String,
        deepThinkingEnabled: Boolean = false,
        searchEnabled: Boolean = false,
        onAccepted: () -> Unit = {},
        onRejected: (String) -> Unit = {},
    ) {
        val attachmentIds = _state.value.pendingAttachments.map { it.id }
        if (content.isBlank()) return

        scope.launch {
            val targetSessionId = sessionId.ifBlank { ensureSessionForNewMessage() }
            if (targetSessionId.isBlank()) return@launch
            setSessionThinkingEnabled(targetSessionId, deepThinkingEnabled)
            ensureDefaultTextProvider()
            val payload = sendPayloadJson(attachmentIds, deepThinkingEnabled, searchEnabled)
            Log.i(
                "ThinkingToggle",
                "dispatch SendMessage target=$targetSessionId deepThinking=$deepThinkingEnabled payload=$payload",
            )
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "SendMessage",
                    idempotencyKey = "message:${System.currentTimeMillis()}:${nextCommandOrdinal()}",
                    sessionId = targetSessionId,
                    content = content,
                    reasoning = "Routing through the configured OpenAI-compatible text provider.",
                    payloadJson = payload,
                ),
            )
            applyRejectedAck(ack)
            if (ack.accepted) {
                onAccepted()
            } else {
                onRejected(ack.message.ifBlank { ack.rejectionCode })
            }
        }
    }

    fun regenerateMessage(sessionId: String, sourceMessageId: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val payload = sendPayloadJson(
                attachmentIds = emptyList(),
                deepThinkingEnabled = _state.value.thinkingEnabledForSession(sessionId),
                searchEnabled = false,
            )
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "RegenerateMessage",
                    idempotencyKey = "message:$sourceMessageId:regenerate:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                    payloadJson = payload,
                ),
            )
            applyRejectedAck(ack)
        }
    }

    fun retryMessage(sessionId: String, sourceMessageId: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val payload = sendPayloadJson(
                attachmentIds = emptyList(),
                deepThinkingEnabled = _state.value.thinkingEnabledForSession(sessionId),
                searchEnabled = false,
            )
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "RetryTurn",
                    idempotencyKey = "message:$sourceMessageId:retry:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                    payloadJson = payload,
                ),
            )
            applyRejectedAck(ack)
        }
    }

    fun editMessage(sessionId: String, sourceMessageId: String, content: String) {
        if (sessionId.isBlank() || sourceMessageId.isBlank() || content.isBlank()) return
        scope.launch {
            ensureDefaultTextProvider()
            val payload = sendPayloadJson(
                attachmentIds = emptyList(),
                deepThinkingEnabled = _state.value.thinkingEnabledForSession(sessionId),
                searchEnabled = false,
            )
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "EditMessage",
                    idempotencyKey = "message:$sourceMessageId:edit:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                    sourceMessageId = sourceMessageId,
                    content = content,
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
        bytesBase64: String = "",
    ) {
        if (sessionId.isBlank()) return
        val kind = if (mimeType.startsWith("image/")) "image" else "file"
        val escapedName = displayName.jsonEscaped()
        val escapedMime = mimeType.jsonEscaped()
        val escapedUri = originalUri.jsonEscaped()
        val escapedPath = sourcePath.jsonEscaped()
        val escapedBytes = bytesBase64.jsonEscaped()
        val payload = """
            {"displayName":"$escapedName","mimeType":"$escapedMime","byteSize":$byteSize,"originalUri":"$escapedUri","sourcePath":"$escapedPath","bytesBase64":"$escapedBytes","originType":"content_uri","kind":"$kind"}
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

    fun resolveSandboxHostPath(sessionId: String, sandboxPath: String): String {
        if (sessionId.isBlank() || sandboxPath.isBlank()) return ""
        return runCatching {
            runtime.resolveSandboxFile(sessionId, sandboxPath).hostPath
        }.getOrDefault("")
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
        var sessionSnapshot = runCatching {
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
        val settingsSnapshot = runCatching {
            runtime.getSettingsSnapshot()
        }.getOrNull()

        val selectedSessionId = resolveStartupSessionId(sessionSnapshot, settingsSnapshot).also {
            sessionSnapshot = runCatching {
                runtime.getSessionListSnapshot(limit = 100u, offset = 0u)
            }.getOrDefault(sessionSnapshot)
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
        val markdownBlockPayloads = timelinePage?.markdownBlockPayloads.orEmpty()

        val bufferedEvents = synchronized(startupLock) {
            _state.update {
                it.applyBaseline(
                    snapshot = sessionSnapshot,
                    selectedSessionId = selectedSessionId,
                    timelineItems = timelineItems,
                    markdownBlockPayloads = markdownBlockPayloads,
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
        rememberSessionCache(_state.value, selectedSessionId)
        refreshVisibleMessageSnapshots()
        refreshKnowledgeSnapshots()
        runRootfsWarmup(selectedSessionId)
    }

    private fun resolveStartupSessionId(
        sessionSnapshot: SessionListSnapshotDto,
        settingsSnapshot: SettingsSnapshotDto?,
    ): String {
        val defaultSessionId = sessionSnapshot.selectedSessionId.ifBlank {
            sessionSnapshot.sessions.firstOrNull()?.id.orEmpty()
        }
        if (settingsSnapshot.settingValue("startupChatMode", "last_chat") == "new_chat") {
            return createRealSession(title = "New chat")
                .also { (ack, _) -> applyRejectedAck(ack) }
                .second
                .ifBlank { defaultSessionId }
        }
        return defaultSessionId.ifBlank {
            createRealSession(title = "New chat")
                .also { (ack, _) -> applyRejectedAck(ack) }
                .second
        }
    }

    private fun createRealSession(title: String): Pair<CommandAck, String> {
        val ack = runtime.dispatch(
            backendCommand(
                kind = "CreateSession",
                idempotencyKey = "session:create:${nextCommandOrdinal()}",
                title = title.ifBlank { "New chat" },
            ),
        )
        if (!ack.accepted) return ack to ""
        val sessionId = runCatching {
            runtime.getSessionListSnapshot(limit = 1u, offset = 0u).selectedSessionId
        }.getOrDefault("")
        return ack to sessionId
    }

    private fun runCommand(
        commandKind: String = "Command",
        targetSessionId: String = "",
        block: () -> CommandAck,
    ) {
        val enqueueNs = ChatJankTracer.nowNs()
        scope.launch {
            ChatJankTracer.markDuration(
                phase = "command_queue_delay",
                startNs = enqueueNs,
                targetSessionId = targetSessionId,
                warnAtMs = 4.0,
                extra = "kind=$commandKind",
            )
            val dispatchStartNs = ChatJankTracer.nowNs()
            val ack = runCatching(block).getOrElse { error ->
                ChatJankTracer.markDuration(
                    phase = "command_dispatch_failed",
                    startNs = dispatchStartNs,
                    targetSessionId = targetSessionId,
                    warnAtMs = 1.0,
                    always = true,
                    extra = "kind=$commandKind error=${error.message.orEmpty()}",
                )
                _state.update {
                    it.copy(
                        runtimeStatus = "Error",
                        footer = error.message ?: "Backend command failed",
                    )
                }
                return@launch
            }

            ChatJankTracer.markDuration(
                phase = "command_dispatch",
                startNs = dispatchStartNs,
                targetSessionId = targetSessionId,
                warnAtMs = 4.0,
                always = commandKind == "OpenSession",
                extra = "kind=$commandKind accepted=${ack.accepted} rejection=${ack.rejectionCode}",
            )
            applyRejectedAck(ack)
        }
    }

    private fun applyEvent(event: BackendEvent) {
        val eventStartNs = ChatJankTracer.nowNs()
        val targetSessionId = event.traceTargetSessionId()
        val eventStats = event.traceStats()
        if (event.message.startsWith("ThinkingToggle ")) {
            Log.i("ThinkingToggle", event.message)
        }
        if (event.kind == "SessionCreated" || event.kind == "RuntimeError" || event.kind == "RuntimeClosed") {
            synchronized(sessionCacheLock) {
                creatingSession = false
            }
        }
        if (event.kind == "AssistantReasoningDelta") {
            Log.i(
                "ThinkingToggle",
                "ui event AssistantReasoningDelta session=${event.sessionId} turn=${event.turnId} deltaLen=${event.message.length}",
            )
        }
        if (event.kind == "PlatformRequest") {
            handlePlatformRequest(event)
        }
        if (event.kind == "MarkdownRenderUpdate") {
            ChatJankTracer.markSessionSwitch(
                phase = "markdown_event_enqueue",
                targetSessionId = targetSessionId,
                extra = eventStats,
            )
            enqueueMarkdownEvent(event)
            return
        }
        val markdownEvents = drainMarkdownEvents()
        ChatJankTracer.markSessionSwitch(
            phase = "event_apply_start",
            targetSessionId = targetSessionId,
            extra = "$eventStats drainedMarkdown=${markdownEvents.size}",
        )
        _state.update { state ->
            ChatJankTracer.timeSessionSwitch(
                phase = "state_reduce",
                targetSessionId = targetSessionId,
                warnAtMs = 4.0,
                always = event.kind == "SessionOpened",
                extra = "$eventStats drainedMarkdown=${markdownEvents.size}",
            ) {
                markdownEvents.fold(state) { nextState, markdownEvent ->
                    nextState.reduce(markdownEvent)
                }.reduce(event)
            }
        }
        rememberSessionCache(_state.value)
        if (event.kind == "SettingsChanged" || event.kind == "ModelsUpdated") {
            refreshSettingsSnapshot()
        }
        when (event.kind) {
            "SessionCreated",
            "SessionOpened",
            "MessageUpserted",
            "AssistantMessageStarted",
            "AssistantContentDelta",
            "AssistantReasoningDelta",
            "AssistantMessageFinished",
            "TurnFinished",
            "TurnFailed",
            "TurnCancelled" -> refreshVisibleMessageSnapshots()
        }
        ChatJankTracer.markDuration(
            phase = "event_apply",
            startNs = eventStartNs,
            targetSessionId = targetSessionId,
            warnAtMs = 6.0,
            always = event.kind == "SessionOpened",
            extra = "$eventStats drainedMarkdown=${markdownEvents.size}",
        )
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
        rememberSessionCache(_state.value)
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
                footer = ack.message.ifBlank { ack.rejectionCode },
            )
        }
    }

    private fun applyCachedSession(sessionId: String) {
        val cached = synchronized(sessionCacheLock) {
            sessionCache[sessionId]
        } ?: return

        _state.update { state ->
            if (state.selectedSessionId == sessionId) {
                state
            } else {
                val sessionExists = state.sessions.any { it.id == sessionId }
                state.copy(
                    latestEventKind = "CachedSessionOpened",
                    footer = "Cached session opened",
                    selectedSessionId = sessionId,
                    timelineItems = cached.timelineItems,
                    messagesById = cached.messagesById,
                    reasoningByMessageId = cached.reasoningByMessageId,
                    pendingAttachments = cached.pendingAttachments,
                    markdownBlocksByPayloadRef = cached.markdownBlocksByPayloadRef,
                    activePreviewPath = "",
                    snapshotSequence = if (sessionExists) state.snapshotSequence else cached.snapshotSequence,
                )
            }
        }
    }

    private fun rememberSessionCache(state: HamburUiState, sessionId: String = state.selectedSessionId) {
        if (sessionId.isBlank()) return
        val entry = UiSessionCacheEntry(
            timelineItems = state.timelineItems,
            markdownBlocksByPayloadRef = state.markdownBlocksByPayloadRef,
            messagesById = state.messagesById,
            reasoningByMessageId = state.reasoningByMessageId,
            pendingAttachments = state.pendingAttachments,
            snapshotSequence = state.snapshotSequence,
        )
        synchronized(sessionCacheLock) {
            sessionCache[sessionId] = entry
            val keys = sessionCache.keys.toList()
            if (keys.size > SESSION_CACHE_LIMIT) {
                keys.take(keys.size - SESSION_CACHE_LIMIT).forEach { sessionCache.remove(it) }
            }
        }
    }

    private fun persistSessionUiState() {
        val shouldSchedule = synchronized(sessionCacheLock) {
            if (sessionUiStatePersistScheduled) {
                false
            } else {
                sessionUiStatePersistScheduled = true
                true
            }
        }
        if (!shouldSchedule) return

        scope.launch {
            delay(250)
            val snapshot = synchronized(sessionCacheLock) {
                sessionUiStatePersistScheduled = false
                PersistedSessionUiState(
                    thinkingEnabledBySession = _state.value.thinkingEnabledBySession,
                    scrollPositions = sessionScrollPositions.toMap(),
                )
            }
            runCatching {
                sessionUiStateFile.parentFile?.mkdirs()
                sessionUiStateFile.writeText(snapshot.toJsonString())
            }.onFailure { error ->
                Log.w("HamburUiStore", "Failed to persist session UI state: ${error.message}")
            }
        }
    }

    private fun ensureSessionForNewMessage(): String {
        val currentState = _state.value
        if (currentState.selectedSessionId.isNotBlank()) {
            val sessionId = currentState.selectedSessionId
            val ack = runtime.dispatch(
                backendCommand(
                    kind = "OpenSession",
                    idempotencyKey = "$sessionId:open:send:${nextCommandOrdinal()}",
                    sessionId = sessionId,
                ),
            )
            applyRejectedAck(ack)
            return if (ack.accepted) sessionId else ""
        }

        val (ack, sessionId) = createRealSession("New chat")
        applyRejectedAck(ack)
        if (!ack.accepted) return ""
        if (sessionId.isNotBlank()) {
            val draftThinking = _state.value.thinkingEnabledForSession("")
            _state.update {
                it.copy(
                    thinkingEnabledBySession = it.thinkingEnabledBySession + (sessionId to draftThinking),
                )
            }
            persistSessionUiState()
        }
        return sessionId
    }

    private fun refreshSettingsSnapshot() {
        val snapshot = runCatching {
            runtime.getSettingsSnapshot()
        }.getOrNull() ?: return
        _state.update { state ->
            state.applySettingsSnapshot(snapshot)
        }
        refreshKnowledgeSnapshots()
    }

    fun refreshKnowledgeSnapshots() {
        scope.launch {
            val skills = runCatching { runtime.listSkills().map { it.toUiSkillSummary() } }
                .getOrDefault(emptyList())
            val memoryFiles = runCatching { runtime.listMemoryFiles().map { it.toUiMemoryFileSummary() } }
                .getOrDefault(emptyList())
            _state.update {
                it.copy(skills = skills, memoryFiles = memoryFiles)
            }
        }
    }

    fun loadSkillDetail(skillId: String, filePath: String = "") {
        if (skillId.isBlank()) return
        scope.launch {
            val detail = runCatching {
                runtime.getSkillDetail(skillId, filePath).toUiSkillDetail()
            }.getOrNull() ?: return@launch
            val key = if (filePath.isBlank()) skillId else "$skillId::$filePath"
            _state.update {
                it.copy(skillDetails = it.skillDetails + (key to detail))
            }
        }
    }

    fun deleteSkill(skillId: String) {
        if (skillId.isBlank()) return
        runCommand {
            runtime.deleteSkill(skillId)
        }
        refreshKnowledgeSnapshots()
    }

    fun loadMemoryFileDetail(name: String) {
        if (name.isBlank()) return
        scope.launch {
            val detail = runCatching {
                runtime.getMemoryFileDetail(name).toUiMemoryFileDetail()
            }.getOrNull() ?: return@launch
            _state.update {
                it.copy(memoryFileDetails = it.memoryFileDetails + (name to detail))
            }
        }
    }

    private fun refreshVisibleMessageSnapshots() {
        val current = _state.value
        val userMessageIds = current.timelineItems
            .asSequence()
            .filter { it.contentType == "user_message" && it.payloadRef.isNotBlank() }
            .map { it.payloadRef to it.versionSequence }
        val assistantMessageIds = current.timelineItems
            .asSequence()
            .filter { it.isAssistantMarkdownBlock() && it.payloadRef.isNotBlank() }
            .mapNotNull { item ->
                val messageId = current.markdownBlocksByPayloadRef[item.payloadRef]?.messageId
                    ?.takeIf { it.isNotBlank() }
                    ?: return@mapNotNull null
                messageId to item.versionSequence
            }
        val messageItems = (userMessageIds + assistantMessageIds)
            .filter { (messageId, versionSequence) ->
                val cached = current.messagesById[messageId]
                cached == null || cached.versionSequence < versionSequence
            }
            .map { it.first }
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
            loaded.forEach { message ->
                Log.i(
                    "ThinkingToggle",
                    "ui snapshot message=${message.id} role=${message.role} contentLen=${message.contentText.length} reasoningLen=${message.reasoningContent.length}",
                )
            }
            _state.update { state ->
                val loadedReasoning = loaded
                    .asSequence()
                    .filter { it.role == "assistant" && it.reasoningContent.isNotBlank() }
                    .associate { it.id to it.reasoningContent }
                state.copy(
                    messagesById = state.messagesById + loaded.associateBy { it.id },
                    reasoningByMessageId = state.reasoningByMessageId + loadedReasoning,
                )
            }
        }
    }

    private suspend fun ensureDefaultTextProvider() {
        if (defaultProviderConfigured) return

        val providerId = "provider-openai-compatible-default"
        if (_state.value.providers.any { it.id == providerId }) {
            defaultProviderConfigured = true
            return
        }

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
    markdownBlockPayloads: List<MarkdownBlockPayloadDto>,
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
                purpose = it.purpose,
                createdAtMs = it.createdAtMs,
                updatedAtMs = it.updatedAtMs,
                pinnedAtMs = it.pinnedAtMs,
                messageCount = it.messageCount,
                latestPreview = it.latestPreview,
            )
        },
        selectedSessionId = selectedSessionId,
        timelineItems = timelineItems.toUiTimelineItems(),
        messagesById = emptyMap(),
        reasoningByMessageId = emptyMap(),
        providers = settingsSnapshot?.providers?.toUiProviders().orEmpty(),
        providerModels = settingsSnapshot?.providerModels?.toUiProviderModels().orEmpty(),
        modelGroups = settingsSnapshot?.modelGroups?.toUiModelGroups().orEmpty(),
        modelGroupMembers = settingsSnapshot?.modelGroupMembers?.toUiModelGroupMembers().orEmpty(),
        defaultModelGroups = settingsSnapshot?.defaultModelGroups?.toUiDefaultModelGroups().orEmpty(),
        appSettings = settingsSnapshot?.settings?.toUiAppSettings().orEmpty(),
        configAudits = settingsSnapshot?.configAudits?.toUiConfigAudits().orEmpty(),
        markdownBlocksByPayloadRef = markdownBlockPayloads.toMarkdownBlockMap(),
        activePreviewPath = "",
        pendingAttachments = emptyList(),
        snapshotSequence = baselineSequence,
        lastAppliedSequence = baselineSequence,
        appliedEventIds = emptySet(),
        activeTurnIds = emptyMap(),
        defaultThinkingEnabled = settingsSnapshot.settingBool("defaultDeepThinkingEnabled", false),
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
        defaultThinkingEnabled = snapshot.settingBool("defaultDeepThinkingEnabled", defaultThinkingEnabled),
    )
}

private fun HamburUiState.reduce(event: BackendEvent): HamburUiState {
    if (event.eventId in appliedEventIds || event.sequence <= lastAppliedSequence) {
        return this
    }

    val nextAppliedEventIds = rememberEventId(event.eventId)
    val targetsVisibleSession = event.targetsVisibleSession(selectedSessionId)
    if (isStaleTurnEvent(event)) {
        return copy(
            latestEventKind = event.kind,
            lastAppliedSequence = if (targetsVisibleSession) event.sequence else lastAppliedSequence,
            appliedEventIds = nextAppliedEventIds,
        )
    }

    val snapshot = event.snapshot
    if (!targetsVisibleSession) {
        return copy(
            latestEventKind = event.kind,
            sessions = event.toUiSessionSummaries(),
            lastAppliedSequence = lastAppliedSequence,
            appliedEventIds = nextAppliedEventIds,
            activeTurnIds = updateActiveTurnIds(event),
        )
    }

    val status = when (event.kind) {
        "RuntimeReady",
        "SessionCreated",
        "SessionOpened",
        "SessionDeleted",
        "SessionRenamed",
        "SessionPinnedChanged",
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
        event.kind == "SessionRenamed" -> "Session renamed"
        event.kind == "SessionPinnedChanged" -> "Session pinned state updated"
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

    val nextSelectedSessionId = if (event.switchesVisibleSession()) {
        snapshot.selectedSessionId
    } else {
        selectedSessionId.ifBlank { snapshot.selectedSessionId }
    }
    val sessionChanged = nextSelectedSessionId != selectedSessionId
    val nextTimelineItems = snapshot.timelineItems.toUiTimelineItems()
    val visibleMarkdownPayloadRefs = nextTimelineItems
        .asSequence()
        .filter { it.isAssistantMarkdownBlock() }
        .map { it.payloadRef }
        .filter { it.isNotBlank() }
        .toSet()
    val snapshotMarkdownBlocksByPayloadRef = snapshot.markdownBlockPayloads.toMarkdownBlockMap()
    val nextMarkdownBlocksByPayloadRef = (
        if (sessionChanged) {
            snapshotMarkdownBlocksByPayloadRef
        } else {
            markdownBlocksByPayloadRef + snapshotMarkdownBlocksByPayloadRef
        }
    ).filterKeys { it in visibleMarkdownPayloadRefs }
    val snapshotReasoningByMessageId = messagesById
        .values
        .asSequence()
        .filter { it.role == "assistant" && it.reasoningContent.isNotBlank() }
        .associate { it.id to it.reasoningContent }
    val nextReasoningByMessageId = if (sessionChanged) {
        snapshotReasoningByMessageId
    } else {
        reasoningByMessageId + snapshotReasoningByMessageId
    }

    return copy(
        runtimeStatus = status,
        latestEventKind = event.kind,
        footer = footer,
        sessions = event.toUiSessionSummaries(),
        selectedSessionId = nextSelectedSessionId,
        timelineItems = nextTimelineItems,
        messagesById = if (sessionChanged) emptyMap() else messagesById,
        reasoningByMessageId = nextReasoningByMessageId,
        pendingAttachments = snapshot.pendingAttachments.toUiPendingAttachments(),
        markdownBlocksByPayloadRef = nextMarkdownBlocksByPayloadRef,
        activePreviewPath = if (sessionChanged) "" else activePreviewPath,
        sharedBrowser = sharedBrowser,
        lastAppliedSequence = event.sequence,
        appliedEventIds = nextAppliedEventIds,
        activeTurnIds = updateActiveTurnIds(event),
    )
}

private fun BackendEvent.switchesVisibleSession(): Boolean {
    return when (kind) {
        "RuntimeReady",
        "SessionCreated",
        "SessionOpened",
        "SessionDeleted" -> true
        else -> false
    }
}

private fun BackendEvent.traceTargetSessionId(): String {
    return when {
        snapshot.selectedSessionId.isNotBlank() -> snapshot.selectedSessionId
        sessionId.isNotBlank() -> sessionId
        else -> ""
    }
}

private fun BackendEvent.traceStats(): String {
    return "kind=$kind sequence=$sequence eventSession=${traceShortId(sessionId)} " +
        "selected=${traceShortId(snapshot.selectedSessionId)} " +
        "sessions=${snapshot.sessions.size} timelineItems=${snapshot.timelineItems.size} " +
        "markdownPayloads=${snapshot.markdownBlockPayloads.size}"
}

private fun BackendEvent.targetsVisibleSession(selectedSessionId: String): Boolean {
    if (switchesVisibleSession()) return true
    if (selectedSessionId.isBlank()) return false
    return if (sessionId.isBlank()) {
        snapshot.selectedSessionId == selectedSessionId
    } else {
        sessionId == selectedSessionId
    }
}

private fun BackendEvent.toUiSessionSummaries(): List<UiSessionSummary> {
    return snapshot.sessions.map {
        UiSessionSummary(
            id = it.id,
            title = it.title,
            purpose = it.purpose,
            createdAtMs = it.createdAtMs,
            updatedAtMs = it.updatedAtMs,
            pinnedAtMs = it.pinnedAtMs,
            messageCount = it.messageCount,
            latestPreview = it.latestPreview,
        )
    }
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

fun HamburUiState.thinkingEnabledForSession(sessionId: String = selectedSessionId): Boolean {
    if (sessionId.isBlank()) {
        return thinkingEnabledBySession[NEW_SESSION_THINKING_KEY] ?: defaultThinkingEnabled
    }
    return thinkingEnabledBySession[sessionId] ?: defaultThinkingEnabled
}

fun HamburUiState.isNewSessionBlank(): Boolean {
    if (selectedSessionId.isBlank()) return true
    val selected = sessions.firstOrNull { it.id == selectedSessionId } ?: return false
    return selected.messageCount == 0u && timelineItems.isEmpty()
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
            attachments = it.attachments.toUiPendingAttachments(),
        )
    }
}

private fun List<MarkdownBlockPayloadDto>.toMarkdownBlockMap(): Map<String, MarkdownBlockNodeDto> {
    return associate { it.id to it.node }
}

private fun UiTimelineItem.isAssistantMarkdownBlock(): Boolean {
    return contentType == "assistant_markdown_block" || contentType == "assistant_pending_block"
}

private fun traceShortId(id: String): String {
    if (id.isBlank()) return "-"
    return if (id.length <= 10) id else id.take(4) + ".." + id.takeLast(6)
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
        attachments = attachments.toUiPendingAttachments(),
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
            originalUri = it.originalUri,
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
            iconName = it.iconName,
            apiType = it.apiType,
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
            supportsStructuredOutput = it.supportsStructuredOutput,
            supportsTemperature = it.supportsTemperature,
            contextLimit = it.contextLimit,
            outputLimit = it.outputLimit,
            reasoningField = it.reasoningField,
            metadataJson = it.metadataJson,
            syncedAtMs = it.syncedAtMs,
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

private fun SkillSummaryDto.toUiSkillSummary(): UiSkillSummary {
    return UiSkillSummary(
        name = name,
        description = description,
        path = path,
        category = category,
        tags = tags,
        builtIn = builtIn,
        enabled = enabled,
        createdAtMs = createdAtMs,
        modifiedAtMs = modifiedAtMs,
        files = files,
    )
}

private fun SkillDetailDto.toUiSkillDetail(): UiSkillDetail {
    return UiSkillDetail(
        summary = summary.toUiSkillSummary(),
        content = content,
        linkedFilesJson = linkedFilesJson,
        selectedFilePath = selectedFilePath,
        selectedFileContent = selectedFileContent,
    )
}

private fun MemoryFileSummaryDto.toUiMemoryFileSummary(): UiMemoryFileSummary {
    return UiMemoryFileSummary(
        name = name,
        sizeBytes = sizeBytes,
        modifiedAtMs = modifiedAtMs,
        entryCount = entryCount,
        preview = preview,
    )
}

private fun MemoryFileDetailDto.toUiMemoryFileDetail(): UiMemoryFileDetail {
    return UiMemoryFileDetail(
        name = name,
        sizeBytes = sizeBytes,
        modifiedAtMs = modifiedAtMs,
        entryCount = entryCount,
        content = content,
    )
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

private fun sendPayloadJson(
    attachmentIds: List<String>,
    deepThinkingEnabled: Boolean,
    searchEnabled: Boolean,
): String {
    val attachments = attachmentIds.joinToString(prefix = "[\"", separator = "\",\"", postfix = "\"]") {
        it.jsonEscaped()
    }
    return """{"attachmentIds":$attachments,"deepThinkingEnabled":$deepThinkingEnabled,"searchEnabled":$searchEnabled}"""
}

private fun loadPersistedSessionUiState(file: File): PersistedSessionUiState {
    return runCatching {
        val root = JSONObject(file.takeIf { it.exists() }?.readText().orEmpty())
        val thinking = mutableMapOf<String, Boolean>()
        root.optJSONObject("thinkingEnabledBySession")?.let { objectValue ->
            objectValue.keys().forEach { key ->
                thinking[key] = objectValue.optBoolean(key, false)
            }
        }
        val scroll = mutableMapOf<String, UiSessionScrollPosition>()
        root.optJSONObject("scrollPositions")?.let { objectValue ->
            objectValue.keys().forEach { key ->
                val position = objectValue.optJSONObject(key) ?: return@forEach
                scroll[key] = UiSessionScrollPosition(
                    firstVisibleItemIndex = position.optInt("firstVisibleItemIndex", 0).coerceAtLeast(0),
                    firstVisibleItemScrollOffset = position.optInt("firstVisibleItemScrollOffset", 0).coerceAtLeast(0),
                )
            }
        }
        PersistedSessionUiState(
            thinkingEnabledBySession = thinking,
            scrollPositions = scroll,
        )
    }.getOrDefault(PersistedSessionUiState())
}

private fun PersistedSessionUiState.toJsonString(): String {
    val thinking = JSONObject().apply {
        thinkingEnabledBySession.forEach { (sessionId, enabled) ->
            put(sessionId, enabled)
        }
    }
    val scroll = JSONObject().apply {
        scrollPositions.forEach { (sessionId, position) ->
            put(
                sessionId,
                JSONObject().apply {
                    put("firstVisibleItemIndex", position.firstVisibleItemIndex)
                    put("firstVisibleItemScrollOffset", position.firstVisibleItemScrollOffset)
                },
            )
        }
    }
    return JSONObject().apply {
        put("thinkingEnabledBySession", thinking)
        put("scrollPositions", scroll)
    }.toString()
}

private fun providerRefreshPayload(secretRef: String): String {
    return JSONObject().apply {
        if (secretRef.isNotBlank()) put("secretRef", secretRef)
    }.toString()
}

private fun maxSequence(first: ULong, second: ULong): ULong {
    return if (first >= second) first else second
}

private fun SettingsSnapshotDto?.settingValue(key: String, fallback: String): String {
    return this
        ?.settings
        ?.firstOrNull { it.key == key }
        ?.value
        ?: fallback
}

private fun SettingsSnapshotDto?.settingBool(key: String, fallback: Boolean): Boolean {
    return settingValue(key, if (fallback) "true" else "false") == "true"
}

private const val SESSION_CACHE_LIMIT = 8
private const val NEW_SESSION_THINKING_KEY = "__new_session__"
