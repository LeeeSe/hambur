package com.hambur.chat.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
                        onMessageChange = { draftMessage = it },
                        onSend = {
                            store.sendMessage(state.selectedSessionId, draftMessage)
                            draftMessage = ""
                        },
                        onStop = {
                            store.cancelActiveTurn(state.selectedSessionId)
                        },
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
    onMessageChange: (String) -> Unit,
    onSend: () -> Unit,
    onStop: () -> Unit,
) {
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
                enabled = enabled && message.isNotBlank(),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                ),
            ) {
                Text("Send")
            }
        }
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
