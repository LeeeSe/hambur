package com.hambur.chat.ui.app

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.ui.browser.SharedBrowserScreen
import com.hambur.chat.ui.chat.HamburChatScreen
import com.hambur.chat.ui.preview.HamburFilePreviewScreen
import com.hambur.chat.ui.settings.AppearanceSettingsScreen
import com.hambur.chat.ui.settings.FeatureUnavailableScreen
import com.hambur.chat.ui.settings.MemoryDetailScreen
import com.hambur.chat.ui.settings.MemoryFilesListScreen
import com.hambur.chat.ui.settings.HamburSettingsHomeScreen
import com.hambur.chat.ui.settings.ModelDetailScreen
import com.hambur.chat.ui.settings.ModelGroupDetailScreen
import com.hambur.chat.ui.settings.ModelGroupsListScreen
import com.hambur.chat.ui.settings.ProviderDetailScreen
import com.hambur.chat.ui.settings.ProviderNewScreen
import com.hambur.chat.ui.settings.ProvidersListScreen
import com.hambur.chat.ui.settings.RootfsSettingsScreen
import com.hambur.chat.ui.settings.SkillDetailScreen
import com.hambur.chat.ui.settings.SkillsListScreen
import com.hambur.chat.ui.settings.StartupTaskDetailScreen
import com.hambur.chat.ui.settings.StartupTasksListScreen
import com.hambur.chat.ui.settings.ToolDetailScreen
import com.hambur.chat.ui.settings.ToolsListScreen
import com.hambur.chat.ui.theme.HamburThemeDefaults
import com.hambur.chat.ui.theme.HamburThemeProvider
import com.hambur.chat.ui.theme.HamburThemeSettingKeys

sealed class HamburScreen {
    data object Chat : HamburScreen()
    data object Settings : HamburScreen()
    data object ProvidersList : HamburScreen()
    data object NewProvider : HamburScreen()
    data class ProviderDetail(val providerId: String) : HamburScreen()
    data class ModelDetail(val providerId: String, val modelId: String) : HamburScreen()
    data object ModelGroupsList : HamburScreen()
    data class ModelGroupDetail(val groupId: String) : HamburScreen()
    data object SkillsList : HamburScreen()
    data class SkillDetail(val skillId: String) : HamburScreen()
    data object MemoryFilesList : HamburScreen()
    data class MemoryFileDetail(val memoryKey: String) : HamburScreen()
    data object ToolsList : HamburScreen()
    data class ToolDetail(val toolName: String) : HamburScreen()
    data object StartupTasksList : HamburScreen()
    data class StartupTaskDetail(val taskId: String) : HamburScreen()
    data object Rootfs : HamburScreen()
    data object Appearance : HamburScreen()
    data object Logs : HamburScreen()
    data object TokenUsage : HamburScreen()
    data object Persona : HamburScreen()
    data object EnvironmentVariables : HamburScreen()
    data object Browser : HamburScreen()
    data class FilePreview(val path: String) : HamburScreen()
}

@Composable
fun HamburApp(
    appFilesDir: String,
    nativeLibraryDir: String,
    platformAdapter: AndroidPlatformAdapter,
    onPickImage: ((String, String, ULong, String, String) -> Unit) -> Unit = {},
    onPickFile: ((String, String, ULong, String, String) -> Unit) -> Unit = {},
) {
    val store = remember(appFilesDir, nativeLibraryDir, platformAdapter) {
        HamburUiStore(
            appFilesDir = appFilesDir,
            nativeLibraryDir = nativeLibraryDir,
            platformAdapter = platformAdapter,
        )
    }
    val state by store.state.collectAsState()
    val backStack = remember { mutableStateListOf<HamburScreen>(HamburScreen.Chat) }

    DisposableEffect(store) {
        onDispose { store.shutdown() }
    }

    val navigate: (HamburScreen) -> Unit = { screen ->
        if (screen is HamburScreen.FilePreview) {
            backStack.removeAll { it is HamburScreen.FilePreview }
        } else {
            backStack.removeAll { it::class == screen::class && it !is HamburScreen.Chat }
        }
        backStack.add(screen)
    }
    val goBack: () -> Unit = {
        if (backStack.size > 1) {
            backStack.removeAt(backStack.lastIndex)
        }
    }

    if (backStack.size > 1) {
        BackHandler(onBack = goBack)
    }

    HamburTheme(state = state) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.background),
            ) {
                when (val screen = backStack.last()) {
                    HamburScreen.Chat -> HamburChatScreen(
                        state = state,
                        store = store,
                        onOpenSettings = { navigate(HamburScreen.Settings) },
                        onOpenBrowser = { navigate(HamburScreen.Browser) },
                        onOpenFile = { navigate(HamburScreen.FilePreview(it)) },
                        onPickImage = { onPicked ->
                            onPickImage { displayName, mimeType, byteSize, uri, sourcePath ->
                                onPicked(displayName, mimeType, byteSize, uri, sourcePath)
                            }
                        },
                        onPickFile = { onPicked ->
                            onPickFile { displayName, mimeType, byteSize, uri, sourcePath ->
                                onPicked(displayName, mimeType, byteSize, uri, sourcePath)
                            }
                        },
                    )
                    HamburScreen.Settings -> HamburSettingsHomeScreen(
                        state = state,
                        onBack = goBack,
                        onOpenProviders = { navigate(HamburScreen.ProvidersList) },
                        onOpenModelGroups = { navigate(HamburScreen.ModelGroupsList) },
                        onOpenSkills = { navigate(HamburScreen.SkillsList) },
                        onOpenMemory = { navigate(HamburScreen.MemoryFilesList) },
                        onOpenTools = { navigate(HamburScreen.ToolsList) },
                        onOpenStartupTasks = { navigate(HamburScreen.StartupTasksList) },
                        onOpenRootfs = { navigate(HamburScreen.Rootfs) },
                        onOpenAppearance = { navigate(HamburScreen.Appearance) },
                        onOpenLogs = { navigate(HamburScreen.Logs) },
                        onOpenTokenUsage = { navigate(HamburScreen.TokenUsage) },
                        onOpenPersona = { navigate(HamburScreen.Persona) },
                        onOpenEnvironmentVariables = { navigate(HamburScreen.EnvironmentVariables) },
                    )
                    HamburScreen.ProvidersList -> ProvidersListScreen(
                        state = state,
                        onBack = goBack,
                        onNewProvider = { navigate(HamburScreen.NewProvider) },
                        onOpenProvider = { navigate(HamburScreen.ProviderDetail(it)) },
                    )
                    HamburScreen.NewProvider -> ProviderNewScreen(
                        state = state,
                        store = store,
                        onBack = goBack,
                    )
                    is HamburScreen.ProviderDetail -> ProviderDetailScreen(
                        state = state,
                        store = store,
                        providerId = screen.providerId,
                        onBack = goBack,
                        onOpenModel = { providerId, modelId ->
                            navigate(HamburScreen.ModelDetail(providerId, modelId))
                        },
                    )
                    is HamburScreen.ModelDetail -> ModelDetailScreen(
                        state = state,
                        store = store,
                        providerId = screen.providerId,
                        modelId = screen.modelId,
                        onBack = goBack,
                    )
                    HamburScreen.ModelGroupsList -> ModelGroupsListScreen(
                        state = state,
                        store = store,
                        onBack = goBack,
                        onNewGroup = { navigate(HamburScreen.ModelGroupDetail("")) },
                        onOpenGroup = { navigate(HamburScreen.ModelGroupDetail(it)) },
                    )
                    is HamburScreen.ModelGroupDetail -> ModelGroupDetailScreen(
                        state = state,
                        store = store,
                        groupId = screen.groupId,
                        onBack = goBack,
                    )
                    HamburScreen.SkillsList -> SkillsListScreen(
                        state = state,
                        store = store,
                        onBack = goBack,
                        onOpenSkill = { navigate(HamburScreen.SkillDetail(it)) },
                    )
                    is HamburScreen.SkillDetail -> SkillDetailScreen(
                        state = state,
                        store = store,
                        skillId = screen.skillId,
                        onBack = goBack,
                    )
                    HamburScreen.MemoryFilesList -> MemoryFilesListScreen(
                        state = state,
                        onBack = goBack,
                        onOpenMemory = { navigate(HamburScreen.MemoryFileDetail(it)) },
                    )
                    is HamburScreen.MemoryFileDetail -> MemoryDetailScreen(
                        state = state,
                        store = store,
                        memoryKey = screen.memoryKey,
                        onBack = goBack,
                    )
                    HamburScreen.ToolsList -> ToolsListScreen(
                        state = state,
                        onBack = goBack,
                        onOpenTool = { navigate(HamburScreen.ToolDetail(it)) },
                    )
                    is HamburScreen.ToolDetail -> ToolDetailScreen(
                        state = state,
                        store = store,
                        toolName = screen.toolName,
                        onBack = goBack,
                    )
                    HamburScreen.StartupTasksList -> StartupTasksListScreen(
                        state = state,
                        onBack = goBack,
                        onNewTask = { navigate(HamburScreen.StartupTaskDetail("")) },
                        onOpenTask = { navigate(HamburScreen.StartupTaskDetail(it)) },
                    )
                    is HamburScreen.StartupTaskDetail -> StartupTaskDetailScreen(
                        state = state,
                        store = store,
                        taskId = screen.taskId,
                        onBack = goBack,
                    )
                    HamburScreen.Rootfs -> RootfsSettingsScreen(
                        state = state,
                        store = store,
                        onBack = goBack,
                    )
                    HamburScreen.Appearance -> AppearanceSettingsScreen(
                        state = state,
                        store = store,
                        onBack = goBack,
                    )
                    HamburScreen.Logs -> FeatureUnavailableScreen(
                        title = "Logs",
                        summary = "In-app log capture, filtering, and clearing are not exposed by the new backend yet.",
                        onBack = goBack,
                    )
                    HamburScreen.TokenUsage -> FeatureUnavailableScreen(
                        title = "Token Usage",
                        summary = "Per-provider usage accounting and billing summaries are not exposed by the new backend yet.",
                        onBack = goBack,
                    )
                    HamburScreen.Persona -> FeatureUnavailableScreen(
                        title = "Persona",
                        summary = "SOUL.md/personality editing is not exposed by the new backend yet.",
                        onBack = goBack,
                    )
                    HamburScreen.EnvironmentVariables -> FeatureUnavailableScreen(
                        title = "Environment Variables",
                        summary = "Sandbox environment variable management is not exposed by the new backend yet.",
                        onBack = goBack,
                    )
                    HamburScreen.Browser -> SharedBrowserScreen(
                        state = state,
                        platformAdapter = platformAdapter,
                        onBack = goBack,
                    )
                    is HamburScreen.FilePreview -> HamburFilePreviewScreen(
                        path = screen.path,
                        state = state,
                        store = store,
                        onBack = goBack,
                        onOpenFile = { navigate(HamburScreen.FilePreview(it)) },
                    )
                }
            }
        }
    }
}

@Composable
private fun HamburTheme(
    state: HamburUiState,
    content: @Composable () -> Unit,
) {
    HamburThemeProvider(
        themeMode = state.settingValue(
            HamburThemeSettingKeys.ThemeMode,
            HamburThemeDefaults.ThemeMode,
        ),
        fontScale = state.settingValue(
            HamburThemeSettingKeys.FontScale,
            HamburThemeDefaults.FontScale,
        ),
        content = content,
    )
}

private fun HamburUiState.settingValue(key: String, fallback: String): String {
    return appSettings.firstOrNull { it.key == key }?.value ?: fallback
}
