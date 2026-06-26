package com.hambur.chat.ui.chat

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image as ComposeImage
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.gestures.FlingBehavior
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
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
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.composables.icons.lucide.Brain
import com.composables.icons.lucide.ChartNoAxesGantt
import com.composables.icons.lucide.CircleArrowUp
import com.composables.icons.lucide.CirclePause
import com.composables.icons.lucide.CirclePlus
import com.composables.icons.lucide.CircleX
import com.composables.icons.lucide.Copy
import com.composables.icons.lucide.FileText
import com.composables.icons.lucide.Globe
import com.composables.icons.lucide.Image
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.MessageCirclePlus
import com.composables.icons.lucide.RefreshCw
import com.composables.icons.lucide.Search
import com.composables.icons.lucide.Settings
import com.composables.icons.lucide.User
import com.composables.icons.lucide.X
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import com.hambur.chat.R
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.UiMessageSnapshot
import com.hambur.chat.reducer.UiPendingAttachment
import com.hambur.chat.reducer.UiSessionSummary
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.ui.components.SecondaryActionButton
import com.hambur.chat.ui.components.SummaryLine
import com.hambur.chat.ui.markdown.MarkdownBlock
import com.hambur.chat.ui.markdown.MarkdownRenderCache
import com.hambur.chat.ui.markdown.MarkdownStyle
import com.hambur.chat.ui.markdown.rememberMarkdownRenderCache
import com.hambur.chat.ui.markdown.rememberMarkdownStyle
import com.hambur.chat.ui.theme.HamburTheme
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

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
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var draftTitle by rememberSaveable { mutableStateOf("") }
    var draftMessage by rememberSaveable { mutableStateOf("") }
    var editingMessageId by rememberSaveable { mutableStateOf("") }
    var selectingText by rememberSaveable { mutableStateOf("") }
    var unavailableAction by rememberSaveable { mutableStateOf("") }
    var thinkingEnabled by rememberSaveable { mutableStateOf(false) }
    var searchEnabled by rememberSaveable { mutableStateOf(false) }
    var attachmentPanelOpen by rememberSaveable { mutableStateOf(false) }
    var bottomInputHeightPx by remember { mutableStateOf(0) }
    val density = LocalDensity.current
    val configuration = LocalConfiguration.current
    val isDarkTheme = isSystemInDarkTheme()
    val drawerBackgroundColor = MaterialTheme.colorScheme.background
    val drawerWidth = configuration.screenWidthDp.dp * 0.82f
    val maxDrawerOffset = with(density) { drawerWidth.toPx() }
    var drawerOffset by remember { mutableFloatStateOf(0f) }
    val drawerAnimation = remember { Animatable(0f) }
    val messageListBottomPadding = with(density) {
        bottomInputHeightPx.toDp()
    } + HamburTheme.tokens.chat.timelineBottomGap
    val consumeDrawerDelta: (Float) -> Float = { delta ->
        val previousOffset = drawerOffset
        drawerOffset = (drawerOffset + delta).coerceIn(0f, maxDrawerOffset)
        drawerOffset - previousOffset
    }
    val animateDrawerTo: suspend (Float) -> Unit = { targetOffset ->
        drawerAnimation.snapTo(drawerOffset)
        drawerAnimation.animateTo(targetOffset) {
            drawerOffset = value
        }
        drawerOffset = targetOffset
    }
    val drawerScrollableState = rememberScrollableState { delta ->
        consumeDrawerDelta(delta)
    }
    val drawerNestedScrollConnection = remember {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (source != NestedScrollSource.UserInput || drawerOffset <= 0f || available.x == 0f) {
                    return Offset.Zero
                }
                val consumed = consumeDrawerDelta(available.x)
                return Offset(consumed, 0f)
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
    val mainScale = 1f - (0.08f * drawerProgress)
    val mainCornerRadius = if (drawerOffset > 0f) 30.dp else 0.dp
    val mainShadowElevation = 40.dp * drawerProgress
    val mainAmbientShadowColor = if (isDarkTheme) {
        Color.White.copy(alpha = 0.14f * drawerProgress)
    } else {
        Color.Black.copy(alpha = 0.20f * drawerProgress)
    }
    val mainSpotShadowColor = if (isDarkTheme) {
        Color.White.copy(alpha = 0.30f * drawerProgress)
    } else {
        Color.Black.copy(alpha = 0.42f * drawerProgress)
    }

    LaunchedEffect(maxDrawerOffset) {
        drawerOffset = drawerOffset.coerceIn(0f, maxDrawerOffset)
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
                    store.createSession(draftTitle.ifBlank { "新对话" })
                    draftTitle = ""
                    scope.launch { animateDrawerTo(0f) }
                },
                onOpenSession = {
                    store.openSession(it)
                    scope.launch { animateDrawerTo(0f) }
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
                .graphicsLayer {
                    compositingStrategy = androidx.compose.ui.graphics.CompositingStrategy.Offscreen
                }
                .drawWithContent {
                    drawContent()
                    if (drawerProgress > 0f) {
                        val scrimColor = if (isDarkTheme) Color.Black else Color.White
                        val scrimAlpha = if (isDarkTheme) 0.35f * drawerProgress else 0.45f * drawerProgress
                        drawRect(scrimColor.copy(alpha = scrimAlpha))
                    }
                }
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    enabled = drawerOffset > 0f,
                    onClick = {
                        scope.launch { animateDrawerTo(0f) }
                    },
                )
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding(),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize(),
            ) {
                Column(modifier = Modifier.fillMaxSize()) {
                    ChatHeader(
                        title = state.selectedSessionTitle(),
                        onOpenDrawer = {
                            scope.launch { animateDrawerTo(maxDrawerOffset) }
                        },
                        onNewChat = { store.createSession("New chat") },
                        onOpenBrowser = onOpenBrowser,
                    )

                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxWidth(),
                    ) {
                        ChatTimeline(
                            state = state,
                            store = store,
                            onOpenFile = onOpenFile,
                            onEditMessage = { message ->
                                editingMessageId = message.id
                                draftMessage = message.contentText
                            },
                            onSelectText = { selectingText = it },
                            bottomPadding = messageListBottomPadding,
                            modifier = Modifier.fillMaxSize(),
                        )

                        ChatInputPanel(
                            message = draftMessage,
                            onMessageChange = { draftMessage = it },
                            enabled = state.selectedSessionId.isNotBlank() || state.runtimeStatus == "Ready",
                            generating = state.activeTurnIds.containsKey(state.selectedSessionId),
                            thinkingEnabled = thinkingEnabled,
                            attachmentPanelOpen = attachmentPanelOpen,
                            pendingAttachments = state.pendingAttachments,
                            editing = editingMessageId.isNotBlank(),
                            onToggleThinking = { thinkingEnabled = !thinkingEnabled },
                            onToggleAttachmentPanel = { attachmentPanelOpen = !attachmentPanelOpen },
                            onAddImage = {
                                onPickImage { displayName, mimeType, byteSize, uri, sourcePath ->
                                    store.importAttachmentMetadata(
                                        sessionId = state.selectedSessionId,
                                        displayName = displayName,
                                        mimeType = mimeType.ifBlank { "image/*" },
                                        byteSize = byteSize,
                                        originalUri = uri,
                                        sourcePath = sourcePath,
                                    )
                                }
                            },
                            onAddFile = {
                                onPickFile { displayName, mimeType, byteSize, uri, sourcePath ->
                                    store.importAttachmentMetadata(
                                        sessionId = state.selectedSessionId,
                                        displayName = displayName,
                                        mimeType = mimeType.ifBlank { "application/octet-stream" },
                                        byteSize = byteSize,
                                        originalUri = uri,
                                        sourcePath = sourcePath,
                                    )
                                }
                            },
                            onRemoveAttachment = { store.removePendingAttachment(state.selectedSessionId, it) },
                            onClearAttachments = { store.clearPendingAttachments(state.selectedSessionId) },
                            onStop = { store.cancelActiveTurn(state.selectedSessionId) },
                            onCancelEdit = {
                                editingMessageId = ""
                                draftMessage = ""
                            },
                            onSend = {
                                if (editingMessageId.isNotBlank()) {
                                    store.editMessage(state.selectedSessionId, editingMessageId, draftMessage)
                                    editingMessageId = ""
                                } else {
                                    store.sendMessage(
                                        sessionId = state.selectedSessionId,
                                        content = draftMessage,
                                        deepThinkingEnabled = thinkingEnabled,
                                        searchEnabled = searchEnabled,
                                    )
                                }
                                draftMessage = ""
                                attachmentPanelOpen = false
                            },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .onSizeChanged { size ->
                                    if (bottomInputHeightPx != size.height) {
                                        bottomInputHeightPx = size.height
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
    onNewChat: () -> Unit,
    onOpenBrowser: () -> Unit,
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
            ChatIconButton(onClick = onOpenBrowser, size = tokens.headerIconButtonSize) {
                Icon(
                    imageVector = Lucide.Globe,
                    contentDescription = "Browser",
                    tint = MaterialTheme.colorScheme.onBackground,
                    modifier = Modifier.size(tokens.headerBrowserIconSize),
                )
            }
            ChatIconButton(onClick = onNewChat, size = tokens.headerIconButtonSize) {
                Icon(
                    imageVector = Lucide.MessageCirclePlus,
                    contentDescription = "New chat",
                    tint = MaterialTheme.colorScheme.onBackground,
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
        groupDrawerSessions(
            sessions = if (searchQuery.isBlank()) {
                sessions
            } else {
                sessions.filter {
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
                                title = session.title.ifBlank { "新对话" },
                                selected = session.id == selectedSessionId,
                                onClick = { onOpenSession(session.id) },
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
        val timestamp = session.updatedAtMs.takeIf { it > 0UL } ?: session.createdAtMs
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
    title: String,
    selected: Boolean,
    onClick: () -> Unit,
    selectedBackground: Color,
    contentColor: Color,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) selectedBackground else Color.Transparent)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = null,
                role = Role.Button,
                onClick = onClick,
            )
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = title,
            color = contentColor.copy(alpha = if (selected) 1f else 0.82f),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = if (selected) FontWeight.Bold else FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
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
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()
    var followTail by remember(state.selectedSessionId) { mutableStateOf(true) }
    val displayItems = remember(
        state.timelineItems,
        state.markdownBlocksByPayloadRef,
    ) {
        state.timelineItems.toChatDisplayItems(state.markdownBlocksByPayloadRef)
    }
    val latestAssistantMessageId = remember(displayItems) {
        displayItems
            .filterIsInstance<ChatDisplayItem.AssistantMarkdownGroup>()
            .lastOrNull { group -> group.nodes.any { it.raw.isNotBlank() || it.text.isNotBlank() } }
            ?.messageId
            .orEmpty()
    }
    val visibleCount = displayItems.size + 1

    LaunchedEffect(listState, state.selectedSessionId) {
        snapshotFlow { listState.isNearBottom() }
            .distinctUntilChanged()
            .collect { nearBottom -> followTail = nearBottom }
    }

    LaunchedEffect(
        state.selectedSessionId,
        displayItems.size,
        displayItems.lastOrNull()?.versionSequence,
        followTail,
    ) {
        if (followTail && visibleCount > 0) {
            withFrameNanos { }
            listState.scrollToItem(visibleCount - 1)
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
    LazyColumn(
        state = listState,
        modifier = modifier,
        contentPadding = PaddingValues(
            start = 14.dp,
            top = 12.dp,
            end = 14.dp,
            bottom = bottomPadding,
        ),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        itemsIndexed(
            items = displayItems,
            key = { _, item -> item.stableKey },
            contentType = { _, item -> item.contentType },
        ) { _, item ->
            when (item) {
                is ChatDisplayItem.Timeline -> {
                    val timelineItem = item.item
                    when {
                        timelineItem.contentType == "user_message" -> {
                            val message = state.messagesById[timelineItem.payloadRef]
                            MessageTimelineItem(
                                item = timelineItem,
                                message = message,
                                onOpenFile = onOpenFile,
                            )
                        }
                        timelineItem.contentType == "trace" || timelineItem.kind.contains("Trace") -> {
                            ToolTraceItem(item = timelineItem)
                        }
                        else -> {
                            TimelineSummaryItem(item = timelineItem)
                        }
                    }
                }
                is ChatDisplayItem.AssistantMarkdownGroup -> {
                    AssistantMarkdownTimelineItem(
                        group = item,
                        markdownStyle = markdownStyle,
                        markdownCache = markdownCache,
                        onOpenFile = onOpenFile,
                        showActions = item.messageId == latestAssistantMessageId,
                        onRegenerate = {
                            store.regenerateMessage(state.selectedSessionId, item.messageId)
                        },
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
    val tokens = HamburTheme.tokens.chat
    Box(
        modifier = modifier
            .fillMaxSize()
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
) {
    val role = message?.role ?: item.kind
    val isUser = role == "user"

    Box(modifier = Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
        Surface(
            modifier = Modifier.widthIn(max = 324.dp),
            shape = RoundedCornerShape(18.dp),
            color = MaterialTheme.colorScheme.primaryContainer,
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 18.dp, vertical = 13.dp),
            ) {
                if (!message?.reasoningContent.isNullOrBlank()) {
                    Surface(
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(8.dp),
                        color = MaterialTheme.colorScheme.surfaceVariant,
                    ) {
                        Text(
                            text = message.reasoningContent,
                            modifier = Modifier.padding(10.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (!message?.attachments.isNullOrEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        message.attachments.forEach { attachment ->
                            MessageAttachmentRow(
                                attachment = attachment,
                                onOpen = { onOpenFile(attachment.sandboxPath) },
                            )
                        }
                    }
                }
                Text(
                    text = message?.contentText?.ifBlank { item.smallSummary }
                        ?: item.smallSummary.ifBlank { "Loading message..." },
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        }
    }
}

@Composable
private fun AssistantMarkdownTimelineItem(
    group: ChatDisplayItem.AssistantMarkdownGroup,
    markdownStyle: MarkdownStyle,
    markdownCache: MarkdownRenderCache,
    onOpenFile: (String) -> Unit,
    showActions: Boolean,
    onRegenerate: () -> Unit,
) {
    val context = LocalContext.current
    val clipboard = remember(context) {
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    }
    val assistantText = remember(group.nodes) {
        group.nodes.joinToString(separator = "\n\n") { node ->
            node.raw.ifBlank { node.text }
        }.trim()
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            group.nodes.forEach { node ->
                MarkdownBlock(
                    node = node,
                    style = markdownStyle,
                    renderCache = markdownCache,
                    onOpenDestination = onOpenFile,
                )
            }
        }
        if (showActions && assistantText.isNotBlank()) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ChatInlineActionButton(
                    onClick = {
                        clipboard.setPrimaryClip(
                            ClipData.newPlainText("assistant message", assistantText),
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
    }
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
private fun ToolTraceItem(item: UiTimelineItem) {
    val statusColor = when (item.traceStatus) {
        "completed" -> MaterialTheme.colorScheme.primary
        "failed" -> MaterialTheme.colorScheme.error
        "running" -> MaterialTheme.colorScheme.tertiary
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = item.toolName.ifBlank { "Tool" },
                    style = MaterialTheme.typography.labelLarge,
                    color = statusColor,
                    modifier = Modifier.width(92.dp),
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
                )
            }
            if (item.traceContent.isNotBlank()) {
                Text(
                    text = item.traceContent,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 6,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun TimelineSummaryItem(item: UiTimelineItem) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
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
    pendingAttachments: List<UiPendingAttachment>,
    editing: Boolean,
    onToggleThinking: () -> Unit,
    onToggleAttachmentPanel: () -> Unit,
    onAddImage: () -> Unit,
    onAddFile: () -> Unit,
    onRemoveAttachment: (String) -> Unit,
    onClearAttachments: () -> Unit,
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

    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.background)
            .padding(
                start = tokens.inputOuterStartPadding,
                end = tokens.inputOuterEndPadding,
                top = tokens.inputOuterTopPadding,
                bottom = tokens.inputOuterBottomPadding,
            ),
        verticalArrangement = Arrangement.spacedBy(tokens.inputOuterGap),
    ) {
        if (pendingAttachments.isNotEmpty()) {
            LazyRow(
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(pendingAttachments, key = { it.id }) { attachment ->
                    PendingAttachmentChip(
                        attachment = attachment,
                        onRemove = { onRemoveAttachment(attachment.id) },
                    )
                }
                item {
                    TextButton(onClick = onClearAttachments) {
                        Text("Clear")
                    }
                }
            }
        }
        if (editing) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
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
        }
        Surface(
            modifier = Modifier.fillMaxWidth(),
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
                        enabled = enabled && !generating,
                        textStyle = MaterialTheme.typography.bodyLarge.copy(
                            color = primaryText,
                        ),
                        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = tokens.inputTextStartPadding),
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    ChatIconButton(
                        onClick = onToggleThinking,
                        enabled = enabled && !generating,
                        size = tokens.inputIconButtonSize,
                        modifier = Modifier.offset(
                            x = tokens.inputLeftIconOffsetX,
                            y = tokens.inputIconOffsetY,
                        ),
                    ) {
                        Icon(
                            imageVector = Lucide.Brain,
                            contentDescription = "Think",
                            tint = if (thinkingEnabled) MaterialTheme.colorScheme.primary else primaryText,
                            modifier = Modifier.size(tokens.inputIconSize),
                        )
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    ChatIconButton(
                        onClick = onToggleAttachmentPanel,
                        enabled = enabled && !generating,
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
        AnimatedVisibility(visible = attachmentPanelOpen) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Row(
                    modifier = Modifier.padding(12.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SecondaryActionButton(text = "Image", enabled = enabled && !generating, onClick = onAddImage)
                    SecondaryActionButton(text = "File", enabled = enabled && !generating, onClick = onAddFile)
                    Text(
                        text = "Picker metadata is imported; backend file byte ingestion is still pending.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
    }
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
    Surface(
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(
                imageVector = if (attachment.mimeType.startsWith("image/")) {
                    Lucide.Image
                } else {
                    Lucide.FileText
                },
                contentDescription = null,
                modifier = Modifier.size(18.dp),
            )
            Column(modifier = Modifier.widthIn(max = 180.dp)) {
                Text(
                    text = attachment.displayName,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = attachment.mimeType,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            IconButton(onClick = onRemove, modifier = Modifier.size(28.dp)) {
                Icon(imageVector = Lucide.X, contentDescription = "Remove", modifier = Modifier.size(16.dp))
            }
        }
    }
}

private fun HamburUiState.selectedSessionTitle(): String {
    val title = sessions.firstOrNull { it.id == selectedSessionId }?.title.orEmpty()
    return when {
        title.isBlank() || title == "New chat" || title == "Hambur Chat" -> "新对话"
        else -> title
    }
}

private sealed interface ChatDisplayItem {
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

    data class AssistantMarkdownGroup(
        val messageId: String,
        val items: List<UiTimelineItem>,
        val nodes: List<MarkdownBlockNodeDto>,
    ) : ChatDisplayItem {
        override val stableKey: String = "assistant-group:$messageId:${items.firstOrNull()?.stableKey.orEmpty()}"
        override val contentType: String = "assistant_markdown_group"
        override val versionSequence: ULong = items.maxOfOrNull { it.versionSequence } ?: 0UL
    }
}

private fun List<UiTimelineItem>.toChatDisplayItems(
    markdownBlocksByPayloadRef: Map<String, MarkdownBlockNodeDto>,
): List<ChatDisplayItem> {
    val displayItems = mutableListOf<ChatDisplayItem>()
    val groupItems = mutableListOf<UiTimelineItem>()
    val groupNodes = mutableListOf<MarkdownBlockNodeDto>()
    var groupMessageId = ""

    fun flushGroup() {
        if (groupItems.isNotEmpty() && groupNodes.isNotEmpty()) {
            displayItems += ChatDisplayItem.AssistantMarkdownGroup(
                messageId = groupMessageId,
                items = groupItems.toList(),
                nodes = groupNodes.toList(),
            )
        } else {
            groupItems.forEach { displayItems += ChatDisplayItem.Timeline(it) }
        }
        groupItems.clear()
        groupNodes.clear()
        groupMessageId = ""
    }

    for (item in this) {
        val node = if (item.isAssistantMarkdownBlock()) {
            markdownBlocksByPayloadRef[item.payloadRef]
        } else {
            null
        }
        if (node == null) {
            flushGroup()
            displayItems += ChatDisplayItem.Timeline(item)
            continue
        }
        if (groupItems.isNotEmpty() && node.messageId != groupMessageId) {
            flushGroup()
        }
        groupMessageId = node.messageId
        groupItems += item
        groupNodes += node
    }
    flushGroup()
    return displayItems
}

private fun UiTimelineItem.isAssistantMarkdownBlock(): Boolean {
    return contentType == "assistant_markdown_block" || contentType == "assistant_pending_block"
}

private fun LazyListState.isNearBottom(): Boolean {
    val total = layoutInfo.totalItemsCount
    if (total == 0) return true
    val last = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: return true
    return last >= total - 2
}
