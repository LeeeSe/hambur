package com.hambur.chat.ui.chat

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.snap
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image as ComposeImage
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.FlingBehavior
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.windowInsetsBottomHeight
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.text.TextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.zIndex
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import kotlin.math.abs
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.Velocity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import coil3.compose.AsyncImage
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.graphics.vector.ImageVector
import com.composables.icons.lucide.Bot
import com.composables.icons.lucide.Brain
import com.composables.icons.lucide.Camera
import com.composables.icons.lucide.ChartNoAxesGantt
import com.composables.icons.lucide.Check
import com.composables.icons.lucide.ChevronRight
import com.composables.icons.lucide.CircleArrowUp
import com.composables.icons.lucide.CirclePause
import com.composables.icons.lucide.CirclePlus
import com.composables.icons.lucide.CircleX
import com.composables.icons.lucide.Copy
import com.composables.icons.lucide.FileDiff
import com.composables.icons.lucide.FilePen
import com.composables.icons.lucide.FileText
import com.composables.icons.lucide.Folder
import com.composables.icons.lucide.FolderSearch
import com.composables.icons.lucide.Globe
import com.composables.icons.lucide.Image
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.MessageCirclePlus
import com.composables.icons.lucide.MessageSquare
import com.composables.icons.lucide.Pencil
import com.composables.icons.lucide.Pin
import com.composables.icons.lucide.RefreshCw
import com.composables.icons.lucide.Search
import com.composables.icons.lucide.Settings
import com.composables.icons.lucide.Sparkles
import com.composables.icons.lucide.Terminal
import com.composables.icons.lucide.Trash2
import com.composables.icons.lucide.Type
import com.composables.icons.lucide.User
import com.composables.icons.lucide.WandSparkles
import com.composables.icons.lucide.Wrench
import com.composables.icons.lucide.X
import androidx.compose.ui.draw.drawBehind
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.os.Build
import android.util.Log
import android.view.HapticFeedbackConstants
import android.view.View
import android.widget.Toast
import com.hambur.chat.R
import com.hambur.chat.perf.ChatJankTracer
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_EXPAND
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_COLLAPSED
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_MODE_KEY
import com.hambur.chat.reducer.UiMessageSnapshot
import com.hambur.chat.reducer.UiPendingAttachment
import com.hambur.chat.reducer.UiSessionSummary
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.reducer.isNewSessionBlank
import com.hambur.chat.reducer.thinkingEnabledForSession
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.ui.components.SummaryLine
import com.hambur.chat.ui.markdown.MarkdownBlock
import com.hambur.chat.ui.markdown.MarkdownRenderCache
import com.hambur.chat.ui.markdown.MarkdownStyle
import com.hambur.chat.ui.markdown.rememberMarkdownRenderCache
import com.hambur.chat.ui.markdown.rememberMarkdownStyle
import com.hambur.chat.ui.theme.HamburTheme
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.FileOutputStream
import java.util.UUID

private const val SESSION_OPEN_DELAY_MS = 50L

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun HamburChatScreen(
    state: HamburUiState,
    store: HamburUiStore,
    onOpenSettings: () -> Unit,
    onOpenBrowser: () -> Unit,
    onOpenFile: (String) -> Unit,
    onPickImage: (((String, String, ULong, String, String) -> Unit) -> Unit) = {},
    onPickFile: (((String, String, ULong, String, String) -> Unit) -> Unit) = {},
) {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val haptics = rememberSemanticHaptics()
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var draftTitle by rememberSaveable { mutableStateOf("") }
    var draftMessage by rememberSaveable { mutableStateOf("") }
    var editingMessageId by rememberSaveable { mutableStateOf("") }
    var selectingText by rememberSaveable { mutableStateOf("") }
    var unavailableAction by rememberSaveable { mutableStateOf("") }
    var searchEnabled by rememberSaveable { mutableStateOf(false) }
    var attachmentPanelOpen by rememberSaveable { mutableStateOf(false) }
    var pendingOpenSessionId by remember { mutableStateOf("") }
    var delayedOpenSessionJob by remember { mutableStateOf<Job?>(null) }
    var chatInputPanelHeightPx by remember { mutableIntStateOf(0) }
    val density = LocalDensity.current
    val configuration = LocalConfiguration.current
    val chatTokens = HamburTheme.tokens.chat
    val isSessionOpenPending = pendingOpenSessionId.isNotBlank() &&
            pendingOpenSessionId != state.selectedSessionId
    val visibleState = if (isSessionOpenPending) {
        state.copy(
            latestEventKind = "SessionOpenPending",
            selectedSessionId = pendingOpenSessionId,
            timelineItems = emptyList(),
            messagesById = emptyMap(),
            reasoningByMessageId = emptyMap(),
            pendingAttachments = emptyList(),
            messageBlocksByPayloadRef = emptyMap(),
            activePreviewPath = "",
        )
    } else {
        state
    }
    val thinkingEnabled = visibleState.thinkingEnabledForSession()
    val attachmentPanelHeight = 220.dp
    val imeBottomPx = WindowInsets.ime.getBottom(density)
    val isImeActive = imeBottomPx > 0
    val attachmentPanelSlotHeight by animateDpAsState(
        targetValue = if (attachmentPanelOpen) attachmentPanelHeight + chatTokens.inputOuterGap else 0.dp,
        animationSpec = if (isImeActive) {
            snap()
        } else {
            tween(durationMillis = 250, easing = FastOutSlowInEasing)
        },
        label = "attachmentPanelSlotHeight",
    )
    val drawerBackgroundColor = MaterialTheme.colorScheme.background
    val drawerWidth = configuration.screenWidthDp.dp * 0.82f
    val maxDrawerOffset = with(density) { drawerWidth.toPx() }
    var drawerOffset by remember { mutableFloatStateOf(0f) }
    val drawerAnimation = remember { Animatable(0f) }
    val messageListBottomPadding = with(density) {
        chatInputPanelHeightPx.toDp()
    } + chatTokens.timelineBottomGap
    val consumeDrawerDelta: (Float) -> Float = { delta ->
        val previousOffset = drawerOffset
        drawerOffset = (drawerOffset + delta).coerceIn(0f, maxDrawerOffset)
        drawerOffset - previousOffset
    }
    val animateDrawerTo: suspend (Float) -> Unit = { targetOffset ->
        if (targetOffset > 0f) {
            focusManager.clearFocus()
            keyboardController?.hide()
        }
        drawerAnimation.snapTo(drawerOffset)
        drawerAnimation.animateTo(
            targetValue = targetOffset,
            animationSpec = spring(dampingRatio = 0.9f, stiffness = 1500f)
        ) {
            drawerOffset = value
        }
        drawerOffset = targetOffset
    }
    val drawerScrollableState = rememberScrollableState { delta ->
        if (delta > 0f && drawerOffset < maxDrawerOffset) {
            focusManager.clearFocus()
            keyboardController?.hide()
        }
        consumeDrawerDelta(delta)
    }
    val drawerNestedScrollConnection = remember {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (source != NestedScrollSource.UserInput || drawerOffset <= 0f || available.x == 0f) {
                    return Offset.Zero
                }
                if (available.x > 0f && drawerOffset < maxDrawerOffset) {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                }
                val consumed = consumeDrawerDelta(available.x)
                return Offset(consumed, 0f)
            }
        }
    }
    val chatContentNestedScrollConnection = remember {
        object : NestedScrollConnection {
            override fun onPostScroll(
                consumed: Offset,
                available: Offset,
                source: NestedScrollSource,
            ): Offset {
                // When drawer is closed, do NOT allow any child's horizontal scroll margin (overscroll)
                // to bubble up and drag open the drawer.
                if (drawerOffset <= 0f && available.x > 0f) {
                    return Offset(available.x, 0f)
                }
                return Offset.Zero
            }

            override suspend fun onPostFling(
                consumed: Velocity,
                available: Velocity,
            ): Velocity {
                // When drawer is closed, do NOT allow any child's residual horizontal fling velocity
                // to fling open the drawer.
                if (drawerOffset <= 0f && available.x > 0f) {
                    return Velocity(available.x, 0f)
                }
                return Velocity.Zero
            }
        }
    }
    val drawerFlingBehavior = object : FlingBehavior {
        override suspend fun ScrollScope.performFling(initialVelocity: Float): Float {
            val targetOffset = when {
                initialVelocity > 600f -> maxDrawerOffset
                initialVelocity < -600f -> 0f
                drawerOffset > maxDrawerOffset * 0.3f -> maxDrawerOffset
                else -> 0f
            }
            animateDrawerTo(targetOffset)
            return 0f
        }
    }
    val drawerProgress = if (maxDrawerOffset > 0f) {
        (drawerOffset / maxDrawerOffset).coerceIn(0f, 1f)
    } else {
        0f
    }
    val newSessionBlank = state.isNewSessionBlank()
    fun handleNewChatClick() {
        if (newSessionBlank) {
            haptics.reject()
            Toast.makeText(context, "已经在新聊天", Toast.LENGTH_SHORT).show()
            return
        }
        haptics.confirm()
        store.createSession("New chat")
    }

    val mainScale = 1f - (0.08f * drawerProgress)
    val mainContentAlpha = 1f - (0.55f * drawerProgress)
    val mainCornerRadius = if (drawerOffset > 0f) 30.dp else 0.dp
    val mainShadowElevation = 56.dp * drawerProgress
    val mainAmbientShadowColor = Color.Black.copy(alpha = 0.34f * drawerProgress)
    val mainSpotShadowColor = Color.Black.copy(alpha = 0.68f * drawerProgress)

    LaunchedEffect(maxDrawerOffset) {
        drawerOffset = drawerOffset.coerceIn(0f, maxDrawerOffset)
    }

    LaunchedEffect(state.selectedSessionId, pendingOpenSessionId) {
        if (pendingOpenSessionId.isNotBlank() && state.selectedSessionId == pendingOpenSessionId) {
            pendingOpenSessionId = ""
            delayedOpenSessionJob = null
        }
    }

    BackHandler(enabled = drawerOffset > 0f) {
        scope.launch {
            animateDrawerTo(0f)
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(drawerBackgroundColor)
            .drawWithContent {
                drawRect(drawerBackgroundColor)
                drawContent()
            }
            .nestedScroll(drawerNestedScrollConnection)
            .scrollable(
                state = drawerScrollableState,
                orientation = Orientation.Horizontal,
                flingBehavior = drawerFlingBehavior,
            ),
    ) {
        if (drawerProgress > 0.001f) {
            ChatDrawerContent(
                sessions = state.sessions,
                selectedSessionId = state.selectedSessionId,
                searchQuery = searchQuery,
                onSearchQueryChange = { searchQuery = it },
                onNewSession = {
                    handleNewChatClick()
                    draftTitle = ""
                    scope.launch { animateDrawerTo(0f) }
                },
                onOpenSession = {
                    val target = state.sessions.firstOrNull { session -> session.id == it }
                    ChatJankTracer.beginSessionSwitch(
                        source = "drawer_tap",
                        currentSessionId = state.selectedSessionId,
                        targetSessionId = it,
                        targetMessageCount = target?.messageCount?.toString().orEmpty(),
                        drawerProgress = drawerProgress,
                        extra = "timelineItems=${state.timelineItems.size} markdownBlocks=${state.messageBlocksByPayloadRef.size}",
                    )
                    delayedOpenSessionJob?.cancel()
                    pendingOpenSessionId = it
                    delayedOpenSessionJob = scope.launch {
                        launch { animateDrawerTo(0f) }
                        delay(SESSION_OPEN_DELAY_MS)
                        store.openSession(it)
                    }
                },
                onDeleteSession = store::deleteSession,
                onRenameSession = { sessionId ->
                    store.renameSession(sessionId, draftTitle.ifBlank { "New chat" })
                    draftTitle = ""
                },
                onSetSessionPinned = store::setSessionPinned,
                onUnavailableAction = { unavailableAction = it },
                onOpenSettings = {
                    scope.launch { animateDrawerTo(0f) }
                    onOpenSettings()
                },
                drawerWidth = drawerWidth,
                modifier = Modifier.graphicsLayer {
                    val drawerScale = 0.9f + (drawerProgress * 0.1f)
                    alpha = drawerProgress
                    scaleX = drawerScale
                    scaleY = drawerScale
                    translationX = -drawerWidth.toPx() * 0.2f * (1f - drawerProgress)
                    transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0f, 0.5f)
                },
            )
        }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .nestedScroll(chatContentNestedScrollConnection)
                .graphicsLayer {
                    translationX = drawerOffset
                    scaleX = mainScale
                    scaleY = mainScale
                    transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0f, 0.5f)
                }
                .shadow(
                    elevation = mainShadowElevation,
                    shape = RoundedCornerShape(mainCornerRadius),
                    clip = false,
                    ambientColor = mainAmbientShadowColor,
                    spotColor = mainSpotShadowColor,
                )
                .clip(RoundedCornerShape(mainCornerRadius))
                .background(MaterialTheme.colorScheme.background)
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    enabled = drawerOffset > 0f,
                    onClick = {
                        scope.launch { animateDrawerTo(0f) }
                    },
                )
                .statusBarsPadding(),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize(),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .graphicsLayer {
                            alpha = mainContentAlpha
                        },
                ) {
                    ChatHeader(
                        title = visibleState.selectedSessionTitle(),
                        onOpenDrawer = {
                            focusManager.clearFocus()
                            keyboardController?.hide()
                            scope.launch { animateDrawerTo(maxDrawerOffset) }
                        },
                        canCreateNewChat = !newSessionBlank,
                        onNewChat = ::handleNewChatClick,
                    )

                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxWidth()
                            .clickable(
                                interactionSource = remember { MutableInteractionSource() },
                                indication = null,
                                onClick = {
                                    focusManager.clearFocus()
                                    keyboardController?.hide()
                                    attachmentPanelOpen = false
                                },
                            ),
                    ) {
                        ChatTimeline(
                            state = visibleState,
                            store = store,
                            onOpenFile = onOpenFile,
                            onEditMessage = { message ->
                                editingMessageId = message.id
                                draftMessage = message.contentText
                            },
                            onSelectText = { selectingText = it },
                            bottomPadding = messageListBottomPadding,
                            blank = isSessionOpenPending,
                            modifier = Modifier.fillMaxSize(),
                        )

                        ChatInputPanel(
                            message = draftMessage,
                            onMessageChange = { draftMessage = it },
                            enabled = visibleState.selectedSessionId.isNotBlank() && !isSessionOpenPending,
                            generating = visibleState.activeTurnIds.containsKey(visibleState.selectedSessionId),
                            thinkingEnabled = thinkingEnabled,
                            attachmentPanelOpen = attachmentPanelOpen,
                            attachmentPanelSlotHeight = attachmentPanelSlotHeight,
                            attachmentPanelHeight = attachmentPanelHeight,
                            pendingAttachments = visibleState.pendingAttachments,
                            editing = editingMessageId.isNotBlank(),
                            onToggleThinking = {
                                haptics.toggle(turningOn = !thinkingEnabled)
                                Log.i(
                                    "ThinkingToggle",
                                    "brain click session=${visibleState.selectedSessionId.ifBlank { "<draft>" }} from=$thinkingEnabled to=${!thinkingEnabled}",
                                )
                                store.setSessionThinkingEnabled(
                                    visibleState.selectedSessionId,
                                    !thinkingEnabled,
                                )
                            },
                            onToggleAttachmentPanel = {
                                val willOpen = !attachmentPanelOpen
                                haptics.toggle(turningOn = willOpen)
                                if (willOpen) {
                                    focusManager.clearFocus()
                                    keyboardController?.hide()
                                }
                                attachmentPanelOpen = willOpen
                            },
                            onCloseAttachmentPanel = { attachmentPanelOpen = false },
                            onImportAttachment = { displayName, mimeType, byteSize, uri, sourcePath ->
                                store.importAttachmentMetadata(
                                    sessionId = visibleState.selectedSessionId,
                                    displayName = displayName,
                                    mimeType = mimeType,
                                    byteSize = byteSize,
                                    originalUri = uri,
                                    sourcePath = sourcePath,
                                )
                            },
                            onPickImage = onPickImage,
                            onPickFile = onPickFile,
                            onRemoveAttachment = { store.removePendingAttachment(visibleState.selectedSessionId, it) },
                            onStop = { store.cancelActiveTurn(visibleState.selectedSessionId) },
                            onCancelEdit = {
                                editingMessageId = ""
                                draftMessage = ""
                            },
                            onSend = {
                                haptics.confirm()
                                if (editingMessageId.isNotBlank()) {
                                    store.editMessage(visibleState.selectedSessionId, editingMessageId, draftMessage)
                                    editingMessageId = ""
                                    draftMessage = ""
                                    attachmentPanelOpen = false
                                } else {
                                    Log.i(
                                        "ThinkingToggle",
                                        "send click session=${visibleState.selectedSessionId.ifBlank { "<draft>" }} thinking=$thinkingEnabled textLen=${draftMessage.length}",
                                    )
                                    store.sendMessage(
                                        sessionId = visibleState.selectedSessionId,
                                        content = draftMessage,
                                        deepThinkingEnabled = thinkingEnabled,
                                        searchEnabled = searchEnabled,
                                        onAccepted = {
                                            scope.launch {
                                                draftMessage = ""
                                                attachmentPanelOpen = false
                                            }
                                        },
                                        onRejected = { message ->
                                            scope.launch {
                                                Toast.makeText(
                                                    context,
                                                    message.ifBlank { "发送失败，请检查模型能力" },
                                                    Toast.LENGTH_SHORT,
                                                ).show()
                                            }
                                        },
                                    )
                                }
                            },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .onSizeChanged { size ->
                                    if (chatInputPanelHeightPx != size.height) {
                                        chatInputPanelHeightPx = size.height
                                    }
                                },
                        )
                    }
                }

            }
        }
    }

    if (selectingText.isNotBlank()) {
        SelectTextDialog(
            text = selectingText,
            onDismiss = { selectingText = "" },
        )
    }
    if (unavailableAction.isNotBlank()) {
        BackendGapDialog(
            title = unavailableAction,
            onDismiss = { unavailableAction = "" },
        )
    }
}

@Composable
private fun ChatHeader(
    title: String,
    onOpenDrawer: () -> Unit,
    canCreateNewChat: Boolean,
    onNewChat: () -> Unit,
) {
    val tokens = HamburTheme.tokens.chat
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(tokens.headerHeight)
            .padding(horizontal = tokens.headerHorizontalPadding),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        ChatIconButton(onClick = onOpenDrawer, size = tokens.headerIconButtonSize) {
            Icon(
                imageVector = Lucide.ChartNoAxesGantt,
                contentDescription = "Conversations",
                tint = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.size(tokens.headerMenuIconSize),
            )
        }

        Text(
            text = title.ifBlank { "新对话" },
            color = MaterialTheme.colorScheme.onBackground,
            fontSize = tokens.headerTitleFontSize,
            fontWeight = FontWeight.Bold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
            modifier = Modifier
                .weight(1f)
                .padding(horizontal = tokens.headerTitleHorizontalPadding),
        )

        Row {
            ChatIconButton(
                onClick = onNewChat,
                size = tokens.headerIconButtonSize,
            ) {
                Icon(
                    imageVector = Lucide.MessageCirclePlus,
                    contentDescription = "New chat",
                    tint = MaterialTheme.colorScheme.onBackground.copy(
                        alpha = if (canCreateNewChat) 1f else 0.32f,
                    ),
                    modifier = Modifier.size(tokens.headerNewChatIconSize),
                )
            }
        }
    }
}

@Composable
private fun ChatDrawerContent(
    sessions: List<UiSessionSummary>,
    selectedSessionId: String,
    searchQuery: String,
    onSearchQueryChange: (String) -> Unit,
    onNewSession: () -> Unit,
    onOpenSession: (String) -> Unit,
    onDeleteSession: (String) -> Unit,
    onRenameSession: (String) -> Unit,
    onSetSessionPinned: (String, Boolean) -> Unit,
    onUnavailableAction: (String) -> Unit,
    onOpenSettings: () -> Unit,
    drawerWidth: Dp,
    modifier: Modifier = Modifier,
) {
    val colorScheme = MaterialTheme.colorScheme
    val drawerBackground = colorScheme.background
    val searchBackground = colorScheme.primaryContainer
    val selectedBackground = colorScheme.primaryContainer
    val mutedText = colorScheme.onSurfaceVariant
    val groupedSessions = remember(sessions, searchQuery) {
        val chatSessions = sessions.filter { it.purpose == "chat" && it.messageCount > 0u }
        groupDrawerSessions(
            sessions = if (searchQuery.isBlank()) {
                chatSessions
            } else {
                chatSessions.filter {
                    it.title.contains(searchQuery, ignoreCase = true) ||
                            it.latestPreview.contains(searchQuery, ignoreCase = true)
                }
            },
            nowMs = System.currentTimeMillis().coerceAtLeast(0).toULong(),
        )
    }

    Surface(
        modifier = modifier
            .fillMaxHeight()
            .width(drawerWidth),
        color = drawerBackground,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .navigationBarsPadding()
                .padding(horizontal = 16.dp),
        ) {
            Spacer(modifier = Modifier.height(16.dp))

            DrawerSearchField(
                value = searchQuery,
                onValueChange = onSearchQueryChange,
                background = searchBackground,
                contentColor = colorScheme.onBackground,
                placeholderColor = mutedText,
            )

            Spacer(modifier = Modifier.height(22.dp))

            LazyColumn(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
                contentPadding = PaddingValues(bottom = 18.dp),
            ) {
                groupedSessions.forEach { group ->
                    if (group.sessions.isNotEmpty()) {
                        item(key = "header-${group.title}") {
                            DrawerCategoryHeader(
                                text = group.title,
                                color = mutedText,
                            )
                        }
                        items(group.sessions, key = { it.id }) { session ->
                            DrawerSessionRow(
                                session = session,
                                selected = session.id == selectedSessionId,
                                onClick = { onOpenSession(session.id) },
                                onDelete = { onDeleteSession(session.id) },
                                onPin = { onSetSessionPinned(session.id, session.pinnedAtMs == 0UL) },
                                selectedBackground = selectedBackground,
                                contentColor = colorScheme.onBackground,
                            )
                        }
                    }
                }
            }

            DrawerBottomBar(
                onOpenSettings = onOpenSettings,
                avatarBackground = colorScheme.primaryContainer,
                contentColor = colorScheme.onBackground,
                mutedColor = mutedText,
            )
        }
    }
}

@Composable
private fun DrawerSearchField(
    value: String,
    onValueChange: (String) -> Unit,
    background: Color,
    contentColor: Color,
    placeholderColor: Color,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(26.dp))
            .background(background)
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Lucide.Search,
            contentDescription = null,
            tint = placeholderColor,
            modifier = Modifier.size(22.dp),
        )
        Spacer(modifier = Modifier.width(10.dp))
        Box(modifier = Modifier.weight(1f)) {
            if (value.isEmpty()) {
                Text(
                    text = "搜索对话内容...",
                    color = placeholderColor,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            BasicTextField(
                value = value,
                onValueChange = onValueChange,
                singleLine = true,
                textStyle = MaterialTheme.typography.titleMedium.copy(
                    color = contentColor,
                    fontWeight = FontWeight.SemiBold,
                ),
                cursorBrush = SolidColor(contentColor),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

private data class DrawerSessionGroup(
    val title: String,
    val sessions: List<UiSessionSummary>,
)

private fun groupDrawerSessions(
    sessions: List<UiSessionSummary>,
    nowMs: ULong,
): List<DrawerSessionGroup> {
    val dayMs = 24UL * 60UL * 60UL * 1000UL
    val today = mutableListOf<UiSessionSummary>()
    val week = mutableListOf<UiSessionSummary>()
    val month = mutableListOf<UiSessionSummary>()
    val earlier = mutableListOf<UiSessionSummary>()

    sessions.forEach { session ->
        val timestamp = session.createdAtMs
        val ageMs = if (timestamp > nowMs) 0UL else nowMs - timestamp
        when {
            ageMs < dayMs -> today += session
            ageMs < 7UL * dayMs -> week += session
            ageMs < 30UL * dayMs -> month += session
            else -> earlier += session
        }
    }

    return listOf(
        DrawerSessionGroup("今天", today),
        DrawerSessionGroup("7 天内", week),
        DrawerSessionGroup("30 天内", month),
        DrawerSessionGroup("更早", earlier),
    )
}

@Composable
private fun DrawerCategoryHeader(
    text: String,
    color: Color,
) {
    Text(
        text = text,
        color = color,
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.SemiBold,
        modifier = Modifier.padding(vertical = 7.dp),
    )
}

@Composable
private fun DrawerSessionRow(
    session: UiSessionSummary,
    selected: Boolean,
    onClick: () -> Unit,
    onDelete: () -> Unit,
    onPin: () -> Unit,
    selectedBackground: Color,
    contentColor: Color,
) {
    var showMenu by remember { mutableStateOf(false) }
    var pressOffset by remember { mutableStateOf(Offset.Zero) }
    val density = LocalDensity.current
    val longPressHaptic = rememberSystemLongPressHapticFeedback()

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) selectedBackground else Color.Transparent)
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { onClick() },
                    onLongPress = { offset ->
                        longPressHaptic()
                        pressOffset = offset
                        showMenu = true
                    },
                )
            }
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = session.title.ifBlank { "新对话" },
            color = contentColor.copy(alpha = if (selected) 1f else 0.82f),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = if (selected) FontWeight.Bold else FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )

        if (showMenu) {
            HamburContextMenu(
                pressOffset = pressOffset,
                density = density,
                onDismiss = { showMenu = false },
            ) {
                ContextMenuActionRow(
                    icon = Lucide.Pin,
                    label = if (session.pinnedAtMs > 0UL) "取消置顶" else "置顶",
                    onClick = {
                        showMenu = false
                        onPin()
                    },
                )
                ContextMenuDivider()
                ContextMenuActionRow(
                    icon = Lucide.Trash2,
                    label = "删除",
                    danger = true,
                    onClick = {
                        showMenu = false
                        onDelete()
                    },
                )
            }
        }
    }
}

@Composable
private fun DrawerBottomBar(
    onOpenSettings: () -> Unit,
    avatarBackground: Color,
    contentColor: Color,
    mutedColor: Color,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(64.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                modifier = Modifier
                    .size(42.dp)
                    .clip(CircleShape)
                    .background(avatarBackground),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Lucide.User,
                    contentDescription = null,
                    tint = mutedColor,
                    modifier = Modifier.size(24.dp),
                )
            }
            Spacer(modifier = Modifier.width(12.dp))
            Text(
                text = "153******85",
                color = contentColor,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
        }

        ChatIconButton(
            onClick = onOpenSettings,
            size = 48.dp,
        ) {
            Icon(
                imageVector = Lucide.Settings,
                contentDescription = "Settings",
                tint = contentColor,
                modifier = Modifier.size(32.dp),
            )
        }
    }
}

@Composable
private fun BackendGapDialog(
    title: String,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("OK")
            }
        },
        title = { Text(title) },
        text = {
            Text(
                text = "The old UI had this action, but the new backend does not expose a command or persisted field for it yet.",
                style = MaterialTheme.typography.bodyMedium,
            )
        },
    )
}

@Composable
private fun ChatTimeline(
    state: HamburUiState,
    store: HamburUiStore,
    onOpenFile: (String) -> Unit,
    onEditMessage: (UiMessageSnapshot) -> Unit,
    onSelectText: (String) -> Unit,
    bottomPadding: Dp,
    blank: Boolean = false,
    modifier: Modifier = Modifier,
) {
    if (blank) {
        Box(modifier = modifier)
        return
    }

    val sessionId = state.selectedSessionId
    val initialScroll = remember(sessionId) {
        store.sessionScrollPosition(sessionId)
    }
    val listState = key(sessionId) {
        rememberLazyListState(
            initialFirstVisibleItemIndex = initialScroll.firstVisibleItemIndex,
            initialFirstVisibleItemScrollOffset = initialScroll.firstVisibleItemScrollOffset,
        )
    }
    SideEffect {
        ChatJankTracer.markSessionSwitch(
            phase = "chat_timeline_recompose",
            targetSessionId = sessionId,
            extra = "timelineItems=${state.timelineItems.size} markdownBlocks=${state.messageBlocksByPayloadRef.size}",
        )
    }
    val displayItems = remember(
        state.timelineItems,
        state.messageBlocksByPayloadRef,
        state.messagesById,
    ) {
        ChatJankTracer.timeSessionSwitch(
            phase = "display_items_build",
            targetSessionId = state.selectedSessionId,
            warnAtMs = 3.0,
            always = true,
            extra = "timelineItems=${state.timelineItems.size} markdownBlocks=${state.messageBlocksByPayloadRef.size}",
        ) {
            state.timelineItems.toChatDisplayItems(
                messageBlocksByPayloadRef = state.messageBlocksByPayloadRef,
                messagesById = state.messagesById,
            )
        }
    }
    LaunchedEffect(sessionId, listState) {
        if (sessionId.isBlank()) return@LaunchedEffect
        snapshotFlow {
            listState.firstVisibleItemIndex to listState.firstVisibleItemScrollOffset
        }
            .distinctUntilChanged()
            .collect { (index, offset) ->
                store.updateSessionScrollPosition(sessionId, index, offset)
            }
    }
    if (state.selectedSessionId.isBlank() || state.timelineItems.isEmpty()) {
        EmptyChatState(
            bottomPadding = bottomPadding,
            modifier = modifier,
        )
        return
    }

    val markdownStyle = rememberMarkdownStyle()
    val markdownCache = rememberMarkdownRenderCache()
    val thinkingDisplayMode = state.thinkingBlockDisplayMode()
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val dismissKeyboardScrollConnection = remember(focusManager, keyboardController) {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (source == NestedScrollSource.UserInput && (abs(available.y) > 0.5f || abs(available.x) > 0.5f)) {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                }
                return Offset.Zero
            }
        }
    }

    val density = LocalDensity.current
    val isDragged by listState.interactionSource.collectIsDraggedAsState()
    val isNearBottom by remember(listState) {
        derivedStateOf {
            val layoutInfo = listState.layoutInfo
            val visibleItems = layoutInfo.visibleItemsInfo
            if (visibleItems.isEmpty()) return@derivedStateOf true
            val total = layoutInfo.totalItemsCount
            if (total <= 2) return@derivedStateOf true
            visibleItems.last().index >= total - 2
        }
    }

    var lastBottomPaddingPx by remember(sessionId) {
        mutableFloatStateOf(with(density) { bottomPadding.toPx() })
    }

    LaunchedEffect(bottomPadding) {
        val currentBottomPx = with(density) { bottomPadding.toPx() }
        val deltaPx = currentBottomPx - lastBottomPaddingPx
        lastBottomPaddingPx = currentBottomPx
        if (deltaPx > 0.5f && isNearBottom && !isDragged) {
            listState.scrollBy(deltaPx)
        }
    }

    var previousItemCount by remember(sessionId) {
        mutableIntStateOf(displayItems.size)
    }
    LaunchedEffect(displayItems.size) {
        if (displayItems.size > previousItemCount) {
            if (isNearBottom && !isDragged && displayItems.isNotEmpty()) {
                listState.animateScrollToItem(displayItems.size)
            }
        }
        previousItemCount = displayItems.size
    }

    LazyColumn(
        state = listState,
        modifier = modifier
            .nestedScroll(dismissKeyboardScrollConnection)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                },
            )
            .onGloballyPositioned {
            ChatJankTracer.markSessionSwitchOnce(
                phase = "timeline_first_layout",
                key = "timeline_first_layout:$sessionId",
                targetSessionId = sessionId,
                extra = "displayItems=${displayItems.size} viewport=${it.size.width}x${it.size.height}",
            )
        },
        contentPadding = PaddingValues(
            start = 14.dp,
            top = 12.dp,
            end = 14.dp,
            bottom = bottomPadding,
        ),
        verticalArrangement = Arrangement.spacedBy(0.dp),
    ) {
        itemsIndexed(
            items = displayItems,
            key = { _, item -> item.stableKey },
            contentType = { _, item -> item.contentType },
        ) { index, item ->
            val previousItem = displayItems.getOrNull(index - 1)
            val topPadding = item.topSpacingAfter(previousItem)
            when (item) {
                is ChatDisplayItem.Timeline -> {
                    val timelineItem = item.item
                    when {
                        timelineItem.contentType == "user_message" && timelineItem.kind != "SyntheticUserMessage" -> {
                            val message = state.messagesById[timelineItem.payloadRef]
                            MessageTimelineItem(
                                item = timelineItem,
                                message = message,
                                onOpenFile = onOpenFile,
                                onSelectText = onSelectText,
                                onRetryMessage = {
                                    message?.let {
                                        store.regenerateMessage(state.selectedSessionId, it.id)
                                    }
                                },
                                onEditMessage = { message?.let(onEditMessage) },
                                thinkingDisplayMode = thinkingDisplayMode,
                                modifier = Modifier.padding(top = topPadding),
                            )
                        }

                        timelineItem.contentType == "trace" || timelineItem.kind.contains("Trace") -> {
                            StandaloneToolTraceItem(
                                item = timelineItem,
                                modifier = Modifier.padding(top = topPadding),
                            )
                        }

                        else -> {
                            TimelineSummaryItem(
                                item = timelineItem,
                                modifier = Modifier.padding(top = topPadding),
                            )
                        }
                    }
                }

                is ChatDisplayItem.AssistantProcessBlock -> {
                    val activeTurnId = state.activeTurnIds[sessionId].orEmpty()
                    val blockTurnId = if (item.turnId.isNotBlank()) item.turnId else state.messagesById[item.messageId]?.turnId.orEmpty()
                    val hasMarkdownAfterInTurn = displayItems.subList(index + 1, displayItems.size).any {
                        if (it !is ChatDisplayItem.AssistantMarkdownBlock) return@any false
                        val mdTurnId = state.messagesById[it.messageId]?.turnId.orEmpty()
                        blockTurnId.isBlank() || mdTurnId == blockTurnId
                    }
                    val isLastProcessInTurn = displayItems.subList(index + 1, displayItems.size).none {
                        if (it !is ChatDisplayItem.AssistantProcessBlock) return@none false
                        val otherTurnId = if (it.turnId.isNotBlank()) it.turnId else state.messagesById[it.messageId]?.turnId.orEmpty()
                        blockTurnId.isBlank() || otherTurnId == blockTurnId
                    }
                    val isGenerating = activeTurnId.isNotBlank() &&
                        (blockTurnId == activeTurnId || (blockTurnId.isBlank() && index >= displayItems.size - 2)) &&
                        !hasMarkdownAfterInTurn &&
                        isLastProcessInTurn
                    val durationSec = if (item.estimatedDurationSeconds > 0) {
                        item.estimatedDurationSeconds
                    } else {
                        state.turnDurationSeconds[blockTurnId]
                    }
                    ExecutionProcessBlock(
                        steps = item.steps,
                        isGenerating = isGenerating,
                        durationSeconds = durationSec,
                        displayMode = thinkingDisplayMode,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = topPadding),
                    )
                }

                is ChatDisplayItem.AssistantMarkdownBlock -> {
                    AssistantMarkdownBlockTimelineItem(
                        item = item,
                        sessionId = sessionId,
                        markdownStyle = markdownStyle,
                        markdownCache = markdownCache,
                        onOpenFile = onOpenFile,
                        onRegenerate = {
                            store.regenerateMessage(state.selectedSessionId, item.messageId)
                        },
                        onSelectText = onSelectText,
                        onResolveHostPath = { path ->
                            store.resolveSandboxHostPath(sessionId, path)
                        },
                        modifier = Modifier.padding(top = topPadding),
                    )
                }

                is ChatDisplayItem.AssistantActions -> {
                    AssistantMarkdownActionsItem(
                        item = item,
                        onRegenerate = {
                            store.regenerateMessage(state.selectedSessionId, item.messageId)
                        },
                        modifier = Modifier.padding(top = topPadding),
                    )
                }
            }
        }
        item(key = "bottom-anchor", contentType = "bottom-anchor") {
            Spacer(modifier = Modifier.height(1.dp))
        }
    }
}

@Composable
private fun EmptyChatState(
    bottomPadding: Dp,
    modifier: Modifier = Modifier,
) {
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val tokens = HamburTheme.tokens.chat
    Box(
        modifier = modifier
            .fillMaxSize()
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = {
                    focusManager.clearFocus()
                    keyboardController?.hide()
                },
            )
            .padding(bottom = bottomPadding),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = tokens.emptyStateHorizontalPadding),
        ) {
            ComposeImage(
                painter = painterResource(id = R.drawable.hambur_empty_icon_vector),
                contentDescription = null,
                contentScale = ContentScale.Fit,
                colorFilter = ColorFilter.tint(MaterialTheme.colorScheme.onBackground),
                modifier = Modifier.size(tokens.emptyStateIconSize),
            )
            Spacer(modifier = Modifier.height(tokens.emptyStateIconTextGap))
            Text(
                text = "我是 Hambur，有什么我可以帮您的？",
                color = MaterialTheme.colorScheme.onBackground,
                fontSize = tokens.emptyStateTitleFontSize,
                lineHeight = tokens.emptyStateTitleLineHeight,
                fontWeight = FontWeight.Medium,
                textAlign = TextAlign.Center,
                maxLines = 2,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun SelectTextDialog(
    text: String,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Select text") },
        text = {
            SelectionContainer {
                Text(
                    text = text,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = 360.dp)
                        .verticalScroll(rememberScrollState()),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("Done")
            }
        },
    )
}

@Composable
private fun MessageTimelineItem(
    item: UiTimelineItem,
    message: UiMessageSnapshot?,
    onOpenFile: (String) -> Unit,
    onSelectText: (String) -> Unit,
    onRetryMessage: () -> Unit,
    onEditMessage: () -> Unit,
    thinkingDisplayMode: String,
    modifier: Modifier = Modifier,
) {
    val role = message?.role ?: item.kind
    val isUser = role == "user"
    val context = LocalContext.current
    val messageText = message?.contentText?.ifBlank { item.smallSummary }
        ?: item.smallSummary.ifBlank { "Loading message..." }
    val attachments = message?.attachments ?: item.attachments

    Box(
        modifier = modifier.fillMaxWidth(),
        contentAlignment = Alignment.CenterEnd,
    ) {
        MessageLongPressMenuBox(
            enabled = isUser && message != null && messageText.isNotBlank(),
            onCopy = {
                copyTextToClipboard(context = context, text = messageText)
            },
            onSelectText = { onSelectText(messageText) },
            onRetry = onRetryMessage,
            onEdit = onEditMessage,
        ) {
            Surface(
                modifier = Modifier.widthIn(max = 324.dp),
                shape = RoundedCornerShape(18.dp),
                color = MaterialTheme.colorScheme.primaryContainer,
            ) {
                Column(
                    modifier = Modifier.padding(horizontal = 18.dp, vertical = 13.dp),
                ) {
                    if (!message?.reasoningContent.isNullOrBlank()) {
                        ExecutionProcessBlock(
                            steps = listOf(
                                ProcessStep.Reasoning(
                                    id = message.id,
                                    text = message.reasoningContent,
                                )
                            ),
                            isGenerating = false,
                            durationSeconds = null,
                            displayMode = thinkingDisplayMode,
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(bottom = 8.dp),
                        )
                    }
                    if (attachments.isNotEmpty()) {
                        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            attachments.forEach { attachment ->
                                MessageAttachmentRow(
                                    attachment = attachment,
                                    onOpen = { onOpenFile(attachment.sandboxPath) },
                                )
                            }
                        }
                    }
                    Text(
                        text = messageText,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
            }
        }
    }
}

@Composable
private fun MessageLongPressMenuBox(
    enabled: Boolean,
    onCopy: () -> Unit,
    onSelectText: () -> Unit,
    onRetry: (() -> Unit)? = null,
    onEdit: (() -> Unit)? = null,
    onRegenerate: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    var showMenu by remember { mutableStateOf(false) }
    var pressOffset by remember { mutableStateOf(Offset.Zero) }
    val density = LocalDensity.current
    val longPressHaptic = rememberSystemLongPressHapticFeedback()

    Box(
        modifier = Modifier.pointerInput(enabled) {
            awaitEachGesture {
                val down = awaitFirstDown(
                    requireUnconsumed = false,
                    pass = PointerEventPass.Initial,
                )
                if (!enabled) return@awaitEachGesture
                val longPressTimeout = viewConfiguration.longPressTimeoutMillis
                var currentPosition = down.position
                var cancelled = false
                val slop = viewConfiguration.touchSlop
                val result = withTimeoutOrNull(longPressTimeout) {
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        val change = event.changes.firstOrNull { it.id == down.id }
                        if (change == null || !change.pressed) {
                            cancelled = true
                            break
                        }
                        currentPosition = change.position
                        val dx = currentPosition.x - down.position.x
                        val dy = currentPosition.y - down.position.y
                        if (dx * dx + dy * dy > slop * slop) {
                            cancelled = true
                            break
                        }
                    }
                }
                if (result == null && !cancelled) {
                    longPressHaptic()
                    pressOffset = currentPosition
                    showMenu = true
                }
            }
        },
    ) {
        content()
        if (showMenu) {
            HamburContextMenu(
                pressOffset = pressOffset,
                density = density,
                onDismiss = { showMenu = false },
            ) {
                ContextMenuActionRow(
                    icon = Lucide.Copy,
                    label = "复制",
                    onClick = {
                        showMenu = false
                        onCopy()
                    },
                )
                ContextMenuDivider()
                ContextMenuActionRow(
                    icon = Lucide.Type,
                    label = "选择文本",
                    onClick = {
                        showMenu = false
                        onSelectText()
                    },
                )
                onRetry?.let { retry ->
                    ContextMenuDivider()
                    ContextMenuActionRow(
                        icon = Lucide.RefreshCw,
                        label = "重新生成",
                        onClick = {
                            showMenu = false
                            retry()
                        },
                    )
                }
                onRegenerate?.let { regenerate ->
                    ContextMenuDivider()
                    ContextMenuActionRow(
                        icon = Lucide.RefreshCw,
                        label = "重新生成",
                        onClick = {
                            showMenu = false
                            regenerate()
                        },
                    )
                }
                onEdit?.let { edit ->
                    ContextMenuDivider()
                    ContextMenuActionRow(
                        icon = Lucide.Pencil,
                        label = "编辑",
                        onClick = {
                            showMenu = false
                            edit()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun ExecutionProcessBlock(
    steps: List<ProcessStep>,
    isGenerating: Boolean,
    durationSeconds: Int?,
    displayMode: String,
    modifier: Modifier = Modifier,
) {
    if (steps.isEmpty()) return

    var isExpanded by rememberSaveable {
        mutableStateOf(initialThinkingBlockExpanded(displayMode, isGenerating))
    }
    var manuallyToggled by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(isGenerating, displayMode) {
        if (manuallyToggled) return@LaunchedEffect
        when (displayMode) {
            THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE -> isExpanded = isGenerating
            THINKING_BLOCK_DISPLAY_AUTO_EXPAND -> if (isGenerating) isExpanded = true
            else -> Unit
        }
    }

    val infiniteTransition = rememberInfiniteTransition(label = "process_pulse")
    val alpha by infiniteTransition.animateFloat(
        initialValue = 0.4f,
        targetValue = 1.0f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "process_alpha"
    )

    val rotationAngle by animateFloatAsState(
        targetValue = if (isExpanded) 90f else 0f,
        animationSpec = tween(durationMillis = 200, easing = FastOutSlowInEasing),
        label = "chevron_rotation"
    )

    val outlineColor = MaterialTheme.colorScheme.outlineVariant
    val contentColor = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.85f)
    val stepTextStyle = MaterialTheme.typography.bodyMedium.copy(
        fontSize = 13.sp,
        lineHeight = 20.sp,
        fontWeight = FontWeight.Normal,
    )

    val headerTitle = when {
        isGenerating -> "正在思考..."
        durationSeconds != null && durationSeconds > 0 -> "已思考（用时 $durationSeconds 秒）"
        else -> "已思考"
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(8.dp))
                .clickable {
                    manuallyToggled = true
                    isExpanded = !isExpanded
                }
                .padding(vertical = 6.dp, horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                imageVector = Lucide.Sparkles,
                contentDescription = null,
                tint = if (isGenerating) {
                    MaterialTheme.colorScheme.primary.copy(alpha = alpha)
                } else {
                    MaterialTheme.colorScheme.primary.copy(alpha = 0.85f)
                },
                modifier = Modifier.size(15.dp)
            )

            Spacer(modifier = Modifier.width(8.dp))

            Text(
                text = headerTitle,
                style = MaterialTheme.typography.bodyMedium.copy(
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Medium,
                    lineHeight = 18.sp,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.88f),
                modifier = Modifier.weight(1f)
            )

            Icon(
                imageVector = Lucide.ChevronRight,
                contentDescription = if (isExpanded) "收起" else "展开",
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                modifier = Modifier
                    .size(15.dp)
                    .graphicsLayer(rotationZ = rotationAngle)
            )
        }

        if (isExpanded) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(start = 2.dp, top = 4.dp, bottom = 6.dp)
            ) {
                val totalSteps = steps.size
                steps.forEachIndexed { index, step ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .drawBehind {
                                if (totalSteps > 1) {
                                    val railX = 9.dp.toPx()
                                    val nodeY = 10.dp.toPx()
                                    val strokeW = 1.2.dp.toPx()
                                    val lineColor = outlineColor.copy(alpha = 0.35f)

                                    if (index > 0) {
                                        drawLine(
                                            color = lineColor,
                                            start = Offset(railX, 0f),
                                            end = Offset(railX, nodeY),
                                            strokeWidth = strokeW,
                                        )
                                    }
                                    if (index < totalSteps - 1) {
                                        drawLine(
                                            color = lineColor,
                                            start = Offset(railX, nodeY),
                                            end = Offset(railX, size.height),
                                            strokeWidth = strokeW,
                                        )
                                    }
                                }
                            }
                            .padding(vertical = 3.dp),
                        verticalAlignment = Alignment.Top,
                    ) {
                        Box(
                            modifier = Modifier
                                .width(18.dp)
                                .height(20.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            when (step) {
                                is ProcessStep.Reasoning -> {
                                    Box(
                                        modifier = Modifier
                                            .size(5.dp)
                                            .background(
                                                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f),
                                                shape = CircleShape,
                                            )
                                    )
                                }
                                is ProcessStep.ToolCall -> {
                                    Icon(
                                        imageVector = toolIconFor(step.toolName),
                                        contentDescription = null,
                                        tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.75f),
                                        modifier = Modifier.size(14.dp),
                                    )
                                }
                            }
                        }

                        Spacer(modifier = Modifier.width(8.dp))

                        Column(
                            modifier = Modifier
                                .weight(1f)
                                .padding(end = 4.dp)
                        ) {
                            when (step) {
                                is ProcessStep.Reasoning -> {
                                    Text(
                                        text = step.text,
                                        style = stepTextStyle,
                                        color = contentColor,
                                        modifier = Modifier.fillMaxWidth(),
                                    )
                                }
                                is ProcessStep.ToolCall -> {
                                    ProcessToolStepRow(
                                        step = step,
                                        textStyle = stepTextStyle,
                                        contentColor = contentColor,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ProcessToolStepRow(
    step: ProcessStep.ToolCall,
    textStyle: TextStyle,
    contentColor: Color,
    modifier: Modifier = Modifier,
) {
    val displayTitle = formatToolStepTitle(step.toolName, step.title)

    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            text = displayTitle,
            style = textStyle,
            color = contentColor,
            modifier = Modifier.weight(1f, fill = false),
        )
        if (step.status == "running") {
            Text(
                text = "(执行中...)",
                style = textStyle.copy(
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = MaterialTheme.colorScheme.primary,
            )
        } else if (step.status == "failed" || step.status == "error") {
            Text(
                text = "(失败)",
                style = textStyle.copy(
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

private fun toolIconFor(toolName: String): ImageVector {
    return when (toolName.lowercase().trim()) {
        "web_search", "search" -> Lucide.Search
        "browser_use", "browser", "browse" -> Lucide.Globe
        "terminal", "run_command", "bash", "sh", "android_cli" -> Lucide.Terminal
        "process" -> Lucide.Terminal
        "read_file", "file_read", "cat" -> Lucide.FileText
        "write_file", "file_write" -> Lucide.FilePen
        "patch", "edit", "file_edit" -> Lucide.FileDiff
        "search_files", "find_by_name", "grep_search" -> Lucide.FolderSearch
        "session_search" -> Lucide.MessageSquare
        "view_image" -> Lucide.Image
        "skills_list", "skill_view" -> Lucide.WandSparkles
        "delegate_task", "subagent", "agent" -> Lucide.Bot
        else -> Lucide.Wrench
    }
}

private fun formatToolStepTitle(toolName: String, rawTitle: String): String {
    val title = rawTitle.trim()
    return when {
        title.startsWith("Search web: ") -> "搜索网页: " + title.removePrefix("Search web: ")
        title == "Search web" -> "搜索网页"
        title.startsWith("Search files: ") -> "搜索文件: " + title.removePrefix("Search files: ")
        title == "Search files" -> "搜索文件"
        title.startsWith("Browser: ") -> "浏览网页: " + title.removePrefix("Browser: ")
        title == "Use browser" -> "浏览网页"
        title.startsWith("Run terminal command: ") -> "执行终端命令: " + title.removePrefix("Run terminal command: ")
        title == "Run terminal command" -> "执行终端命令"
        title.startsWith("Read file: ") -> "读取文件: " + title.removePrefix("Read file: ")
        title.startsWith("Write file: ") -> "写入文件: " + title.removePrefix("Write file: ")
        title.startsWith("Patch file: ") -> "编辑文件: " + title.removePrefix("Patch file: ")
        title.startsWith("Search sessions: ") -> "搜索历史对话: " + title.removePrefix("Search sessions: ")
        title.startsWith("View image: ") -> "查看图片: " + title.removePrefix("View image: ")
        title.startsWith("Delegate session") -> "委派子任务"
        title.isNotBlank() -> title
        toolName.isNotBlank() -> defaultToolTitle(toolName)
        else -> "调用工具"
    }
}

private fun defaultToolTitle(toolName: String): String {
    return when (toolName.lowercase().trim()) {
        "web_search", "search" -> "搜索网页"
        "browser_use", "browser" -> "浏览网页"
        "terminal", "run_command", "bash", "android_cli" -> "执行终端命令"
        "process" -> "控制进程"
        "read_file" -> "读取文件"
        "write_file" -> "写入文件"
        "patch" -> "编辑文件"
        "search_files" -> "搜索文件"
        "session_search" -> "搜索历史对话"
        "view_image" -> "查看图片"
        "skills_list", "skill_view" -> "加载技能"
        "delegate_task" -> "委派子任务"
        else -> "调用工具: $toolName"
    }
}

@Composable
private fun AssistantMarkdownBlockTimelineItem(
    item: ChatDisplayItem.AssistantMarkdownBlock,
    sessionId: String,
    markdownStyle: MarkdownStyle,
    markdownCache: MarkdownRenderCache,
    onOpenFile: (String) -> Unit,
    onRegenerate: () -> Unit,
    onSelectText: (String) -> Unit,
    onResolveHostPath: ((String) -> String)? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val node = item.node
    val assistantText = item.assistantText

    SideEffect {
        ChatJankTracer.markSessionSwitchOnce(
            phase = "markdown_block_composed",
            key = "markdown_block:${node.messageId}:${node.blockId}",
            targetSessionId = sessionId,
            extra = "kind=${node.nodeKind} message=${shortTraceId(node.messageId)} block=${node.blockId}",
        )
    }
    MessageLongPressMenuBox(
        enabled = assistantText.isNotBlank(),
        onCopy = {
            copyTextToClipboard(context = context, text = assistantText)
        },
        onSelectText = { onSelectText(assistantText) },
        onRegenerate = onRegenerate,
    ) {
        Column(modifier = modifier.fillMaxWidth()) {
            MarkdownBlock(
                node = node,
                modifier = Modifier.fillMaxWidth(),
                style = markdownStyle,
                renderCache = markdownCache,
                onOpenDestination = onOpenFile,
                onResolveHostPath = onResolveHostPath,
            )
        }
    }
}

@Composable
private fun AssistantMarkdownActionsItem(
    item: ChatDisplayItem.AssistantActions,
    onRegenerate: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val clipboard = remember(context) {
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }

    if (item.assistantText.isBlank()) return

    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ChatInlineActionButton(
            onClick = {
                clipboard.setPrimaryClip(
                    ClipData.newPlainText("assistant message", item.assistantText),
                )
                Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
            },
            contentDescription = "Copy",
        ) {
            Icon(
                imageVector = Lucide.Copy,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(16.dp),
            )
        }
        ChatInlineActionButton(
            onClick = onRegenerate,
            contentDescription = "Regenerate",
        ) {
            Icon(
                imageVector = Lucide.RefreshCw,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(16.dp),
            )
        }
    }
}

@Composable
private fun HamburContextMenu(
    pressOffset: Offset,
    density: Density,
    onDismiss: () -> Unit,
    content: @Composable () -> Unit,
) {
    val isDark = MaterialTheme.colorScheme.background.luminance() <= 0.5f
    val popupBgColor = if (isDark) Color(0xFF2C2C2E) else Color.White
    val popupBorderColor = if (isDark) Color.White.copy(alpha = 0.08f) else Color(0xFFE5E5EA)

    Popup(
        popupPositionProvider = ContextMenuPositionProvider(pressOffset, density),
        onDismissRequest = onDismiss,
        properties = PopupProperties(focusable = true),
    ) {
        Card(
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = popupBgColor),
            modifier = Modifier
                .width(160.dp)
                .border(0.5.dp, popupBorderColor, RoundedCornerShape(16.dp)),
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                content()
            }
        }
    }
}

@Composable
private fun ContextMenuActionRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val color = if (danger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onBackground
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(44.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = color,
            modifier = Modifier.size(16.dp),
        )
        Spacer(modifier = Modifier.width(12.dp))
        Text(
            text = label,
            color = color,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
        )
    }
}

@Composable
private fun ContextMenuDivider() {
    val isDark = MaterialTheme.colorScheme.background.luminance() <= 0.5f
    HorizontalDivider(
        thickness = 0.5.dp,
        color = if (isDark) Color(0xFF3E3E40) else Color(0xFFE5E5EA),
    )
}

private class ContextMenuPositionProvider(
    private val pressOffset: Offset,
    private val density: Density,
) : PopupPositionProvider {
    override fun calculatePosition(
        anchorBounds: IntRect,
        windowSize: IntSize,
        layoutDirection: LayoutDirection,
        popupContentSize: IntSize,
    ): IntOffset {
        val touchX = anchorBounds.left + pressOffset.x
        val touchY = anchorBounds.top + pressOffset.y
        val marginPx = with(density) { 16.dp.toPx() }.toInt()
        val gapPx = with(density) { 10.dp.toPx() }

        var x = (touchX - popupContentSize.width / 2f).toInt()
        if (x < marginPx) x = marginPx
        if (x + popupContentSize.width > windowSize.width - marginPx) {
            x = windowSize.width - popupContentSize.width - marginPx
        }

        var y = if (touchY - popupContentSize.height - gapPx > 0f) {
            (touchY - popupContentSize.height - gapPx).toInt()
        } else {
            (touchY + gapPx).toInt()
        }
        if (y < marginPx) y = marginPx
        if (y + popupContentSize.height > windowSize.height - marginPx) {
            y = windowSize.height - popupContentSize.height - marginPx
        }

        return IntOffset(x, y)
    }
}

@Composable
private fun rememberSystemLongPressHapticFeedback(): () -> Unit {
    val view = LocalView.current
    return remember(view) {
        { view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS) }
    }
}

// TOGGLE_ON/TOGGLE_OFF/CONFIRM/REJECT 是 API 34 新增常量，minSdk 31 需版本守卫
private class SemanticHaptics(private val view: View) {
    fun toggle(turningOn: Boolean) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            view.performHapticFeedback(
                if (turningOn) HapticFeedbackConstants.TOGGLE_ON else HapticFeedbackConstants.TOGGLE_OFF,
            )
        }
    }

    fun confirm() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
        }
    }

    fun reject() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            view.performHapticFeedback(HapticFeedbackConstants.REJECT)
        }
    }
}

@Composable
private fun rememberSemanticHaptics(): SemanticHaptics {
    val view = LocalView.current
    return remember(view) { SemanticHaptics(view) }
}

private fun copyTextToClipboard(
    context: Context,
    text: String,
) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText("text", text))
    Toast.makeText(context, "已复制到剪贴板", Toast.LENGTH_SHORT).show()
}

@Composable
private fun ChatInlineActionButton(
    onClick: () -> Unit,
    contentDescription: String,
    content: @Composable () -> Unit,
) {
    IconButton(
        onClick = onClick,
        modifier = Modifier.size(28.dp),
    ) {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center,
        ) {
            content()
        }
    }
}

@Composable
private fun MessageAttachmentRow(
    attachment: UiPendingAttachment,
    onOpen: () -> Unit,
) {
    Surface(
        onClick = onOpen,
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(
                imageVector = if (attachment.mimeType.startsWith("image/")) Lucide.Image else Lucide.FileText,
                contentDescription = null,
                modifier = Modifier.size(18.dp),
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = attachment.displayName,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = listOf(attachment.mimeType, "${attachment.byteSize} bytes")
                        .filter { it.isNotBlank() }
                        .joinToString(" - "),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun StandaloneToolTraceItem(
    item: UiTimelineItem,
    modifier: Modifier = Modifier,
) {
    val displayTitle = formatToolStepTitle(item.toolName, item.traceTitle.ifBlank { item.smallSummary })
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(
            imageVector = toolIconFor(item.toolName),
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.8f),
            modifier = Modifier.size(15.dp),
        )
        Text(
            text = displayTitle,
            style = MaterialTheme.typography.bodyMedium.copy(
                fontSize = 13.sp,
                fontWeight = FontWeight.Normal,
            ),
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.85f),
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (item.traceStatus == "running") {
            Text(
                text = "(执行中...)",
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 11.sp),
                color = MaterialTheme.colorScheme.primary.copy(alpha = 0.85f),
            )
        }
    }
}

@Composable
private fun TimelineSummaryItem(
    item: UiTimelineItem,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        SummaryLine(
            label = item.kind.ifBlank { item.contentType },
            value = item.smallSummary,
            modifier = Modifier.padding(12.dp),
        )
    }
}

@Composable
private fun ChatInputPanel(
    message: String,
    onMessageChange: (String) -> Unit,
    enabled: Boolean,
    generating: Boolean,
    thinkingEnabled: Boolean,
    attachmentPanelOpen: Boolean,
    attachmentPanelSlotHeight: Dp,
    attachmentPanelHeight: Dp,
    pendingAttachments: List<UiPendingAttachment>,
    editing: Boolean,
    onToggleThinking: () -> Unit,
    onToggleAttachmentPanel: () -> Unit,
    onCloseAttachmentPanel: () -> Unit = {},
    onImportAttachment: (String, String, ULong, String, String) -> Unit,
    onPickImage: (((String, String, ULong, String, String) -> Unit) -> Unit),
    onPickFile: (((String, String, ULong, String, String) -> Unit) -> Unit),
    onRemoveAttachment: (String) -> Unit,
    onStop: () -> Unit,
    onCancelEdit: () -> Unit,
    onSend: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val tokens = HamburTheme.tokens.chat
    val panelShape = RoundedCornerShape(tokens.inputCornerRadius)
    val panelBorder = MaterialTheme.colorScheme.outlineVariant.copy(alpha = tokens.inputBorderAlpha)
    val primaryText = MaterialTheme.colorScheme.onSurface
    val secondaryText = MaterialTheme.colorScheme.onSurfaceVariant
    val canSend = enabled && !generating && (message.isNotBlank() || pendingAttachments.isNotEmpty())

    val navInsets = WindowInsets.navigationBars
    val imeInsets = WindowInsets.ime
    val density = LocalDensity.current
    val bgColor = MaterialTheme.colorScheme.background

    var isInputFocused by remember { mutableStateOf(false) }

    val navBottom = with(density) { navInsets.getBottom(this).toDp() }
    val imeBottom = with(density) { imeInsets.getBottom(this).toDp() }
    val systemBottom = with(density) { navInsets.union(imeInsets).getBottom(this).toDp() }

    val panelTotalHeight = if (attachmentPanelSlotHeight > 0.dp) {
        attachmentPanelSlotHeight + navBottom
    } else {
        0.dp
    }
    val effectiveBottomSlotHeight = maxOf(panelTotalHeight, systemBottom)

    LaunchedEffect(isInputFocused, attachmentPanelOpen) {
        if (isInputFocused && attachmentPanelOpen) {
            var elapsed = 0
            while (isInputFocused && attachmentPanelOpen && elapsed < 350) {
                delay(32)
                elapsed += 32
                val currentSystem = navInsets.union(imeInsets).getBottom(density)
                val currentPanelPx = with(density) { panelTotalHeight.roundToPx() }
                val currentIme = imeInsets.getBottom(density)
                if (currentIme > 0 && (currentSystem >= currentPanelPx || elapsed >= 250)) {
                    onCloseAttachmentPanel()
                    break
                }
            }
        }
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(top = tokens.inputOuterTopPadding)
            .drawBehind {
                val bottomAreaPx = with(density) { (tokens.inputOuterBottomPadding + effectiveBottomSlotHeight).toPx() }
                val extraHeightPx = (tokens.inputOuterBottomPadding * 4).toPx()
                val totalHeightPx = bottomAreaPx + extraHeightPx

                drawRect(
                    color = bgColor,
                    topLeft = Offset(0f, size.height - totalHeightPx),
                    size = androidx.compose.ui.geometry.Size(size.width, totalHeightPx)
                )
            },
    ) {
        if (editing) {
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = tokens.inputOuterStartPadding),
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        text = "Editing message",
                        style = MaterialTheme.typography.labelMedium,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = onCancelEdit) {
                        Text("Cancel")
                    }
                }
            }
            Spacer(modifier = Modifier.height(tokens.inputOuterGap))
        }
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = tokens.inputOuterStartPadding)
                .zIndex(1f),
            shape = panelShape,
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(tokens.inputBorderWidth, panelBorder),
            shadowElevation = tokens.inputShadowElevation,
        ) {
            Column(
                modifier = Modifier.padding(
                    start = tokens.inputInnerStartPadding,
                    top = tokens.inputInnerTopPadding,
                    end = tokens.inputInnerEndPadding,
                    bottom = tokens.inputInnerBottomPadding,
                ),
                verticalArrangement = Arrangement.spacedBy(tokens.inputContentGap),
            ) {
                if (pendingAttachments.isNotEmpty()) {
                    LazyRow(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        items(pendingAttachments, key = { it.id }) { attachment ->
                            PendingAttachmentChip(
                                attachment = attachment,
                                onRemove = { onRemoveAttachment(attachment.id) },
                            )
                        }
                    }
                }
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(
                            min = tokens.inputTextMinHeight,
                            max = tokens.inputTextMaxHeight,
                        )
                        .verticalScroll(rememberScrollState()),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (message.isBlank()) {
                        Text(
                            text = "发消息或按住说话",
                            color = secondaryText.copy(alpha = tokens.inputPlaceholderAlpha),
                            style = MaterialTheme.typography.bodyLarge.copy(
                                color = secondaryText.copy(alpha = tokens.inputPlaceholderAlpha),
                            ),
                            modifier = Modifier.padding(start = tokens.inputTextStartPadding),
                        )
                    }
                    BasicTextField(
                        value = message,
                        onValueChange = onMessageChange,
                        enabled = enabled,
                        textStyle = MaterialTheme.typography.bodyLarge.copy(
                            color = primaryText,
                        ),
                        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = tokens.inputTextStartPadding)
                            .onFocusChanged { focusState ->
                                isInputFocused = focusState.isFocused
                            },
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    ChatIconButton(
                        onClick = onToggleThinking,
                        enabled = enabled,
                        size = tokens.inputIconButtonSize,
                        modifier = Modifier.offset(
                            x = tokens.inputLeftIconOffsetX,
                            y = tokens.inputIconOffsetY,
                        ),
                    ) {
                        Icon(
                            imageVector = Lucide.Brain,
                            contentDescription = "Think",
                            tint = if (thinkingEnabled) Color(THINKING_ACTIVE_COLOR) else primaryText,
                            modifier = Modifier.size(tokens.inputIconSize),
                        )
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    ChatIconButton(
                        onClick = onToggleAttachmentPanel,
                        enabled = enabled,
                        size = tokens.inputIconButtonSize,
                        modifier = Modifier.offset(
                            x = tokens.inputIconOffsetX,
                            y = tokens.inputIconOffsetY,
                        ),
                    ) {
                        Icon(
                            imageVector = if (attachmentPanelOpen) Lucide.CircleX else Lucide.CirclePlus,
                            contentDescription = "Attachments",
                            tint = primaryText,
                            modifier = Modifier.size(tokens.inputIconSize),
                        )
                    }
                    Spacer(modifier = Modifier.width(tokens.inputRightIconGap))
                    if (generating) {
                        ChatIconButton(
                            onClick = onStop,
                            enabled = enabled,
                            size = tokens.inputIconButtonSize,
                            modifier = Modifier.offset(
                                x = tokens.inputIconOffsetX,
                                y = tokens.inputIconOffsetY,
                            ),
                        ) {
                            Icon(
                                imageVector = Lucide.CirclePause,
                                contentDescription = "Stop",
                                tint = MaterialTheme.colorScheme.error,
                                modifier = Modifier.size(tokens.inputIconSize),
                            )
                        }
                    } else {
                        ChatIconButton(
                            onClick = onSend,
                            enabled = canSend,
                            size = tokens.inputIconButtonSize,
                            modifier = Modifier.offset(
                                x = tokens.inputIconOffsetX,
                                y = tokens.inputIconOffsetY,
                            ),
                        ) {
                            Icon(
                                imageVector = Lucide.CircleArrowUp,
                                contentDescription = "Send",
                                tint = if (canSend) {
                                    primaryText
                                } else {
                                    secondaryText.copy(alpha = tokens.inputPlaceholderAlpha)
                                },
                                modifier = Modifier.size(tokens.inputIconSize),
                            )
                        }
                    }
                }
            }
        }
        Spacer(
            modifier = Modifier
                .fillMaxWidth()
                .height(tokens.inputOuterBottomPadding)
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(effectiveBottomSlotHeight)
                .clipToBounds(),
        ) {
            if (attachmentPanelSlotHeight > 0.dp) {
                AttachmentPickerPanel(
                    enabled = enabled,
                    pendingAttachments = pendingAttachments,
                    onImportAttachment = onImportAttachment,
                    onPickImage = onPickImage,
                    onPickFile = onPickFile,
                    onRemoveAttachment = onRemoveAttachment,
                    modifier = Modifier
                        .padding(top = tokens.inputOuterGap)
                        .height(attachmentPanelHeight),
                )
            }
        }
    }
}

@Composable
private fun AttachmentPickerPanel(
    enabled: Boolean,
    pendingAttachments: List<UiPendingAttachment>,
    onImportAttachment: (String, String, ULong, String, String) -> Unit,
    onPickImage: (((String, String, ULong, String, String) -> Unit) -> Unit),
    onPickFile: (((String, String, ULong, String, String) -> Unit) -> Unit),
    onRemoveAttachment: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val isDark = MaterialTheme.colorScheme.background.luminance() <= 0.5f
    val panelBgColor = MaterialTheme.colorScheme.background
    val labelColor = if (isDark) Color.White else Color.Black
    val textPrimary = MaterialTheme.colorScheme.onBackground
    val searchBoxBackground = if (isDark) Color(0xFF1E1E20) else Color(0xFFE5E5EA)
    val imgBorderColor = if (isDark) Color.White.copy(alpha = 0.12f) else Color.Black.copy(alpha = 0.08f)
    val galleryPermissions = imageReadPermissions()
    var recentImages by remember { mutableStateOf<List<Uri>>(emptyList()) }

    fun checkAndLoadImages() {
        scope.launch(Dispatchers.IO) {
            val images = if (hasImageReadPermission(context)) {
                fetchRecentImages(context)
            } else {
                emptyList()
            }
            withContext(Dispatchers.Main) {
                recentImages = images
            }
        }
    }

    fun importUri(uri: Uri, fallbackName: String, fallbackMime: String) {
        scope.launch(Dispatchers.IO) {
            val displayName = getDisplayNameForUri(context, uri).ifBlank { fallbackName }
            val mimeType = context.contentResolver.getType(uri).orEmpty().ifBlank { fallbackMime }
            val size = getSizeForUri(context, uri).coerceAtLeast(0L).toULong()
            val cachedPath = copyUriToAttachmentCache(context, uri, displayName)
            withContext(Dispatchers.Main) {
                onImportAttachment(
                    displayName,
                    mimeType,
                    size,
                    uri.toString(),
                    cachedPath,
                )
            }
        }
    }

    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current

    LaunchedEffect(Unit) {
        focusManager.clearFocus()
        keyboardController?.hide()
        checkAndLoadImages()
    }

    val cameraPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { isGranted ->
        Toast.makeText(
            context,
            if (isGranted) "相机权限已获取" else "相机权限已被拒绝",
            Toast.LENGTH_SHORT,
        ).show()
    }

    val galleryPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants ->
        if (grants.values.any { it }) {
            Toast.makeText(context, "相册权限已获取", Toast.LENGTH_SHORT).show()
            checkAndLoadImages()
        } else {
            Toast.makeText(context, "相册权限已被拒绝", Toast.LENGTH_SHORT).show()
        }
    }

    val takePictureLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicturePreview(),
    ) { bitmap: android.graphics.Bitmap? ->
        if (bitmap != null) {
            val savedUri = saveCameraPreviewToCache(context, bitmap)
            if (savedUri != null) {
                importUri(
                    savedUri,
                    savedUri.lastPathSegment ?: "camera_${System.currentTimeMillis()}.jpg",
                    "image/jpeg",
                )
                Toast.makeText(context, "已拍摄照片", Toast.LENGTH_SHORT).show()
            } else {
                Toast.makeText(context, "照片保存失败", Toast.LENGTH_SHORT).show()
            }
        }
    }

    val pickerNestedScrollConnection = remember {
        object : NestedScrollConnection {
            override fun onPostScroll(
                consumed: Offset,
                available: Offset,
                source: NestedScrollSource,
            ): Offset {
                // Absorb any excess horizontal scroll delta so it never bubbles up
                return if (available.x != 0f) Offset(available.x, 0f) else Offset.Zero
            }

            override suspend fun onPostFling(
                consumed: Velocity,
                available: Velocity,
            ): Velocity {
                // Absorb any excess horizontal fling velocity so it never bubbles up
                return if (available.x != 0f) Velocity(available.x, 0f) else Velocity.Zero
            }
        }
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .height(220.dp)
            .background(panelBgColor)
            .nestedScroll(pickerNestedScrollConnection)
            .scrollable(
                state = rememberScrollableState { 0f },
                orientation = Orientation.Horizontal,
            )
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                onClick = {},
            )
            .padding(vertical = 16.dp, horizontal = 12.dp),
    ) {
        if (recentImages.isNotEmpty()) {
            val recentImageListState = rememberLazyListState()
            LazyRow(
                state = recentImageListState,
                modifier = Modifier
                    .fillMaxWidth()
                    .nestedScroll(pickerNestedScrollConnection)
                    .padding(bottom = 20.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(recentImages, key = { it.toString() }) { uri ->
                    val selectedAttachment = pendingAttachments.find { it.originalUri == uri.toString() }
                    val isSelected = selectedAttachment != null
                    Box(
                        modifier = Modifier
                            .size(75.dp)
                            .clip(RoundedCornerShape(14.dp))
                            .border(0.5.dp, imgBorderColor, RoundedCornerShape(14.dp))
                            .background(searchBoxBackground)
                            .clickable(enabled = enabled) {
                                if (selectedAttachment != null) {
                                    onRemoveAttachment(selectedAttachment.id)
                                } else {
                                    importUri(
                                        uri,
                                        "image_${System.currentTimeMillis()}.jpg",
                                        "image/*",
                                    )
                                }
                            },
                    ) {
                        AsyncImage(
                            model = uri,
                            contentDescription = null,
                            modifier = Modifier.fillMaxSize(),
                            contentScale = ContentScale.Crop,
                        )

                        if (isSelected) {
                            Box(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .background(Color.Black.copy(alpha = 0.4f)),
                            )
                        }

                        Box(
                            modifier = Modifier
                                .align(Alignment.TopEnd)
                                .padding(6.dp)
                                .size(18.dp)
                                .clip(CircleShape)
                                .border(1.5.dp, Color.White, CircleShape)
                                .background(
                                    if (isSelected) {
                                        MaterialTheme.colorScheme.tertiary
                                    } else {
                                        Color.Black.copy(alpha = 0.4f)
                                    },
                                ),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (isSelected) {
                                Icon(
                                    imageVector = Lucide.Check,
                                    contentDescription = null,
                                    tint = Color.White,
                                    modifier = Modifier.size(12.dp),
                                )
                            }
                        }
                    }
                }
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val buttons = remember {
                listOf(
                    Triple("拍照", Lucide.Camera, "camera"),
                    Triple("相册", Lucide.Image, "gallery"),
                    Triple("文件", Lucide.Folder, "file"),
                )
            }

            buttons.forEach { (label, icon, type) ->
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .height(72.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(searchBoxBackground.copy(alpha = 0.5f))
                        .clickable(enabled = enabled) {
                            when (type) {
                                "camera" -> {
                                    if (androidx.core.content.ContextCompat.checkSelfPermission(
                                            context,
                                            android.Manifest.permission.CAMERA,
                                        ) == android.content.pm.PackageManager.PERMISSION_GRANTED
                                    ) {
                                        takePictureLauncher.launch(null)
                                    } else {
                                        cameraPermissionLauncher.launch(android.Manifest.permission.CAMERA)
                                    }
                                }

                                "gallery" -> {
                                    if (hasImageReadPermission(context)) {
                                        onPickImage { displayName, mimeType, byteSize, uri, sourcePath ->
                                            onImportAttachment(displayName, mimeType, byteSize, uri, sourcePath)
                                        }
                                    } else {
                                        galleryPermissionLauncher.launch(galleryPermissions)
                                    }
                                }

                                "file" -> {
                                    onPickFile { displayName, mimeType, byteSize, uri, sourcePath ->
                                        onImportAttachment(displayName, mimeType, byteSize, uri, sourcePath)
                                    }
                                }
                            }
                        },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Icon(
                        imageVector = icon,
                        contentDescription = label,
                        tint = textPrimary,
                        modifier = Modifier.size(22.dp),
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = label,
                        color = labelColor,
                        fontSize = 13.sp,
                        fontWeight = FontWeight.Medium,
                    )
                }
            }
        }
    }
}

private fun imageReadPermissions(): Array<String> {
    return when {
        android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.UPSIDE_DOWN_CAKE -> arrayOf(
            android.Manifest.permission.READ_MEDIA_IMAGES,
            android.Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
        )

        android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU -> arrayOf(
            android.Manifest.permission.READ_MEDIA_IMAGES,
        )

        else -> arrayOf(android.Manifest.permission.READ_EXTERNAL_STORAGE)
    }
}

private fun hasImageReadPermission(context: Context): Boolean {
    return imageReadPermissions().any { permission ->
        androidx.core.content.ContextCompat.checkSelfPermission(
            context,
            permission,
        ) == android.content.pm.PackageManager.PERMISSION_GRANTED
    }
}

private fun fetchRecentImages(context: Context): List<Uri> {
    val images = mutableListOf<Uri>()
    if (!hasImageReadPermission(context)) return emptyList()

    val projection = arrayOf(
        android.provider.MediaStore.Images.Media._ID,
        android.provider.MediaStore.Images.Media.DATE_ADDED,
    )
    val sortOrder = "${android.provider.MediaStore.Images.Media.DATE_ADDED} DESC"

    runCatching {
        val cursor = context.contentResolver.query(
            android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            projection,
            null,
            null,
            sortOrder,
        )
        cursor?.use {
            val idColumn = it.getColumnIndexOrThrow(android.provider.MediaStore.Images.Media._ID)
            var count = 0
            while (it.moveToNext() && count < 5) {
                val id = it.getLong(idColumn)
                images += android.content.ContentUris.withAppendedId(
                    android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                    id,
                )
                count++
            }
        }
    }
    return images
}

private fun getDisplayNameForUri(context: Context, uri: Uri): String {
    if (uri.scheme == "file") return uri.lastPathSegment.orEmpty()
    return runCatching {
        val cursor = context.contentResolver.query(
            uri,
            arrayOf(android.provider.OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )
        cursor?.use {
            if (it.moveToFirst()) {
                val index = it.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                if (index >= 0) it.getString(index).orEmpty() else ""
            } else {
                ""
            }
        }.orEmpty()
    }.getOrDefault("")
}

private fun getSizeForUri(context: Context, uri: Uri): Long {
    if (uri.scheme == "file") {
        return runCatching { File(uri.path.orEmpty()).length() }.getOrDefault(0L)
    }
    return runCatching {
        val cursor = context.contentResolver.query(
            uri,
            arrayOf(android.provider.OpenableColumns.SIZE),
            null,
            null,
            null,
        )
        cursor?.use {
            if (it.moveToFirst()) {
                val index = it.getColumnIndex(android.provider.OpenableColumns.SIZE)
                if (index >= 0) it.getLong(index) else 0L
            } else {
                0L
            }
        } ?: 0L
    }.getOrDefault(0L)
}

private fun saveCameraPreviewToCache(
    context: Context,
    bitmap: android.graphics.Bitmap,
): Uri? {
    return runCatching {
        val directory = File(context.cacheDir, "attachments").apply {
            if (!exists()) mkdirs()
        }
        val file = File(directory, "camera_${System.currentTimeMillis()}.jpg")
        FileOutputStream(file).use { output ->
            bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 90, output)
        }
        Uri.fromFile(file)
    }.getOrNull()
}

private fun copyUriToAttachmentCache(context: Context, uri: Uri, displayName: String): String {
    if (uri.scheme == "file") return uri.path.orEmpty()
    return runCatching {
        val dir = File(context.cacheDir, "hambur_attachments").also { it.mkdirs() }
        val safeName = displayName
            .ifBlank { uri.lastPathSegment ?: "attachment" }
            .map { ch ->
                if (ch.isLetterOrDigit() || ch == '.' || ch == '-' || ch == '_') ch else '_'
            }
            .joinToString("")
            .ifBlank { "attachment" }
            .take(96)
        val file = File(dir, "${UUID.randomUUID()}-$safeName")
        context.contentResolver.openInputStream(uri)?.use { input ->
            file.outputStream().use { output -> input.copyTo(output) }
        } ?: return@runCatching ""
        file.absolutePath
    }.getOrDefault("")
}

@Composable
private fun ChatIconButton(
    onClick: () -> Unit,
    enabled: Boolean = true,
    size: Dp = 48.dp,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier = modifier
            .size(size)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                enabled = enabled,
                role = Role.Button,
                onClick = onClick,
            ),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

@Composable
private fun InputToggle(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    text: String,
    active: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(999.dp),
        color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                modifier = Modifier.size(16.dp),
                tint = if (active) {
                    MaterialTheme.colorScheme.onPrimaryContainer
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
            Text(
                text = text,
                style = MaterialTheme.typography.labelMedium,
                color = if (active) {
                    MaterialTheme.colorScheme.onPrimaryContainer
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
    }
}

@Composable
private fun PendingAttachmentChip(
    attachment: UiPendingAttachment,
    onRemove: () -> Unit,
) {
    val isDark = MaterialTheme.colorScheme.background.luminance() <= 0.5f
    val mediaBorderColor = if (isDark) {
        Color.White.copy(alpha = 0.12f)
    } else {
        Color.Black.copy(alpha = 0.08f)
    }
    val tileBackground = if (isDark) Color(0xFF2C2C2E) else Color(0xFFEDEDF2)
    val iconBackground = if (isDark) Color(0xFF1E1E20) else Color(0xFFF8F8FA)
    val imageSource = attachment.originalUri
        .ifBlank { attachment.sandboxPath }
        .ifBlank { attachment.displayName }

    if (attachment.mimeType.startsWith("image/")) {
        Box(
            modifier = Modifier
                .size(72.dp)
                .clip(RoundedCornerShape(14.dp))
                .border(0.5.dp, mediaBorderColor, RoundedCornerShape(14.dp))
                .background(tileBackground),
        ) {
            if (imageSource.startsWith("content://") || imageSource.startsWith("file://") || imageSource.startsWith("/")) {
                AsyncImage(
                    model = imageSource,
                    contentDescription = null,
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
            } else {
                Icon(
                    imageVector = Lucide.Image,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .align(Alignment.Center)
                        .size(24.dp),
                )
            }
            Box(
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(4.dp)
                    .size(18.dp)
                    .clip(CircleShape)
                    .border(1.5.dp, Color.White, CircleShape)
                    .background(MaterialTheme.colorScheme.error)
                    .clickable(onClick = onRemove),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Lucide.X,
                    contentDescription = "Remove",
                    tint = Color.White,
                    modifier = Modifier.size(12.dp),
                )
            }
        }
    } else {
        Row(
            modifier = Modifier
                .width(200.dp)
                .height(72.dp)
                .clip(RoundedCornerShape(12.dp))
                .background(tileBackground)
                .border(0.5.dp, mediaBorderColor, RoundedCornerShape(12.dp))
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(40.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(iconBackground),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Lucide.Folder,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(24.dp),
                )
            }
            Spacer(modifier = Modifier.width(10.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = attachment.displayName,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(modifier = Modifier.height(2.dp))
                Text(
                    text = attachment.byteSize.toAttachmentSizeText(),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Spacer(modifier = Modifier.width(6.dp))
            Icon(
                imageVector = Lucide.X,
                contentDescription = "Remove",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .size(16.dp)
                    .clickable(onClick = onRemove),
            )
        }
    }
}

private fun ULong.toAttachmentSizeText(): String {
    val bytes = toDouble()
    return when {
        bytes >= 1024.0 * 1024.0 -> String.format("%.1f MB", bytes / 1024.0 / 1024.0)
        bytes >= 1024.0 -> String.format("%.1f KB", bytes / 1024.0)
        else -> "$this bytes"
    }
}

private fun HamburUiState.selectedSessionTitle(): String {
    val title = sessions.firstOrNull { it.id == selectedSessionId }?.title.orEmpty()
    return when {
        title.isBlank() || title == "New chat" || title == "Hambur Chat" -> "新对话"
        else -> title
    }
}

internal sealed interface ProcessStep {
    val id: String
    val displaySequence: ULong

    data class Reasoning(
        override val id: String,
        val text: String,
        override val displaySequence: ULong = 0UL,
    ) : ProcessStep

    data class ToolCall(
        override val id: String,
        val toolName: String,
        val title: String,
        val content: String = "",
        val status: String = "",
        override val displaySequence: ULong = 0UL,
    ) : ProcessStep
}

internal sealed interface ChatDisplayItem {
    val stableKey: String
    val contentType: String
    val versionSequence: ULong

    data class Timeline(
        val item: UiTimelineItem,
    ) : ChatDisplayItem {
        override val stableKey: String = item.stableKey
        override val contentType: String = item.contentType
        override val versionSequence: ULong = item.versionSequence
    }

    data class AssistantMarkdownBlock(
        val messageId: String,
        val item: UiTimelineItem,
        val node: MarkdownBlockNodeDto,
        val assistantText: String,
    ) : ChatDisplayItem {
        override val stableKey: String = "assistant-block:$messageId:${node.blockId}:${item.stableKey}"
        override val contentType: String = "assistant_markdown_block_item"
        override val versionSequence: ULong = item.versionSequence
    }

    data class AssistantProcessBlock(
        val messageId: String,
        val turnId: String,
        val steps: List<ProcessStep>,
        val firstStableKey: String,
        val lastVersionSequence: ULong,
        val estimatedDurationSeconds: Int = 0,
    ) : ChatDisplayItem {
        override val stableKey: String = "assistant-process:$messageId:$turnId:$firstStableKey"
        override val contentType: String = "assistant_process_block"
        override val versionSequence: ULong = lastVersionSequence
    }

    data class AssistantActions(
        val messageId: String,
        val firstStableKey: String,
        val lastVersionSequence: ULong,
        val assistantText: String,
    ) : ChatDisplayItem {
        override val stableKey: String = "assistant-actions:$messageId:$firstStableKey"
        override val contentType: String = "assistant_actions"
        override val versionSequence: ULong = lastVersionSequence
    }
}

private fun List<ProcessStep>.mergeWith(newSteps: List<ProcessStep>): List<ProcessStep> {
    if (isEmpty()) return newSteps.sortedBy { it.displaySequence }
    if (newSteps.isEmpty()) return this
    val result = this.toMutableList()
    for (newStep in newSteps) {
        val existingIndex = result.indexOfFirst { it.id == newStep.id }
        if (existingIndex >= 0) {
            result[existingIndex] = newStep
            continue
        }
        if (newStep is ProcessStep.Reasoning) {
            val duplicateText = result.any { it is ProcessStep.Reasoning && it.text == newStep.text }
            if (duplicateText) {
                continue
            }
        }
        result.add(newStep)
    }
    result.sortBy { it.displaySequence }
    return result
}

internal fun List<UiTimelineItem>.toChatDisplayItems(
    messageBlocksByPayloadRef: Map<String, MarkdownBlockNodeDto>,
    messagesById: Map<String, UiMessageSnapshot> = emptyMap(),
): List<ChatDisplayItem> {
    val displayItems = mutableListOf<ChatDisplayItem>()
    val groupMarkdownItems = mutableListOf<UiTimelineItem>()
    val groupMarkdownNodes = mutableListOf<MarkdownBlockNodeDto>()
    var groupMarkdownMessageId = ""

    val groupProcessSteps = mutableListOf<ProcessStep>()
    val groupProcessItems = mutableListOf<UiTimelineItem>()
    var groupProcessMessageId = ""
    var currentTurnId = ""
    var lastUserMessageIndex = -1
    var lastRemovedAction: ChatDisplayItem.AssistantActions? = null

    fun flushMarkdownGroup() {
        if (groupMarkdownItems.isNotEmpty() && groupMarkdownNodes.isNotEmpty()) {
            val assistantText = groupMarkdownNodes
                .joinToString(separator = "\n\n") { node ->
                    node.raw.ifBlank { node.text }
                }
                .trim()
            if (assistantText.isNotBlank()) {
                groupMarkdownItems.zip(groupMarkdownNodes).forEach { (item, node) ->
                    displayItems += ChatDisplayItem.AssistantMarkdownBlock(
                        messageId = groupMarkdownMessageId,
                        item = item,
                        node = node,
                        assistantText = assistantText,
                    )
                }
                displayItems += ChatDisplayItem.AssistantActions(
                    messageId = groupMarkdownMessageId,
                    firstStableKey = groupMarkdownItems.firstOrNull()?.stableKey.orEmpty(),
                    lastVersionSequence = groupMarkdownItems.maxOfOrNull { it.versionSequence } ?: 0UL,
                    assistantText = assistantText,
                )
            }
        }
        groupMarkdownItems.clear()
        groupMarkdownNodes.clear()
        groupMarkdownMessageId = ""
    }

    fun flushProcessGroup() {
        if (groupProcessSteps.isNotEmpty()) {
            val firstKey = groupProcessItems.firstOrNull()?.stableKey.orEmpty().ifBlank {
                groupProcessSteps.firstOrNull()?.id.orEmpty()
            }
            val maxSeq = groupProcessItems.maxOfOrNull { it.versionSequence } ?: 0UL
            val resolvedTurnId = currentTurnId.ifBlank {
                messagesById[groupProcessMessageId]?.turnId.orEmpty()
            }

            val minDisplaySeq = groupProcessItems.minOfOrNull { it.displaySequence } ?: 0UL
            val maxDisplaySeq = groupProcessItems.maxOfOrNull { it.displaySequence } ?: 0UL
            val estimatedDuration = if (maxDisplaySeq > minDisplaySeq && (maxDisplaySeq - minDisplaySeq) in 500UL..600_000UL) {
                maxOf(1, ((maxDisplaySeq - minDisplaySeq) / 1000UL).toInt())
            } else {
                0
            }

            val lastMarkdownIndex = displayItems.indexOfLast { it is ChatDisplayItem.AssistantMarkdownBlock }
            val existingProcessIndex = displayItems.indexOfLast { it is ChatDisplayItem.AssistantProcessBlock }
            val sortedSteps = groupProcessSteps.sortedBy { it.displaySequence }
            if (existingProcessIndex > lastUserMessageIndex && existingProcessIndex > lastMarkdownIndex) {
                val existing = displayItems[existingProcessIndex] as ChatDisplayItem.AssistantProcessBlock
                val mergedSteps = existing.steps.mergeWith(sortedSteps)
                val newDuration = maxOf(existing.estimatedDurationSeconds, estimatedDuration)
                val newMaxSeq = maxOf(existing.lastVersionSequence, maxSeq)
                val finalTurnId = existing.turnId.ifBlank { resolvedTurnId }
                val finalMsgId = existing.messageId.ifBlank { groupProcessMessageId }
                displayItems[existingProcessIndex] = existing.copy(
                    messageId = finalMsgId,
                    turnId = finalTurnId,
                    steps = mergedSteps,
                    lastVersionSequence = newMaxSeq,
                    estimatedDurationSeconds = newDuration,
                )
            } else {
                if (displayItems.lastOrNull() is ChatDisplayItem.AssistantActions) {
                    lastRemovedAction = displayItems.removeAt(displayItems.lastIndex) as ChatDisplayItem.AssistantActions
                }
                displayItems += ChatDisplayItem.AssistantProcessBlock(
                    messageId = groupProcessMessageId,
                    turnId = resolvedTurnId,
                    steps = sortedSteps,
                    firstStableKey = firstKey,
                    lastVersionSequence = maxSeq,
                    estimatedDurationSeconds = estimatedDuration,
                )
            }
        }
        groupProcessSteps.clear()
        groupProcessItems.clear()
        groupProcessMessageId = ""
    }

    for (item in this) {
        if (item.kind == "SyntheticUserMessage") {
            continue
        }

        if (item.contentType == "user_message" || item.kind == "UserMessage") {
            flushProcessGroup()
            flushMarkdownGroup()
            displayItems += ChatDisplayItem.Timeline(item)
            lastUserMessageIndex = displayItems.size - 1
            val userMsg = messagesById[item.payloadRef] ?: messagesById[item.stableKey]
            currentTurnId = userMsg?.turnId.orEmpty()
            continue
        }

        if (item.isAssistantReasoningBlock()) {
            flushMarkdownGroup()
            val node = messageBlocksByPayloadRef[item.payloadRef]
                ?: messageBlocksByPayloadRef[item.stableKey]
                ?: messageBlocksByPayloadRef["reasoning:${item.stableKey.removeSuffix(":reasoning")}"]
                ?: messageBlocksByPayloadRef["${item.stableKey.removePrefix("reasoning:")}:reasoning"]
            val reasoningText = node?.raw?.ifBlank { node.text }?.ifBlank { item.smallSummary }?.trim()
                ?: item.smallSummary.trim()
            if (reasoningText.isNotBlank()) {
                if (groupProcessMessageId.isBlank() && node != null && node.messageId.isNotBlank()) {
                    groupProcessMessageId = node.messageId
                }
                val stepTurnId = node?.messageId?.let { messagesById[it]?.turnId.orEmpty() }.orEmpty()
                if (stepTurnId.isNotBlank() && currentTurnId.isBlank()) {
                    currentTurnId = stepTurnId
                }
                val stepId = item.stableKey.ifBlank { item.id }
                val reasoningStep = ProcessStep.Reasoning(
                    id = stepId,
                    text = reasoningText,
                    displaySequence = item.displaySequence,
                )
                val existingIndex = groupProcessSteps.indexOfFirst { it.id == stepId }
                if (existingIndex >= 0) {
                    groupProcessSteps[existingIndex] = reasoningStep
                } else if (groupProcessSteps.none { it is ProcessStep.Reasoning && it.text == reasoningText }) {
                    groupProcessSteps += reasoningStep
                }
                groupProcessItems += item
            }
            continue
        }

        if (item.isToolTraceItem()) {
            flushMarkdownGroup()
            val title = item.traceTitle.ifBlank { item.smallSummary }
            val stepId = item.stableKey.ifBlank { item.id }
            val toolStep = ProcessStep.ToolCall(
                id = stepId,
                toolName = item.toolName,
                title = title,
                content = item.traceContent,
                status = item.traceStatus,
                displaySequence = item.displaySequence,
            )
            val existingIndex = groupProcessSteps.indexOfFirst { it.id == stepId }
            if (existingIndex >= 0) {
                groupProcessSteps[existingIndex] = toolStep
            } else {
                groupProcessSteps += toolStep
            }
            groupProcessItems += item
            continue
        }

        val node = if (item.isAssistantMarkdownBlock()) {
            messageBlocksByPayloadRef[item.payloadRef] ?: messageBlocksByPayloadRef[item.stableKey]
        } else {
            null
        }

        val nodeText = node?.raw?.ifBlank { node.text }?.trim().orEmpty()
        if (item.isAssistantMarkdownBlock() && (node == null || nodeText.isBlank())) {
            continue
        }

        if (node == null) {
            flushProcessGroup()
            flushMarkdownGroup()
            displayItems += ChatDisplayItem.Timeline(item)
            continue
        }

        flushProcessGroup()

        if (groupMarkdownItems.isNotEmpty() && node.messageId != groupMarkdownMessageId) {
            flushMarkdownGroup()
        }
        groupMarkdownMessageId = node.messageId
        groupMarkdownItems += item
        groupMarkdownNodes += node
    }

    flushProcessGroup()
    flushMarkdownGroup()

    val lastActionIndex = displayItems.indexOfLast { it is ChatDisplayItem.AssistantActions }
    val resultItems = if (lastActionIndex >= 0) {
        displayItems.filterIndexed { index, item ->
            item !is ChatDisplayItem.AssistantActions || index == lastActionIndex
        }.toMutableList()
    } else {
        displayItems.toMutableList()
    }
    if (resultItems.none { it is ChatDisplayItem.AssistantActions } && lastRemovedAction != null) {
        resultItems.add(lastRemovedAction)
    }
    return resultItems
}

private fun ChatDisplayItem.topSpacingAfter(previous: ChatDisplayItem?): Dp {
    if (previous == null) return 0.dp
    return when {
        this is ChatDisplayItem.AssistantMarkdownBlock &&
                previous is ChatDisplayItem.AssistantMarkdownBlock &&
                messageId == previous.messageId -> 10.dp

        this is ChatDisplayItem.AssistantMarkdownBlock &&
                previous is ChatDisplayItem.AssistantProcessBlock -> 8.dp

        this is ChatDisplayItem.AssistantProcessBlock &&
                previous is ChatDisplayItem.AssistantMarkdownBlock -> 12.dp

        this is ChatDisplayItem.AssistantActions &&
                previous is ChatDisplayItem.AssistantMarkdownBlock &&
                messageId == previous.messageId -> 8.dp

        else -> 12.dp
    }
}

private fun UiTimelineItem.isAssistantMarkdownBlock(): Boolean {
    return contentType == "assistant_markdown_block" || contentType == "assistant_pending_block"
}

private fun UiTimelineItem.isAssistantReasoningBlock(): Boolean {
    return contentType == "assistant_reasoning_block"
}

private fun UiTimelineItem.isToolTraceItem(): Boolean {
    return contentType == "trace" || kind == "ToolTrace" || kind.contains("Trace")
}

private fun shortTraceId(id: String): String {
    if (id.isBlank()) return "-"
    return if (id.length <= 10) id else id.take(4) + ".." + id.takeLast(6)
}

private const val THINKING_ACTIVE_COLOR = 0xFF4D6BFE

internal fun initialThinkingBlockExpanded(displayMode: String, isGenerating: Boolean): Boolean {
    return when (displayMode) {
        THINKING_BLOCK_DISPLAY_COLLAPSED -> false
        THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE -> isGenerating
        else -> true
    }
}

private fun HamburUiState.thinkingBlockDisplayMode(): String {
    return appSettings.firstOrNull { it.key == THINKING_BLOCK_DISPLAY_MODE_KEY }?.value
        ?: THINKING_BLOCK_DISPLAY_AUTO_EXPAND
}
