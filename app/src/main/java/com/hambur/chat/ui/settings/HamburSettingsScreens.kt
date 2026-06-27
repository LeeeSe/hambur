package com.hambur.chat.ui.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.ui.platform.LocalContext
import android.widget.Toast
import com.composables.icons.lucide.Trash2
import com.composables.icons.lucide.Cloud
import com.composables.icons.lucide.Settings
import com.composables.icons.lucide.MessageSquare
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.composables.icons.lucide.Brain
import com.composables.icons.lucide.Database
import com.composables.icons.lucide.HardDrive
import com.composables.icons.lucide.KeyRound
import com.composables.icons.lucide.Layers
import com.composables.icons.lucide.ListTodo
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.Terminal
import com.composables.icons.lucide.Wrench
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.UiConfigAudit
import com.hambur.chat.reducer.UiModelGroupSettings
import com.hambur.chat.reducer.UiProviderModelSettings
import com.hambur.chat.reducer.UiProviderSettings
import com.hambur.chat.reducer.UiSkillSummary
import com.hambur.chat.ui.components.ConfirmDangerDialog
import com.hambur.chat.ui.components.HamburSection
import com.hambur.chat.ui.components.HamburTopBar
import com.hambur.chat.ui.components.SecondaryActionButton
import com.hambur.chat.ui.components.SettingsNavigationRow
import com.hambur.chat.ui.components.StatusPill
import com.hambur.chat.ui.components.SummaryLine
import com.hambur.chat.ui.theme.HamburThemeDefaults
import com.hambur.chat.ui.theme.HamburThemeSettingKeys
import java.text.DateFormat
import java.util.Date
import org.json.JSONObject

@Composable
fun HamburSettingsHomeScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onOpenProviders: () -> Unit,
    onOpenModelGroups: () -> Unit,
    onOpenSkills: () -> Unit,
    onOpenMemory: () -> Unit,
    onOpenTools: () -> Unit,
    onOpenStartupTasks: () -> Unit,
    onOpenRootfs: () -> Unit,
    onOpenAppearance: () -> Unit,
    onOpenLogs: () -> Unit,
    onOpenTokenUsage: () -> Unit,
    onOpenPersona: () -> Unit,
    onOpenEnvironmentVariables: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .statusBarsPadding(),
    ) {
        HamburTopBar(
            title = "Settings",
            subtitle = "${state.providers.size} providers / ${state.modelGroups.size} model groups",
            onBack = onBack,
        )
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item {
                HamburSection(title = "Models") {
                    SettingsNavigationRow(
                        icon = Lucide.KeyRound,
                        title = "Providers",
                        summary = "API compatible providers, API keys, model refresh",
                        onClick = onOpenProviders,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Layers,
                        title = "Model groups",
                        summary = "Routing strategy, fallback policy, default groups",
                        onClick = onOpenModelGroups,
                    )
                }
            }
            item {
                HamburSection(title = "Agent Runtime") {
                    SettingsNavigationRow(
                        icon = Lucide.ListTodo,
                        title = "Skills",
                        summary = "Skill enablement and skill JSON payloads",
                        onClick = onOpenSkills,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Database,
                        title = "Memory",
                        summary = "Persistent memory projection settings",
                        onClick = onOpenMemory,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Wrench,
                        title = "Tools",
                        summary = "Tool settings and browser tool settings",
                        onClick = onOpenTools,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Terminal,
                        title = "Startup tasks",
                        summary = "Tasks that require explicit approval",
                        onClick = onOpenStartupTasks,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.HardDrive,
                        title = "Rootfs",
                        summary = "Rootfs settings, warmup, reset",
                        onClick = onOpenRootfs,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Wrench,
                        title = "Environment variables",
                        summary = "Inject variables into sandbox sessions",
                        onClick = onOpenEnvironmentVariables,
                    )
                }
            }
            item {
                HamburSection(title = "App") {
                    SettingsNavigationRow(
                        icon = Lucide.Settings,
                        title = "Appearance",
                        summary = "Theme, font scale, and startup chat behavior",
                        onClick = onOpenAppearance,
                    )
                }
            }
            item {
                HamburSection(
                    title = "Waiting for backend",
                    subtitle = "Kept visible so old UI feature coverage is explicit",
                ) {
                    SettingsNavigationRow(
                        icon = Lucide.Terminal,
                        title = "Logs",
                        summary = "In-app log capture and log viewer are not exposed by the new backend yet",
                        onClick = onOpenLogs,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Database,
                        title = "Token usage",
                        summary = "Usage accounting and provider billing summaries are not exposed yet",
                        onClick = onOpenTokenUsage,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Brain,
                        title = "Persona",
                        summary = "SOUL.md/personality editing is not exposed by the new backend yet",
                        onClick = onOpenPersona,
                    )
                }
            }
            item {
                ConfigAuditPreview(configAudits = state.configAudits)
            }
        }
    }
}


@Composable
fun ProvidersListScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onNewProvider: () -> Unit,
    onOpenProvider: (String) -> Unit,
) {
    SettingsPage(
        title = "Providers",
        subtitle = "OpenAI-compatible provider list",
        onBack = onBack,
    ) {
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onNewProvider) {
                    Text("New provider")
                }
            }
        }
        item {
            HamburSection(title = "OpenAI Compatible") {
                if (state.providers.isEmpty()) {
                    Text("No providers configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.providers.forEach { provider ->
                        ProviderListRow(
                            provider = provider,
                            modelCount = state.providerModels.count { it.providerId == provider.id },
                            onClick = { onOpenProvider(provider.id) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun ProviderNewScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    ProviderEditorScreen(
        title = "New Provider",
        state = state,
        store = store,
        providerId = "",
        onBack = onBack,
    )
}

@Composable
fun ProviderDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    providerId: String,
    onBack: () -> Unit,
    onOpenModel: (String, String) -> Unit,
) {
    val provider = state.providers.firstOrNull { it.id == providerId }
    var pendingDelete by rememberSaveable { mutableStateOf(false) }
    if (pendingDelete) {
        ConfirmDangerDialog(
            title = "Delete provider",
            text = "This deletes the provider configuration after explicit approval.",
            confirmText = "Delete",
            onConfirm = {
                store.deleteProvider(providerId, true)
                pendingDelete = false
                onBack()
            },
            onDismiss = { pendingDelete = false },
        )
    }

    ProviderEditorScreen(
        title = provider?.name ?: "Provider Detail",
        state = state,
        store = store,
        providerId = providerId,
        onBack = onBack,
        extraContent = {
            item {
                HamburSection(title = "Models", subtitle = "Tap a model for detail") {
                    val models = state.providerModels.filter { it.providerId == providerId }
                    if (models.isEmpty()) {
                        Text("No models synced", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    } else {
                        models.forEach { model ->
                            ModelListRow(
                                model = model,
                                onClick = { onOpenModel(providerId, model.modelId) },
                            )
                        }
                    }
                }
            }
            item {
                HamburSection(title = "Danger zone") {
                    TextButton(onClick = { pendingDelete = true }) {
                        Text("Delete provider", color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        },
    )
}

@Composable
fun ModelDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    providerId: String,
    modelId: String,
    onBack: () -> Unit,
) {
    val model = state.providerModels.firstOrNull {
        it.providerId == providerId && it.modelId == modelId
    }
    val provider = state.providers.firstOrNull { it.id == providerId }
    val metadata = remember(model?.metadataJson) { model?.metadataJson.orEmpty().toModelMetadataSummary() }
    var displayName by rememberSaveable(modelId) { mutableStateOf(model?.displayName ?: modelId) }
    var supportsTool by rememberSaveable(modelId) { mutableStateOf(model?.supportsToolCall ?: true) }
    var supportsReasoning by rememberSaveable(modelId) { mutableStateOf(model?.supportsReasoning ?: true) }
    var supportsImage by rememberSaveable(modelId) { mutableStateOf(model?.supportsImageInput ?: false) }
    var contextLimit by rememberSaveable(modelId) { mutableStateOf((model?.contextLimit ?: 32000u).toString()) }
    var outputLimit by rememberSaveable(modelId) { mutableStateOf((model?.outputLimit ?: 4096u).toString()) }

    SettingsPage(title = displayName.ifBlank { modelId }, subtitle = provider?.name ?: providerId, onBack = onBack) {
        item {
            HamburSection(title = "模型") {
                SummaryLine(label = "模型 ID", value = modelId)
                SummaryLine(label = "提供商", value = provider?.name ?: providerId)
                SummaryLine(label = "同步时间", value = model?.syncedAtMs?.toDateTimeText().orEmpty().ifBlank { "暂无" })
                OutlinedTextField(
                    value = displayName,
                    onValueChange = { displayName = it },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text("显示名称") },
                )
            }
        }
        item {
            HamburSection(title = "基本信息") {
                SummaryLine(label = "模型家族", value = metadata.family.ifBlank { "暂无" })
                SummaryLine(label = "知识截止", value = metadata.knowledgeCutoff.ifBlank { "暂无" })
                SummaryLine(label = "发布时间", value = metadata.releaseDate.ifBlank { "暂无" })
                SummaryLine(label = "最近更新", value = metadata.lastUpdated.ifBlank { "暂无" })
                SummaryLine(label = "状态", value = metadata.status.ifBlank { "暂无" })
            }
        }
        item {
            HamburSection(title = "模型能力") {
                SummaryLine(label = "输入模态", value = metadata.inputModalities.joinToString(" / ").ifBlank { if (supportsImage) "text / image" else "text" })
                SummaryLine(label = "输出模态", value = metadata.outputModalities.joinToString(" / ").ifBlank { "text" })
                CapabilityIndicator("支持附件", metadata.supportsAttachments || supportsImage)
                CapabilitySwitch("支持推理", supportsReasoning) { supportsReasoning = it }
                CapabilitySwitch("支持工具调用", supportsTool) { supportsTool = it }
                CapabilitySwitch("图片输入", supportsImage) { supportsImage = it }
                CapabilityIndicator("结构化输出", model?.supportsStructuredOutput ?: false)
                CapabilityIndicator("可调温度", model?.supportsTemperature ?: true)
                CapabilityIndicator("开放权重", metadata.openWeights)
                SummaryLine(label = "推理内容字段", value = model?.reasoningField.orEmpty().ifBlank { metadata.interleavedField.ifBlank { "暂无" } })
                SummaryLine(label = "推理选项", value = metadata.reasoningOptions.joinToString("、").ifBlank { "暂无" })
            }
        }
        item {
            HamburSection(title = "限制与价格") {
                OutlinedTextField(value = contextLimit, onValueChange = { contextLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("上下文长度") })
                OutlinedTextField(value = outputLimit, onValueChange = { outputLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("最大输出") })
                SummaryLine(label = "输入价格", value = metadata.inputCostPerMillion.toCostText())
                SummaryLine(label = "输出价格", value = metadata.outputCostPerMillion.toCostText())
                SummaryLine(label = "缓存读取价格", value = metadata.cacheReadCostPerMillion.toCostText())
            }
        }
        item {
            HamburSection(title = "参考资料") {
                SummaryLine(label = "权重链接", value = metadata.weightLinks.joinToString { it.label.ifBlank { it.url } }.ifBlank { "暂无" })
                SummaryLine(label = "基准测试", value = if (metadata.benchmarks.isEmpty()) "暂无" else "${metadata.benchmarks.size} 项")
            }
        }
        item {
            HamburSection(title = "覆盖设置") {
                Button(
                    enabled = providerId.isNotBlank() && modelId.isNotBlank(),
                    onClick = {
                        store.saveModelOverride(
                            providerId = providerId,
                            modelId = modelId,
                            displayName = displayName,
                            supportsToolCall = supportsTool,
                            supportsReasoning = supportsReasoning,
                            supportsImageInput = supportsImage,
                            contextLimit = contextLimit.toUIntOrNull() ?: 32000u,
                            outputLimit = outputLimit.toUIntOrNull() ?: 4096u,
                        )
                    },
                ) {
                    Text("保存覆盖")
                }
            }
        }
    }
}

@Composable
private fun ProviderEditorScreen(
    title: String,
    state: HamburUiState,
    store: HamburUiStore,
    providerId: String,
    onBack: () -> Unit,
    extraContent: androidx.compose.foundation.lazy.LazyListScope.() -> Unit = {},
) {
    val provider = state.providers.firstOrNull { it.id == providerId }
    var id by rememberSaveable(providerId) { mutableStateOf(provider?.id ?: "prv_" + java.util.UUID.randomUUID().toString().replace("-", "")) }
    var name by rememberSaveable(providerId) { mutableStateOf(provider?.name ?: "") }
    var baseUrl by rememberSaveable(providerId) { mutableStateOf(provider?.baseUrl ?: "https://api.openai.com/v1") }
    var secretRef by rememberSaveable(providerId) { mutableStateOf(provider?.secretLabel ?: "android-secret://providers/$id") }
    var apiKey by rememberSaveable(providerId) { mutableStateOf("") }
    var enabled by rememberSaveable(providerId) { mutableStateOf(provider?.enabled ?: true) }
    var iconName by rememberSaveable(providerId) { mutableStateOf(provider?.iconName ?: "brain") }
    var apiType by rememberSaveable(providerId) { mutableStateOf(provider?.apiType ?: "OpenAiCompatible") }
    var refreshModelId by rememberSaveable(providerId) { mutableStateOf("hambur-openai-compatible-text") }

    val context = LocalContext.current
    var lastCommandSequence by rememberSaveable(providerId) { mutableStateOf(0L) }
    var pendingAction by rememberSaveable(providerId) { mutableStateOf("") }

    LaunchedEffect(state.lastAppliedSequence) {
        val currentSequence = state.lastAppliedSequence.toLong()
        if (lastCommandSequence > 0L && currentSequence > lastCommandSequence) {
            if (pendingAction == "Refresh" && state.latestEventKind == "ModelsUpdated") {
                Toast.makeText(context, state.footer.ifBlank { "Models refreshed successfully" }, Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (pendingAction == "Save" && state.latestEventKind == "SettingsChanged") {
                Toast.makeText(context, "Provider saved successfully", Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (state.runtimeStatus == "Error") {
                Toast.makeText(context, "Operation failed: ${state.footer}", Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            }
        }
    }

    SettingsPage(title = title, subtitle = name, onBack = onBack) {
        item {
            HamburSection(title = "Provider") {
                if (provider != null) {
                    SummaryLine(label = "Provider ID", value = id)
                }
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })
                OutlinedTextField(value = baseUrl, onValueChange = { baseUrl = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Base URL") })
                OutlinedTextField(value = apiKey, onValueChange = { apiKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("API Key (leave blank to keep current)") })
                
                Text("Select Icon", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    listOf("brain", "cloud", "api", "chat").forEach { option ->
                        val selected = iconName == option
                        OutlinedCard(
                            onClick = { iconName = option },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
                            ),
                            modifier = Modifier.size(50.dp)
                        ) {
                            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                Icon(
                                    imageVector = getProviderIcon(option),
                                    contentDescription = null,
                                    tint = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                }

                Row(verticalAlignment = Alignment.CenterVertically) {
                    Switch(checked = enabled, onCheckedChange = { enabled = it })
                    Text("Enabled", modifier = Modifier.padding(start = 8.dp))
                }
                Button(
                    enabled = id.isNotBlank() && baseUrl.isNotBlank() && name.isNotBlank() && secretRef.isNotBlank(),
                    onClick = {
                        lastCommandSequence = state.lastAppliedSequence.toLong()
                        pendingAction = "Save"
                        Toast.makeText(context, "Saving provider...", Toast.LENGTH_SHORT).show()
                        store.saveProvider(id, name, baseUrl, secretRef, apiKey, enabled, iconName, apiType)
                    },
                ) {
                    Text("Save provider")
                }
            }
        }
        item {
            HamburSection(title = "Model sync") {
                OutlinedTextField(value = refreshModelId, onValueChange = { refreshModelId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Model id") })
                SecondaryActionButton(
                    text = "Refresh models",
                    enabled = id.isNotBlank(),
                    onClick = {
                        lastCommandSequence = state.lastAppliedSequence.toLong()
                        pendingAction = "Refresh"
                        Toast.makeText(context, "Syncing models from provider...", Toast.LENGTH_SHORT).show()
                        store.refreshProviderModels(
                            providerId = id,
                            baseUrl = baseUrl,
                            apiKey = apiKey,
                            secretRef = secretRef,
                            modelId = refreshModelId
                        )
                    },
                )
            }
        }
        extraContent()
    }
}

@Composable
fun ModelGroupsListScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
    onNewGroup: () -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    var showPrimaryMenu by remember { mutableStateOf(false) }
    var showSecondaryMenu by remember { mutableStateOf(false) }

    val primaryGroup = state.defaultModelGroups.firstOrNull { it.key == "primary" }?.groupId.orEmpty()
    val secondaryGroup = state.defaultModelGroups.firstOrNull { it.key == "secondary" }?.groupId.orEmpty()

    SettingsPage(title = "Model Groups", onBack = onBack) {
        item {
            Button(onClick = onNewGroup, modifier = Modifier.fillMaxWidth()) {
                Text("New model group")
            }
        }
        item {
            HamburSection(title = "Groups") {
                if (state.modelGroups.isEmpty()) {
                    Text("No model groups configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.modelGroups.forEach { group ->
                        ModelGroupListRow(
                            group = group,
                            memberCount = state.modelGroupMembers.count { it.groupId == group.id },
                            onClick = { onOpenGroup(group.id) },
                        )
                    }
                }
            }
        }
        item {
            HamburSection(title = "Default model groups") {
                val primaryGroupName = state.modelGroups.firstOrNull { it.id == primaryGroup }?.name ?: primaryGroup.ifBlank { "Select group" }
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    onClick = { showPrimaryMenu = true }
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Text("Primary Model Group (Main chat)", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                        Text(primaryGroupName, color = MaterialTheme.colorScheme.primary)
                    }
                }

                val secondaryGroupName = state.modelGroups.firstOrNull { it.id == secondaryGroup }?.name ?: secondaryGroup.ifBlank { "Select group" }
                Surface(
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    onClick = { showSecondaryMenu = true }
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Text("Secondary Model Group (Fallback tasks)", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                        Text(secondaryGroupName, color = MaterialTheme.colorScheme.primary)
                    }
                }
            }
        }
    }

    if (showPrimaryMenu) {
        AlertDialog(
            onDismissRequest = { showPrimaryMenu = false },
            title = { Text("Select Primary Group") },
            text = {
                LazyColumn(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    items(state.modelGroups) { group ->
                        Surface(
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(8.dp),
                            color = if (group.id == primaryGroup) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                            onClick = {
                                store.setDefaultModelGroup("primary", group.id)
                                showPrimaryMenu = false
                            }
                        ) {
                            Text(group.name, modifier = Modifier.padding(16.dp), fontWeight = FontWeight.SemiBold)
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showPrimaryMenu = false }) { Text("Cancel") }
            }
        )
    }

    if (showSecondaryMenu) {
        AlertDialog(
            onDismissRequest = { showSecondaryMenu = false },
            title = { Text("Select Secondary Group") },
            text = {
                LazyColumn(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    items(state.modelGroups) { group ->
                        Surface(
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(8.dp),
                            color = if (group.id == secondaryGroup) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                            onClick = {
                                store.setDefaultModelGroup("secondary", group.id)
                                showSecondaryMenu = false
                            }
                        ) {
                            Text(group.name, modifier = Modifier.padding(16.dp), fontWeight = FontWeight.SemiBold)
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showSecondaryMenu = false }) { Text("Cancel") }
            }
        )
    }
}

@Composable
fun ModelGroupDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    groupId: String,
    onBack: () -> Unit,
) {
    val existing = state.modelGroups.firstOrNull { it.id == groupId }
    var id by rememberSaveable(groupId) { mutableStateOf(existing?.id ?: "grp_" + java.util.UUID.randomUUID().toString().replace("-", "")) }
    var name by rememberSaveable(groupId) { mutableStateOf(existing?.name ?: "") }
    var routingStrategy by rememberSaveable(groupId) { mutableStateOf(existing?.routingStrategy ?: "fallback") }
    var fallbackPolicy by rememberSaveable(groupId) { mutableStateOf(existing?.fallbackPolicy ?: "default") }

    val context = LocalContext.current
    var lastCommandSequence by rememberSaveable(groupId) { mutableStateOf(0L) }
    var pendingAction by rememberSaveable(groupId) { mutableStateOf("") }

    LaunchedEffect(state.lastAppliedSequence) {
        val currentSequence = state.lastAppliedSequence.toLong()
        if (lastCommandSequence > 0L && currentSequence > lastCommandSequence) {
            if (pendingAction == "SaveGroup" && state.latestEventKind == "SettingsChanged") {
                Toast.makeText(context, "Model group saved successfully", Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (state.runtimeStatus == "Error") {
                Toast.makeText(context, "Operation failed: ${state.footer}", Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            }
        }
    }

    var showAddModelDialog by remember { mutableStateOf(false) }
    var pendingDeleteGroup by remember { mutableStateOf(false) }

    if (pendingDeleteGroup) {
        ConfirmDangerDialog(
            title = "Delete model group",
            text = "Are you sure you want to delete this model group? This action cannot be undone.",
            confirmText = "Delete",
            onConfirm = {
                store.deleteModelGroup(id, true)
                pendingDeleteGroup = false
                onBack()
            },
            onDismiss = { pendingDeleteGroup = false }
        )
    }

    SettingsPage(title = if (existing == null) "New Model Group" else existing.name, subtitle = name, onBack = onBack) {
        item {
            HamburSection(title = "Group Settings") {
                if (existing != null) {
                    SummaryLine(label = "Group ID", value = id)
                }
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })

                Text("Routing Strategy", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    listOf("fallback" to "Fallback", "load_balance" to "Load Balance").forEach { (strategyKey, strategyLabel) ->
                        val selected = routingStrategy == strategyKey
                        OutlinedCard(
                            onClick = { routingStrategy = strategyKey },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
                            ),
                            modifier = Modifier.weight(1f).height(50.dp)
                        ) {
                            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                Text(strategyLabel, fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal)
                            }
                        }
                    }
                }

                Text("Fallback Policy", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    listOf("default" to "Default", "always" to "Always").forEach { (policyKey, policyLabel) ->
                        val selected = fallbackPolicy == policyKey
                        OutlinedCard(
                            onClick = { fallbackPolicy = policyKey },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
                            ),
                            modifier = Modifier.weight(1f).height(50.dp)
                        ) {
                            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                Text(policyLabel, fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal)
                            }
                        }
                    }
                }

                Button(
                    onClick = {
                        lastCommandSequence = state.lastAppliedSequence.toLong()
                        pendingAction = "SaveGroup"
                        Toast.makeText(context, "Saving group...", Toast.LENGTH_SHORT).show()
                        store.saveModelGroup(id, name, routingStrategy, fallbackPolicy)
                    },
                    enabled = id.isNotBlank() && name.isNotBlank(),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Save group")
                }
            }
        }
        item {
            HamburSection(title = "Members", subtitle = "Routing priority of models in this group") {
                val members = state.modelGroupMembers.filter { it.groupId == id }
                if (members.isEmpty()) {
                    Text("No members configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    members.sortedBy { it.position }.forEach { member ->
                        Surface(
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            shape = RoundedCornerShape(8.dp),
                            color = MaterialTheme.colorScheme.surfaceVariant,
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                        ) {
                            Row(
                                modifier = Modifier.padding(12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(member.modelDisplayName.ifBlank { member.modelId }, fontWeight = FontWeight.SemiBold)
                                    Text(member.providerName.ifBlank { member.providerId }, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                                IconButton(
                                    onClick = { store.deleteModelGroupMember(id, member.providerId, member.modelId) }
                                ) {
                                    Icon(
                                        imageVector = Lucide.Trash2,
                                        contentDescription = "Delete member",
                                        tint = MaterialTheme.colorScheme.error
                                    )
                                }
                            }
                        }
                    }
                }

                Button(
                    onClick = { showAddModelDialog = true },
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp)
                ) {
                    Text("Add model")
                }
            }
        }
        if (existing != null) {
            item {
                HamburSection(title = "Danger zone") {
                    TextButton(onClick = { pendingDeleteGroup = true }) {
                        Text("Delete model group", color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        }
    }

    if (showAddModelDialog) {
        val availableModels = state.providerModels.filter { model ->
            state.modelGroupMembers.none { it.groupId == id && it.providerId == model.providerId && it.modelId == model.modelId }
        }
        AlertDialog(
            onDismissRequest = { showAddModelDialog = false },
            title = { Text("Add Model") },
            text = {
                if (availableModels.isEmpty()) {
                    Text("No available models found. Please configure a provider and sync models first.")
                } else {
                    LazyColumn(
                        modifier = Modifier.fillMaxWidth().height(300.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        items(availableModels) { model ->
                            Surface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(8.dp),
                                color = MaterialTheme.colorScheme.surface,
                                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                                onClick = {
                                    val members = state.modelGroupMembers.filter { it.groupId == id }
                                    store.addModelGroupMember(
                                        groupId = id,
                                        providerId = model.providerId,
                                        modelId = model.modelId,
                                        position = members.size.toUInt(),
                                        enabled = true
                                    )
                                    showAddModelDialog = false
                                }
                            ) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    Text(model.displayName.ifBlank { model.modelId }, fontWeight = FontWeight.SemiBold)
                                    val providerName = state.providers.firstOrNull { it.id == model.providerId }?.name ?: model.providerId
                                    Text(providerName, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showAddModelDialog = false }) {
                    Text("Cancel")
                }
            }
        )
    }
}


@Composable
fun SkillsListScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
    onOpenSkill: (String) -> Unit,
) {
    SettingsPage(title = "Skills", onBack = onBack) {
        val builtInSkills = state.skills.filter { it.builtIn }
        val userSkills = state.skills.filterNot { it.builtIn }
        item {
            SkillSection(
                title = "Built-in skills",
                emptyText = "No built-in skills discovered",
                skills = builtInSkills,
                store = store,
                onOpenSkill = onOpenSkill,
            )
        }
        item {
            SkillSection(
                title = "User skills",
                emptyText = "No user skills discovered",
                skills = userSkills,
                store = store,
                onOpenSkill = onOpenSkill,
            )
        }
        item {
            HamburSection(title = "Actions") {
                SecondaryActionButton(text = "Refresh", onClick = store::refreshKnowledgeSnapshots)
            }
        }
    }
}

@Composable
fun SkillDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    skillId: String,
    onBack: () -> Unit,
) {
    LaunchedEffect(skillId) {
        store.loadSkillDetail(skillId)
    }
    val detail = state.skillDetails[skillId]
    val selectedSkill = detail?.summary ?: state.skills.firstOrNull { it.path == skillId }
    var showDeleteDialog by rememberSaveable(skillId) { mutableStateOf(false) }
    if (showDeleteDialog) {
        ConfirmDangerDialog(
            title = "Delete skill",
            text = "Delete ${selectedSkill?.name ?: skillId}? This removes the skill directory and all files inside it.",
            confirmText = "Delete",
            onConfirm = {
                showDeleteDialog = false
                store.deleteSkill(skillId)
                onBack()
            },
            onDismiss = { showDeleteDialog = false },
        )
    }
    SettingsPage(title = "Skill Detail", subtitle = skillId, onBack = onBack) {
        item {
            HamburSection(title = "Skill") {
                SummaryLine(label = "Name", value = selectedSkill?.name ?: skillId)
                SummaryLine(label = "Path", value = selectedSkill?.path ?: skillId)
                SummaryLine(label = "Created", value = selectedSkill?.createdAtMs?.toDateTimeText().orEmpty())
                SummaryLine(label = "Modified", value = selectedSkill?.modifiedAtMs?.toDateTimeText().orEmpty())
                CapabilitySwitch("Enabled", selectedSkill?.enabled ?: true) { checked ->
                    store.setSkillEnabled(skillId, checked)
                }
                SecondaryActionButton(text = "Delete skill", onClick = { showDeleteDialog = true })
            }
        }
        item {
            HamburSection(title = "Description") {
                Text(
                    text = selectedSkill?.description.orEmpty().ifBlank { "No description" },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = "Files") {
                val files = selectedSkill?.files.orEmpty()
                if (files.isEmpty()) {
                    Text("No files", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    files.forEach { file ->
                        Text(file, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
        item {
            HamburSection(title = "Content") {
                Text(
                    text = detail?.content ?: "Loading skill...",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
fun MemoryFilesListScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onOpenMemory: (String) -> Unit,
) {
    SettingsPage(title = "Memory", onBack = onBack) {
        item {
            HamburSection(title = "Memory files") {
                if (state.memoryFiles.isEmpty()) {
                    Text("No memory files discovered", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.memoryFiles.forEach { file ->
                        MemoryListRow(file.name, file.preview.ifBlank { "${file.entryCount} entries" }, onOpenMemory)
                    }
                }
            }
        }
    }
}

@Composable
fun MemoryDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    memoryKey: String,
    onBack: () -> Unit,
) {
    LaunchedEffect(memoryKey) {
        store.loadMemoryFileDetail(memoryKey)
    }
    val detail = state.memoryFileDetails[memoryKey]
    SettingsPage(title = "Memory Detail", subtitle = memoryKey, onBack = onBack) {
        item {
            HamburSection(title = "File") {
                SummaryLine(label = "Size", value = "${detail?.sizeBytes ?: 0UL} bytes")
                SummaryLine(label = "Entries", value = "${detail?.entryCount ?: 0u}")
                Text(
                    text = detail?.content ?: "Loading memory file...",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
fun ToolsListScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onOpenTool: (String) -> Unit,
) {
    SettingsPage(title = "Tools", onBack = onBack) {
        item {
            HamburSection(title = "Built-in tools") {
                listOf(
                    "get_current_time" to "Get the current date and time from the user's device.",
                    "skills_list" to "List available skills with minimal metadata.",
                    "skill_view" to "Load a skill's main content or linked file.",
                    "terminal" to "Run a shell command inside the Linux sandbox.",
                    "process" to "Manage background processes started by terminal.",
                    "read_file" to "Read a text file with line numbers and pagination.",
                    "write_file" to "Write content to a sandbox file.",
                    "patch" to "Targeted find-and-replace edits in files.",
                    "search_files" to "Search file contents or find files by name.",
                    "hambur_config" to "Read and update Hambur app configuration.",
                    "web_search" to "Search the web for information.",
                    "web_fetch" to "Extract readable text from web page URLs.",
                    "browser_use" to "Control the shared Android WebView browser.",
                    "session_search" to "Search past chat sessions stored locally.",
                    "memory" to "Save durable information to persistent memory.",
                    "delegate_task" to "Spawn isolated leaf subagents.",
                    "view_image" to "View a local image file from the sandbox.",
                ).forEach { (name, description) ->
                    ToolListRow(name, description, onOpenTool)
                }
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = "tool")
        }
    }
}

@Composable
fun ToolDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    toolName: String,
    onBack: () -> Unit,
) {
    var toolSettings by rememberSaveable(toolName) {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "tool_settings" }?.value ?: """{"enabled":true}""")
    }
    val webFetchBackend = state.settingValue("webFetchBackend", "local")
    val viewImageScaleMode = state.settingValue("viewImageScaleMode", "resize_fit")
    val browserSettings = state.browserToolSettings()
    SettingsPage(title = "Tool Detail", subtitle = toolName, onBack = onBack) {
        item {
            HamburSection(title = "Description") {
                SummaryLine(label = "Name", value = toolName)
                SummaryLine(label = "Backend", value = if (toolName == "browser_use") "AndroidPlatformAdapter browser actions" else "Rust tool registry")
            }
        }
        if (toolName == "web_fetch") {
            item {
                HamburSection(title = "Configuration") {
                    ChoiceRow(
                        title = "Backend",
                        current = webFetchBackend,
                        options = listOf("local" to "Local", "tinyfish" to "TinyFish"),
                        onSelect = { store.saveRawAppSetting("webFetchBackend", it) },
                    )
                }
            }
        }
        if (toolName == "view_image") {
            item {
                HamburSection(title = "Configuration") {
                    ChoiceRow(
                        title = "Image size handling",
                        current = viewImageScaleMode,
                        options = listOf("resize_fit" to "Resize fit", "original" to "Original"),
                        onSelect = { store.saveRawAppSetting("viewImageScaleMode", it) },
                    )
                }
            }
        }
        if (toolName == "browser_use") {
            item {
                HamburSection(title = "Browser settings") {
                    SettingsSwitchRow(
                        title = "Accept cookies",
                        summary = if (browserSettings.acceptCookies) "WebView stores and sends site cookies" else "New browser tabs reject cookies",
                        checked = browserSettings.acceptCookies,
                        onCheckedChange = {
                            store.saveBrowserToolSettings(browserSettings.copy(acceptCookies = it, acceptThirdPartyCookies = it && browserSettings.acceptThirdPartyCookies).toJson())
                        },
                    )
                    SettingsSwitchRow(
                        title = "Third-party cookies",
                        summary = if (browserSettings.acceptThirdPartyCookies) "Useful for login redirects and embedded auth" else "Only first-party cookies are allowed",
                        checked = browserSettings.acceptCookies && browserSettings.acceptThirdPartyCookies,
                        enabled = browserSettings.acceptCookies,
                        onCheckedChange = {
                            store.saveBrowserToolSettings(browserSettings.copy(acceptThirdPartyCookies = it).toJson())
                        },
                    )
                    ChoiceRow(
                        title = "Fetch download limit",
                        current = browserSettings.maxFetchBytes.toString(),
                        options = listOf(
                            "1000000" to "1.00 MB",
                            "2000000" to "2.00 MB",
                            "5000000" to "5.00 MB",
                            "10000000" to "10.00 MB",
                        ),
                        onSelect = {
                            store.saveBrowserToolSettings(browserSettings.copy(maxFetchBytes = it.toInt()).toJson())
                        },
                    )
                    ChoiceRow(
                        title = "Idle auto close",
                        current = browserSettings.autoCloseMinutes.toString(),
                        options = listOf(
                            "0" to "Never",
                            "5" to "5 minutes",
                            "15" to "15 minutes",
                            "30" to "30 minutes",
                            "60" to "60 minutes",
                            "120" to "120 minutes",
                        ),
                        onSelect = {
                            store.saveBrowserToolSettings(browserSettings.copy(autoCloseMinutes = it.toInt()).toJson())
                        },
                    )
                }
            }
        }
        item {
            HamburSection(title = "Tool settings") {
                OutlinedTextField(value = toolSettings, onValueChange = { toolSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("tool_settings") })
                Button(onClick = { store.saveAppSetting("tool_settings", toolSettings, false) }) {
                    Text("Save tool settings")
                }
            }
        }
    }
}

@Composable
fun StartupTasksListScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onNewTask: () -> Unit,
    onOpenTask: (String) -> Unit,
) {
    val tasks = state.appSettings.filter {
        it.key == "startup_tasks" || it.key.startsWith("startup_task:")
    }
    SettingsPage(title = "Startup Tasks", onBack = onBack) {
        item {
            Button(onClick = onNewTask) {
                Text("New startup task")
            }
        }
        item {
            HamburSection(title = "Tasks") {
                if (tasks.isEmpty()) {
                    Text("No startup tasks configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    SecondaryActionButton(text = "Open startup_tasks", onClick = { onOpenTask("startup-default") })
                } else {
                    tasks.forEach { setting ->
                        StartupTaskListRow(setting.key.removePrefix("startup_task:"), setting.value, onOpenTask)
                    }
                }
            }
        }
    }
}

@Composable
fun StartupTaskDetailScreen(
    state: HamburUiState,
    store: HamburUiStore,
    taskId: String,
    onBack: () -> Unit,
) {
    var id by rememberSaveable(taskId) { mutableStateOf(taskId.ifBlank { "startup-default" }) }
    var payload by rememberSaveable(taskId) {
        mutableStateOf(
            state.appSettings.firstOrNull { it.key == "startup_task:$id" }?.value
                ?: """{"id":"$id","enabled":true,"command":"echo ready"}""",
        )
    }
    var pendingDelete by rememberSaveable { mutableStateOf(false) }

    if (pendingDelete) {
        ConfirmDangerDialog(
            title = "Delete startup task",
            text = "Startup task changes require approval.",
            confirmText = "Delete",
            onConfirm = {
                store.deleteStartupTask(id, true)
                pendingDelete = false
                onBack()
            },
            onDismiss = { pendingDelete = false },
        )
    }

    SettingsPage(title = "Startup Task Detail", subtitle = id, onBack = onBack) {
        item {
            HamburSection(title = "Task") {
                OutlinedTextField(value = id, onValueChange = { id = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Task id") })
                OutlinedTextField(value = payload, onValueChange = { payload = it }, modifier = Modifier.fillMaxWidth(), minLines = 8, label = { Text("Task JSON") })
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.saveStartupTask(id, payload, true) }, enabled = id.isNotBlank() && payload.isNotBlank()) {
                        Text("Approve and save")
                    }
                    SecondaryActionButton(text = "Delete", enabled = id.isNotBlank(), onClick = { pendingDelete = true })
                }
            }
        }
    }
}

@Composable
fun AppearanceSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    val themeMode = state.settingValue(
        HamburThemeSettingKeys.ThemeMode,
        HamburThemeDefaults.ThemeMode,
    )
    val fontScale = state.settingValue(
        HamburThemeSettingKeys.FontScale,
        HamburThemeDefaults.FontScale,
    )
    val startupChatMode = state.settingValue("startupChatMode", "last_chat")

    SettingsPage(title = "Appearance", onBack = onBack) {
        item {
            HamburSection(title = "Theme") {
                ChoiceRow(
                    title = "Color theme",
                    current = themeMode,
                    options = listOf(
                        "system" to "System",
                        "light" to "Light",
                        "dark" to "Dark",
                    ),
                    onSelect = { store.saveRawAppSetting("themeMode", it) },
                )
            }
        }
        item {
            HamburSection(title = "Text") {
                ChoiceRow(
                    title = "Font scale",
                    current = fontScale,
                    options = listOf(
                        "small" to "Small",
                        "default" to "Default",
                        "large" to "Large",
                        "extra_large" to "Extra large",
                    ),
                    onSelect = { store.saveRawAppSetting("fontScale", it) },
                )
            }
        }
        item {
            HamburSection(title = "Startup") {
                ChoiceRow(
                    title = "Chat on app start",
                    current = startupChatMode,
                    options = listOf(
                        "last_chat" to "Last chat",
                        "new_chat" to "New chat",
                    ),
                    onSelect = { store.saveRawAppSetting("startupChatMode", it) },
                )
            }
        }
        item {
            HamburSection(title = "Current settings") {
                SummaryLine(label = "themeMode", value = themeMode)
                SummaryLine(label = "fontScale", value = fontScale)
                SummaryLine(label = "startupChatMode", value = startupChatMode)
            }
        }
    }
}

@Composable
fun GenericJsonSettingsScreen(
    title: String,
    summary: String,
    settingKey: String,
    defaultJson: String,
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var value by rememberSaveable(settingKey) {
        mutableStateOf(state.appSettings.firstOrNull { it.key == settingKey }?.value ?: defaultJson)
    }
    var skillId by rememberSaveable { mutableStateOf("system/skill-creator") }
    var skillEnabled by rememberSaveable { mutableStateOf(true) }

    SettingsPage(title = title, subtitle = summary, onBack = onBack) {
        item {
            HamburSection(title = "JSON payload") {
                OutlinedTextField(
                    value = value,
                    onValueChange = { value = it },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 7,
                    maxLines = 14,
                    label = { Text(settingKey) },
                )
                Button(onClick = { store.saveAppSetting(settingKey, value, false) }, enabled = value.isNotBlank()) {
                    Text("Save")
                }
            }
        }
        if (settingKey == "skills") {
            item {
                HamburSection(title = "Skill enabled flag") {
                    OutlinedTextField(value = skillId, onValueChange = { skillId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Skill id") })
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Switch(checked = skillEnabled, onCheckedChange = { skillEnabled = it })
                        Text("Enabled", modifier = Modifier.padding(start = 8.dp))
                    }
                    SecondaryActionButton(
                        text = "Save skill flag",
                        enabled = skillId.isNotBlank(),
                        onClick = { store.setSkillEnabled(skillId, skillEnabled) },
                    )
                }
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = settingKey.substringBefore('_'))
        }
    }
}

@Composable
fun ToolSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var toolSettings by rememberSaveable {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "tool_settings" }?.value ?: """{"enabled":true}""")
    }
    var browserSettings by rememberSaveable {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "browser_tool_settings" }?.value ?: DEFAULT_BROWSER_TOOL_SETTINGS_JSON)
    }
    SettingsPage(title = "Tools", onBack = onBack) {
        item {
            HamburSection(title = "Tool settings") {
                OutlinedTextField(value = toolSettings, onValueChange = { toolSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("tool_settings") })
                Button(onClick = { store.saveAppSetting("tool_settings", toolSettings, false) }) {
                    Text("Save tool settings")
                }
            }
        }
        item {
            HamburSection(title = "Browser tool settings") {
                OutlinedTextField(value = browserSettings, onValueChange = { browserSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("browser_tool_settings") })
                SecondaryActionButton(text = "Save browser settings", onClick = { store.saveBrowserToolSettings(browserSettings) })
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = "tool")
        }
    }
}

@Composable
fun StartupTaskSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var taskId by rememberSaveable { mutableStateOf("startup-default") }
    var payload by rememberSaveable { mutableStateOf("""{"id":"startup-default","enabled":true,"command":"echo ready"}""") }
    var pendingDelete by rememberSaveable { mutableStateOf("") }

    if (pendingDelete.isNotBlank()) {
        ConfirmDangerDialog(
            title = "Delete startup task",
            text = "Startup task changes require approval.",
            confirmText = "Delete",
            onConfirm = {
                store.deleteStartupTask(pendingDelete, true)
                pendingDelete = ""
            },
            onDismiss = { pendingDelete = "" },
        )
    }

    SettingsPage(title = "Startup Tasks", onBack = onBack) {
        item {
            HamburSection(title = "Task editor", subtitle = "Saved through approved backend commands") {
                OutlinedTextField(value = taskId, onValueChange = { taskId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Task id") })
                OutlinedTextField(value = payload, onValueChange = { payload = it }, modifier = Modifier.fillMaxWidth(), minLines = 6, label = { Text("Task JSON") })
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.saveStartupTask(taskId, payload, true) }, enabled = taskId.isNotBlank() && payload.isNotBlank()) {
                        Text("Approve and save")
                    }
                    SecondaryActionButton(text = "Delete", enabled = taskId.isNotBlank(), onClick = { pendingDelete = taskId })
                }
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = "startup")
        }
    }
}

@Composable
fun RootfsSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var key by rememberSaveable { mutableStateOf("proot") }
    var value by rememberSaveable { mutableStateOf("""{"enabled":true}""") }
    var confirmReset by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        store.refreshRootfsStatus()
    }

    if (confirmReset) {
        var preserveRoot by remember { mutableStateOf(true) }
        AlertDialog(
            onDismissRequest = { confirmReset = false },
            title = { Text("重置 RootFS") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("重置会重新下载并解包 Alpine rootfs。这需要一点时间并会清除现有环境。")
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text("是否保留用户数据 (/root)", modifier = Modifier.weight(1f))
                        Switch(
                            checked = preserveRoot,
                            onCheckedChange = { preserveRoot = it }
                        )
                    }
                }
            },
            confirmButton = {
                Button(
                    onClick = {
                        store.resetRootfs(preserveRoot, true)
                        confirmReset = false
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError
                    )
                ) {
                    Text("重置")
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmReset = false }) {
                    Text("取消")
                }
            }
        )
    }

    SettingsPage(title = "Rootfs 管理", onBack = onBack) {
        item {
            val status = state.rootfsStatus
            HamburSection(title = "Rootfs 状态") {
                SummaryLine(
                    label = "RootFS 版本",
                    value = status?.let { if (it.rootfsInstalled) it.version.ifBlank { "未知" } else "未初始化" } ?: "未知"
                )
                SummaryLine(
                    label = "在线 RootFS",
                    value = status?.let { "已配置" } ?: "未检测"
                )
                SummaryLine(
                    label = "存储占用",
                    value = status?.rootfsSizeBytes?.toLong()?.toReadableSize() ?: "未知"
                )
                SummaryLine(
                    label = "root 权限",
                    value = status?.let { if (it.rootAvailable) "是" else "否" } ?: "未知"
                )
                SummaryLine(
                    label = "chroot 状态",
                    value = status?.let { if (it.chrootAvailable) "是" else "否" } ?: "未知"
                )
                SummaryLine(
                    label = "proot 状态",
                    value = status?.let { if (it.prootAvailable) "是" else "否" } ?: "未知"
                )
                SummaryLine(
                    label = "浏览路径",
                    value = status?.rootfsPath ?: "未初始化"
                )
            }
        }
        item {
            HamburSection(title = "后端选择") {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("当前后端", style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                    
                    val currentBackend = state.appSettings.find { it.key == "rootfsBackend" || it.key == "rootfs_setting:rootfsBackend" }?.value?.trim('"', ' ') ?: "proot"
                    val chrootSupported = state.rootfsStatus?.rootAvailable == true
                    
                    Row {
                        OutlinedCard(
                            onClick = {
                                if (chrootSupported) {
                                    store.saveRootfsSetting("rootfsBackend", "chroot", true)
                                    store.refreshRootfsStatus()
                                }
                            },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (currentBackend == "chroot") MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(1.dp, if (currentBackend == "chroot") MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant),
                            modifier = Modifier.padding(end = 8.dp)
                        ) {
                            Text(
                                "chroot",
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                                color = if (chrootSupported) {
                                    if (currentBackend == "chroot") MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface
                                } else {
                                    MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
                                },
                                style = MaterialTheme.typography.bodyMedium
                            )
                        }
                        
                        OutlinedCard(
                            onClick = {
                                store.saveRootfsSetting("rootfsBackend", "proot", true)
                                store.refreshRootfsStatus()
                            },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (currentBackend == "proot") MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(1.dp, if (currentBackend == "proot") MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant)
                        ) {
                            Text(
                                "proot",
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                                color = if (currentBackend == "proot") MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
                                style = MaterialTheme.typography.bodyMedium
                            )
                        }
                    }
                }
            }
        }
        item {
            HamburSection(title = "控制") {
                SummaryLine(label = "当前会话", value = state.selectedSessionId)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.runRootfsWarmup(state.selectedSessionId) }) {
                        Text("Warm up")
                    }
                    SecondaryActionButton(text = "Reset", onClick = { confirmReset = true })
                }
                Text(
                    text = state.footer,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = "高级配置") {
                OutlinedTextField(value = key, onValueChange = { key = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("配置项 Key") })
                OutlinedTextField(value = value, onValueChange = { value = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("配置项 Value (JSON/Text)") })
                Button(onClick = { store.saveRootfsSetting(key, value, true) }, enabled = key.isNotBlank() && value.isNotBlank()) {
                    Text("保存并应用")
                }
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = "rootfs")
        }
    }
}

@Composable
private fun SettingsPage(
    title: String,
    subtitle: String = "",
    onBack: () -> Unit,
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .statusBarsPadding(),
    ) {
        HamburTopBar(title = title, subtitle = subtitle, onBack = onBack)
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            content = content,
        )
    }
}

@Composable
private fun ProviderRow(
    provider: UiProviderSettings,
    onUse: () -> Unit,
    onDelete: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(provider.name, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                StatusPill(text = if (provider.enabled) "Enabled" else "Disabled", active = provider.enabled)
            }
            SummaryLine(label = "ID", value = provider.id)
            SummaryLine(label = "Base URL", value = provider.baseUrl)
            SummaryLine(label = "Secret", value = provider.secretLabel)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryActionButton(text = "Edit", onClick = onUse)
                TextButton(onClick = onDelete) {
                    Text("Delete", color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun ProviderModelsSection(models: List<UiProviderModelSettings>) {
    HamburSection(title = "Provider models", subtitle = "Most recent model metadata known by backend") {
        if (models.isEmpty()) {
            Text("No models synced", color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            models.take(20).forEach { model ->
                SummaryLine(
                    label = model.modelId,
                    value = "ctx ${model.contextLimit}, out ${model.outputLimit}, tools ${model.supportsToolCall}, image ${model.supportsImageInput}",
                )
            }
        }
    }
}

@Composable
private fun CurrentSettingsSection(
    state: HamburUiState,
    prefix: String,
) {
    HamburSection(title = "Current backend settings") {
        val settings = state.appSettings.filter { it.key.startsWith(prefix) || it.key.contains(prefix) }
        if (settings.isEmpty()) {
            Text("No matching settings", color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            settings.forEach { setting ->
                SummaryLine(label = setting.key, value = setting.value)
            }
        }
    }
}

@Composable
private fun ConfigAuditPreview(configAudits: List<UiConfigAudit>) {
    HamburSection(title = "Config audit", subtitle = "Recent backend configuration writes") {
        if (configAudits.isEmpty()) {
            Text("No config changes recorded", color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            configAudits.take(8).forEach { audit ->
                SummaryLine(
                    label = audit.action,
                    value = "${audit.targetKind}:${audit.targetId} ${audit.redactedSummary}",
                )
            }
        }
    }
}

@Composable
fun FeatureUnavailableScreen(
    title: String,
    summary: String,
    onBack: () -> Unit,
) {
    var toggleOne by rememberSaveable(title) { mutableStateOf(false) }
    var textValue by rememberSaveable(title) { mutableStateOf("") }
    var choiceValue by rememberSaveable(title) { mutableStateOf("") }
    SettingsPage(title = title, subtitle = "Waiting for backend", onBack = onBack) {
        item {
            HamburSection(title = "Backend gap") {
                Text(
                    text = summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = "UI surface") {
                when (title) {
                    "Logs" -> {
                        CapabilitySwitch("Record logs", toggleOne) { toggleOne = it }
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryActionButton(text = "All", onClick = { choiceValue = "All" })
                            SecondaryActionButton(text = "Jank", onClick = { choiceValue = "Jank" })
                            SecondaryActionButton(text = "Warnings", onClick = { choiceValue = "Warnings" })
                        }
                        OutlinedTextField(
                            value = textValue,
                            onValueChange = { textValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            minLines = 8,
                            label = { Text("Log viewer") },
                            placeholder = { Text("Runtime logs are not exposed to UI state yet.") },
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryActionButton(text = "Clear logs", enabled = false, onClick = {})
                            SummaryLine(label = "Selected tab", value = choiceValue.ifBlank { "All" })
                        }
                    }
                    "Token Usage" -> {
                        SummaryLine(label = "Prompt tokens", value = "Not exposed")
                        SummaryLine(label = "Completion tokens", value = "Not exposed")
                        SummaryLine(label = "Cost", value = "Not exposed")
                        OutlinedTextField(
                            value = choiceValue,
                            onValueChange = { choiceValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            label = { Text("Provider filter") },
                        )
                    }
                    "Persona" -> {
                        OutlinedTextField(
                            value = textValue,
                            onValueChange = { textValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            minLines = 10,
                            label = { Text("SOUL.md") },
                            placeholder = { Text("Persona text editor surface") },
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryActionButton(text = "Reload", enabled = false, onClick = {})
                            SecondaryActionButton(text = "Save", enabled = false, onClick = {})
                        }
                    }
                    "Environment Variables" -> {
                        OutlinedTextField(
                            value = textValue,
                            onValueChange = { textValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            minLines = 8,
                            label = { Text("Environment") },
                            placeholder = { Text("KEY=value") },
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryActionButton(text = "Validate", enabled = false, onClick = {})
                            SecondaryActionButton(text = "Save", enabled = false, onClick = {})
                        }
                    }
                    else -> Text("No local UI controls defined", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Text(
                    text = "These controls are intentionally local-only until the backend exposes a contract.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ProviderListRow(
    provider: UiProviderSettings,
    modelCount: Int,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(
                    imageVector = getProviderIcon(provider.iconName),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(24.dp)
                )
                Text(provider.name, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                StatusPill(text = if (provider.enabled) "Enabled" else "Disabled", active = provider.enabled)
            }
            SummaryLine(label = "Models", value = "$modelCount available")
            SummaryLine(label = "Base URL", value = provider.baseUrl)
        }
    }
}

@Composable
private fun ModelListRow(
    model: UiProviderModelSettings,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(model.displayName.ifBlank { model.modelId }, fontWeight = FontWeight.SemiBold)
            SummaryLine(label = "Model ID", value = model.modelId)
            SummaryLine(label = "Capabilities", value = "tools ${model.supportsToolCall}, reasoning ${model.supportsReasoning}, image ${model.supportsImageInput}")
        }
    }
}

@Composable
private fun ModelGroupListRow(
    group: UiModelGroupSettings,
    memberCount: Int,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(group.name, fontWeight = FontWeight.SemiBold)
            SummaryLine(label = "ID", value = group.id)
            SummaryLine(label = "Models", value = "$memberCount members")
            SummaryLine(label = "Routing", value = "${group.routingStrategy} / ${group.fallbackPolicy}")
        }
    }
}

@Composable
private fun SkillSection(
    title: String,
    emptyText: String,
    skills: List<UiSkillSummary>,
    store: HamburUiStore,
    onOpenSkill: (String) -> Unit,
) {
    HamburSection(title = title) {
        if (skills.isEmpty()) {
            Text(emptyText, color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            skills.forEach { skill ->
                SkillListRow(
                    skill = skill,
                    onToggle = { checked -> store.setSkillEnabled(skill.path, checked) },
                    onOpenSkill = onOpenSkill,
                )
            }
        }
    }
}

@Composable
private fun SkillListRow(
    skill: UiSkillSummary,
    onToggle: (Boolean) -> Unit,
    onOpenSkill: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = { onOpenSkill(skill.path) },
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(skill.name, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    StatusPill(text = if (skill.builtIn) "Built-in" else "User", active = skill.builtIn)
                }
                Text(
                    skill.description.ifBlank { "No description" },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Switch(checked = skill.enabled, onCheckedChange = onToggle)
            }
        }
    }
}

@Composable
private fun MemoryListRow(
    key: String,
    value: String,
    onOpenMemory: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = { onOpenMemory(key) },
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(key, fontWeight = FontWeight.SemiBold)
            Text(value, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun ToolListRow(
    name: String,
    description: String,
    onOpenTool: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = { onOpenTool(name) },
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(name, fontWeight = FontWeight.SemiBold)
            Text(description, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ChoiceRow(
    title: String,
    current: String,
    options: List<Pair<String, String>>,
    onSelect: (String) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, fontWeight = FontWeight.Medium)
        Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
            options.forEach { (value, label) ->
                val selected = current == value
                if (selected) {
                    Button(onClick = { onSelect(value) }) {
                        Text(label)
                    }
                } else {
                    SecondaryActionButton(text = label, onClick = { onSelect(value) })
                }
            }
        }
    }
}

@Composable
private fun SettingsSwitchRow(
    title: String,
    summary: String,
    checked: Boolean,
    enabled: Boolean = true,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, fontWeight = FontWeight.Medium)
            Text(summary, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
        }
        Switch(
            checked = checked,
            enabled = enabled,
            onCheckedChange = if (enabled) onCheckedChange else null,
        )
    }
}

private data class UiBrowserToolSettings(
    val acceptCookies: Boolean = true,
    val acceptThirdPartyCookies: Boolean = true,
    val maxFetchBytes: Int = 2_000_000,
    val autoCloseMinutes: Int = 15,
) {
    fun normalized(): UiBrowserToolSettings {
        return copy(
            maxFetchBytes = maxFetchBytes.coerceIn(250_000, 10_000_000),
            autoCloseMinutes = autoCloseMinutes.coerceIn(0, 240),
            acceptThirdPartyCookies = acceptCookies && acceptThirdPartyCookies,
        )
    }

    fun toJson(): String {
        val normalized = normalized()
        return """{"acceptCookies":${normalized.acceptCookies},"acceptThirdPartyCookies":${normalized.acceptThirdPartyCookies},"maxFetchBytes":${normalized.maxFetchBytes},"autoCloseMinutes":${normalized.autoCloseMinutes}}"""
    }
}

private const val DEFAULT_BROWSER_TOOL_SETTINGS_JSON =
    """{"acceptCookies":true,"acceptThirdPartyCookies":true,"maxFetchBytes":2000000,"autoCloseMinutes":15}"""

private fun HamburUiState.settingValue(key: String, fallback: String): String {
    return appSettings.firstOrNull { it.key == key }?.value ?: fallback
}

private fun HamburUiState.browserToolSettings(): UiBrowserToolSettings {
    val raw = appSettings.firstOrNull { it.key == "browser_tool_settings" }?.value.orEmpty()
    return UiBrowserToolSettings(
        acceptCookies = raw.jsonBool("acceptCookies", true),
        acceptThirdPartyCookies = raw.jsonBool("acceptThirdPartyCookies", true),
        maxFetchBytes = raw.jsonInt("maxFetchBytes", 2_000_000),
        autoCloseMinutes = raw.jsonInt("autoCloseMinutes", 15),
    ).normalized()
}

private fun String.jsonBool(key: String, fallback: Boolean): Boolean {
    val match = Regex(""""$key"\s*:\s*(true|false)""").find(this) ?: return fallback
    return match.groupValues[1] == "true"
}

private fun String.jsonInt(key: String, fallback: Int): Int {
    val match = Regex(""""$key"\s*:\s*(\d+)""").find(this) ?: return fallback
    return match.groupValues[1].toIntOrNull() ?: fallback
}

@Composable
private fun StartupTaskListRow(
    taskId: String,
    value: String,
    onOpenTask: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = { onOpenTask(taskId) },
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(taskId.ifBlank { "startup_tasks" }, fontWeight = FontWeight.SemiBold)
            Text(value, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun CapabilitySwitch(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Switch(checked = checked, onCheckedChange = onCheckedChange)
        Text(label, modifier = Modifier.padding(start = 8.dp))
    }
}

@Composable
private fun CapabilityIndicator(
    label: String,
    checked: Boolean,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Checkbox(checked = checked, onCheckedChange = null)
        Text(label, modifier = Modifier.padding(start = 8.dp))
    }
}

private data class ModelMetadataSummary(
    val family: String = "",
    val knowledgeCutoff: String = "",
    val releaseDate: String = "",
    val lastUpdated: String = "",
    val status: String = "",
    val supportsAttachments: Boolean = false,
    val openWeights: Boolean = false,
    val inputModalities: List<String> = emptyList(),
    val outputModalities: List<String> = emptyList(),
    val inputCostPerMillion: Double? = null,
    val outputCostPerMillion: Double? = null,
    val cacheReadCostPerMillion: Double? = null,
    val reasoningOptions: List<String> = emptyList(),
    val interleavedField: String = "",
    val weightLinks: List<ModelLinkSummary> = emptyList(),
    val benchmarks: List<String> = emptyList(),
)

private data class ModelLinkSummary(
    val label: String = "",
    val url: String = "",
)

private fun String.toModelMetadataSummary(): ModelMetadataSummary {
    if (isBlank()) return ModelMetadataSummary()
    return runCatching {
        val root = JSONObject(this)
        val modalities = root.optJSONObject("modalities")
        val cost = root.optJSONObject("cost")
        val interleaved = root.optJSONObject("interleaved")
        ModelMetadataSummary(
            family = root.optString("family"),
            knowledgeCutoff = root.optString("knowledge"),
            releaseDate = root.optString("release_date"),
            lastUpdated = root.optString("last_updated"),
            status = root.optString("status"),
            supportsAttachments = root.optBoolean("attachment", false),
            openWeights = root.optBoolean("open_weights", false),
            inputModalities = modalities?.optStringArray("input").orEmpty(),
            outputModalities = modalities?.optStringArray("output").orEmpty(),
            inputCostPerMillion = cost?.optNullableDouble("input"),
            outputCostPerMillion = cost?.optNullableDouble("output"),
            cacheReadCostPerMillion = cost?.optNullableDouble("cache_read"),
            reasoningOptions = root.optJSONArray("reasoning_options").toStringList(),
            interleavedField = interleaved?.optString("field").orEmpty().ifBlank {
                root.optString("interleaved")
            },
            weightLinks = root.optJSONArray("weights").toModelLinks(),
            benchmarks = root.optJSONArray("benchmarks").toBenchmarkLabels(),
        )
    }.getOrDefault(ModelMetadataSummary())
}

private fun org.json.JSONObject.optStringArray(key: String): List<String> {
    return optJSONArray(key).toStringList()
}

private fun org.json.JSONObject.optNullableDouble(key: String): Double? {
    return if (has(key) && !isNull(key)) optDouble(key) else null
}

private fun org.json.JSONArray?.toStringList(): List<String> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { index ->
        val value = opt(index)
        when (value) {
            is String -> value
            is JSONObject -> value.optString("type").ifBlank { value.optString("name") }
            else -> null
        }
    }.filter { it.isNotBlank() }
}

private fun org.json.JSONArray?.toModelLinks(): List<ModelLinkSummary> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { index ->
        val value = optJSONObject(index) ?: return@mapNotNull null
        ModelLinkSummary(
            label = value.optString("label"),
            url = value.optString("url"),
        )
    }.filter { it.label.isNotBlank() || it.url.isNotBlank() }
}

private fun org.json.JSONArray?.toBenchmarkLabels(): List<String> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { index ->
        val value = optJSONObject(index) ?: return@mapNotNull null
        value.optString("name").ifBlank { value.optString("metric") }
    }.filter { it.isNotBlank() }
}

private fun Double?.toCostText(): String {
    return this?.let { "$$it / 1M tokens" } ?: "暂无"
}

private fun Long.toReadableSize(): String {
    if (this <= 0) return "0 B"
    val units = arrayOf("B", "KB", "MB", "GB", "TB")
    val digitGroups = (Math.log10(this.toDouble()) / Math.log10(1024.0)).toInt()
    return String.format("%.2f %s", this / Math.pow(1024.0, digitGroups.toDouble()), units[digitGroups])
}

private fun ULong.toDateTimeText(): String {
    if (this == 0uL) return ""
    return DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
        .format(Date(this.toLong()))
}

private fun getProviderIcon(name: String): androidx.compose.ui.graphics.vector.ImageVector {
    return when (name) {
        "brain" -> Lucide.Brain
        "cloud" -> Lucide.Cloud
        "api" -> Lucide.Settings
        "chat" -> Lucide.MessageSquare
        else -> Lucide.Brain
    }
}
