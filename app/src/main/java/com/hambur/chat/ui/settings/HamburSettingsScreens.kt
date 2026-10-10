package com.hambur.chat.ui.settings

import androidx.annotation.StringRes
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.res.stringResource
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.composables.icons.lucide.Brain
import com.composables.icons.lucide.ChevronRight
import com.composables.icons.lucide.Database
import com.composables.icons.lucide.Globe
import com.composables.icons.lucide.HardDrive
import com.composables.icons.lucide.KeyRound
import com.composables.icons.lucide.Layers
import com.composables.icons.lucide.ListTodo
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.Terminal
import com.composables.icons.lucide.Wrench
import com.hambur.chat.R
import com.hambur.chat.diagnostics.DiagLevel
import com.hambur.chat.diagnostics.DiagLogEntry
import com.hambur.chat.diagnostics.DiagSource
import com.hambur.chat.uniffi.BackendEventKind
import com.hambur.chat.platform.HamburAppLocale
import com.hambur.chat.reducer.AUTO_FOCUS_INPUT_AFTER_REPLY_KEY
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.PERSONA_BYTE_LIMIT
import com.hambur.chat.reducer.PERSONA_SETTING_KEY
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_EXPAND
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_COLLAPSED
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_MODE_KEY
import com.hambur.chat.reducer.UiConfigAudit
import com.hambur.chat.reducer.UiModelGroupSettings
import com.hambur.chat.reducer.UiProviderModelSettings
import com.hambur.chat.reducer.UiProviderSettings
import com.hambur.chat.reducer.UiSkillSummary
import com.hambur.chat.reducer.UiTokenUsageGroup
import com.hambur.chat.reducer.UiTokenUsageSnapshot
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
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.delay
import org.json.JSONObject

/** 日志时间戳只用时分秒毫秒，避免每条都建 SimpleDateFormat。 */
private val LogTimeFormatter: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm:ss.SSS")

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
            title = stringResource(R.string.settings_title),
            subtitle = stringResource(
                R.string.settings_subtitle_providers_groups,
                state.providers.size,
                state.modelGroups.size,
            ),
            onBack = onBack,
        )
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            item {
                HamburSection(title = stringResource(R.string.settings_section_models)) {
                    SettingsNavigationRow(
                        icon = Lucide.KeyRound,
                        title = stringResource(R.string.settings_providers_title),
                        summary = stringResource(R.string.settings_providers_summary),
                        onClick = onOpenProviders,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Layers,
                        title = stringResource(R.string.settings_model_groups_title),
                        summary = stringResource(R.string.settings_model_groups_summary),
                        onClick = onOpenModelGroups,
                    )
                }
            }
            item {
                HamburSection(title = stringResource(R.string.settings_section_agent_runtime)) {
                    SettingsNavigationRow(
                        icon = Lucide.ListTodo,
                        title = stringResource(R.string.settings_skills_title),
                        summary = stringResource(R.string.settings_skills_summary),
                        onClick = onOpenSkills,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Database,
                        title = stringResource(R.string.settings_memory_title),
                        summary = stringResource(R.string.settings_memory_summary),
                        onClick = onOpenMemory,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Wrench,
                        title = stringResource(R.string.settings_tools_title),
                        summary = stringResource(R.string.settings_tools_summary),
                        onClick = onOpenTools,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Terminal,
                        title = stringResource(R.string.settings_startup_tasks_title),
                        summary = stringResource(R.string.settings_startup_tasks_summary),
                        onClick = onOpenStartupTasks,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.HardDrive,
                        title = stringResource(R.string.settings_rootfs_title),
                        summary = stringResource(R.string.settings_rootfs_summary),
                        onClick = onOpenRootfs,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Wrench,
                        title = stringResource(R.string.settings_env_vars_title),
                        summary = stringResource(R.string.settings_env_vars_summary),
                        onClick = onOpenEnvironmentVariables,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Brain,
                        title = stringResource(R.string.settings_persona_title),
                        summary = stringResource(R.string.settings_persona_summary),
                        onClick = onOpenPersona,
                    )
                }
            }
            item {
                HamburSection(title = stringResource(R.string.settings_section_app)) {
                    SettingsNavigationRow(
                        icon = Lucide.Settings,
                        title = stringResource(R.string.appearance_title),
                        summary = stringResource(R.string.settings_appearance_summary),
                        onClick = onOpenAppearance,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Terminal,
                        title = stringResource(R.string.settings_logs_title),
                        summary = stringResource(R.string.settings_logs_summary),
                        onClick = onOpenLogs,
                    )
                    SettingsNavigationRow(
                        icon = Lucide.Database,
                        title = stringResource(R.string.settings_token_usage_title),
                        summary = stringResource(R.string.settings_token_usage_summary),
                        onClick = onOpenTokenUsage,
                    )
                }
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
        title = stringResource(R.string.settings_providers_title),
        subtitle = stringResource(R.string.settings_providers_subtitle),
        onBack = onBack,
    ) {
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onNewProvider) {
                    Text(stringResource(R.string.settings_provider_new))
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_providers_section_openai)) {
                if (state.providers.isEmpty()) {
                    Text(
                        stringResource(R.string.settings_providers_empty),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
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
        title = stringResource(R.string.settings_provider_new_title),
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
            title = stringResource(R.string.settings_provider_delete_title),
            text = stringResource(R.string.settings_provider_delete_message),
            confirmText = stringResource(R.string.common_delete),
            onConfirm = {
                store.deleteProvider(providerId, true)
                pendingDelete = false
                onBack()
            },
            onDismiss = { pendingDelete = false },
        )
    }

    ProviderEditorScreen(
        title = provider?.name ?: stringResource(R.string.settings_provider_detail_title),
        state = state,
        store = store,
        providerId = providerId,
        onBack = onBack,
        extraContent = {
            item {
                HamburSection(
                    title = stringResource(R.string.settings_section_models),
                    subtitle = stringResource(R.string.settings_provider_models_subtitle),
                ) {
                    val models = state.providerModels.filter { it.providerId == providerId }
                    if (models.isEmpty()) {
                        Text(
                            stringResource(R.string.settings_provider_models_empty),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
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
                HamburSection(title = stringResource(R.string.settings_danger_zone)) {
                    TextButton(onClick = { pendingDelete = true }) {
                        Text(
                            stringResource(R.string.settings_provider_delete_title),
                            color = MaterialTheme.colorScheme.error,
                        )
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
            HamburSection(title = stringResource(R.string.settings_model_section)) {
                SummaryLine(label = stringResource(R.string.settings_model_id), value = modelId)
                SummaryLine(label = stringResource(R.string.settings_model_provider), value = provider?.name ?: providerId)
                SummaryLine(
                    label = stringResource(R.string.settings_model_synced_at),
                    value = model?.syncedAtMs?.toDateTimeText().orEmpty()
                        .ifBlank { stringResource(R.string.common_not_available) },
                )
                OutlinedTextField(
                    value = displayName,
                    onValueChange = { displayName = it },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                    label = { Text(stringResource(R.string.settings_model_display_name)) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_basic_info)) {
                SummaryLine(label = stringResource(R.string.settings_model_family), value = metadata.family.ifBlank { stringResource(R.string.common_not_available) })
                SummaryLine(label = stringResource(R.string.settings_model_knowledge_cutoff), value = metadata.knowledgeCutoff.ifBlank { stringResource(R.string.common_not_available) })
                SummaryLine(label = stringResource(R.string.settings_model_release_date), value = metadata.releaseDate.ifBlank { stringResource(R.string.common_not_available) })
                SummaryLine(label = stringResource(R.string.settings_model_last_updated), value = metadata.lastUpdated.ifBlank { stringResource(R.string.common_not_available) })
                SummaryLine(label = stringResource(R.string.settings_model_status), value = metadata.status.ifBlank { stringResource(R.string.common_not_available) })
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_capabilities)) {
                SummaryLine(label = stringResource(R.string.settings_model_input_modalities), value = metadata.inputModalities.joinToString(" / ").ifBlank { if (supportsImage) "text / image" else "text" })
                SummaryLine(label = stringResource(R.string.settings_model_output_modalities), value = metadata.outputModalities.joinToString(" / ").ifBlank { "text" })
                CapabilityIndicator(stringResource(R.string.settings_model_supports_attachments), metadata.supportsAttachments || supportsImage)
                CapabilitySwitch(stringResource(R.string.settings_model_supports_reasoning), supportsReasoning) { supportsReasoning = it }
                CapabilitySwitch(stringResource(R.string.settings_model_supports_tool_call), supportsTool) { supportsTool = it }
                CapabilitySwitch(stringResource(R.string.settings_model_image_input), supportsImage) { supportsImage = it }
                CapabilityIndicator(stringResource(R.string.settings_model_structured_output), model?.supportsStructuredOutput ?: false)
                CapabilityIndicator(stringResource(R.string.settings_model_temperature), model?.supportsTemperature ?: true)
                CapabilityIndicator(stringResource(R.string.settings_model_open_weights), metadata.openWeights)
                SummaryLine(label = stringResource(R.string.settings_model_reasoning_field), value = model?.reasoningField.orEmpty().ifBlank { metadata.interleavedField.ifBlank { stringResource(R.string.common_not_available) } })
                SummaryLine(label = stringResource(R.string.settings_model_reasoning_options), value = metadata.reasoningOptions.joinToString("、").ifBlank { stringResource(R.string.common_not_available) })
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_limits_pricing)) {
                OutlinedTextField(value = contextLimit, onValueChange = { contextLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_model_context_limit)) })
                OutlinedTextField(value = outputLimit, onValueChange = { outputLimit = it.filter(Char::isDigit) }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_model_max_output)) })
                SummaryLine(label = stringResource(R.string.settings_model_input_price), value = metadata.inputCostPerMillion.toCostText())
                SummaryLine(label = stringResource(R.string.settings_model_output_price), value = metadata.outputCostPerMillion.toCostText())
                SummaryLine(label = stringResource(R.string.settings_model_cache_read_price), value = metadata.cacheReadCostPerMillion.toCostText())
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_references)) {
                SummaryLine(label = stringResource(R.string.settings_model_weight_links), value = metadata.weightLinks.joinToString { it.label.ifBlank { it.url } }.ifBlank { stringResource(R.string.common_not_available) })
                SummaryLine(
                    label = stringResource(R.string.settings_model_benchmarks),
                    value = if (metadata.benchmarks.isEmpty()) {
                        stringResource(R.string.common_not_available)
                    } else {
                        stringResource(R.string.settings_model_benchmarks_count, metadata.benchmarks.size)
                    },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_override_section)) {
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
                    Text(stringResource(R.string.settings_model_override_save))
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_danger_zone)) {
                var pendingDeleteModel by rememberSaveable(modelId) { mutableStateOf(false) }
                if (pendingDeleteModel) {
                    ConfirmDangerDialog(
                        title = stringResource(R.string.settings_model_delete_title),
                        text = stringResource(R.string.settings_model_delete_message, modelId),
                        confirmText = stringResource(R.string.common_delete),
                        onConfirm = {
                            store.deleteProviderModel(providerId, modelId)
                            pendingDeleteModel = false
                            onBack()
                        },
                        onDismiss = { pendingDeleteModel = false }
                    )
                }
                TextButton(onClick = { pendingDeleteModel = true }) {
                    Text(stringResource(R.string.settings_model_delete_title), color = MaterialTheme.colorScheme.error)
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
    var showAddCustomModelDialog by rememberSaveable(providerId) { mutableStateOf(false) }
    var customModelId by rememberSaveable(providerId) { mutableStateOf("") }
    var customDisplayName by rememberSaveable(providerId) { mutableStateOf("") }
    var customSupportsReasoning by rememberSaveable(providerId) { mutableStateOf(true) }
    var customSupportsToolCall by rememberSaveable(providerId) { mutableStateOf(true) }
    var customSupportsImageInput by rememberSaveable(providerId) { mutableStateOf(false) }
    var customContextLimit by rememberSaveable(providerId) { mutableStateOf("32000") }
    var customOutputLimit by rememberSaveable(providerId) { mutableStateOf("4096") }

    val context = LocalContext.current
    var lastCommandSequence by rememberSaveable(providerId) { mutableStateOf(0L) }
    var pendingAction by rememberSaveable(providerId) { mutableStateOf("") }

    LaunchedEffect(state.lastAppliedSequence) {
        val currentSequence = state.lastAppliedSequence.toLong()
        if (lastCommandSequence > 0L && currentSequence > lastCommandSequence) {
            if (pendingAction == "Refresh" && state.latestEventKind == BackendEventKind.MODELS_UPDATED.name) {
                Toast.makeText(context, state.footer.ifBlank { context.getString(R.string.settings_toast_models_refreshed) }, Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (pendingAction == "Save" && state.latestEventKind == BackendEventKind.SETTINGS_CHANGED.name) {
                Toast.makeText(context, context.getString(R.string.settings_toast_provider_saved), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (pendingAction == "AddModel" && state.latestEventKind == BackendEventKind.SETTINGS_CHANGED.name) {
                Toast.makeText(context, context.getString(R.string.settings_toast_model_added), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (state.runtimeStatus == "Error") {
                Toast.makeText(context, context.getString(R.string.settings_toast_operation_failed, state.footer), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            }
        }
    }

    SettingsPage(title = title, subtitle = name, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_provider_editor_section)) {
                if (provider != null) {
                    SummaryLine(label = stringResource(R.string.settings_provider_id), value = id)
                }
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_provider_name)) })
                OutlinedTextField(value = baseUrl, onValueChange = { baseUrl = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_provider_base_url)) })
                OutlinedTextField(value = apiKey, onValueChange = { apiKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_provider_api_key)) })
                
                Text(stringResource(R.string.settings_provider_api_protocol), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    listOf(
                        "OpenAiCompatible" to stringResource(R.string.settings_provider_protocol_chat_completions),
                        "OpenAiResponses" to stringResource(R.string.settings_provider_protocol_responses)
                    ).forEach { (typeKey, label) ->
                        val selected = apiType == typeKey
                        OutlinedCard(
                            onClick = { apiType = typeKey },
                            colors = CardDefaults.outlinedCardColors(
                                containerColor = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface
                            ),
                            border = BorderStroke(
                                width = if (selected) 2.dp else 1.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
                            ),
                            modifier = Modifier.weight(1f)
                        ) {
                            Box(
                                modifier = Modifier.fillMaxWidth().padding(vertical = 10.dp, horizontal = 8.dp),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = label,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                }

                Text(stringResource(R.string.settings_provider_select_icon), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
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
                    Text(stringResource(R.string.common_enabled), modifier = Modifier.padding(start = 8.dp))
                }
                Button(
                    enabled = id.isNotBlank() && baseUrl.isNotBlank() && name.isNotBlank() && secretRef.isNotBlank(),
                    onClick = {
                        lastCommandSequence = state.lastAppliedSequence.toLong()
                        pendingAction = "Save"
                        Toast.makeText(context, context.getString(R.string.settings_toast_provider_saving), Toast.LENGTH_SHORT).show()
                        store.saveProvider(id, name, baseUrl, secretRef, apiKey, enabled, iconName, apiType)
                    },
                ) {
                    Text(stringResource(R.string.settings_provider_save))
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_provider_model_sync_section)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    SecondaryActionButton(
                        text = stringResource(R.string.settings_provider_refresh_models),
                        enabled = id.isNotBlank() && provider != null,
                        onClick = {
                            lastCommandSequence = state.lastAppliedSequence.toLong()
                            pendingAction = "Refresh"
                            Toast.makeText(context, context.getString(R.string.settings_toast_models_syncing), Toast.LENGTH_SHORT).show()
                            store.refreshProviderModels(
                                providerId = id,
                                baseUrl = baseUrl,
                                apiKey = apiKey,
                                secretRef = secretRef,
                                modelId = ""
                            )
                        },
                    )
                    SecondaryActionButton(
                        text = stringResource(R.string.settings_provider_add_model),
                        enabled = id.isNotBlank() && provider != null,
                        onClick = {
                            showAddCustomModelDialog = true
                        },
                    )
                }
                if (provider == null) {
                    Text(
                        stringResource(R.string.settings_provider_save_first_hint),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
            }
        }
        extraContent()
    }

    if (showAddCustomModelDialog) {
        AlertDialog(
            onDismissRequest = { showAddCustomModelDialog = false },
            title = { Text(stringResource(R.string.settings_provider_add_model)) },
            text = {
                Column(
                    modifier = Modifier.verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    OutlinedTextField(
                        value = customModelId,
                        onValueChange = { customModelId = it },
                        label = { Text(stringResource(R.string.settings_model_id_required)) },
                        placeholder = { Text(stringResource(R.string.settings_model_id_placeholder)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = customDisplayName,
                        onValueChange = { customDisplayName = it },
                        label = { Text(stringResource(R.string.settings_model_display_name)) },
                        placeholder = { Text(stringResource(R.string.settings_optional)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = customContextLimit,
                        onValueChange = { customContextLimit = it.filter(Char::isDigit) },
                        label = { Text(stringResource(R.string.settings_model_context_limit)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = customOutputLimit,
                        onValueChange = { customOutputLimit = it.filter(Char::isDigit) },
                        label = { Text(stringResource(R.string.settings_model_max_output)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text(stringResource(R.string.settings_model_supports_reasoning))
                        Switch(
                            checked = customSupportsReasoning,
                            onCheckedChange = { customSupportsReasoning = it }
                        )
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text(stringResource(R.string.settings_model_supports_tool_call))
                        Switch(
                            checked = customSupportsToolCall,
                            onCheckedChange = { customSupportsToolCall = it }
                        )
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text(stringResource(R.string.settings_model_supports_vision))
                        Switch(
                            checked = customSupportsImageInput,
                            onCheckedChange = { customSupportsImageInput = it }
                        )
                    }
                }
            },
            confirmButton = {
                Button(
                    enabled = customModelId.isNotBlank(),
                    onClick = {
                        lastCommandSequence = state.lastAppliedSequence.toLong()
                        pendingAction = "AddModel"
                        Toast.makeText(context, context.getString(R.string.settings_toast_model_adding), Toast.LENGTH_SHORT).show()
                        store.saveModelOverride(
                            providerId = id,
                            modelId = customModelId.trim(),
                            displayName = customDisplayName.trim().ifBlank { customModelId.trim() },
                            supportsToolCall = customSupportsToolCall,
                            supportsReasoning = customSupportsReasoning,
                            supportsImageInput = customSupportsImageInput,
                            contextLimit = customContextLimit.toUIntOrNull() ?: 32000u,
                            outputLimit = customOutputLimit.toUIntOrNull() ?: 4096u,
                            custom = true,
                        )
                        showAddCustomModelDialog = false
                        customModelId = ""
                        customDisplayName = ""
                        customSupportsReasoning = true
                        customSupportsToolCall = true
                        customSupportsImageInput = false
                        customContextLimit = "32000"
                        customOutputLimit = "4096"
                    }
                ) {
                    Text(stringResource(R.string.settings_add))
                }
            },
            dismissButton = {
                TextButton(onClick = { showAddCustomModelDialog = false }) {
                    Text(stringResource(R.string.common_cancel))
                }
            }
        )
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

    SettingsPage(title = stringResource(R.string.settings_model_groups_title), onBack = onBack) {
        item {
            Button(onClick = onNewGroup, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.settings_model_group_new))
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_model_groups_section)) {
                if (state.modelGroups.isEmpty()) {
                    Text(stringResource(R.string.settings_model_groups_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
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
            HamburSection(title = stringResource(R.string.settings_model_groups_default_section)) {
                val primaryGroupName = state.modelGroups.firstOrNull { it.id == primaryGroup }?.name
                    ?: primaryGroup.ifBlank { stringResource(R.string.settings_model_group_select) }
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    onClick = { showPrimaryMenu = true }
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Text(stringResource(R.string.settings_model_group_primary), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                        Text(primaryGroupName, color = MaterialTheme.colorScheme.primary)
                    }
                }

                val secondaryGroupName = state.modelGroups.firstOrNull { it.id == secondaryGroup }?.name
                    ?: secondaryGroup.ifBlank { stringResource(R.string.settings_model_group_select) }
                Surface(
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.surfaceVariant,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    onClick = { showSecondaryMenu = true }
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Text(stringResource(R.string.settings_model_group_secondary), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                        Text(secondaryGroupName, color = MaterialTheme.colorScheme.primary)
                    }
                }
            }
        }
    }

    if (showPrimaryMenu) {
        AlertDialog(
            onDismissRequest = { showPrimaryMenu = false },
            title = { Text(stringResource(R.string.settings_model_group_select_primary)) },
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
                TextButton(onClick = { showPrimaryMenu = false }) { Text(stringResource(R.string.common_cancel)) }
            }
        )
    }

    if (showSecondaryMenu) {
        AlertDialog(
            onDismissRequest = { showSecondaryMenu = false },
            title = { Text(stringResource(R.string.settings_model_group_select_secondary)) },
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
                TextButton(onClick = { showSecondaryMenu = false }) { Text(stringResource(R.string.common_cancel)) }
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
            if (pendingAction == "SaveGroup" && state.latestEventKind == BackendEventKind.SETTINGS_CHANGED.name) {
                Toast.makeText(context, context.getString(R.string.settings_toast_model_group_saved), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            } else if (state.runtimeStatus == "Error") {
                Toast.makeText(context, context.getString(R.string.settings_toast_operation_failed, state.footer), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
                pendingAction = ""
            }
        }
    }

    var showAddModelDialog by remember { mutableStateOf(false) }
    var pendingDeleteGroup by remember { mutableStateOf(false) }

    if (pendingDeleteGroup) {
        ConfirmDangerDialog(
            title = stringResource(R.string.settings_model_group_delete_title),
            text = stringResource(R.string.settings_model_group_delete_message),
            confirmText = stringResource(R.string.common_delete),
            onConfirm = {
                store.deleteModelGroup(id, true)
                pendingDeleteGroup = false
                onBack()
            },
            onDismiss = { pendingDeleteGroup = false }
        )
    }

    SettingsPage(title = if (existing == null) stringResource(R.string.settings_model_group_new) else existing.name, subtitle = name, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_model_group_settings_section)) {
                if (existing != null) {
                    SummaryLine(label = stringResource(R.string.settings_model_group_id), value = id)
                }
                OutlinedTextField(value = name, onValueChange = { name = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_provider_name)) })

                Text(stringResource(R.string.settings_model_group_routing_strategy), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    listOf(
                        "fallback" to stringResource(R.string.settings_model_group_strategy_fallback),
                        "load_balance" to stringResource(R.string.settings_model_group_strategy_load_balance),
                    ).forEach { (strategyKey, strategyLabel) ->
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

                Text(stringResource(R.string.settings_model_group_fallback_policy), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    listOf(
                        "default" to stringResource(R.string.settings_model_group_policy_default),
                        "always" to stringResource(R.string.settings_model_group_policy_always),
                    ).forEach { (policyKey, policyLabel) ->
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
                        Toast.makeText(context, context.getString(R.string.settings_toast_group_saving), Toast.LENGTH_SHORT).show()
                        store.saveModelGroup(id, name, routingStrategy, fallbackPolicy)
                    },
                    enabled = id.isNotBlank() && name.isNotBlank(),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text(stringResource(R.string.settings_model_group_save))
                }
            }
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_model_group_members),
                subtitle = stringResource(R.string.settings_model_group_members_subtitle),
            ) {
                val members = state.modelGroupMembers.filter { it.groupId == id }
                if (members.isEmpty()) {
                    Text(stringResource(R.string.settings_model_group_members_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
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
                                        contentDescription = stringResource(R.string.settings_model_group_delete_member),
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
                    Text(stringResource(R.string.settings_provider_add_model))
                }
            }
        }
        if (existing != null) {
            item {
                HamburSection(title = stringResource(R.string.settings_danger_zone)) {
                    TextButton(onClick = { pendingDeleteGroup = true }) {
                        Text(stringResource(R.string.settings_model_group_delete_title), color = MaterialTheme.colorScheme.error)
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
            title = { Text(stringResource(R.string.settings_provider_add_model)) },
            text = {
                if (availableModels.isEmpty()) {
                    Text(stringResource(R.string.settings_model_group_no_available_models))
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
                    Text(stringResource(R.string.common_cancel))
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
    SettingsPage(title = stringResource(R.string.settings_skills_title), onBack = onBack) {
        val builtInSkills = state.skills.filter { it.builtIn }
        val userSkills = state.skills.filterNot { it.builtIn }
        item {
            SkillSection(
                title = stringResource(R.string.settings_skills_builtin),
                emptyText = stringResource(R.string.settings_skills_builtin_empty),
                skills = builtInSkills,
                store = store,
                onOpenSkill = onOpenSkill,
            )
        }
        item {
            SkillSection(
                title = stringResource(R.string.settings_skills_user),
                emptyText = stringResource(R.string.settings_skills_user_empty),
                skills = userSkills,
                store = store,
                onOpenSkill = onOpenSkill,
            )
        }
        item {
            HamburSection(title = stringResource(R.string.settings_section_actions)) {
                SecondaryActionButton(text = stringResource(R.string.settings_refresh), onClick = store::refreshKnowledgeSnapshots)
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
            title = stringResource(R.string.settings_skill_delete_title),
            text = stringResource(
                R.string.settings_skill_delete_message,
                selectedSkill?.name ?: skillId,
            ),
            confirmText = stringResource(R.string.common_delete),
            onConfirm = {
                showDeleteDialog = false
                store.deleteSkill(skillId)
                onBack()
            },
            onDismiss = { showDeleteDialog = false },
        )
    }
    SettingsPage(title = stringResource(R.string.settings_skill_detail_title), subtitle = skillId, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_skill_section)) {
                SummaryLine(label = stringResource(R.string.settings_provider_name), value = selectedSkill?.name ?: skillId)
                SummaryLine(label = stringResource(R.string.settings_skill_path), value = selectedSkill?.path ?: skillId)
                SummaryLine(label = stringResource(R.string.settings_skill_created), value = selectedSkill?.createdAtMs?.toDateTimeText().orEmpty())
                SummaryLine(label = stringResource(R.string.settings_skill_modified), value = selectedSkill?.modifiedAtMs?.toDateTimeText().orEmpty())
                CapabilitySwitch(stringResource(R.string.common_enabled), selectedSkill?.enabled ?: true) { checked ->
                    store.setSkillEnabled(skillId, checked)
                }
                SecondaryActionButton(text = stringResource(R.string.settings_skill_delete_title), onClick = { showDeleteDialog = true })
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_section_description)) {
                Text(
                    text = selectedSkill?.description.orEmpty().ifBlank { stringResource(R.string.settings_skill_no_description) },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_skill_files)) {
                val files = selectedSkill?.files.orEmpty()
                if (files.isEmpty()) {
                    Text(stringResource(R.string.settings_skill_no_files), color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    files.forEach { file ->
                        Text(file, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_skill_content)) {
                Text(
                    text = detail?.content ?: stringResource(R.string.settings_skill_loading),
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
    SettingsPage(title = stringResource(R.string.settings_memory_title), onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_memory_files_section)) {
                if (state.memoryFiles.isEmpty()) {
                    Text(stringResource(R.string.settings_memory_files_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
                } else {
                    state.memoryFiles.forEach { file ->
                        MemoryListRow(
                            file.name,
                            file.preview.ifBlank { stringResource(R.string.settings_memory_entries_count, file.entryCount.toString()) },
                            onOpenMemory,
                        )
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
    SettingsPage(title = stringResource(R.string.settings_memory_detail_title), subtitle = memoryKey, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_memory_file_section)) {
                SummaryLine(
                    label = stringResource(R.string.settings_memory_size),
                    value = stringResource(R.string.settings_memory_bytes, (detail?.sizeBytes ?: 0UL).toString()),
                )
                SummaryLine(label = stringResource(R.string.settings_memory_entries), value = "${detail?.entryCount ?: 0u}")
                Text(
                    text = detail?.content ?: stringResource(R.string.settings_memory_loading),
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
    onOpenWebTools: () -> Unit,
) {
    SettingsPage(title = stringResource(R.string.settings_tools_title), onBack = onBack) {
        item {
            HamburSection(
                title = stringResource(R.string.settings_tools_web_section),
                subtitle = stringResource(R.string.settings_tools_web_section_subtitle),
            ) {
                val search = state.webSearchSettings()
                SettingsNavigationRow(
                    icon = Lucide.Globe,
                    title = stringResource(R.string.settings_tools_web_search_fetch),
                    summary = stringResource(
                        R.string.settings_tools_web_summary,
                        search.provider,
                        stringResource(if (state.webFetchSettings().enabled) R.string.settings_on else R.string.settings_off),
                    ),
                    onClick = onOpenWebTools,
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_tools_builtin_section)) {
                listOf(
                    "skills_list" to stringResource(R.string.settings_tool_desc_skills_list),
                    "skill_view" to stringResource(R.string.settings_tool_desc_skill_view),
                    "terminal" to stringResource(R.string.settings_tool_desc_terminal),
                    "process" to stringResource(R.string.settings_tool_desc_process),
                    "read_file" to stringResource(R.string.settings_tool_desc_read_file),
                    "write_file" to stringResource(R.string.settings_tool_desc_write_file),
                    "patch" to stringResource(R.string.settings_tool_desc_patch),
                    "search_files" to stringResource(R.string.settings_tool_desc_search_files),
                    "hambur_config" to stringResource(R.string.settings_tool_desc_hambur_config),
                    "web_search" to stringResource(R.string.settings_tool_desc_web_search),
                    "web_fetch" to stringResource(R.string.settings_tool_desc_web_fetch),
                    "browser_use" to stringResource(R.string.settings_tool_desc_browser_use),
                    "android_cli" to stringResource(R.string.settings_tool_desc_android_cli),
                    "session_search" to stringResource(R.string.settings_tool_desc_session_search),
                    "memory" to stringResource(R.string.settings_tool_desc_memory),
                    "delegate_task" to stringResource(R.string.settings_tool_desc_delegate_task),
                    "view_image" to stringResource(R.string.settings_tool_desc_view_image),
                ).forEach { (name, description) ->
                    ToolListRow(
                        name = name,
                        description = description,
                        enabled = state.settingValue("tool_enabled:$name", "true") == "true",
                        onOpenTool = onOpenTool,
                    )
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
    val viewImageScaleMode = state.settingValue("viewImageScaleMode", "resize_fit")
    val browserSettings = state.browserToolSettings()
    val toolEnabled = state.settingValue("tool_enabled:$toolName", "true") == "true"
    SettingsPage(title = stringResource(R.string.settings_tool_detail_title), subtitle = toolName, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_tool_status_section)) {
                SettingsSwitchRow(
                    title = stringResource(R.string.common_enabled),
                    summary = if (toolEnabled) stringResource(R.string.settings_tool_enabled_summary)
                    else stringResource(R.string.settings_tool_disabled_summary),
                    checked = toolEnabled,
                    onCheckedChange = {
                        store.saveRawAppSetting("tool_enabled:$toolName", it.toString())
                    },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_section_description)) {
                SummaryLine(label = stringResource(R.string.settings_provider_name), value = toolName)
                SummaryLine(
                    label = stringResource(R.string.settings_tool_backend),
                    value = when (toolName) {
                        "browser_use" -> "AndroidPlatformAdapter browser actions"
                        "android_cli" -> "AndroidPlatformAdapter native device CLI"
                        else -> "Rust tool registry"
                    }
                )
            }
        }
        if (toolName == "view_image") {
            item {
                HamburSection(title = stringResource(R.string.settings_section_configuration)) {
                    ChoiceRow(
                        title = stringResource(R.string.settings_tool_view_image_scale),
                        current = viewImageScaleMode,
                        options = listOf(
                            "resize_fit" to stringResource(R.string.settings_tool_scale_resize_fit),
                            "original" to stringResource(R.string.settings_tool_scale_original),
                        ),
                        onSelect = { store.saveRawAppSetting("viewImageScaleMode", it) },
                    )
                }
            }
        }
        if (toolName == "browser_use") {
            item {
                HamburSection(title = stringResource(R.string.settings_tool_browser_section)) {
                    SettingsSwitchRow(
                        title = stringResource(R.string.settings_tool_accept_cookies),
                        summary = if (browserSettings.acceptCookies) stringResource(R.string.settings_tool_accept_cookies_on) else stringResource(R.string.settings_tool_accept_cookies_off),
                        checked = browserSettings.acceptCookies,
                        onCheckedChange = {
                            store.saveBrowserToolSettings(browserSettings.copy(acceptCookies = it, acceptThirdPartyCookies = it && browserSettings.acceptThirdPartyCookies).toJson())
                        },
                    )
                    SettingsSwitchRow(
                        title = stringResource(R.string.settings_tool_third_party_cookies),
                        summary = if (browserSettings.acceptThirdPartyCookies) stringResource(R.string.settings_tool_third_party_cookies_on) else stringResource(R.string.settings_tool_third_party_cookies_off),
                        checked = browserSettings.acceptCookies && browserSettings.acceptThirdPartyCookies,
                        enabled = browserSettings.acceptCookies,
                        onCheckedChange = {
                            store.saveBrowserToolSettings(browserSettings.copy(acceptThirdPartyCookies = it).toJson())
                        },
                    )
                    ChoiceRow(
                        title = stringResource(R.string.settings_tool_fetch_download_limit),
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
                        title = stringResource(R.string.settings_tool_idle_auto_close),
                        current = browserSettings.autoCloseMinutes.toString(),
                        options = listOf(
                            "0" to stringResource(R.string.settings_never),
                            "5" to stringResource(R.string.settings_minutes_count, 5),
                            "15" to stringResource(R.string.settings_minutes_count, 15),
                            "30" to stringResource(R.string.settings_minutes_count, 30),
                            "60" to stringResource(R.string.settings_minutes_count, 60),
                            "120" to stringResource(R.string.settings_minutes_count, 120),
                        ),
                        onSelect = {
                            store.saveBrowserToolSettings(browserSettings.copy(autoCloseMinutes = it.toInt()).toJson())
                        },
                    )
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_tool_settings_section)) {
                OutlinedTextField(value = toolSettings, onValueChange = { toolSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("tool_settings") })
                Button(onClick = { store.saveAppSetting("tool_settings", toolSettings, false) }) {
                    Text(stringResource(R.string.settings_tool_save))
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
    SettingsPage(title = stringResource(R.string.settings_startup_tasks_title), onBack = onBack) {
        item {
            Button(onClick = onNewTask) {
                Text(stringResource(R.string.settings_startup_task_new))
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_startup_tasks_section)) {
                if (tasks.isEmpty()) {
                    Text(stringResource(R.string.settings_startup_tasks_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    SecondaryActionButton(text = stringResource(R.string.settings_startup_task_open_default), onClick = { onOpenTask("startup-default") })
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
            title = stringResource(R.string.settings_startup_task_delete_title),
            text = stringResource(R.string.settings_startup_task_delete_message),
            confirmText = stringResource(R.string.common_delete),
            onConfirm = {
                store.deleteStartupTask(id, true)
                pendingDelete = false
                onBack()
            },
            onDismiss = { pendingDelete = false },
        )
    }

    SettingsPage(title = stringResource(R.string.settings_startup_task_detail_title), subtitle = id, onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_startup_task_section)) {
                OutlinedTextField(value = id, onValueChange = { id = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_startup_task_id)) })
                OutlinedTextField(value = payload, onValueChange = { payload = it }, modifier = Modifier.fillMaxWidth(), minLines = 8, label = { Text(stringResource(R.string.settings_startup_task_json)) })
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.saveStartupTask(id, payload, true) }, enabled = id.isNotBlank() && payload.isNotBlank()) {
                        Text(stringResource(R.string.settings_startup_task_approve_save))
                    }
                    SecondaryActionButton(text = stringResource(R.string.common_delete), enabled = id.isNotBlank(), onClick = { pendingDelete = true })
                }
            }
        }
    }
}

/** 语言选项的显示名。未知标签按「跟随系统」显示，正常不会走到。 */
@StringRes
private fun languageLabelRes(tag: String): Int = when (tag) {
    HamburAppLocale.English -> R.string.appearance_language_english
    HamburAppLocale.SimplifiedChinese -> R.string.appearance_language_chinese
    else -> R.string.appearance_language_system
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
    val thinkingBlockMode = state.settingValue(
        THINKING_BLOCK_DISPLAY_MODE_KEY,
        THINKING_BLOCK_DISPLAY_AUTO_EXPAND,
    )
    val focusInputAfterReply = state.settingValue(AUTO_FOCUS_INPUT_AFTER_REPLY_KEY, "false") == "true"
    // 语言不在 app_settings 里：它由 AppCompat/系统保存（见 HamburAppLocale）。
    // 下面的 stringResource 已经订阅了 Configuration，所以切语言时这里会跟着重组重读。
    val languageTag = HamburAppLocale.current()

    SettingsPage(title = stringResource(R.string.appearance_title), onBack = onBack) {
        item {
            HamburSection(
                title = stringResource(R.string.appearance_language_section),
                subtitle = stringResource(R.string.appearance_language_summary),
            ) {
                ChoiceRow(
                    title = stringResource(R.string.appearance_language_title),
                    current = languageTag,
                    // 选项列表以 HamburAppLocale.options 为准（和 res/xml/locales_config.xml 对齐），
                    // 这里只负责把值映射成显示名。
                    options = HamburAppLocale.options.map { it to stringResource(languageLabelRes(it)) },
                    onSelect = { HamburAppLocale.apply(it) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_theme)) {
                ChoiceRow(
                    title = stringResource(R.string.appearance_theme_title),
                    current = themeMode,
                    options = listOf(
                        "system" to stringResource(R.string.appearance_theme_system),
                        "light" to stringResource(R.string.appearance_theme_light),
                        "dark" to stringResource(R.string.appearance_theme_dark),
                    ),
                    onSelect = { store.saveRawAppSetting("themeMode", it) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_text)) {
                ChoiceRow(
                    title = stringResource(R.string.appearance_font_scale_title),
                    current = fontScale,
                    options = listOf(
                        "small" to stringResource(R.string.appearance_font_scale_small),
                        "default" to stringResource(R.string.appearance_font_scale_default),
                        "large" to stringResource(R.string.appearance_font_scale_large),
                        "extra_large" to stringResource(R.string.appearance_font_scale_extra_large),
                    ),
                    onSelect = { store.saveRawAppSetting("fontScale", it) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_startup)) {
                ChoiceRow(
                    title = stringResource(R.string.appearance_startup_title),
                    current = startupChatMode,
                    options = listOf(
                        "last_chat" to stringResource(R.string.appearance_startup_last_chat),
                        "new_chat" to stringResource(R.string.appearance_startup_new_chat),
                    ),
                    onSelect = { store.saveRawAppSetting("startupChatMode", it) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_thinking)) {
                ChoiceRow(
                    title = stringResource(R.string.appearance_thinking_title),
                    current = thinkingBlockMode,
                    options = listOf(
                        THINKING_BLOCK_DISPLAY_AUTO_EXPAND to stringResource(R.string.appearance_thinking_auto_expand),
                        THINKING_BLOCK_DISPLAY_COLLAPSED to stringResource(R.string.appearance_thinking_collapsed),
                        THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE to stringResource(R.string.appearance_thinking_auto_collapse),
                    ),
                    onSelect = { store.saveRawAppSetting(THINKING_BLOCK_DISPLAY_MODE_KEY, it) },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_input)) {
                SettingsSwitchRow(
                    title = stringResource(R.string.appearance_focus_after_reply_title),
                    summary = stringResource(R.string.appearance_focus_after_reply_summary),
                    checked = focusInputAfterReply,
                    onCheckedChange = {
                        store.saveRawAppSetting(AUTO_FOCUS_INPUT_AFTER_REPLY_KEY, it.toString())
                    },
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.appearance_section_current)) {
                SummaryLine(label = "language", value = languageTag)
                SummaryLine(label = "themeMode", value = themeMode)
                SummaryLine(label = "fontScale", value = fontScale)
                SummaryLine(label = "startupChatMode", value = startupChatMode)
                SummaryLine(label = "thinkingBlockDisplayMode", value = thinkingBlockMode)
                SummaryLine(label = AUTO_FOCUS_INPUT_AFTER_REPLY_KEY, value = focusInputAfterReply.toString())
            }
        }
    }
}

@Composable
fun PersonaSettingsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    val savedPersona = state.settingValue(PERSONA_SETTING_KEY, "").trim()
    var draft by rememberSaveable { mutableStateOf(savedPersona) }
    var loadedPersona by rememberSaveable { mutableStateOf(savedPersona) }
    var lastCommandSequence by rememberSaveable { mutableStateOf(0L) }
    val context = LocalContext.current
    val trimmedDraft = draft.trim()
    // 后端按 UTF-8 字节数限长，Kotlin String.length 是 UTF-16 code unit，非 ASCII 会低估。
    val draftBytes = trimmedDraft.toByteArray(Charsets.UTF_8).size
    val dirty = trimmedDraft != loadedPersona
    val tooLong = draftBytes > PERSONA_BYTE_LIMIT

    // 后端值变了（保存成功，或 agent 通过 hambur_config 改了设置）就同步到编辑框，
    // 但本地有未保存修改时不覆盖，避免把用户正在写的内容冲掉。
    LaunchedEffect(savedPersona) {
        if (trimmedDraft == loadedPersona) {
            draft = savedPersona
            loadedPersona = savedPersona
        }
    }

    LaunchedEffect(state.lastAppliedSequence) {
        if (lastCommandSequence > 0L && state.lastAppliedSequence.toLong() > lastCommandSequence) {
            if (state.latestEventKind == BackendEventKind.SETTINGS_CHANGED.name) {
                // 只把"已保存内容"对齐到刚提交的文本，编辑框保持用户输入不动：
                // 设置快照刷新可能晚一拍到达，这里不依赖它，避免把草稿冲掉。
                loadedPersona = trimmedDraft
                Toast.makeText(context, context.getString(R.string.settings_toast_persona_saved), Toast.LENGTH_SHORT).show()
                lastCommandSequence = 0L
            } else if (state.runtimeStatus == "Error") {
                Toast.makeText(context, context.getString(R.string.settings_toast_save_failed, state.footer), Toast.LENGTH_LONG).show()
                lastCommandSequence = 0L
            }
        }
    }

    SettingsPage(title = stringResource(R.string.settings_persona_title), subtitle = "SOUL.md", onBack = onBack) {
        item {
            HamburSection(
                title = "SOUL.md",
                subtitle = stringResource(R.string.settings_persona_section_subtitle),
            ) {
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 10,
                    label = { Text(stringResource(R.string.settings_persona_title)) },
                    placeholder = {
                        Text(stringResource(R.string.settings_persona_placeholder))
                    },
                    isError = tooLong,
                )
                SummaryLine(label = stringResource(R.string.settings_persona_length), value = "$draftBytes / $PERSONA_BYTE_LIMIT")
                if (tooLong) {
                    Text(
                        text = stringResource(R.string.settings_persona_too_long, PERSONA_BYTE_LIMIT),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    SecondaryActionButton(
                        text = stringResource(R.string.common_reload),
                        enabled = dirty,
                        onClick = {
                            draft = savedPersona
                            loadedPersona = savedPersona
                        },
                    )
                    SecondaryActionButton(
                        text = stringResource(R.string.common_save),
                        enabled = dirty && !tooLong,
                        onClick = {
                            lastCommandSequence = state.lastAppliedSequence.toLong()
                            Toast.makeText(context, context.getString(R.string.settings_toast_persona_saving), Toast.LENGTH_SHORT).show()
                            store.savePersona(draft)
                        },
                    )
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_persona_how_section)) {
                SummaryLine(label = stringResource(R.string.settings_persona_stored_as), value = PERSONA_SETTING_KEY)
                SummaryLine(label = stringResource(R.string.settings_persona_applied_to), value = stringResource(R.string.settings_persona_applied_to_value))
                Text(
                    text = stringResource(R.string.settings_persona_how_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
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
            HamburSection(title = stringResource(R.string.settings_json_payload_section)) {
                OutlinedTextField(
                    value = value,
                    onValueChange = { value = it },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 7,
                    maxLines = 14,
                    label = { Text(settingKey) },
                )
                Button(onClick = { store.saveAppSetting(settingKey, value, false) }, enabled = value.isNotBlank()) {
                    Text(stringResource(R.string.common_save))
                }
            }
        }
        if (settingKey == "skills") {
            item {
                HamburSection(title = stringResource(R.string.settings_json_skill_flag_section)) {
                    OutlinedTextField(value = skillId, onValueChange = { skillId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_skill_id)) })
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Switch(checked = skillEnabled, onCheckedChange = { skillEnabled = it })
                        Text(stringResource(R.string.common_enabled), modifier = Modifier.padding(start = 8.dp))
                    }
                    SecondaryActionButton(
                        text = stringResource(R.string.settings_json_skill_flag_save),
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
    val webSearchStored = state.webSearchSettings()
    val webFetchStored = state.webFetchSettings()
    var webSearchProvider by rememberSaveable { mutableStateOf(webSearchStored.provider) }
    var webSearchEndpoint by rememberSaveable { mutableStateOf(webSearchStored.endpoint) }
    var webSearchModel by rememberSaveable { mutableStateOf(webSearchStored.model) }
    var webSearchMaxResults by rememberSaveable { mutableStateOf(webSearchStored.maxResults.toString()) }
    var webSearchMaxQueries by rememberSaveable { mutableStateOf(webSearchStored.maxQueries.toString()) }
    var webSearchMaxUses by rememberSaveable { mutableStateOf(webSearchStored.maxUses.toString()) }
    var webFetchEnabled by rememberSaveable { mutableStateOf(webFetchStored.enabled) }
    var webFetchAllowPrivateHosts by rememberSaveable { mutableStateOf(webFetchStored.allowPrivateHosts) }
    var webFetchMaxChars by rememberSaveable { mutableStateOf(webFetchStored.maxChars.toString()) }
    var webSearchJson by rememberSaveable { mutableStateOf(state.settingValue("web_search_settings", DEFAULT_WEB_SEARCH_SETTINGS_JSON)) }
    var webFetchJson by rememberSaveable { mutableStateOf(state.settingValue("web_fetch_settings", DEFAULT_WEB_FETCH_SETTINGS_JSON)) }
    var deepseekSearchKey by remember { mutableStateOf("") }
    var exaSearchKey by remember { mutableStateOf("") }
    val context = LocalContext.current

    // 表单值盖在已存配置上，未暴露的字段（apiVersion/maxTokens/apiKeyRef/Exa 等）原样回写。
    val webSearchForm = webSearchStored.copy(
        provider = webSearchProvider,
        endpoint = webSearchEndpoint,
        model = webSearchModel,
        maxResults = webSearchMaxResults.trim().toIntOrNull() ?: webSearchStored.maxResults,
        maxQueries = webSearchMaxQueries.trim().toIntOrNull() ?: webSearchStored.maxQueries,
        maxUses = webSearchMaxUses.trim().toIntOrNull() ?: webSearchStored.maxUses,
    )
    val webFetchForm = webFetchStored.copy(
        enabled = webFetchEnabled,
        allowPrivateHosts = webFetchAllowPrivateHosts,
        maxChars = webFetchMaxChars.trim().toIntOrNull() ?: webFetchStored.maxChars,
    )

    SettingsPage(title = stringResource(R.string.settings_tools_title), onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_tool_settings_section)) {
                OutlinedTextField(value = toolSettings, onValueChange = { toolSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("tool_settings") })
                Button(onClick = { store.saveAppSetting("tool_settings", toolSettings, false) }) {
                    Text(stringResource(R.string.settings_tool_save))
                }
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_tools_browser_settings_section)) {
                OutlinedTextField(value = browserSettings, onValueChange = { browserSettings = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("browser_tool_settings") })
                SecondaryActionButton(text = stringResource(R.string.settings_tools_save_browser), onClick = { store.saveBrowserToolSettings(browserSettings) })
            }
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_tools_web_search_section),
                subtitle = stringResource(R.string.settings_tools_web_search_subtitle),
            ) {
                ChoiceRow(
                    title = stringResource(R.string.settings_model_provider),
                    current = webSearchProvider,
                    options = listOf(
                        "auto" to stringResource(R.string.settings_tools_provider_auto),
                        "deepseek-official" to stringResource(R.string.settings_tools_provider_deepseek),
                        "exa" to stringResource(R.string.settings_tools_provider_exa),
                        "off" to stringResource(R.string.settings_tools_provider_off),
                    ),
                    onSelect = { webSearchProvider = it },
                )
                OutlinedTextField(value = webSearchEndpoint, onValueChange = { webSearchEndpoint = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_tools_endpoint)) })
                OutlinedTextField(value = webSearchModel, onValueChange = { webSearchModel = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_model_section)) })
                OutlinedTextField(value = webSearchMaxResults, onValueChange = { webSearchMaxResults = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_tools_max_results)) })
                OutlinedTextField(value = webSearchMaxQueries, onValueChange = { webSearchMaxQueries = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_tools_max_queries)) })
                OutlinedTextField(value = webSearchMaxUses, onValueChange = { webSearchMaxUses = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_tools_max_uses)) })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_web_search),
                    enabled = webSearchEndpoint.isNotBlank() && webSearchModel.isNotBlank(),
                    onClick = {
                        val json = webSearchForm.toJson()
                        store.saveAppSetting("web_search_settings", json, false)
                        webSearchJson = json
                    },
                )
            }
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_tools_web_search_keys_section),
                subtitle = stringResource(R.string.settings_tools_web_search_keys_subtitle),
            ) {
                OutlinedTextField(value = deepseekSearchKey, onValueChange = { deepseekSearchKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, visualTransformation = PasswordVisualTransformation(), label = { Text(stringResource(R.string.settings_tools_deepseek_key, WEB_SEARCH_DEEPSEEK_SECRET_REF)) })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_deepseek_key),
                    enabled = deepseekSearchKey.isNotBlank(),
                    onClick = {
                        if (store.saveWebSearchSecret(WEB_SEARCH_DEEPSEEK_SECRET_REF, deepseekSearchKey)) {
                            // apiKeyRef 为空时说明还在复用官方 DeepSeek provider 的密钥，
                            // 这时把引用指向刚写入的 Secret；用户手填过引用就不动它。
                            if (webSearchStored.apiKeyRef.isBlank()) {
                                val json = webSearchForm.copy(apiKeyRef = WEB_SEARCH_DEEPSEEK_SECRET_REF).toJson()
                                store.saveAppSetting("web_search_settings", json, false)
                                webSearchJson = json
                            }
                            deepseekSearchKey = ""
                            Toast.makeText(context, context.getString(R.string.settings_toast_deepseek_key_saved), Toast.LENGTH_SHORT).show()
                        } else {
                            Toast.makeText(context, context.getString(R.string.settings_toast_deepseek_key_save_failed), Toast.LENGTH_LONG).show()
                        }
                    },
                )
                OutlinedTextField(value = exaSearchKey, onValueChange = { exaSearchKey = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, visualTransformation = PasswordVisualTransformation(), label = { Text(stringResource(R.string.settings_tools_exa_key, WEB_SEARCH_EXA_SECRET_REF)) })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_exa_key),
                    enabled = exaSearchKey.isNotBlank(),
                    onClick = {
                        if (store.saveWebSearchSecret(WEB_SEARCH_EXA_SECRET_REF, exaSearchKey)) {
                            exaSearchKey = ""
                            Toast.makeText(context, context.getString(R.string.settings_toast_exa_key_saved), Toast.LENGTH_SHORT).show()
                        } else {
                            Toast.makeText(context, context.getString(R.string.settings_toast_exa_key_save_failed), Toast.LENGTH_LONG).show()
                        }
                    },
                )
            }
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_tools_web_fetch_section),
                subtitle = stringResource(R.string.settings_tools_web_fetch_subtitle),
            ) {
                SettingsSwitchRow(
                    title = stringResource(R.string.common_enabled),
                    summary = if (webFetchEnabled) stringResource(R.string.settings_tools_web_fetch_on) else stringResource(R.string.settings_tool_disabled_summary),
                    checked = webFetchEnabled,
                    onCheckedChange = { webFetchEnabled = it },
                )
                SettingsSwitchRow(
                    title = stringResource(R.string.settings_tools_allow_private_hosts),
                    summary = if (webFetchAllowPrivateHosts) stringResource(R.string.settings_tools_allow_private_hosts_on) else stringResource(R.string.settings_tools_allow_private_hosts_off),
                    checked = webFetchAllowPrivateHosts,
                    onCheckedChange = { webFetchAllowPrivateHosts = it },
                )
                OutlinedTextField(value = webFetchMaxChars, onValueChange = { webFetchMaxChars = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_tools_max_chars)) })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_web_fetch),
                    onClick = {
                        val json = webFetchForm.toJson()
                        store.saveAppSetting("web_fetch_settings", json, false)
                        webFetchJson = json
                    },
                )
            }
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_tools_raw_json_section),
                subtitle = stringResource(R.string.settings_tools_raw_json_subtitle),
            ) {
                OutlinedTextField(value = webSearchJson, onValueChange = { webSearchJson = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("web_search_settings") })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_raw_web_search),
                    enabled = webSearchJson.isNotBlank(),
                    onClick = { store.saveAppSetting("web_search_settings", webSearchJson, false) },
                )
                OutlinedTextField(value = webFetchJson, onValueChange = { webFetchJson = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text("web_fetch_settings") })
                SecondaryActionButton(
                    text = stringResource(R.string.settings_tools_save_raw_web_fetch),
                    enabled = webFetchJson.isNotBlank(),
                    onClick = { store.saveAppSetting("web_fetch_settings", webFetchJson, false) },
                )
            }
        }
        item {
            CurrentSettingsSection(state = state, prefix = "tool")
        }
        item {
            CurrentSettingsSection(state = state, prefix = "web")
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
            title = stringResource(R.string.settings_startup_task_delete_title),
            text = stringResource(R.string.settings_startup_task_delete_message),
            confirmText = stringResource(R.string.common_delete),
            onConfirm = {
                store.deleteStartupTask(pendingDelete, true)
                pendingDelete = ""
            },
            onDismiss = { pendingDelete = "" },
        )
    }

    SettingsPage(title = stringResource(R.string.settings_startup_tasks_title), onBack = onBack) {
        item {
            HamburSection(
                title = stringResource(R.string.settings_startup_task_editor_section),
                subtitle = stringResource(R.string.settings_startup_task_editor_subtitle),
            ) {
                OutlinedTextField(value = taskId, onValueChange = { taskId = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_startup_task_id)) })
                OutlinedTextField(value = payload, onValueChange = { payload = it }, modifier = Modifier.fillMaxWidth(), minLines = 6, label = { Text(stringResource(R.string.settings_startup_task_json)) })
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.saveStartupTask(taskId, payload, true) }, enabled = taskId.isNotBlank() && payload.isNotBlank()) {
                        Text(stringResource(R.string.settings_startup_task_approve_save))
                    }
                    SecondaryActionButton(text = stringResource(R.string.common_delete), enabled = taskId.isNotBlank(), onClick = { pendingDelete = taskId })
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
            title = { Text(stringResource(R.string.settings_rootfs_reset_dialog_title)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(stringResource(R.string.settings_rootfs_reset_dialog_message))
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(stringResource(R.string.settings_rootfs_reset_preserve), modifier = Modifier.weight(1f))
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
                    Text(stringResource(R.string.settings_reset))
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmReset = false }) {
                    Text(stringResource(R.string.common_cancel))
                }
            }
        )
    }

    SettingsPage(title = stringResource(R.string.settings_rootfs_screen_title), onBack = onBack) {
        item {
            val status = state.rootfsStatus
            HamburSection(title = stringResource(R.string.settings_rootfs_status_section)) {
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_version),
                    value = status?.let {
                        if (it.rootfsInstalled) {
                            it.version.ifBlank { stringResource(R.string.common_unknown) }
                        } else {
                            stringResource(R.string.settings_rootfs_uninitialized)
                        }
                    } ?: stringResource(R.string.common_unknown)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_online),
                    value = status?.let { stringResource(R.string.settings_rootfs_configured) }
                        ?: stringResource(R.string.settings_rootfs_not_detected)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_storage_used),
                    value = status?.rootfsSizeBytes?.toLong()?.toReadableSize() ?: stringResource(R.string.common_unknown)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_root_access),
                    value = status?.let { if (it.rootAvailable) stringResource(R.string.common_yes) else stringResource(R.string.common_no) }
                        ?: stringResource(R.string.common_unknown)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_chroot_status),
                    value = status?.let { if (it.chrootAvailable) stringResource(R.string.common_yes) else stringResource(R.string.common_no) }
                        ?: stringResource(R.string.common_unknown)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_proot_status),
                    value = status?.let { if (it.prootAvailable) stringResource(R.string.common_yes) else stringResource(R.string.common_no) }
                        ?: stringResource(R.string.common_unknown)
                )
                SummaryLine(
                    label = stringResource(R.string.settings_rootfs_browse_path),
                    value = status?.rootfsPath ?: stringResource(R.string.settings_rootfs_uninitialized)
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_rootfs_backend_section)) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(stringResource(R.string.settings_rootfs_current_backend), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                    
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
            HamburSection(title = stringResource(R.string.settings_rootfs_control_section)) {
                SummaryLine(label = stringResource(R.string.settings_rootfs_current_session), value = state.selectedSessionId)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { store.runRootfsWarmup(state.selectedSessionId) }) {
                        Text(stringResource(R.string.settings_rootfs_warmup))
                    }
                    SecondaryActionButton(text = stringResource(R.string.settings_reset), onClick = { confirmReset = true })
                }
                Text(
                    text = state.footer,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_rootfs_advanced_section)) {
                OutlinedTextField(value = key, onValueChange = { key = it }, modifier = Modifier.fillMaxWidth(), singleLine = true, label = { Text(stringResource(R.string.settings_rootfs_config_key)) })
                OutlinedTextField(value = value, onValueChange = { value = it }, modifier = Modifier.fillMaxWidth(), minLines = 5, label = { Text(stringResource(R.string.settings_rootfs_config_value)) })
                Button(onClick = { store.saveRootfsSetting(key, value, true) }, enabled = key.isNotBlank() && value.isNotBlank()) {
                    Text(stringResource(R.string.settings_rootfs_save_apply))
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
                StatusPill(text = if (provider.enabled) stringResource(R.string.common_enabled) else stringResource(R.string.common_disabled), active = provider.enabled)
            }
            SummaryLine(label = stringResource(R.string.settings_id), value = provider.id)
            SummaryLine(label = stringResource(R.string.settings_provider_base_url), value = provider.baseUrl)
            SummaryLine(label = stringResource(R.string.settings_provider_secret), value = provider.secretLabel)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryActionButton(text = stringResource(R.string.common_edit), onClick = onUse)
                TextButton(onClick = onDelete) {
                    Text(stringResource(R.string.common_delete), color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun ProviderModelsSection(models: List<UiProviderModelSettings>) {
    HamburSection(
        title = stringResource(R.string.settings_provider_models_section),
        subtitle = stringResource(R.string.settings_provider_models_section_subtitle),
    ) {
        if (models.isEmpty()) {
            Text(stringResource(R.string.settings_provider_models_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            models.take(20).forEach { model ->
                SummaryLine(
                    label = model.modelId,
                    value = stringResource(
                        R.string.settings_provider_model_summary,
                        model.contextLimit.toString(),
                        model.outputLimit.toString(),
                        model.supportsToolCall.toString(),
                        model.supportsImageInput.toString(),
                    ),
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
    HamburSection(title = stringResource(R.string.settings_current_settings_section)) {
        val settings = state.appSettings.filter { it.key.startsWith(prefix) || it.key.contains(prefix) }
        if (settings.isEmpty()) {
            Text(stringResource(R.string.settings_current_settings_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            settings.forEach { setting ->
                SummaryLine(label = setting.key, value = setting.value)
            }
        }
    }
}

@Composable
private fun ConfigAuditPreview(configAudits: List<UiConfigAudit>) {
    HamburSection(
        title = stringResource(R.string.settings_config_audit_section),
        subtitle = stringResource(R.string.settings_config_audit_subtitle),
    ) {
        if (configAudits.isEmpty()) {
            Text(stringResource(R.string.settings_config_audit_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
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

/**
 * 「后端还没暴露这个功能」的占位页类型。
 *
 * 以前这个页面拿标题字面量做分支（`when (title) { "Logs" -> ... }`），文案外提之后 title 是
 * 本地化字符串——中文下 "Logs" 变成「日志」，分支会全部落空只剩兜底文案。所以改成枚举：
 * 标题只用来显示，分支走类型。
 */
enum class BackendGapFeature(
    @StringRes val titleRes: Int,
    @StringRes val summaryRes: Int,
) {
    EnvironmentVariables(
        R.string.app_environment_variables_title,
        R.string.app_environment_variables_summary,
    ),
}

@Composable
fun FeatureUnavailableScreen(
    feature: BackendGapFeature,
    onBack: () -> Unit,
) {
    val title = stringResource(feature.titleRes)
    val summary = stringResource(feature.summaryRes)
    var textValue by rememberSaveable(feature) { mutableStateOf("") }
    SettingsPage(title = title, subtitle = stringResource(R.string.settings_section_waiting_backend), onBack = onBack) {
        item {
            HamburSection(title = stringResource(R.string.settings_feature_backend_gap)) {
                Text(
                    text = summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        item {
            HamburSection(title = stringResource(R.string.settings_feature_ui_surface)) {
                when (feature) {
                    BackendGapFeature.EnvironmentVariables -> {
                        OutlinedTextField(
                            value = textValue,
                            onValueChange = { textValue = it },
                            modifier = Modifier.fillMaxWidth(),
                            minLines = 8,
                            label = { Text(stringResource(R.string.settings_feature_environment)) },
                            placeholder = { Text("KEY=value") },
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            SecondaryActionButton(text = stringResource(R.string.settings_validate), enabled = false, onClick = {})
                            SecondaryActionButton(text = stringResource(R.string.common_save), enabled = false, onClick = {})
                        }
                    }
                }
                Text(
                    text = stringResource(R.string.settings_feature_local_only_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Settings → 日志 的筛选页签。 */
private enum class LogFilterTab(@StringRes val labelRes: Int) {
    All(R.string.settings_logs_tab_all),
    Jank(R.string.settings_logs_tab_jank),
    Warnings(R.string.settings_logs_tab_warnings),
    ;

    fun matches(entry: DiagLogEntry): Boolean = when (this) {
        All -> true
        Jank -> entry.source == DiagSource.JANK
        Warnings -> entry.level.rank >= DiagLevel.WARN.rank
    }
}

/**
 * Settings → 日志：应用内诊断日志查看器。
 *
 * 数据来自进程内环形缓冲区（[com.hambur.chat.diagnostics.DiagnosticLogs]），包含三类来源：
 * 应用日志、会话切换卡顿跟踪（[com.hambur.chat.perf.ChatJankTracer]）、Rust 结构化 tracing。
 * 「清空日志」只清这个内存缓冲区，不写数据库也不删聊天数据；记录开关经
 * app_settings.loggingEnabled 落库，同时交给 Rust 侧决定是否产出 tracing 条目。
 */
@Composable
fun DiagnosticsLogsScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    var tab by rememberSaveable { mutableStateOf(LogFilterTab.All) }
    // 页面存活期间轮询 Rust 侧增量；离开页面 LaunchedEffect 取消，不会后台常驻。
    LaunchedEffect(Unit) {
        while (true) {
            store.refreshDiagnostics()
            delay(1000)
        }
    }
    val entries = remember(state.diagnosticLogs, tab) {
        state.diagnosticLogs.filter(tab::matches)
    }
    SettingsPage(
        title = stringResource(R.string.settings_logs_title),
        subtitle = stringResource(R.string.settings_logs_summary),
        onBack = onBack,
    ) {
        item {
            HamburSection(title = stringResource(R.string.settings_logs_section_record)) {
                CapabilitySwitch(
                    label = stringResource(R.string.settings_logs_record_title),
                    checked = state.loggingEnabled,
                    onCheckedChange = store::setLoggingEnabled,
                )
                Text(
                    text = stringResource(R.string.settings_logs_record_summary),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        // 配置审计属于诊断信息：从主设置页移到这里，和日志放一起。
        item {
            ConfigAuditPreview(configAudits = state.configAudits)
        }
        item {
            HamburSection(
                title = stringResource(R.string.settings_logs_section_entries),
                subtitle = stringResource(R.string.settings_logs_count, entries.size),
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    LogFilterTab.entries.forEach { candidate ->
                        if (candidate == tab) {
                            Button(onClick = { tab = candidate }) {
                                Text(stringResource(candidate.labelRes))
                            }
                        } else {
                            SecondaryActionButton(
                                text = stringResource(candidate.labelRes),
                                onClick = { tab = candidate },
                            )
                        }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    SecondaryActionButton(
                        text = stringResource(R.string.settings_logs_clear),
                        enabled = state.diagnosticLogs.isNotEmpty(),
                        onClick = store::clearDiagnosticLogs,
                    )
                }
                Text(
                    text = stringResource(R.string.settings_logs_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (entries.isEmpty()) {
            item {
                Text(
                    text = stringResource(
                        if (state.loggingEnabled) {
                            R.string.settings_logs_empty
                        } else {
                            R.string.settings_logs_disabled
                        },
                    ),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            items(items = entries, key = { it.id }) { entry ->
                DiagnosticLogRow(entry)
            }
        }
    }
}

@Composable
private fun DiagnosticLogRow(entry: DiagLogEntry) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(modifier = Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                StatusPill(
                    text = entry.level.label,
                    active = entry.level.rank >= DiagLevel.WARN.rank,
                )
                Text(
                    text = entry.source.label,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = entry.tag,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                Text(
                    text = formatLogTimestamp(entry.timestampMs),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                text = entry.message,
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
            )
            if (entry.sequence > 0L) {
                Text(
                    text = "#${entry.sequence}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun formatLogTimestamp(timestampMs: Long): String {
    return Instant.ofEpochMilli(timestampMs)
        .atZone(ZoneId.systemDefault())
        .format(LogTimeFormatter)
}

/**
 * 把 token 数格式化成 1.2k / 3.4M / 5.6B 这样的紧凑文本（与用量页截图一致的风格）。
 */
internal fun formatTokenCount(value: ULong): String {
    val number = value.toDouble()
    return when {
        value >= 1_000_000_000UL -> String.format(Locale.US, "%.1fB", number / 1_000_000_000.0)
        value >= 1_000_000UL -> String.format(Locale.US, "%.1fM", number / 1_000_000.0)
        value >= 1_000UL -> String.format(Locale.US, "%.1fk", number / 1_000.0)
        else -> value.toString()
    }
}

/** 缓存命中率 = 缓存读取 / 总输入。没有输入时按 0 处理，避免除零。 */
internal fun formatCacheHitRate(cachedTokens: ULong, promptTokens: ULong): String {
    if (promptTokens == 0UL) return "0.0%"
    val rate = cachedTokens.toDouble() / promptTokens.toDouble() * 100.0
    return String.format(Locale.US, "%.1f%%", rate)
}

/** 用量页里的一行模型（标题已解析成显示名，缺失时退回 id）。 */
internal data class TokenUsageModelRow(
    val providerId: String,
    val modelId: String,
    val title: String,
    /** provider 还在配置里，但这个模型已经不在当前模型列表里。 */
    val notConfigured: Boolean,
    val group: UiTokenUsageGroup,
)

/** 用量页里按 provider 分组的一段。 */
internal data class TokenUsageProviderGroup(
    val providerId: String,
    val title: String,
    /** provider 已不在当前配置中，标题改用「未知」并排到最后。 */
    val unknownProvider: Boolean,
    val rows: List<TokenUsageModelRow>,
)

/**
 * 把 byModel 汇总按 provider 分组：已知 provider 在前，未知 provider 排最后；
 * 组内与组间都按总 token 倒序，展示名缺失时回退到 id。
 */
internal fun buildTokenUsageGroups(
    usage: UiTokenUsageSnapshot,
    providerNames: Map<String, String>,
    modelNames: Map<Pair<String, String>, String>,
): List<TokenUsageProviderGroup> {
    val groups = usage.byModel.groupBy { it.providerId }.map { (providerId, rows) ->
        val known = providerNames.containsKey(providerId)
        TokenUsageProviderGroup(
            providerId = providerId,
            title = providerNames[providerId].orEmpty().ifBlank { providerId },
            unknownProvider = !known,
            rows = rows.sortedByDescending { it.totalTokens }.map { group ->
                val displayName = modelNames[group.providerId to group.modelId].orEmpty()
                TokenUsageModelRow(
                    providerId = group.providerId,
                    modelId = group.modelId,
                    title = displayName.ifBlank { group.modelId },
                    notConfigured = known && displayName.isBlank(),
                    group = group,
                )
            },
        )
    }
    return groups.sortedWith(
        compareBy(
            { it.unknownProvider },
            { -it.rows.sumOf { row -> row.group.totalTokens.toLong() } },
            { it.title },
        ),
    )
}


/**
 * Settings → Token 用量：模仿 iOS 风格「用量」页。
 *
 * 顶部一张白色总用量卡片，下面按 provider 分组列出各模型，行右侧是「输入 / 输出」，
 * 点进去看该模型的细分。数据来自 Rust 的 `token_usage` 表（每个模型请求一行），
 * 只统计 token，不显示价格或费用——provider schema 里没有价格字段。
 */
@Composable
fun TokenUsageScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
) {
    LaunchedEffect(Unit) { store.refreshTokenUsage() }
    val usage = state.tokenUsage
    // 用量表里只有 provider/model id，展示名从已有设置快照里取，避免为展示再查一次库。
    val providerNames = remember(state.providers) {
        state.providers.associate { it.id to it.name }
    }
    val modelNames = remember(state.providerModels) {
        state.providerModels.associate { (it.providerId to it.modelId) to it.displayName }
    }
    val groups = remember(usage, providerNames, modelNames) {
        buildTokenUsageGroups(usage, providerNames, modelNames)
    }
    var detailKey by rememberSaveable { mutableStateOf("") }
    val detailRow = groups.asSequence()
        .flatMap { it.rows.asSequence() }
        .firstOrNull { "${it.providerId}|${it.modelId}" == detailKey }

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding(),
        ) {
            HamburTopBar(
                title = stringResource(R.string.settings_token_usage_title),
                onBack = onBack,
            )
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(18.dp),
            ) {
                item { TokenUsageTotalsCard(usage) }
                if (groups.isEmpty()) {
                    item {
                        Text(
                            text = stringResource(R.string.token_usage_empty),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 4.dp),
                        )
                    }
                }
                items(items = groups, key = { "provider:${it.providerId}" }) { group ->
                    TokenUsageProviderSection(
                        group = group,
                        onOpenModel = { row -> detailKey = "${row.providerId}|${row.modelId}" },
                    )
                }
            }
        }
    }

    if (detailRow != null) {
        TokenUsageModelDetailDialog(row = detailRow, onDismiss = { detailKey = "" })
    }
}

@Composable
private fun TokenUsageTotalsCard(usage: UiTokenUsageSnapshot) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = stringResource(R.string.token_usage_section_totals),
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(start = 4.dp),
        )
        UsageCard {
            UsageMetricRow(
                label = stringResource(R.string.token_usage_total_input),
                value = formatTokenCount(usage.promptTokens),
                showDivider = true,
            )
            UsageMetricRow(
                label = stringResource(R.string.token_usage_output),
                value = formatTokenCount(usage.completionTokens),
                showDivider = true,
            )
            UsageMetricRow(
                label = stringResource(R.string.token_usage_cache_read),
                value = formatTokenCount(usage.cachedTokens),
                showDivider = true,
            )
            UsageMetricRow(
                label = stringResource(R.string.token_usage_cache_hit_rate),
                value = formatCacheHitRate(usage.cachedTokens, usage.promptTokens),
                showDivider = false,
            )
        }
    }
}

@Composable
private fun TokenUsageProviderSection(
    group: TokenUsageProviderGroup,
    onOpenModel: (TokenUsageModelRow) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = if (group.unknownProvider) {
                stringResource(R.string.common_unknown)
            } else {
                group.title.uppercase(Locale.getDefault())
            },
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 4.dp),
        )
        UsageCard {
            group.rows.forEachIndexed { index, row ->
                UsageModelRow(
                    row = row,
                    showDivider = index != group.rows.lastIndex,
                    onClick = { onOpenModel(row) },
                )
            }
        }
    }
}

@Composable
private fun UsageModelRow(
    row: TokenUsageModelRow,
    showDivider: Boolean,
    onClick: () -> Unit,
) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    text = row.title,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Medium,
                )
                if (row.notConfigured) {
                    Text(
                        text = stringResource(R.string.token_usage_model_not_configured),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                text = "${formatTokenCount(row.group.promptTokens)} / " +
                    formatTokenCount(row.group.completionTokens),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Icon(
                imageVector = Lucide.ChevronRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(20.dp),
            )
        }
        if (showDivider) {
            HorizontalDivider(
                modifier = Modifier.padding(start = 16.dp),
                color = MaterialTheme.colorScheme.outlineVariant,
            )
        }
    }
}

@Composable
private fun TokenUsageModelDetailDialog(row: TokenUsageModelRow, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(R.string.common_close))
            }
        },
        title = { Text(row.title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                SummaryLine(
                    label = stringResource(R.string.token_usage_requests),
                    value = row.group.requestCount.toString(),
                )
                SummaryLine(
                    label = stringResource(R.string.token_usage_prompt),
                    value = formatTokenCount(row.group.promptTokens),
                )
                SummaryLine(
                    label = stringResource(R.string.token_usage_completion),
                    value = formatTokenCount(row.group.completionTokens),
                )
                SummaryLine(
                    label = stringResource(R.string.token_usage_total),
                    value = formatTokenCount(row.group.totalTokens),
                )
            }
        },
    )
}

/** 白色圆角卡片：截图里的分组容器。 */
@Composable
private fun UsageCard(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(content = content)
    }
}

@Composable
private fun UsageMetricRow(label: String, value: String, showDivider: Boolean) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            Text(
                text = value,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
        }
        if (showDivider) {
            HorizontalDivider(
                modifier = Modifier.padding(start = 16.dp),
                color = MaterialTheme.colorScheme.outlineVariant,
            )
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
                StatusPill(text = if (provider.enabled) stringResource(R.string.common_enabled) else stringResource(R.string.common_disabled), active = provider.enabled)
            }
            SummaryLine(label = stringResource(R.string.settings_section_models), value = stringResource(R.string.settings_provider_models_available, modelCount))
            SummaryLine(label = stringResource(R.string.settings_provider_base_url), value = provider.baseUrl)
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
            SummaryLine(label = stringResource(R.string.settings_model_id), value = model.modelId)
            SummaryLine(
                label = stringResource(R.string.settings_model_capabilities),
                value = stringResource(
                    R.string.settings_model_capabilities_summary,
                    model.supportsToolCall.toString(),
                    model.supportsReasoning.toString(),
                    model.supportsImageInput.toString(),
                ),
            )
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
            SummaryLine(label = stringResource(R.string.settings_id), value = group.id)
            SummaryLine(label = stringResource(R.string.settings_section_models), value = stringResource(R.string.settings_model_group_members_count, memberCount))
            SummaryLine(label = stringResource(R.string.settings_model_group_routing), value = "${group.routingStrategy} / ${group.fallbackPolicy}")
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
                    StatusPill(text = if (skill.builtIn) stringResource(R.string.settings_skill_badge_builtin) else stringResource(R.string.settings_skill_badge_user), active = skill.builtIn)
                }
                Text(
                    skill.description.ifBlank { stringResource(R.string.settings_skill_no_description) },
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
    enabled: Boolean,
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
            Text(
                if (enabled) name else stringResource(R.string.settings_tool_disabled_suffix, name),
                fontWeight = FontWeight.SemiBold,
                color = if (enabled) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
            )
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

private const val DEFAULT_WEB_SEARCH_SETTINGS_JSON =
    """{"provider":"auto","endpoint":"https://api.deepseek.com/anthropic/v1","model":"deepseek-chat","apiVersion":"2023-06-01","maxTokens":4096,"maxUses":5,"maxResults":8,"maxQueries":4,"apiKeyRef":"","exaEndpoint":"https://api.exa.ai","exaApiKeyRef":"android-secret://web_search/exa","exaSnippetChars":1200}"""

private const val DEFAULT_WEB_FETCH_SETTINGS_JSON =
    """{"enabled":true,"maxBytes":2097152,"timeoutMs":20000,"maxRedirects":4,"allowPrivateHosts":false,"maxChars":48000}"""

private const val WEB_SEARCH_DEEPSEEK_SECRET_REF = "android-secret://web_search/deepseek"
private const val WEB_SEARCH_EXA_SECRET_REF = "android-secret://web_search/exa"

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

private fun HamburUiState.webSearchSettings(): UiWebSearchSettings {
    return settingValue("web_search_settings", "").toUiWebSearchSettings()
}

private fun HamburUiState.webFetchSettings(): UiWebFetchSettings {
    return settingValue("web_fetch_settings", "").toUiWebFetchSettings()
}

// 形状与 Rust 侧 WebSearchSettings 一一对应；缺失字段用默认值，保存时整体回写。
private data class UiWebSearchSettings(
    val provider: String = "auto",
    val endpoint: String = "https://api.deepseek.com/anthropic/v1",
    val model: String = "deepseek-chat",
    val apiVersion: String = "2023-06-01",
    val maxTokens: Int = 4096,
    val maxUses: Int = 5,
    val maxResults: Int = 8,
    val maxQueries: Int = 4,
    val apiKeyRef: String = "",
    val exaEndpoint: String = "https://api.exa.ai",
    val exaApiKeyRef: String = WEB_SEARCH_EXA_SECRET_REF,
    val exaSnippetChars: Int = 1200,
) {
    fun normalized(): UiWebSearchSettings {
        return copy(
            provider = provider.trim().ifBlank { "auto" },
            endpoint = endpoint.trim().ifBlank { "https://api.deepseek.com/anthropic/v1" },
            model = model.trim().ifBlank { "deepseek-chat" },
            apiVersion = apiVersion.trim().ifBlank { "2023-06-01" },
            maxTokens = maxTokens.coerceIn(256, 32_000),
            maxUses = maxUses.coerceIn(1, 20),
            maxResults = maxResults.coerceIn(1, 25),
            maxQueries = maxQueries.coerceIn(1, 8),
            exaEndpoint = exaEndpoint.trim().ifBlank { "https://api.exa.ai" },
            exaSnippetChars = exaSnippetChars.coerceIn(120, 8_000),
        )
    }

    fun toJson(): String {
        val normalized = normalized()
        return JSONObject()
            .put("provider", normalized.provider)
            .put("endpoint", normalized.endpoint)
            .put("model", normalized.model)
            .put("apiVersion", normalized.apiVersion)
            .put("maxTokens", normalized.maxTokens)
            .put("maxUses", normalized.maxUses)
            .put("maxResults", normalized.maxResults)
            .put("maxQueries", normalized.maxQueries)
            .put("apiKeyRef", normalized.apiKeyRef)
            .put("exaEndpoint", normalized.exaEndpoint)
            .put("exaApiKeyRef", normalized.exaApiKeyRef)
            .put("exaSnippetChars", normalized.exaSnippetChars)
            .toString()
    }
}

private fun String.toUiWebSearchSettings(): UiWebSearchSettings {
    if (isBlank()) return UiWebSearchSettings()
    return runCatching {
        val defaults = UiWebSearchSettings()
        val root = JSONObject(this)
        UiWebSearchSettings(
            provider = root.optString("provider", defaults.provider),
            endpoint = root.optString("endpoint", defaults.endpoint),
            model = root.optString("model", defaults.model),
            apiVersion = root.optString("apiVersion", defaults.apiVersion),
            maxTokens = root.optInt("maxTokens", defaults.maxTokens),
            maxUses = root.optInt("maxUses", defaults.maxUses),
            maxResults = root.optInt("maxResults", defaults.maxResults),
            maxQueries = root.optInt("maxQueries", defaults.maxQueries),
            apiKeyRef = root.optString("apiKeyRef", defaults.apiKeyRef),
            exaEndpoint = root.optString("exaEndpoint", defaults.exaEndpoint),
            exaApiKeyRef = root.optString("exaApiKeyRef", defaults.exaApiKeyRef),
            exaSnippetChars = root.optInt("exaSnippetChars", defaults.exaSnippetChars),
        )
    }.getOrDefault(UiWebSearchSettings()).normalized()
}

// 形状与 Rust 侧 WebFetchSettings 一一对应。
private data class UiWebFetchSettings(
    val enabled: Boolean = true,
    val maxBytes: Int = 2_097_152,
    val timeoutMs: Int = 20_000,
    val maxRedirects: Int = 4,
    val allowPrivateHosts: Boolean = false,
    val maxChars: Int = 48_000,
) {
    fun normalized(): UiWebFetchSettings {
        return copy(
            maxBytes = maxBytes.coerceIn(8 * 1024, 16 * 1024 * 1024),
            timeoutMs = timeoutMs.coerceIn(1_000, 120_000),
            maxRedirects = maxRedirects.coerceIn(0, 10),
            maxChars = maxChars.coerceIn(1_000, 400_000),
        )
    }

    fun toJson(): String {
        val normalized = normalized()
        return JSONObject()
            .put("enabled", normalized.enabled)
            .put("maxBytes", normalized.maxBytes)
            .put("timeoutMs", normalized.timeoutMs)
            .put("maxRedirects", normalized.maxRedirects)
            .put("allowPrivateHosts", normalized.allowPrivateHosts)
            .put("maxChars", normalized.maxChars)
            .toString()
    }
}

private fun String.toUiWebFetchSettings(): UiWebFetchSettings {
    if (isBlank()) return UiWebFetchSettings()
    return runCatching {
        val defaults = UiWebFetchSettings()
        val root = JSONObject(this)
        UiWebFetchSettings(
            enabled = root.optBoolean("enabled", defaults.enabled),
            maxBytes = root.optInt("maxBytes", defaults.maxBytes),
            timeoutMs = root.optInt("timeoutMs", defaults.timeoutMs),
            maxRedirects = root.optInt("maxRedirects", defaults.maxRedirects),
            allowPrivateHosts = root.optBoolean("allowPrivateHosts", defaults.allowPrivateHosts),
            maxChars = root.optInt("maxChars", defaults.maxChars),
        )
    }.getOrDefault(UiWebFetchSettings()).normalized()
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

// 只被 Composable 调用，价格与「暂无」兜底文案走资源。
@Composable
private fun Double?.toCostText(): String {
    return this?.let { stringResource(R.string.settings_cost_per_million, it.toString()) }
        ?: stringResource(R.string.common_not_available)
}

private fun Long.toReadableSize(): String {
    // 单位是通用缩写，不做翻译。
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
