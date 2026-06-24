package com.hambur.chat.ui.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import com.hambur.chat.ui.components.ConfirmDangerDialog
import com.hambur.chat.ui.components.HamburSection
import com.hambur.chat.ui.components.HamburTopBar
import com.hambur.chat.ui.components.SecondaryActionButton
import com.hambur.chat.ui.components.SettingsNavigationRow
import com.hambur.chat.ui.components.StatusPill
import com.hambur.chat.ui.components.SummaryLine

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
                HamburSection(
                    title = "Waiting for backend",
                    subtitle = "Kept visible so old UI feature coverage is explicit",
                ) {
                    SettingsNavigationRow(
                        icon = Lucide.Wrench,
                        title = "Appearance",
                        summary = "Theme, font scale, and predictive back settings are not in the new backend yet",
                        onClick = onOpenAppearance,
                    )
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
fun ProviderSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var providerId by rememberSaveable { mutableStateOf(state.providers.firstOrNull()?.id.orEmpty()) }
    var name by rememberSaveable { mutableStateOf(state.providers.firstOrNull()?.name ?: "OpenAI Compatible") }
    var baseUrl by rememberSaveable { mutableStateOf(state.providers.firstOrNull()?.baseUrl ?: "https://api.openai.com/v1") }
    var secretRef by rememberSaveable {
        mutableStateOf(state.providers.firstOrNull()?.secretLabel?.takeIf { it.startsWith("android-secret://") }
            ?: "android-secret://providers/default-openai-compatible")
    }
    var apiKey by rememberSaveable { mutableStateOf("") }
    var enabled by rememberSaveable { mutableStateOf(true) }
    var modelId by rememberSaveable { mutableStateOf("hambur-openai-compatible-text") }
    var pendingDelete by rememberSaveable { mutableStateOf("") }

    if (pendingDelete.isNotBlank()) {
        ConfirmDangerDialog(
            title = "Delete provider",
            text = "This deletes the provider configuration after explicit approval.",
            confirmText = "Delete",
            onConfirm = {
                store.deleteProvider(pendingDelete, true)
                pendingDelete = ""
            },
            onDismiss = { pendingDelete = "" },
        )
    }

    SettingsPage(title = "Providers", onBack = onBack) {
        item {
            HamburSection(
                title = "Configured providers",
                subtitle = "Saved provider entries from the Rust backend",
            ) {
                if (state.providers.isEmpty()) {
                    Text("No providers configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.providers.forEach { provider ->
                        ProviderRow(
                            provider = provider,
                            onUse = {
                                providerId = provider.id
                                name = provider.name
                                baseUrl = provider.baseUrl
                                enabled = provider.enabled
                            },
                            onDelete = { pendingDelete = provider.id },
                        )
                    }
                }
            }
        }
        item {
            HamburSection(title = "Provider editor") {
                OutlinedTextField(value = providerId, onValueChange = { providerId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Provider id") })
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })
                OutlinedTextField(value = baseUrl, onValueChange = { baseUrl = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Base URL") })
                OutlinedTextField(value = secretRef, onValueChange = { secretRef = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Secret ref") })
                OutlinedTextField(value = apiKey, onValueChange = { apiKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("API key") })
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = enabled, onCheckedChange = { enabled = it })
                    Text("Enabled")
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = { store.saveProvider(providerId, name, baseUrl, secretRef, apiKey, enabled) },
                        enabled = baseUrl.isNotBlank() && secretRef.isNotBlank(),
                    ) {
                        Text("Save provider")
                    }
                    SecondaryActionButton(
                        text = "Refresh models",
                        enabled = providerId.isNotBlank(),
                        onClick = { store.refreshProviderModels(providerId, modelId) },
                    )
                }
                OutlinedTextField(value = modelId, onValueChange = { modelId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Model id") })
                SecondaryActionButton(
                    text = "Save model override",
                    enabled = providerId.isNotBlank() && modelId.isNotBlank(),
                    onClick = {
                        store.saveModelOverride(
                            providerId = providerId,
                            modelId = modelId,
                            displayName = modelId,
                            supportsToolCall = true,
                            supportsReasoning = true,
                            supportsImageInput = modelId.contains("vision", ignoreCase = true),
                            contextLimit = 32000u,
                            outputLimit = 4096u,
                        )
                    },
                )
            }
        }
        item {
            ProviderModelsSection(models = state.providerModels)
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
    var displayName by rememberSaveable(modelId) { mutableStateOf(model?.displayName ?: modelId) }
    var supportsTool by rememberSaveable(modelId) { mutableStateOf(model?.supportsToolCall ?: true) }
    var supportsReasoning by rememberSaveable(modelId) { mutableStateOf(model?.supportsReasoning ?: true) }
    var supportsImage by rememberSaveable(modelId) { mutableStateOf(model?.supportsImageInput ?: false) }
    var contextLimit by rememberSaveable(modelId) { mutableStateOf((model?.contextLimit ?: 32000u).toString()) }
    var outputLimit by rememberSaveable(modelId) { mutableStateOf((model?.outputLimit ?: 4096u).toString()) }

    SettingsPage(title = "Model Detail", subtitle = modelId, onBack = onBack) {
        item {
            HamburSection(title = "Capabilities") {
                SummaryLine(label = "Provider", value = providerId)
                OutlinedTextField(value = displayName, onValueChange = { displayName = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Display name") })
                CapabilitySwitch("Tool calls", supportsTool) { supportsTool = it }
                CapabilitySwitch("Reasoning", supportsReasoning) { supportsReasoning = it }
                CapabilitySwitch("Image input", supportsImage) { supportsImage = it }
                OutlinedTextField(value = contextLimit, onValueChange = { contextLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Context limit") })
                OutlinedTextField(value = outputLimit, onValueChange = { outputLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Output limit") })
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
                    Text("Save override")
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
    var id by rememberSaveable(providerId) { mutableStateOf(provider?.id ?: providerId) }
    var name by rememberSaveable(providerId) { mutableStateOf(provider?.name ?: "OpenAI Compatible") }
    var baseUrl by rememberSaveable(providerId) { mutableStateOf(provider?.baseUrl ?: "https://api.openai.com/v1") }
    var secretRef by rememberSaveable(providerId) { mutableStateOf("android-secret://providers/${providerId.ifBlank { "new" }}") }
    var apiKey by rememberSaveable(providerId) { mutableStateOf("") }
    var enabled by rememberSaveable(providerId) { mutableStateOf(provider?.enabled ?: true) }
    var refreshModelId by rememberSaveable(providerId) { mutableStateOf("hambur-openai-compatible-text") }

    SettingsPage(title = title, subtitle = id, onBack = onBack) {
        item {
            HamburSection(title = "Provider") {
                OutlinedTextField(value = id, onValueChange = { id = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Provider id") })
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })
                OutlinedTextField(value = baseUrl, onValueChange = { baseUrl = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Base URL") })
                OutlinedTextField(value = secretRef, onValueChange = { secretRef = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Secret ref") })
                OutlinedTextField(value = apiKey, onValueChange = { apiKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("API key") })
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Switch(checked = enabled, onCheckedChange = { enabled = it })
                    Text("Enabled", modifier = Modifier.padding(start = 8.dp))
                }
                Button(
                    enabled = id.isNotBlank() && baseUrl.isNotBlank() && secretRef.isNotBlank(),
                    onClick = { store.saveProvider(id, name, baseUrl, secretRef, apiKey, enabled) },
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
                    onClick = { store.refreshProviderModels(id, refreshModelId) },
                )
            }
        }
        extraContent()
    }
}

@Composable
fun ModelGroupSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var groupId by rememberSaveable { mutableStateOf(state.modelGroups.firstOrNull()?.id ?: "grp_primary_chat") }
    var groupName by rememberSaveable { mutableStateOf(state.modelGroups.firstOrNull()?.name ?: "Primary Chat") }
    var routingStrategy by rememberSaveable { mutableStateOf("fallback") }
    var fallbackPolicy by rememberSaveable { mutableStateOf("default") }
    var defaultKey by rememberSaveable { mutableStateOf("primary") }
    var providerId by rememberSaveable { mutableStateOf(state.providers.firstOrNull()?.id.orEmpty()) }
    var modelId by rememberSaveable { mutableStateOf(state.providerModels.firstOrNull()?.modelId.orEmpty()) }

    SettingsPage(title = "Model Groups", onBack = onBack) {
        item {
            HamburSection(title = "Groups") {
                state.modelGroups.forEach { group ->
                    SummaryLine(label = group.id, value = "${group.name} / ${group.routingStrategy} / ${group.fallbackPolicy}")
                }
                if (state.modelGroups.isEmpty()) {
                    Text("No model groups configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        item {
            HamburSection(title = "Group editor") {
                OutlinedTextField(value = groupId, onValueChange = { groupId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Group id") })
                OutlinedTextField(value = groupName, onValueChange = { groupName = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })
                OutlinedTextField(value = routingStrategy, onValueChange = { routingStrategy = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Routing strategy") })
                OutlinedTextField(value = fallbackPolicy, onValueChange = { fallbackPolicy = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Fallback policy") })
                Button(
                    onClick = { store.saveModelGroup(groupId, groupName, routingStrategy, fallbackPolicy) },
                    enabled = groupId.isNotBlank(),
                ) {
                    Text("Save group")
                }
            }
        }
        item {
            HamburSection(title = "Default and members") {
                OutlinedTextField(value = defaultKey, onValueChange = { defaultKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Default key") })
                SecondaryActionButton(
                    text = "Set default group",
                    enabled = defaultKey.isNotBlank() && groupId.isNotBlank(),
                    onClick = { store.setDefaultModelGroup(defaultKey, groupId) },
                )
                OutlinedTextField(value = providerId, onValueChange = { providerId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Provider id") })
                OutlinedTextField(value = modelId, onValueChange = { modelId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Model id") })
                SecondaryActionButton(
                    text = "Add member",
                    enabled = groupId.isNotBlank() && providerId.isNotBlank() && modelId.isNotBlank(),
                    onClick = { store.addModelGroupMember(groupId, providerId, modelId, 0u, true) },
                )
                state.defaultModelGroups.forEach { SummaryLine(label = "Default ${it.key}", value = it.groupId) }
                state.modelGroupMembers.forEach { member ->
                    SummaryLine(
                        label = member.groupId,
                        value = "${member.providerName.ifBlank { member.providerId }} / ${member.modelDisplayName.ifBlank { member.modelId }}",
                    )
                }
            }
        }
    }
}

@Composable
fun ModelGroupsListScreen(
    state: HamburUiState,
    onBack: () -> Unit,
    onNewGroup: () -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    SettingsPage(title = "Model Groups", onBack = onBack) {
        item {
            Button(onClick = onNewGroup) {
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
                if (state.defaultModelGroups.isEmpty()) {
                    Text("No defaults configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.defaultModelGroups.forEach {
                        SummaryLine(label = it.key, value = it.groupId)
                    }
                }
            }
        }
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
    var id by rememberSaveable(groupId) { mutableStateOf(existing?.id ?: "grp_primary_chat") }
    var name by rememberSaveable(groupId) { mutableStateOf(existing?.name ?: "Primary Chat") }
    var routingStrategy by rememberSaveable(groupId) { mutableStateOf(existing?.routingStrategy ?: "fallback") }
    var fallbackPolicy by rememberSaveable(groupId) { mutableStateOf(existing?.fallbackPolicy ?: "default") }
    var defaultKey by rememberSaveable(groupId) { mutableStateOf("primary") }
    var providerId by rememberSaveable(groupId) { mutableStateOf(state.providers.firstOrNull()?.id.orEmpty()) }
    var modelId by rememberSaveable(groupId) { mutableStateOf(state.providerModels.firstOrNull()?.modelId.orEmpty()) }
    var position by rememberSaveable(groupId) { mutableStateOf("0") }

    SettingsPage(title = if (existing == null) "New Model Group" else existing.name, subtitle = id, onBack = onBack) {
        item {
            HamburSection(title = "Group") {
                OutlinedTextField(value = id, onValueChange = { id = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Group id") })
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Name") })
                OutlinedTextField(value = routingStrategy, onValueChange = { routingStrategy = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Routing strategy") })
                OutlinedTextField(value = fallbackPolicy, onValueChange = { fallbackPolicy = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Fallback policy") })
                Button(onClick = { store.saveModelGroup(id, name, routingStrategy, fallbackPolicy) }, enabled = id.isNotBlank()) {
                    Text("Save group")
                }
            }
        }
        item {
            HamburSection(title = "Members") {
                val members = state.modelGroupMembers.filter { it.groupId == id }
                if (members.isEmpty()) {
                    Text("No members configured", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    members.forEach { member ->
                        SummaryLine(
                            label = "#${member.position}",
                            value = "${member.providerName.ifBlank { member.providerId }} / ${member.modelDisplayName.ifBlank { member.modelId }}",
                        )
                    }
                }
                OutlinedTextField(value = providerId, onValueChange = { providerId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Provider id") })
                OutlinedTextField(value = modelId, onValueChange = { modelId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Model id") })
                OutlinedTextField(value = position, onValueChange = { position = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Position") })
                SecondaryActionButton(
                    text = "Add member",
                    enabled = id.isNotBlank() && providerId.isNotBlank() && modelId.isNotBlank(),
                    onClick = { store.addModelGroupMember(id, providerId, modelId, position.toUIntOrNull() ?: 0u, true) },
                )
            }
        }
        item {
            HamburSection(title = "Default route") {
                OutlinedTextField(value = defaultKey, onValueChange = { defaultKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Default key") })
                SecondaryActionButton(
                    text = "Set as default",
                    enabled = id.isNotBlank() && defaultKey.isNotBlank(),
                    onClick = { store.setDefaultModelGroup(defaultKey, id) },
                )
            }
        }
    }
}

@Composable
fun SkillsListScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
    onOpenSkill: (String) -> Unit,
) {
    val skillSettings = state.appSettings.filter {
        it.key == "skills" || it.key.startsWith("skill_enabled:")
    }
    SettingsPage(title = "Skills", onBack = onBack) {
        item {
            HamburSection(title = "Built-in skills") {
                SkillListRow("system/skill-creator", "Create and maintain Codex skills", true, onOpenSkill)
                SkillListRow("system/openai-docs", "OpenAI product and API documentation workflow", true, onOpenSkill)
            }
        }
        item {
            HamburSection(title = "Backend skill settings") {
                if (skillSettings.isEmpty()) {
                    Text("No skill settings exposed by backend yet", color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    skillSettings.forEach {
                        SummaryLine(label = it.key, value = it.value)
                    }
                }
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
    var enabled by rememberSaveable(skillId) {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "skill_enabled:$skillId" }?.value != "false")
    }
    var skillsJson by rememberSaveable {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "skills" }?.value ?: """{"enabled":true,"paths":[]}""")
    }
    SettingsPage(title = "Skill Detail", subtitle = skillId, onBack = onBack) {
        item {
            HamburSection(title = "Skill") {
                SummaryLine(label = "Path", value = skillId)
                SummaryLine(label = "Files", value = "Backend does not expose skill file listing yet")
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Switch(checked = enabled, onCheckedChange = { enabled = it })
                    Text("Enabled", modifier = Modifier.padding(start = 8.dp))
                }
                Button(onClick = { store.setSkillEnabled(skillId, enabled) }) {
                    Text("Save enabled flag")
                }
            }
        }
        item {
            HamburSection(title = "Global skills JSON") {
                OutlinedTextField(value = skillsJson, onValueChange = { skillsJson = it }, modifier = Modifier.fillMaxWidth(), minLines = 6, label = { Text("skills") })
                SecondaryActionButton(text = "Save skills JSON", onClick = { store.saveAppSetting("skills", skillsJson, false) })
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
    val memorySettings = state.appSettings.filter {
        it.key == "memory_projections" || it.key.startsWith("memory")
    }
    SettingsPage(title = "Memory", onBack = onBack) {
        item {
            HamburSection(title = "Memory files") {
                if (memorySettings.isEmpty()) {
                    Text("No memory file list exposed by backend yet", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    SecondaryActionButton(text = "Open memory_projections", onClick = { onOpenMemory("memory_projections") })
                } else {
                    memorySettings.forEach { setting ->
                        MemoryListRow(setting.key, setting.value, onOpenMemory)
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
    var value by rememberSaveable(memoryKey) {
        mutableStateOf(state.appSettings.firstOrNull { it.key == memoryKey }?.value ?: """{"enabled":true,"files":[]}""")
    }
    SettingsPage(title = "Memory Detail", subtitle = memoryKey, onBack = onBack) {
        item {
            HamburSection(title = "Projection") {
                OutlinedTextField(value = value, onValueChange = { value = it }, modifier = Modifier.fillMaxWidth(), minLines = 8, label = { Text(memoryKey) })
                Button(onClick = { store.saveAppSetting("memory_projections", value, false) }) {
                    Text("Save memory projections")
                }
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
                    "web_fetch" to "Fetch and extract web page content",
                    "view_image" to "Inspect image attachments",
                    "browser_use" to "Shared browser actions",
                    "terminal" to "Sandbox terminal tools",
                    "file" to "Sandbox file tools",
                    "knowledge" to "Memory and knowledge tools",
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
    var browserSettings by rememberSaveable(toolName) {
        mutableStateOf(state.appSettings.firstOrNull { it.key == "browser_tool_settings" }?.value ?: """{"enabled":true,"acceptCookies":true,"maxFetchBytes":1000000}""")
    }
    SettingsPage(title = "Tool Detail", subtitle = toolName, onBack = onBack) {
        item {
            HamburSection(title = "Description") {
                SummaryLine(label = "Name", value = toolName)
                SummaryLine(label = "Backend", value = if (toolName == "browser_use") "AndroidPlatformAdapter browser actions" else "Rust tool registry")
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
        if (toolName == "browser_use") {
            item {
                HamburSection(title = "Browser settings") {
                    OutlinedTextField(value = browserSettings, onValueChange = { browserSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("browser_tool_settings") })
                    SecondaryActionButton(text = "Save browser settings", onClick = { store.saveBrowserToolSettings(browserSettings) })
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
        mutableStateOf(state.appSettings.firstOrNull { it.key == "browser_tool_settings" }?.value ?: """{"enabled":true,"autoCloseMinutes":20}""")
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

    if (confirmReset) {
        ConfirmDangerDialog(
            title = "Reset rootfs",
            text = "This clears the rootfs directory after explicit approval.",
            confirmText = "Reset",
            onConfirm = {
                store.resetRootfs(true)
                confirmReset = false
            },
            onDismiss = { confirmReset = false },
        )
    }

    SettingsPage(title = "Rootfs", onBack = onBack) {
        item {
            HamburSection(title = "Lifecycle") {
                SummaryLine(label = "Selected session", value = state.selectedSessionId)
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
            HamburSection(title = "Rootfs setting") {
                OutlinedTextField(value = key, onValueChange = { key = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text("Setting key") })
                OutlinedTextField(value = value, onValueChange = { value = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("Value") })
                Button(onClick = { store.saveRootfsSetting(key, value, true) }, enabled = key.isNotBlank() && value.isNotBlank()) {
                    Text("Approve and save")
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
    var toggleTwo by rememberSaveable(title) { mutableStateOf(false) }
    var toggleThree by rememberSaveable(title) { mutableStateOf(false) }
    var toggleFour by rememberSaveable(title) { mutableStateOf(false) }
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
                    "Appearance" -> {
                        OutlinedTextField(
                            value = choiceValue,
                            onValueChange = { choiceValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            label = { Text("Font scale") },
                            placeholder = { Text("Small / Default / Large") },
                        )
                        OutlinedTextField(
                            value = textValue,
                            onValueChange = { textValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            label = { Text("Startup chat mode") },
                            placeholder = { Text("Last chat / New chat") },
                        )
                        CapabilitySwitch("Follow system theme", toggleOne) { toggleOne = it }
                        CapabilitySwitch("Dark mode", toggleTwo) { toggleTwo = it }
                        CapabilitySwitch("Predictive back", toggleThree) { toggleThree = it }
                        CapabilitySwitch("FPS overlay", toggleFour) { toggleFour = it }
                    }
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
private fun SkillListRow(
    skillId: String,
    description: String,
    builtIn: Boolean,
    onOpenSkill: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        onClick = { onOpenSkill(skillId) },
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(skillId, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                StatusPill(text = if (builtIn) "Built-in" else "User", active = builtIn)
            }
            Text(description, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
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
