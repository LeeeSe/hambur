package com.hambur.chat.ui.app

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
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
    val themeMode = state.settingValue("themeMode", "dark")
    val darkTheme = when (themeMode) {
        "light" -> false
        "system" -> isSystemInDarkTheme()
        else -> true
    }
    val fontScale = when (state.settingValue("fontScale", "default")) {
        "small" -> 0.90f
        "large" -> 1.15f
        "extra_large" -> 1.30f
        else -> 1.00f
    }
    val density = LocalDensity.current
    CompositionLocalProvider(
        LocalDensity provides Density(
            density = density.density,
            fontScale = density.fontScale * fontScale,
        ),
    ) {
        MaterialTheme(
            colorScheme = if (darkTheme) hamburDarkColorScheme() else hamburLightColorScheme(),
            content = content,
        )
    }
}

private fun hamburDarkColorScheme(): ColorScheme = darkColorScheme(
    primary = Color(0xFF7DD3C7),
    onPrimary = Color(0xFF06201D),
    primaryContainer = Color(0xFF143F39),
    onPrimaryContainer = Color(0xFFD3F8F1),
    secondary = Color(0xFFB8C8C3),
    tertiary = Color(0xFFF6C56B),
    background = Color(0xFF111312),
    surface = Color(0xFF181B1A),
    surfaceVariant = Color(0xFF252A28),
    onSurface = Color(0xFFE6E9E7),
    onSurfaceVariant = Color(0xFFB9C1BE),
    outline = Color(0xFF717A76),
    outlineVariant = Color(0xFF343B38),
    error = Color(0xFFFFB4AB),
)

private fun hamburLightColorScheme(): ColorScheme = lightColorScheme(
    primary = Color(0xFF006A60),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFF9EF2E5),
    onPrimaryContainer = Color(0xFF00201C),
    secondary = Color(0xFF4A635E),
    tertiary = Color(0xFF765B00),
    background = Color(0xFFFAFDFB),
    surface = Color(0xFFFAFDFB),
    surfaceVariant = Color(0xFFDCE5E1),
    onSurface = Color(0xFF191C1B),
    onSurfaceVariant = Color(0xFF404947),
    outline = Color(0xFF707977),
    outlineVariant = Color(0xFFC0C9C5),
    error = Color(0xFFBA1A1A),
)

private fun HamburUiState.settingValue(key: String, fallback: String): String {
    return appSettings.firstOrNull { it.key == key }?.value ?: fallback
}
