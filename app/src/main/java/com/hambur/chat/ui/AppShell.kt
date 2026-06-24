package com.hambur.chat.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.UiAppSetting
import com.hambur.chat.reducer.UiConfigAudit
import com.hambur.chat.reducer.UiDefaultModelGroupSettings
import com.hambur.chat.reducer.UiModelGroupMemberSettings
import com.hambur.chat.reducer.UiModelGroupSettings
import com.hambur.chat.reducer.UiPendingAttachment
import com.hambur.chat.reducer.UiProviderModelSettings
import com.hambur.chat.reducer.UiProviderSettings
import com.hambur.chat.reducer.UiSessionSummary
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import kotlinx.coroutines.flow.distinctUntilChanged

@Composable
fun AppShell(appFilesDir: String) {
    val store = remember(appFilesDir) { HamburUiStore(appFilesDir) }
    val state by store.state.collectAsState()
    var draftTitle by rememberSaveable { mutableStateOf("") }
    var draftMessage by rememberSaveable { mutableStateOf("") }
    var showSettings by rememberSaveable { mutableStateOf(false) }

    DisposableEffect(store) {
        onDispose { store.shutdown() }
    }

    MaterialTheme(
        colorScheme = lightColorScheme(
            primary = Color(0xFF0F766E),
            secondary = Color(0xFF475569),
            tertiary = Color(0xFFB45309),
            surface = Color(0xFFFAFAF7),
            background = Color(0xFFFAFAF7),
        ),
    ) {
        Scaffold { padding ->
            Surface(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                color = MaterialTheme.colorScheme.background,
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(horizontal = 24.dp, vertical = 28.dp),
                    verticalArrangement = Arrangement.spacedBy(22.dp),
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                        Text(
                            text = "Hambur",
                            style = MaterialTheme.typography.headlineMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        RuntimeStatusRow(label = "Backend", value = state.runtimeStatus)
                        RuntimeStatusRow(label = "Event", value = state.latestEventKind)
                    }

                    SessionComposer(
                        title = draftTitle,
                        onTitleChange = { draftTitle = it },
                        onCreate = {
                            store.createSession(draftTitle)
                            draftTitle = ""
                        },
                    )

                    SessionList(
                        sessions = state.sessions,
                        selectedSessionId = state.selectedSessionId,
                        onOpen = store::openSession,
                        onDelete = store::deleteSession,
                        modifier = Modifier.weight(1f),
                    )

                    ChatComposer(
                        message = draftMessage,
                        enabled = state.selectedSessionId.isNotBlank(),
                        streaming = state.activeTurnIds.containsKey(state.selectedSessionId),
                        pendingAttachments = state.pendingAttachments,
                        onMessageChange = { draftMessage = it },
                        onSend = {
                            store.sendMessage(state.selectedSessionId, draftMessage)
                            draftMessage = ""
                        },
                        onStop = {
                            store.cancelActiveTurn(state.selectedSessionId)
                        },
                        onAddImage = {
                            store.importAttachmentMetadata(
                                sessionId = state.selectedSessionId,
                                displayName = "image.png",
                                mimeType = "image/png",
                            )
                        },
                        onRemoveAttachment = { attachmentId ->
                            store.removePendingAttachment(state.selectedSessionId, attachmentId)
                        },
                    )

                    SettingsPanel(
                        expanded = showSettings,
                        onExpandedChange = { showSettings = it },
                        providers = state.providers,
                        providerModels = state.providerModels,
                        modelGroups = state.modelGroups,
                        modelGroupMembers = state.modelGroupMembers,
                        defaultModelGroups = state.defaultModelGroups,
                        appSettings = state.appSettings,
                        configAudits = state.configAudits,
                        onSaveProvider = store::saveProvider,
                        onDeleteProvider = store::deleteProvider,
                        onRefreshModels = store::refreshProviderModels,
                        onSaveModelOverride = store::saveModelOverride,
                        onSaveModelGroup = store::saveModelGroup,
                        onAddModelGroupMember = store::addModelGroupMember,
                        onSetDefaultModelGroup = store::setDefaultModelGroup,
                        onSaveAppSetting = store::saveAppSetting,
                    )

                    TimelineSnapshot(
                        selectedSessionId = state.selectedSessionId,
                        items = state.timelineItems,
                        markdownBlocks = state.markdownBlocks,
                        pendingMarkdownBlock = state.pendingMarkdownBlock,
                        activePreviewPath = state.activePreviewPath,
                        onRenderMarkdown = {
                            store.streamMarkdownPreview(state.selectedSessionId)
                        },
                        onOpenDestination = store::openMarkdownDestination,
                    )

                    Text(
                        text = state.footer,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

@Composable
private fun ChatComposer(
    message: String,
    enabled: Boolean,
    streaming: Boolean,
    pendingAttachments: List<UiPendingAttachment>,
    onMessageChange: (String) -> Unit,
    onSend: () -> Unit,
    onStop: () -> Unit,
    onAddImage: () -> Unit,
    onRemoveAttachment: (String) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (pendingAttachments.isNotEmpty()) {
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 72.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(
                    items = pendingAttachments,
                    key = { it.id },
                ) { attachment ->
                    PendingAttachmentRow(
                        attachment = attachment,
                        onRemove = { onRemoveAttachment(attachment.id) },
                    )
                }
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = message,
                onValueChange = onMessageChange,
                modifier = Modifier.weight(1f),
                minLines = 1,
                maxLines = 3,
                label = { Text("Message") },
                enabled = enabled && !streaming,
            )
            TextButton(
                onClick = onAddImage,
                enabled = enabled && !streaming,
            ) {
                Text("Image")
            }
            if (streaming) {
                Button(
                    onClick = onStop,
                    enabled = enabled,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.tertiary,
                    ),
                ) {
                    Text("Stop")
                }
            } else {
                Button(
                    onClick = onSend,
                    enabled = enabled && (message.isNotBlank() || pendingAttachments.isNotEmpty()),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary,
                    ),
                ) {
                    Text("Send")
                }
            }
        }
    }
}

@Composable
private fun PendingAttachmentRow(
    attachment: UiPendingAttachment,
    onRemove: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = attachment.kind,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.secondary,
                modifier = Modifier.width(56.dp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = attachment.displayName.ifBlank { attachment.mimeType },
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            TextButton(onClick = onRemove) {
                Text("Remove")
            }
        }
    }
}

@Composable
private fun SettingsPanel(
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    providers: List<UiProviderSettings>,
    providerModels: List<UiProviderModelSettings>,
    modelGroups: List<UiModelGroupSettings>,
    modelGroupMembers: List<UiModelGroupMemberSettings>,
    defaultModelGroups: List<UiDefaultModelGroupSettings>,
    appSettings: List<UiAppSetting>,
    configAudits: List<UiConfigAudit>,
    onSaveProvider: (String, String, String, String, Boolean) -> Unit,
    onDeleteProvider: (String, Boolean) -> Unit,
    onRefreshModels: (String, String) -> Unit,
    onSaveModelOverride: (String, String, String, Boolean, Boolean, Boolean, UInt, UInt) -> Unit,
    onSaveModelGroup: (String, String, String, String) -> Unit,
    onAddModelGroupMember: (String, String, String, UInt, Boolean) -> Unit,
    onSetDefaultModelGroup: (String, String) -> Unit,
    onSaveAppSetting: (String, String, Boolean) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Settings",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            TextButton(onClick = { onExpandedChange(!expanded) }) {
                Text(if (expanded) "Hide" else "Open")
            }
        }
        if (!expanded) {
            Text(
                text = "${providers.size} providers, ${modelGroups.size} groups",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            return
        }

        LazyColumn(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = 340.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item {
                ProviderSettingsSection(
                    providers = providers,
                    providerModels = providerModels,
                    onSaveProvider = onSaveProvider,
                    onDeleteProvider = onDeleteProvider,
                    onRefreshModels = onRefreshModels,
                    onSaveModelOverride = onSaveModelOverride,
                )
            }
            item {
                ModelGroupSettingsSection(
                    providers = providers,
                    providerModels = providerModels,
                    modelGroups = modelGroups,
                    modelGroupMembers = modelGroupMembers,
                    defaultModelGroups = defaultModelGroups,
                    onSaveModelGroup = onSaveModelGroup,
                    onAddModelGroupMember = onAddModelGroupMember,
                    onSetDefaultModelGroup = onSetDefaultModelGroup,
                )
            }
            item {
                AppSettingsSection(
                    appSettings = appSettings,
                    onSaveAppSetting = onSaveAppSetting,
                )
            }
            item {
                ConfigAuditSection(configAudits = configAudits)
            }
        }
    }
}

@Composable
private fun ProviderSettingsSection(
    providers: List<UiProviderSettings>,
    providerModels: List<UiProviderModelSettings>,
    onSaveProvider: (String, String, String, String, Boolean) -> Unit,
    onDeleteProvider: (String, Boolean) -> Unit,
    onRefreshModels: (String, String) -> Unit,
    onSaveModelOverride: (String, String, String, Boolean, Boolean, Boolean, UInt, UInt) -> Unit,
) {
    var providerId by rememberSaveable { mutableStateOf("") }
    var name by rememberSaveable { mutableStateOf("OpenAI Compatible") }
    var baseUrl by rememberSaveable { mutableStateOf("https://api.openai.com/v1") }
    var secretRef by rememberSaveable { mutableStateOf("android-secret://providers/default-openai-compatible") }
    var enabled by rememberSaveable { mutableStateOf(true) }
    var modelId by rememberSaveable { mutableStateOf("hambur-openai-compatible-text") }
    var deleteApproved by rememberSaveable { mutableStateOf(false) }

    SettingsSurface {
        Text(
            text = "Providers",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        providers.take(4).forEach { provider ->
            SettingsSummaryRow(
                label = provider.name,
                value = "${provider.baseUrl} / ${provider.secretLabel}",
            )
        }
        OutlinedTextField(
            value = providerId,
            onValueChange = { providerId = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Provider id") },
        )
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Name") },
        )
        OutlinedTextField(
            value = baseUrl,
            onValueChange = { baseUrl = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Base URL") },
        )
        OutlinedTextField(
            value = secretRef,
            onValueChange = { secretRef = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Secret ref") },
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(checked = enabled, onCheckedChange = { enabled = it })
            Text("Enabled", style = MaterialTheme.typography.bodySmall)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = { onSaveProvider(providerId, name, baseUrl, secretRef, enabled) },
                enabled = baseUrl.isNotBlank() && secretRef.isNotBlank(),
            ) {
                Text("Save")
            }
            TextButton(
                onClick = { onDeleteProvider(providerId, deleteApproved) },
                enabled = providerId.isNotBlank() && deleteApproved,
            ) {
                Text("Delete")
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(checked = deleteApproved, onCheckedChange = { deleteApproved = it })
                Text("Approve", style = MaterialTheme.typography.bodySmall)
            }
        }
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = modelId,
                onValueChange = { modelId = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Model id") },
            )
            TextButton(
                onClick = { onRefreshModels(providerId, modelId) },
                enabled = providerId.isNotBlank(),
            ) {
                Text("Refresh")
            }
        }
        providerModels.take(5).forEach { model ->
            SettingsSummaryRow(
                label = model.modelId,
                value = "ctx ${model.contextLimit}, image ${model.supportsImageInput}",
            )
        }
        TextButton(
            onClick = {
                onSaveModelOverride(
                    providerId,
                    modelId,
                    modelId,
                    true,
                    true,
                    modelId.contains("vision", ignoreCase = true),
                    32000u,
                    4096u,
                )
            },
            enabled = providerId.isNotBlank() && modelId.isNotBlank(),
        ) {
            Text("Save override")
        }
    }
}

@Composable
private fun ModelGroupSettingsSection(
    providers: List<UiProviderSettings>,
    providerModels: List<UiProviderModelSettings>,
    modelGroups: List<UiModelGroupSettings>,
    modelGroupMembers: List<UiModelGroupMemberSettings>,
    defaultModelGroups: List<UiDefaultModelGroupSettings>,
    onSaveModelGroup: (String, String, String, String) -> Unit,
    onAddModelGroupMember: (String, String, String, UInt, Boolean) -> Unit,
    onSetDefaultModelGroup: (String, String) -> Unit,
) {
    var groupId by rememberSaveable { mutableStateOf("grp_primary_chat") }
    var groupName by rememberSaveable { mutableStateOf("Primary Chat") }
    var defaultKey by rememberSaveable { mutableStateOf("primary") }
    var providerId by rememberSaveable { mutableStateOf("") }
    var modelId by rememberSaveable { mutableStateOf("") }

    SettingsSurface {
        Text(
            text = "Model groups",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        modelGroups.take(4).forEach { group ->
            SettingsSummaryRow(label = group.id, value = "${group.name} / ${group.routingStrategy}")
        }
        defaultModelGroups.take(4).forEach { default ->
            SettingsSummaryRow(label = "default ${default.key}", value = default.groupId)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = groupId,
                onValueChange = { groupId = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Group id") },
            )
            OutlinedTextField(
                value = groupName,
                onValueChange = { groupName = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Name") },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = { onSaveModelGroup(groupId, groupName, "fallback", "default") },
                enabled = groupId.isNotBlank(),
            ) {
                Text("Save group")
            }
            TextButton(
                onClick = { onSetDefaultModelGroup(defaultKey, groupId) },
                enabled = groupId.isNotBlank() && defaultKey.isNotBlank(),
            ) {
                Text("Set default")
            }
        }
        OutlinedTextField(
            value = defaultKey,
            onValueChange = { defaultKey = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Default key") },
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = providerId,
                onValueChange = { providerId = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Provider id") },
            )
            OutlinedTextField(
                value = modelId,
                onValueChange = { modelId = it },
                modifier = Modifier.weight(1f),
                singleLine = true,
                label = { Text("Model id") },
            )
        }
        TextButton(
            onClick = { onAddModelGroupMember(groupId, providerId, modelId, 0u, true) },
            enabled = groupId.isNotBlank() && providerId.isNotBlank() && modelId.isNotBlank(),
        ) {
            Text("Add member")
        }
        modelGroupMembers.take(5).forEach { member ->
            SettingsSummaryRow(
                label = member.groupId,
                value = "${member.providerName.ifBlank { member.providerId }} / ${member.modelDisplayName}",
            )
        }
        if (providers.isNotEmpty() && providerModels.isNotEmpty()) {
            Text(
                text = "Known: ${providers.first().id} / ${providerModels.first().modelId}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun AppSettingsSection(
    appSettings: List<UiAppSetting>,
    onSaveAppSetting: (String, String, Boolean) -> Unit,
) {
    var key by rememberSaveable { mutableStateOf("tool_settings") }
    var value by rememberSaveable { mutableStateOf("""{"enabled":true}""") }
    var approved by rememberSaveable { mutableStateOf(false) }
    SettingsSurface {
        Text(
            text = "Tools, skills, memory, startup, rootfs",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        appSettings.take(5).forEach { setting ->
            SettingsSummaryRow(label = setting.key, value = setting.value)
        }
        OutlinedTextField(
            value = key,
            onValueChange = { key = it },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Setting key") },
        )
        OutlinedTextField(
            value = value,
            onValueChange = { value = it },
            modifier = Modifier.fillMaxWidth(),
            minLines = 2,
            maxLines = 4,
            label = { Text("JSON") },
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(
                onClick = { onSaveAppSetting(key, value, approved) },
                enabled = key.isNotBlank() && value.isNotBlank(),
            ) {
                Text("Save setting")
            }
            Checkbox(checked = approved, onCheckedChange = { approved = it })
            Text("Approve dangerous", style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ConfigAuditSection(configAudits: List<UiConfigAudit>) {
    SettingsSurface {
        Text(
            text = "Config audit",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
        )
        if (configAudits.isEmpty()) {
            Text(
                text = "No config changes",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            configAudits.take(6).forEach { audit ->
                SettingsSummaryRow(
                    label = audit.action,
                    value = "${audit.targetKind}:${audit.targetId} ${audit.redactedSummary}",
                )
            }
        }
    }
}

@Composable
private fun SettingsSurface(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            content = content,
        )
    }
}

@Composable
private fun SettingsSummaryRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.secondary,
            modifier = Modifier.width(116.dp),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun RuntimeStatusRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.secondary,
        )
        Spacer(modifier = Modifier.width(16.dp))
        Text(
            text = value,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.Medium,
        )
    }
    Spacer(modifier = Modifier.height(2.dp))
}

@Composable
private fun SessionComposer(
    title: String,
    onTitleChange: (String) -> Unit,
    onCreate: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = title,
            onValueChange = onTitleChange,
            modifier = Modifier.weight(1f),
            singleLine = true,
            label = { Text("Session title") },
        )
        Button(
            onClick = onCreate,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.primary,
            ),
        ) {
            Text("Create")
        }
    }
}

@Composable
private fun SessionList(
    sessions: List<UiSessionSummary>,
    selectedSessionId: String,
    onOpen: (String) -> Unit,
    onDelete: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = "Sessions",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )
        if (sessions.isEmpty()) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(8.dp),
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                color = MaterialTheme.colorScheme.surface,
            ) {
                Text(
                    text = "No sessions",
                    modifier = Modifier.padding(16.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(
                    items = sessions,
                    key = { it.id },
                ) { session ->
                    SessionRow(
                        session = session,
                        selected = session.id == selectedSessionId,
                        onOpen = { onOpen(session.id) },
                        onDelete = { onDelete(session.id) },
                    )
                }
            }
        }
    }
}

@Composable
private fun SessionRow(
    session: UiSessionSummary,
    selected: Boolean,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onOpen),
        shape = RoundedCornerShape(8.dp),
        color = if (selected) {
            MaterialTheme.colorScheme.primaryContainer
        } else {
            MaterialTheme.colorScheme.surface
        },
        border = BorderStroke(
            1.dp,
            if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant,
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    text = session.title,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                val preview = session.latestPreview.ifBlank {
                    "${session.messageCount} messages"
                }
                Text(
                    text = preview,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (!selected) {
                TextButton(onClick = onOpen) {
                    Text("Open")
                }
            }
            TextButton(onClick = onDelete) {
                Text("Delete")
            }
        }
    }
}

@Composable
private fun TimelineSnapshot(
    selectedSessionId: String,
    items: List<UiTimelineItem>,
    markdownBlocks: List<MarkdownBlockNodeDto>,
    pendingMarkdownBlock: MarkdownBlockNodeDto?,
    activePreviewPath: String,
    onRenderMarkdown: () -> Unit,
    onOpenDestination: (String) -> Unit,
) {
    val hasMarkdown = markdownBlocks.isNotEmpty() ||
        pendingMarkdownBlock != null ||
        activePreviewPath.isNotBlank()
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 160.dp, max = 320.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Timeline",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            TextButton(
                enabled = selectedSessionId.isNotBlank(),
                onClick = onRenderMarkdown,
            ) {
                Text("Render")
            }
        }
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(8.dp),
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
            color = MaterialTheme.colorScheme.surface,
        ) {
            if (selectedSessionId.isBlank()) {
                Text(
                    text = "No session selected",
                    modifier = Modifier.padding(16.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (items.isEmpty()) {
                if (!hasMarkdown) {
                    Text(
                        text = "No timeline items",
                        modifier = Modifier.padding(16.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    MarkdownTimeline(
                        selectedSessionId = selectedSessionId,
                        items = items,
                        markdownBlocks = markdownBlocks,
                        pendingMarkdownBlock = pendingMarkdownBlock,
                        activePreviewPath = activePreviewPath,
                        onOpenDestination = onOpenDestination,
                    )
                }
            } else {
                MarkdownTimeline(
                    selectedSessionId = selectedSessionId,
                    items = items,
                    markdownBlocks = markdownBlocks,
                    pendingMarkdownBlock = pendingMarkdownBlock,
                    activePreviewPath = activePreviewPath,
                    onOpenDestination = onOpenDestination,
                )
            }
        }
    }
}

@Composable
private fun MarkdownTimeline(
    selectedSessionId: String,
    items: List<UiTimelineItem>,
    markdownBlocks: List<MarkdownBlockNodeDto>,
    pendingMarkdownBlock: MarkdownBlockNodeDto?,
    activePreviewPath: String,
    onOpenDestination: (String) -> Unit,
) {
    val listState = rememberLazyListState()
    val markdownStyle = rememberMarkdownStyle()
    val markdownRenderCache = rememberMarkdownRenderCache()
    var followTail by remember(selectedSessionId) { mutableStateOf(true) }
    val renderedItemCount = items.size +
        markdownBlocks.size +
        (if (pendingMarkdownBlock != null) 1 else 0) +
        (if (activePreviewPath.isNotBlank()) 1 else 0)
    val bottomAnchorIndex = renderedItemCount

    LaunchedEffect(listState) {
        snapshotFlow {
            val layout = listState.layoutInfo
            val total = layout.totalItemsCount
            val lastVisible = layout.visibleItemsInfo.lastOrNull()?.index ?: 0
            total == 0 || lastVisible >= total - 2
        }
            .distinctUntilChanged()
            .collect { nearBottom ->
                followTail = nearBottom
            }
    }

    LaunchedEffect(
        renderedItemCount,
        pendingMarkdownBlock?.stableKey,
        pendingMarkdownBlock?.raw,
        pendingMarkdownBlock?.text,
        followTail,
    ) {
        if (followTail) {
            withFrameNanos { }
            listState.scrollToItem(bottomAnchorIndex)
        }
    }

    LazyColumn(
        state = listState,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(max = 260.dp)
            .padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        items(
            items = items,
            key = { "snapshot-${it.stableKey}" },
            contentType = { it.contentType },
        ) { item ->
            TimelineItemRow(item = item)
        }

        items(
            items = markdownBlocks,
            key = { "markdown-${it.stableKey}" },
            contentType = { it.nodeKind },
        ) { block ->
            MarkdownBlock(
                node = block,
                modifier = Modifier.padding(horizontal = 14.dp),
                style = markdownStyle,
                renderCache = markdownRenderCache,
                onOpenDestination = onOpenDestination,
            )
        }

        if (pendingMarkdownBlock != null) {
            item(
                key = "markdown-pending-${pendingMarkdownBlock.stableKey}",
                contentType = pendingMarkdownBlock.nodeKind,
            ) {
                MarkdownBlock(
                    node = pendingMarkdownBlock,
                    modifier = Modifier.padding(horizontal = 14.dp),
                    style = markdownStyle,
                    renderCache = markdownRenderCache,
                    onOpenDestination = onOpenDestination,
                )
            }
        }

        if (activePreviewPath.isNotBlank()) {
            item(
                key = "markdown-preview-route",
                contentType = "preview-route",
            ) {
                Text(
                    text = activePreviewPath,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 2.dp),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        item(
            key = "timeline-bottom-anchor",
            contentType = "bottom-anchor",
        ) {
            Spacer(modifier = Modifier.height(1.dp))
        }
    }
}

@Composable
private fun TimelineItemRow(item: UiTimelineItem) {
    if (item.contentType == "trace" || item.kind.contains("Trace")) {
        ToolTraceRow(item = item)
        return
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = item.kind,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.secondary,
            modifier = Modifier.width(88.dp),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = item.smallSummary.ifBlank { item.contentType },
            style = MaterialTheme.typography.bodySmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun ToolTraceRow(item: UiTimelineItem) {
    val statusColor = when (item.traceStatus) {
        "completed" -> MaterialTheme.colorScheme.primary
        "failed" -> MaterialTheme.colorScheme.error
        "running" -> MaterialTheme.colorScheme.tertiary
        else -> MaterialTheme.colorScheme.secondary
    }
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(5.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = item.toolName.ifBlank { "tool" },
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.secondary,
                    modifier = Modifier.width(88.dp),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = item.traceTitle.ifBlank { item.smallSummary },
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = item.traceStatus.ifBlank { item.kind },
                    style = MaterialTheme.typography.labelSmall,
                    color = statusColor,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (item.traceContent.isNotBlank()) {
                Text(
                    text = item.traceContent,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}
