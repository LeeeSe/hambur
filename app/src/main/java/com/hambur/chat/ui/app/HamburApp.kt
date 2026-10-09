package com.hambur.chat.ui.app

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.platform.LocalContext
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.SaveableStateHolder
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.zIndex
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.ui.browser.SharedBrowserScreen
import com.hambur.chat.ui.chat.HamburChatScreen
import com.hambur.chat.ui.components.HamburImageZoomSandbox
import com.hambur.chat.ui.components.HamburZoomImageState
import com.hambur.chat.ui.preview.HamburFilePreviewScreen
import com.hambur.chat.ui.settings.AppearanceSettingsScreen
import com.hambur.chat.ui.settings.FeatureUnavailableScreen
import com.hambur.chat.ui.settings.MemoryDetailScreen
import com.hambur.chat.ui.settings.MemoryFilesListScreen
import com.hambur.chat.ui.settings.HamburSettingsHomeScreen
import com.hambur.chat.ui.settings.ModelDetailScreen
import com.hambur.chat.ui.settings.ModelGroupDetailScreen
import com.hambur.chat.ui.settings.ModelGroupsListScreen
import com.hambur.chat.ui.settings.PersonaSettingsScreen
import com.hambur.chat.ui.settings.ProviderDetailScreen
import com.hambur.chat.ui.settings.ProviderNewScreen
import com.hambur.chat.ui.settings.ProvidersListScreen
import com.hambur.chat.ui.settings.RootfsSettingsScreen
import com.hambur.chat.ui.settings.SkillDetailScreen
import com.hambur.chat.ui.settings.SkillsListScreen
import com.hambur.chat.ui.settings.StartupTaskDetailScreen
import com.hambur.chat.ui.settings.StartupTasksListScreen
import com.hambur.chat.ui.settings.ToolDetailScreen
import com.hambur.chat.ui.settings.ToolSettingsScreen
import com.hambur.chat.ui.settings.ToolsListScreen
import kotlinx.coroutines.launch
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
    /// Web search / web fetch configuration: provider, endpoint, credentials, limits.
    data object WebTools : HamburScreen()
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
    // ── 页面转场状态 ───────────────────────────────────────────────────────────
    // 这里不用 AnimatedContent: 它按"内容"缓存 transitionSpec, 转场被打断时复用的旧内容会
    // 带着上一次的方向重新进场, 表现就是一级菜单偶尔从上一次的右侧滑出来。方向和进度
    // 由我们自己持有, 导航那一刻就定死, 所以不存在"反着滑"的可能。
    var currentScreen by remember { mutableStateOf<HamburScreen>(HamburScreen.Chat) }
    var leavingScreen by remember { mutableStateOf<HamburScreen?>(null) }
    var goingDeeper by remember { mutableStateOf(true) }
    val screenTransition = remember { Animatable(1f) }
    val transitionScope = rememberCoroutineScope()
    var transitionToken by remember { mutableIntStateOf(0) }
    var activeZoomImage by remember { mutableStateOf<HamburZoomImageState?>(null) }

    DisposableEffect(store) {
        onDispose { store.shutdown() }
    }

    // 启动一次转场。若上一次还在播, 用 p' = 1 - p 接续: 旧的"进入页"正好成为新的"退出页",
    // 它当时的位移是 (1-p)·W, 而新退出页的位移是 p'·W, 于是 p' = 1-p 能无缝接上, 不会跳。
    // 位移用 tween 而不是 spring: tween 是位置驱动的, 被打断时从当前位置重新计时,
    // 不会像 spring 那样带着上一次的速度冲过头(不会回弹)。
    val startTransition: (HamburScreen, Boolean) -> Unit = { next, deeper ->
        val from = screenTransition.value
        if (next::class == currentScreen::class) {
            // 同一路由只换参数(例如在文件预览里再打开一个文件): 直接换内容, 不播转场。
            currentScreen = next
            leavingScreen = null
            transitionScope.launch { screenTransition.snapTo(1f) }
        } else {
            transitionToken += 1
            val token = transitionToken
            leavingScreen = currentScreen
            currentScreen = next
            goingDeeper = deeper
            transitionScope.launch {
                screenTransition.snapTo(if (from < 1f) 1f - from else 0f)
                val remaining = (ScreenTransitionDurationMs * (1f - screenTransition.value))
                    .toInt().coerceAtLeast(80)
                screenTransition.animateTo(
                    targetValue = 1f,
                    animationSpec = tween(durationMillis = remaining, easing = FastOutSlowInEasing),
                )
                if (token == transitionToken) leavingScreen = null
            }
        }
    }

    val navigate: (HamburScreen) -> Unit = { screen ->
        if (screen is HamburScreen.FilePreview) {
            backStack.removeAll { it is HamburScreen.FilePreview }
        } else {
            backStack.removeAll { it::class == screen::class && it !is HamburScreen.Chat }
        }
        backStack.add(screen)
        startTransition(screen, true)
    }
    val goBack: () -> Unit = {
        if (backStack.size > 1) {
            backStack.removeAt(backStack.lastIndex)
            startTransition(backStack.last(), false)
        }
    }

    if (backStack.size > 1) {
        BackHandler(onBack = goBack)
    }

    val context = LocalContext.current
    val isImageTarget: (String) -> Boolean = remember {
        { path ->
            val clean = path.substringBefore('?').lowercase()
            val imageExtensions = listOf(".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg", ".heic", ".heif", ".ico")
            imageExtensions.any { clean.endsWith(it) }
        }
    }

    val openTarget: (String) -> Unit = remember(context) {
        { target ->
            val isWeb = target.startsWith("http://", ignoreCase = true) || target.startsWith("https://", ignoreCase = true)
            val isWebImage = isWeb && isImageTarget(target)
            if (isImageTarget(target)) {
                val cleanName = target.substringBefore('?').substringAfterLast('/').ifBlank { "图片预览" }
                activeZoomImage = HamburZoomImageState(
                    model = target,
                    title = cleanName,
                )
            } else if (isWeb && !isWebImage) {
                val intent = Intent(Intent.ACTION_VIEW, Uri.parse(target)).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                runCatching { context.startActivity(intent) }
            } else {
                navigate(HamburScreen.FilePreview(target))
            }
        }
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
                val topScreen = backStack.last()
                // 页面离开组合后, 它的 rememberSaveable 状态(列表滚动位置、rememberScrollState 等)
                // 由 holder 按页面保管, 返回时原样恢复。没有它, 每次返回都是全新的一份组合,
                // 二级页回到一级页时滚动位置就会跳回顶部。
                val saveableStateHolder = rememberSaveableStateHolder()

                // 页面内容。退出中的旧页和当前页都调用它。
                val renderScreen: @Composable (HamburScreen) -> Unit = { screen ->
                    when (screen) {
                        HamburScreen.Chat -> HamburChatScreen(
                            state = state,
                            store = store,
                            onOpenSettings = { navigate(HamburScreen.Settings) },
                            onOpenBrowser = { navigate(HamburScreen.Browser) },
                            onOpenFile = openTarget,
                            onOpenImage = { destination, alt ->
                                val cleanName = alt.ifBlank { destination.substringBefore('?').substringAfterLast('/') }
                                activeZoomImage = HamburZoomImageState(
                                    model = destination,
                                    title = cleanName,
                                )
                            },
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
                            onOpenWebTools = { navigate(HamburScreen.WebTools) },
                        )
                        HamburScreen.WebTools -> ToolSettingsScreen(
                            state = state,
                            store = store,
                            onBack = goBack,
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
                        HamburScreen.Persona -> PersonaSettingsScreen(
                            state = state,
                            store = store,
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
                            onOpenFile = openTarget,
                            onOpenZoomImage = { zoomState ->
                                activeZoomImage = zoomState
                            },
                        )
                    }
                }

                // 两层渲染: 正在退出的旧页 + 当前页, 各自做整屏位移。
                // 前进: 新页从 +W 滑到 0, 旧页退到 -W/3(视差);
                // 返回: 旧页滑到 +W, 一级菜单从 -W/3 滑回 0 —— 永远从左边出来。
                // 退出中的旧页留在上层, 所以返回时列表是从详情页下面被"揭开"的。
                val progress = screenTransition.value
                val layers = buildList {
                    leavingScreen?.let { leaving ->
                        add(
                            PageLayer(
                                screen = leaving,
                                fraction = if (goingDeeper) -progress / 3f else progress,
                                layerZIndex = if (goingDeeper) 0f else 1f,
                            ),
                        )
                    }
                    add(
                        PageLayer(
                            screen = topScreen,
                            fraction = if (goingDeeper) 1f - progress else -(1f - progress) / 3f,
                            layerZIndex = if (goingDeeper) 1f else 0f,
                        ),
                    )
                }
                Box(modifier = Modifier.fillMaxSize()) {
                    // 每层都套 key(页面): 页面在"当前页 ↔ 退出页"之间换角色时, Compose 才会按
                    // key 匹配并把它的组合整体搬过去, 而不是销毁重建。否则设置首页在那一下就被
                    // 重新组合, LazyListState 归零, holder 存下的也已经是"顶部"了。
                    layers.forEach { layer ->
                        key(layer.screen.toString()) {
                            ScreenLayer(
                                fraction = layer.fraction,
                                layerZIndex = layer.layerZIndex,
                                stateHolder = saveableStateHolder,
                                stateKey = layer.screen.toString(),
                                background = MaterialTheme.colorScheme.background,
                            ) { renderScreen(layer.screen) }
                        }
                    }
                }

                activeZoomImage?.let { zoomState ->
                    HamburImageZoomSandbox(
                        state = zoomState,
                        onResolveHostPath = { path ->
                            store.resolveSandboxHostPath(state.selectedSessionId, path)
                        },
                        onDismiss = {
                            activeZoomImage = null
                        },
                        modifier = Modifier
                            .fillMaxSize()
                            .zIndex(999f),
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

// 页面转场时长。tween 是位置驱动的: 被打断时从当前位置重新计时, 不会像 spring 那样带着
// 上一次的速度冲过头, 所以既不会反方向也不会回弹。220ms + 减速缓动 = 起步快、尾巴短。
private const val ScreenTransitionDurationMs = 220

/**
 * 转场中的一层页面。
 *
 * 整屏位移用 [fraction] 表示: -1 = 左移一整屏, +1 = 右移一整屏, 0 = 就位。
 * 底色必须由每一层自己铺 —— 设置页那些 Column 都只有 fillMaxSize + statusBarsPadding,
 * 不画背景; 只靠外层那层底色的话, 滑过来的新页面就是一张"镂空贴纸", 旧页面的内容会透出来。
 */
@Composable
private fun ScreenLayer(
    fraction: Float,
    layerZIndex: Float,
    stateHolder: SaveableStateHolder,
    stateKey: String,
    background: Color,
    content: @Composable () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .zIndex(layerZIndex)
            .graphicsLayer { translationX = fraction * size.width }
            .background(background),
    ) {
        stateHolder.SaveableStateProvider(stateKey) { content() }
    }
}

/** 转场中的一层页面: 渲染哪个页面、整屏位移多少、在上层还是下层。 */
private data class PageLayer(
    val screen: HamburScreen,
    val fraction: Float,
    val layerZIndex: Float,
)
