package com.hambur.chat.ui.chat

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.NavigationDrawerItemDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.DrawerValue
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.composables.icons.lucide.Brain
import com.composables.icons.lucide.CirclePause
import com.composables.icons.lucide.CirclePlus
import com.composables.icons.lucide.Copy
import com.composables.icons.lucide.FileText
import com.composables.icons.lucide.Globe
import com.composables.icons.lucide.Image
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.Menu
import com.composables.icons.lucide.MessageCirclePlus
import com.composables.icons.lucide.Paperclip
import com.composables.icons.lucide.Pencil
import com.composables.icons.lucide.RefreshCw
import com.composables.icons.lucide.Search
import com.composables.icons.lucide.SendHorizontal
import com.composables.icons.lucide.Settings
import com.composables.icons.lucide.Trash2
import com.composables.icons.lucide.Type
import com.composables.icons.lucide.X
import android.content.ClipData
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.reducer.UiMessageSnapshot
import com.hambur.chat.reducer.UiPendingAttachment
import com.hambur.chat.reducer.UiSessionSummary
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.ui.components.HamburTopBar
import com.hambur.chat.ui.components.SecondaryActionButton
import com.hambur.chat.ui.components.StatusPill
import com.hambur.chat.ui.components.SummaryLine
import com.hambur.chat.ui.markdown.MarkdownBlock
import com.hambur.chat.ui.markdown.MarkdownRenderCache
import com.hambur.chat.ui.markdown.MarkdownStyle
import com.hambur.chat.ui.markdown.rememberMarkdownRenderCache
import com.hambur.chat.ui.markdown.rememberMarkdownStyle
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
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
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

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ChatDrawerContent(
                sessions = state.sessions,
                selectedSessionId = state.selectedSessionId,
                searchQuery = searchQuery,
                onSearchQueryChange = { searchQuery = it },
                onNewSession = {
                    store.createSession(draftTitle.ifBlank { "New chat" })
                    draftTitle = ""
                    scope.launch { drawerState.close() }
                },
                draftTitle = draftTitle,
                onDraftTitleChange = { draftTitle = it },
                onOpenSession = {
                    store.openSession(it)
                    scope.launch { drawerState.close() }
                },
                onDeleteSession = store::deleteSession,
                onRenameSession = { sessionId ->
                    store.renameSession(sessionId, draftTitle.ifBlank { "New chat" })
                    draftTitle = ""
                },
                onSetSessionPinned = store::setSessionPinned,
                onUnavailableAction = { unavailableAction = it },
                onOpenSettings = {
                    scope.launch { drawerState.close() }
                    onOpenSettings()
                },
            )
        },
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding(),
        ) {
            ChatHeader(
                title = state.selectedSessionTitle(),
                runtimeStatus = state.runtimeStatus,
                latestEvent = state.latestEventKind,
                onOpenDrawer = { scope.launch { drawerState.open() } },
                onNewChat = { store.createSession("New chat") },
                onOpenSettings = onOpenSettings,
                onOpenBrowser = onOpenBrowser,
            )

            Box(modifier = Modifier.weight(1f)) {
            ChatTimeline(
                    state = state,
                    store = store,
                    onOpenFile = onOpenFile,
                    onEditMessage = { message ->
                        editingMessageId = message.id
                        draftMessage = message.contentText
                    },
                    onSelectText = { selectingText = it.contentText },
                    modifier = Modifier.fillMaxSize(),
                )
            }

            ChatInputPanel(
                message = draftMessage,
                onMessageChange = { draftMessage = it },
                enabled = state.selectedSessionId.isNotBlank(),
                generating = state.activeTurnIds.containsKey(state.selectedSessionId),
                thinkingEnabled = thinkingEnabled,
                searchEnabled = searchEnabled,
                attachmentPanelOpen = attachmentPanelOpen,
                pendingAttachments = state.pendingAttachments,
                editing = editingMessageId.isNotBlank(),
                onToggleThinking = { thinkingEnabled = !thinkingEnabled },
                onToggleSearch = { searchEnabled = !searchEnabled },
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
            )
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
    runtimeStatus: String,
    latestEvent: String,
    onOpenDrawer: () -> Unit,
    onNewChat: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenBrowser: () -> Unit,
) {
    HamburTopBar(
        title = title,
        subtitle = "$runtimeStatus / $latestEvent",
        actions = {
            IconButton(onClick = onOpenBrowser) {
                Icon(imageVector = Lucide.Globe, contentDescription = "Browser")
            }
            IconButton(onClick = onNewChat) {
                Icon(imageVector = Lucide.MessageCirclePlus, contentDescription = "New chat")
            }
            IconButton(onClick = onOpenSettings) {
                Icon(imageVector = Lucide.Settings, contentDescription = "Settings")
            }
        },
    )
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 12.dp, end = 12.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        IconButton(onClick = onOpenDrawer) {
            Icon(imageVector = Lucide.Menu, contentDescription = "Conversations")
        }
        Text(
            text = "Conversations",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ChatDrawerContent(
    sessions: List<UiSessionSummary>,
    selectedSessionId: String,
    searchQuery: String,
    onSearchQueryChange: (String) -> Unit,
    draftTitle: String,
    onDraftTitleChange: (String) -> Unit,
    onNewSession: () -> Unit,
    onOpenSession: (String) -> Unit,
    onDeleteSession: (String) -> Unit,
    onRenameSession: (String) -> Unit,
    onSetSessionPinned: (String, Boolean) -> Unit,
    onUnavailableAction: (String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    val filtered = remember(sessions, searchQuery) {
        if (searchQuery.isBlank()) {
            sessions
        } else {
            sessions.filter {
                it.title.contains(searchQuery, ignoreCase = true) ||
                    it.latestPreview.contains(searchQuery, ignoreCase = true)
            }
        }
    }

    Surface(
        modifier = Modifier
            .fillMaxHeight()
            .widthIn(max = 340.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .navigationBarsPadding()
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(
                text = "Hambur",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
            OutlinedTextField(
                value = searchQuery,
                onValueChange = onSearchQueryChange,
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                leadingIcon = { Icon(imageVector = Lucide.Search, contentDescription = null) },
                label = { Text("Search chats") },
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedTextField(
                    value = draftTitle,
                    onValueChange = onDraftTitleChange,
                    modifier = Modifier.weight(1f),
                    singleLine = true,
                    label = { Text("Title") },
                )
                IconButton(onClick = onNewSession) {
                    Icon(imageVector = Lucide.CirclePlus, contentDescription = "Create")
                }
            }

            LazyColumn(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(filtered, key = { it.id }) { session ->
                    NavigationDrawerItem(
                        selected = session.id == selectedSessionId,
                        onClick = { onOpenSession(session.id) },
                        label = {
                            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                Text(
                                    text = session.title.ifBlank { "New chat" },
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = session.latestPreview.ifBlank {
                                        "${session.messageCount} messages"
                                    },
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    TextButton(onClick = { onRenameSession(session.id) }) {
                                        Text("Rename")
                                    }
                                    TextButton(onClick = { onSetSessionPinned(session.id, session.pinnedAtMs == 0UL) }) {
                                        Text(if (session.pinnedAtMs == 0UL) "Pin" else "Unpin")
                                    }
                                }
                            }
                        },
                        badge = {
                            IconButton(onClick = { onDeleteSession(session.id) }) {
                                Icon(
                                    imageVector = Lucide.Trash2,
                                    contentDescription = "Delete",
                                    modifier = Modifier.size(18.dp),
                                )
                            }
                        },
                        colors = NavigationDrawerItemDefaults.colors(
                            selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                            unselectedContainerColor = Color.Transparent,
                        ),
                    )
                }
            }
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable(onClick = onOpenSettings),
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Row(
                    modifier = Modifier.padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Icon(imageVector = Lucide.Settings, contentDescription = null)
                    Text(
                        text = "Settings",
                        modifier = Modifier.weight(1f),
                        fontWeight = FontWeight.Medium,
                    )
                }
            }
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
    onSelectText: (UiMessageSnapshot) -> Unit,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()
    var followTail by remember(state.selectedSessionId) { mutableStateOf(true) }
    val visibleCount = state.timelineItems.size + 1

    LaunchedEffect(listState, state.selectedSessionId) {
        snapshotFlow { listState.isNearBottom() }
            .distinctUntilChanged()
            .collect { nearBottom -> followTail = nearBottom }
    }

    LaunchedEffect(
        state.selectedSessionId,
        state.timelineItems.size,
        state.timelineItems.lastOrNull()?.versionSequence,
        state.pendingMarkdownByMessageId.values.lastOrNull()?.raw,
        followTail,
    ) {
        if (followTail && visibleCount > 0) {
            withFrameNanos { }
            listState.scrollToItem(visibleCount - 1)
        }
    }

    if (state.selectedSessionId.isBlank() || state.timelineItems.isEmpty()) {
        EmptyChatState(
            selected = state.selectedSessionId.isNotBlank(),
            modifier = modifier,
        )
        return
    }

    val markdownStyle = rememberMarkdownStyle()
    val markdownCache = rememberMarkdownRenderCache()
    LazyColumn(
        state = listState,
        modifier = modifier,
        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        itemsIndexed(
            items = state.timelineItems,
            key = { _, item -> item.stableKey },
            contentType = { _, item -> item.contentType },
        ) { _, item ->
            when {
                item.contentType == "message" -> {
                    val message = state.messagesById[item.payloadRef]
                    MessageTimelineItem(
                        item = item,
                        message = message,
                        markdownBlocks = state.markdownBlocksByMessageId[item.payloadRef].orEmpty(),
                        pendingMarkdown = state.pendingMarkdownByMessageId[item.payloadRef],
                        markdownStyle = markdownStyle,
                        markdownCache = markdownCache,
                        onRequestMarkdown = {
                            if (
                                message != null &&
                                message.role == "assistant" &&
                                state.markdownBlocksByMessageId[item.payloadRef].isNullOrEmpty() &&
                                state.pendingMarkdownByMessageId[item.payloadRef] == null
                            ) {
                                store.renderMarkdownText(
                                    sessionId = state.selectedSessionId,
                                    messageId = item.payloadRef,
                                    markdown = message.contentText,
                                )
                            }
                        },
                        onOpenFile = onOpenFile,
                        onRegenerate = {
                            store.regenerateMessage(state.selectedSessionId, item.payloadRef)
                        },
                        onRetry = {
                            store.retryMessage(state.selectedSessionId, item.payloadRef)
                        },
                        onEdit = { message ->
                            onEditMessage(message)
                        },
                        onSelectText = onSelectText,
                    )
                }
                item.contentType == "trace" || item.kind.contains("Trace") -> {
                    ToolTraceItem(item = item)
                }
                else -> {
                    TimelineSummaryItem(item = item)
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
    selected: Boolean,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.padding(24.dp),
        ) {
            Surface(
                modifier = Modifier.size(86.dp),
                shape = CircleShape,
                color = MaterialTheme.colorScheme.primaryContainer,
            ) {
                Icon(
                    imageVector = Lucide.MessageCirclePlus,
                    contentDescription = null,
                    modifier = Modifier.padding(22.dp),
                    tint = MaterialTheme.colorScheme.onPrimaryContainer,
                )
            }
            Text(
                text = if (selected) "How can I help?" else "Create or select a chat",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
            )
            Text(
                text = if (selected) {
                    "Send a message to exercise the new Rust backend."
                } else {
                    "Open the conversation drawer to create a session."
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
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
    markdownBlocks: List<com.hambur.chat.uniffi.MarkdownBlockNodeDto>,
    pendingMarkdown: com.hambur.chat.uniffi.MarkdownBlockNodeDto?,
    markdownStyle: MarkdownStyle,
    markdownCache: MarkdownRenderCache,
    onRequestMarkdown: () -> Unit,
    onOpenFile: (String) -> Unit,
    onRegenerate: () -> Unit,
    onRetry: () -> Unit,
    onEdit: (UiMessageSnapshot) -> Unit,
    onSelectText: (UiMessageSnapshot) -> Unit,
) {
    val role = message?.role ?: item.kind
    val isUser = role == "user"
    val bubbleColor = if (isUser) {
        MaterialTheme.colorScheme.primaryContainer
    } else {
        MaterialTheme.colorScheme.surface
    }
    val alignment = if (isUser) Alignment.CenterEnd else Alignment.CenterStart
    val clipboard = LocalClipboard.current
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(message?.id, message?.contentText, markdownBlocks.size, pendingMarkdown) {
        onRequestMarkdown()
    }

    Box(modifier = Modifier.fillMaxWidth(), contentAlignment = alignment) {
        Surface(
            modifier = Modifier.fillMaxWidth(if (isUser) 0.86f else 1f),
            shape = RoundedCornerShape(8.dp),
            color = bubbleColor,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            Column(
                modifier = Modifier.padding(12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = if (isUser) "You" else "Assistant",
                        style = MaterialTheme.typography.labelLarge,
                        fontWeight = FontWeight.SemiBold,
                    )
                    StatusPill(
                        text = message?.status ?: item.kind,
                        active = message?.status == "streaming" || message?.status == "complete",
                    )
                    Spacer(modifier = Modifier.weight(1f))
                    if (message != null) {
                        IconButton(
                            onClick = {
                                coroutineScope.launch {
                                    clipboard.setClipEntry(
                                        ClipEntry(
                                            ClipData.newPlainText("message", message.contentText),
                                        ),
                                    )
                                }
                            },
                            modifier = Modifier.size(32.dp),
                        ) {
                            Icon(
                                imageVector = Lucide.Copy,
                                contentDescription = "Copy",
                                modifier = Modifier.size(17.dp),
                            )
                        }
                        IconButton(onClick = { onSelectText(message) }, modifier = Modifier.size(32.dp)) {
                            Icon(
                                imageVector = Lucide.Type,
                                contentDescription = "Select text",
                                modifier = Modifier.size(17.dp),
                            )
                        }
                    }
                    if (!isUser) {
                        IconButton(onClick = onRegenerate, modifier = Modifier.size(32.dp)) {
                            Icon(
                                imageVector = Lucide.RefreshCw,
                                contentDescription = "Regenerate",
                                modifier = Modifier.size(17.dp),
                            )
                        }
                    } else {
                        if (message != null) {
                            IconButton(onClick = { onEdit(message) }, modifier = Modifier.size(32.dp)) {
                                Icon(
                                    imageVector = Lucide.Pencil,
                                    contentDescription = "Edit",
                                    modifier = Modifier.size(17.dp),
                                )
                            }
                        }
                        IconButton(onClick = onRetry, modifier = Modifier.size(32.dp)) {
                            Icon(
                                imageVector = Lucide.RefreshCw,
                                contentDescription = "Retry",
                                modifier = Modifier.size(17.dp),
                            )
                        }
                    }
                }
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
                if (!isUser && (markdownBlocks.isNotEmpty() || pendingMarkdown != null)) {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        markdownBlocks.forEach { block ->
                            MarkdownBlock(
                                node = block,
                                style = markdownStyle,
                                renderCache = markdownCache,
                                onOpenDestination = onOpenFile,
                            )
                        }
                        if (pendingMarkdown != null) {
                            MarkdownBlock(
                                node = pendingMarkdown,
                                style = markdownStyle,
                                renderCache = markdownCache,
                                onOpenDestination = onOpenFile,
                            )
                        }
                    }
                } else {
                    Text(
                        text = message?.contentText?.ifBlank { item.smallSummary }
                            ?: item.smallSummary.ifBlank { "Loading message..." },
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                val model = listOfNotNull(
                    message?.providerName?.takeIf { it.isNotBlank() },
                    message?.modelName?.takeIf { it.isNotBlank() },
                ).joinToString(" / ")
                if (model.isNotBlank() || !message?.finishReason.isNullOrBlank()) {
                    Text(
                        text = listOf(model, message?.finishReason.orEmpty())
                            .filter { it.isNotBlank() }
                            .joinToString(" - "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
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
    searchEnabled: Boolean,
    attachmentPanelOpen: Boolean,
    pendingAttachments: List<UiPendingAttachment>,
    editing: Boolean,
    onToggleThinking: () -> Unit,
    onToggleSearch: () -> Unit,
    onToggleAttachmentPanel: () -> Unit,
    onAddImage: () -> Unit,
    onAddFile: () -> Unit,
    onRemoveAttachment: (String) -> Unit,
    onClearAttachments: () -> Unit,
    onStop: () -> Unit,
    onCancelEdit: () -> Unit,
    onSend: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.background)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
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
            shape = RoundedCornerShape(8.dp),
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            Column(
                modifier = Modifier.padding(10.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 44.dp, max = 150.dp)
                        .verticalScroll(rememberScrollState()),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (message.isBlank()) {
                        Text(
                            text = "Message Hambur",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    BasicTextField(
                        value = message,
                        onValueChange = onMessageChange,
                        enabled = enabled && !generating,
                        textStyle = MaterialTheme.typography.bodyLarge.copy(
                            color = MaterialTheme.colorScheme.onSurface,
                        ),
                        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    InputToggle(
                        icon = Lucide.Brain,
                        text = "Think",
                        active = thinkingEnabled,
                        enabled = enabled && !generating,
                        onClick = onToggleThinking,
                    )
                    InputToggle(
                        icon = Lucide.Globe,
                        text = "Search",
                        active = searchEnabled,
                        enabled = enabled && !generating,
                        onClick = onToggleSearch,
                    )
                    IconButton(
                        enabled = enabled && !generating,
                        onClick = onToggleAttachmentPanel,
                    ) {
                        Icon(imageVector = Lucide.Paperclip, contentDescription = "Attachments")
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    if (generating) {
                        IconButton(onClick = onStop, enabled = enabled) {
                            Icon(imageVector = Lucide.CirclePause, contentDescription = "Stop")
                        }
                    } else {
                        IconButton(
                            onClick = onSend,
                            enabled = enabled && (message.isNotBlank() || pendingAttachments.isNotEmpty()),
                        ) {
                            Icon(imageVector = Lucide.SendHorizontal, contentDescription = "Send")
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
    return sessions.firstOrNull { it.id == selectedSessionId }?.title?.ifBlank { "Hambur Chat" }
        ?: "Hambur Chat"
}

private fun LazyListState.isNearBottom(): Boolean {
    val total = layoutInfo.totalItemsCount
    if (total == 0) return true
    val last = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: return true
    return last >= total - 2
}
