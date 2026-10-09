package com.hambur.chat

import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.reducer.reduce
import com.hambur.chat.ui.chat.ChatDisplayItem
import com.hambur.chat.ui.chat.toChatDisplayItems
import com.hambur.chat.uniffi.AppSnapshotDto
import com.hambur.chat.uniffi.BackendEvent
import com.hambur.chat.uniffi.BackendEventKind
import com.hambur.chat.uniffi.BackendEventPayload
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.MarkdownRenderUpdateDto
import com.hambur.chat.uniffi.MessageBlockPayloadDto
import com.hambur.chat.uniffi.TimelineItemDto
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * 流式正文渲染回归测试（StreamingMarkdownTimelineTest）。
 *
 * 背景（修复前的 bug）：一段正文（mdstream 的一个块）提交后只写进 message_blocks，
 * 客户端只在 snapshot 事件里重建 timelineItems，而流式期间没有 snapshot，
 * 于是 UI 上只剩开局那条 pending 占位块——表现为"流式时只显示一段正文，
 * 流式结束后所有正文一块出现"。
 *
 * 后端修复：emit_markdown_persisted 在提交块落库后补发一条带 snapshot 的
 * TurnStateChanged，把新提交的段落即时送进 timelineItems。
 * 这两个用例分别锁定修复后的契约与 reducer 合成 pending 尾部的能力。
 */
class StreamingMarkdownTimelineTest {

    private val sessionId = "s1"
    private val turnId = "turn-1"
    private val messageId = "msg-asst-1"
    private val pendingStableKey = "$messageId:pending"
    private val pendingPayloadRef = "mdb-pending-1"

    private fun node(
        blockId: ULong,
        stableKey: String,
        text: String,
        committed: Boolean,
        nodeKind: String = "Paragraph",
    ): MarkdownBlockNodeDto {
        return MarkdownBlockNodeDto(
            messageId = messageId,
            blockId = blockId,
            stableKey = stableKey,
            sourceKind = nodeKind,
            nodeKind = nodeKind,
            committed = committed,
            level = 0.toUByte(),
            inlines = emptyList(),
            language = "",
            text = text,
            raw = text,
            childrenJson = "",
            itemsJson = "",
            tableHeader = emptyList(),
            tableRows = emptyList(),
            tableAlignments = emptyList(),
            path = "",
            fileKind = "",
        )
    }

    private fun blockRecord(
        id: String,
        blockId: ULong,
        stableKey: String,
        nodeText: String,
        committed: Boolean,
    ): MessageBlockPayloadDto {
        return MessageBlockPayloadDto(
            id = id,
            sessionId = sessionId,
            messageId = messageId,
            blockId = blockId,
            blockType = "content",
            stableKey = stableKey,
            committed = committed,
            node = node(blockId, stableKey, nodeText, committed),
            raw = nodeText,
            smallSummary = nodeText,
            versionSequence = 1UL,
            createdAtMs = 1000UL,
            updatedAtMs = 1000UL,
        )
    }

    private fun timelineItem(
        id: String,
        stableKey: String,
        contentType: String,
        payloadRef: String,
        displaySequence: ULong,
        summary: String,
    ): TimelineItemDto {
        return TimelineItemDto(
            id = id,
            stableKey = stableKey,
            contentType = contentType,
            displaySequence = displaySequence,
            versionSequence = 1UL,
            payloadRef = payloadRef,
            smallSummary = summary,
            kind = if (contentType == "user_message") "UserMessage" else "AssistantMarkdownBlock",
            traceTitle = "",
            traceContent = "",
            traceStatus = "",
            toolCallId = "",
            toolName = "",
            traceArguments = "",
            attachments = emptyList(),
        )
    }

    private fun userItem(displaySequence: ULong = 900UL) = timelineItem(
        id = "tl-user",
        stableKey = "msg-user-1",
        contentType = "user_message",
        payloadRef = "msg-user-1",
        displaySequence = displaySequence,
        summary = "写三段话",
    )

    private fun pendingItem(versionSequence: ULong = 1UL) = UiTimelineItem(
        id = "tl-pending",
        stableKey = pendingStableKey,
        contentType = "assistant_pending_block",
        versionSequence = versionSequence,
        payloadRef = pendingPayloadRef,
        smallSummary = "",
        kind = "AssistantPendingBlock",
        displaySequence = 1000UL,
    )

    private fun snapshotEvent(
        sequence: ULong,
        timelineItems: List<TimelineItemDto>,
        messageBlockPayloads: List<MessageBlockPayloadDto>,
    ): BackendEvent {
        return BackendEvent(
            eventId = "evt-$sequence",
            schemaVersion = 1u,
            sequence = sequence,
            createdAtMs = 1000UL,
            kind = BackendEventKind.TURN_STATE_CHANGED,
            sessionId = sessionId,
            turnId = turnId,
            payload = BackendEventPayload.Snapshot(
                AppSnapshotDto(
                    sessions = emptyList(),
                    selectedSessionId = sessionId,
                    timelineItems = timelineItems,
                    messageBlockPayloads = messageBlockPayloads,
                    pendingAttachments = emptyList(),
                ),
            ),
            errorCode = "",
            message = "MarkdownCommitted",
        )
    }

    private fun markdownEvent(
        sequence: ULong,
        committed: List<MarkdownBlockNodeDto>,
        pending: MarkdownBlockNodeDto?,
    ): BackendEvent {
        return BackendEvent(
            eventId = "evt-$sequence",
            schemaVersion = 1u,
            sequence = sequence,
            createdAtMs = 1000UL,
            kind = BackendEventKind.MARKDOWN_RENDER_UPDATE,
            sessionId = sessionId,
            turnId = turnId,
            payload = BackendEventPayload.Markdown(
                MarkdownRenderUpdateDto(
                    messageId = messageId,
                    reset = false,
                    committedNodes = committed,
                    pendingNode = pending,
                    invalidatedBlockIds = emptyList(),
                ),
            ),
            errorCode = "",
            message = "",
        )
    }

    /** 后端契约：提交块落库后必须有一条 snapshot，让新段落立刻进入 timelineItems。 */
    @Test
    fun commitSnapshotDeliversEveryCommittedParagraphInOrder() {
        val firstBlock = blockRecord("mdb-1", 1UL, "$messageId:1", "第一段已经写完。", committed = true)
        val secondBlock = blockRecord("mdb-2", 2UL, "$messageId:2", "第二段也写完了。", committed = true)

        val timeline = listOf(
            userItem(),
            timelineItem(
                id = "tl-1",
                stableKey = "$messageId:1",
                contentType = "assistant_markdown_block",
                payloadRef = "mdb-1",
                displaySequence = 1001UL,
                summary = "第一段已经写完。",
            ),
            timelineItem(
                id = "tl-2",
                stableKey = "$messageId:2",
                contentType = "assistant_markdown_block",
                payloadRef = "mdb-2",
                displaySequence = 1002UL,
                summary = "第二段也写完了。",
            ),
            timelineItem(
                id = "tl-pending",
                stableKey = pendingStableKey,
                contentType = "assistant_pending_block",
                payloadRef = pendingPayloadRef,
                displaySequence = 1003UL,
                summary = "",
            ),
        )
        val pendingRecord = blockRecord(
            "mdb-pending-1",
            0UL,
            pendingStableKey,
            "第三段正在输入",
            committed = false,
        )

        var state = HamburUiState(selectedSessionId = sessionId)
        state = state.copy(
            timelineItems = listOf(
                UiTimelineItem(
                    id = "tl-user",
                    stableKey = "msg-user-1",
                    contentType = "user_message",
                    versionSequence = 1UL,
                    payloadRef = "msg-user-1",
                    smallSummary = "写三段话",
                    kind = "UserMessage",
                    displaySequence = 900UL,
                ),
                pendingItem(),
            ),
            lastAppliedSequence = 1UL,
        )
        state = state.reduce(
            snapshotEvent(
                sequence = 2UL,
                timelineItems = timeline,
                messageBlockPayloads = listOf(firstBlock, secondBlock, pendingRecord),
            ),
        )
        state = state.reduce(
            markdownEvent(
                sequence = 3UL,
                committed = listOf(firstBlock.node, secondBlock.node),
                pending = pendingRecord.node,
            ),
        )

        val displayItems = state.timelineItems.toChatDisplayItems(state.messageBlocksByPayloadRef)
        val markdown = displayItems.filterIsInstance<ChatDisplayItem.AssistantMarkdownBlock>()

        assertEquals(
            listOf("第一段已经写完。", "第二段也写完了。", "第三段正在输入"),
            markdown.map { it.node.raw },
        )
    }

    /** reducer 侧：pending 块只靠开局那条时间线项，也要能持续渲染流式尾部。 */
    @Test
    fun pendingPlaceholderRendersStreamingTailWithoutSnapshot() {
        val pendingRecord = blockRecord(
            "mdb-pending-1",
            0UL,
            pendingStableKey,
            "第三段正在输入",
            committed = false,
        )

        var state = HamburUiState(selectedSessionId = sessionId)
        state = state.copy(
            timelineItems = listOf(
                UiTimelineItem(
                    id = "tl-user",
                    stableKey = "msg-user-1",
                    contentType = "user_message",
                    versionSequence = 1UL,
                    payloadRef = "msg-user-1",
                    smallSummary = "写三段话",
                    kind = "UserMessage",
                    displaySequence = 900UL,
                ),
                pendingItem(),
            ),
            lastAppliedSequence = 1UL,
        )
        state = state.reduce(
            markdownEvent(
                sequence = 2UL,
                committed = emptyList(),
                pending = pendingRecord.node,
            ),
        )

        val displayItems = state.timelineItems.toChatDisplayItems(state.messageBlocksByPayloadRef)
        val markdown = displayItems.filterIsInstance<ChatDisplayItem.AssistantMarkdownBlock>()

        assertEquals(listOf("第三段正在输入"), markdown.map { it.node.raw })
    }
}
