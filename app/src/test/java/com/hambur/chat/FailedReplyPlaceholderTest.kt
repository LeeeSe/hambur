package com.hambur.chat

import com.hambur.chat.reducer.UiMessageSnapshot
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.ui.chat.ChatDisplayItem
import com.hambur.chat.ui.chat.toChatDisplayItems
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 回复失败可见性回归测试。
 *
 * 现场：真机上供应商返回 HTTP 400，这一轮 assistant 消息一个字都没生成，只留下一条空的
 * pending 块。旧实现把空块整体跳过，于是用户只看到自己发的消息、模型"瞬间不回复"，
 * 既没有失败提示也没有重试入口。现在失败回合必须渲染成一条可见的失败占位行。
 */
class FailedReplyPlaceholderTest {

    private val sessionId = "s1"
    private val assistantMessageId = "msg-asst-1"
    private val pendingPayloadRef = "mdb-pending-1"

    private fun emptyPendingNode() = MarkdownBlockNodeDto(
        messageId = assistantMessageId,
        blockId = 0UL,
        stableKey = "$assistantMessageId:pending",
        sourceKind = "content",
        nodeKind = "Paragraph",
        committed = false,
        level = 0u.toUByte(),
        inlines = emptyList(),
        language = "",
        text = "",
        raw = "",
        childrenJson = "",
        itemsJson = "",
        tableHeader = emptyList(),
        tableRows = emptyList(),
        tableAlignments = emptyList(),
        path = "",
        fileKind = "",
    )

    private fun timelineItems() = listOf(
        UiTimelineItem(
            id = "tl-user",
            stableKey = "msg-user-1",
            contentType = "user_message",
            versionSequence = 1UL,
            payloadRef = "msg-user-1",
            smallSummary = "我装好 Python 了，你直接开始实验吧",
            kind = "UserMessage",
            displaySequence = 900UL,
        ),
        UiTimelineItem(
            id = "tl-pending",
            stableKey = "$assistantMessageId:pending",
            contentType = "assistant_pending_block",
            versionSequence = 2UL,
            payloadRef = pendingPayloadRef,
            smallSummary = "",
            kind = "AssistantPendingBlock",
            displaySequence = 1000UL,
        ),
    )

    private fun messageWithStatus(status: String) = UiMessageSnapshot(
        id = assistantMessageId,
        sessionId = sessionId,
        role = "assistant",
        contentText = "",
        reasoningContent = "",
        status = status,
        turnId = "turn-1",
        providerName = "deepseek",
        modelName = "deepseek-flash",
        finishReason = "error",
        nativeFinishReason = "ProviderUnavailable",
        versionSequence = 2UL,
    )

    @Test
    fun failedEmptyReplyRendersVisibleFailurePlaceholder() {
        val displayItems = timelineItems().toChatDisplayItems(
            messageBlocksByPayloadRef = mapOf(pendingPayloadRef to emptyPendingNode()),
            messagesById = mapOf(assistantMessageId to messageWithStatus("failed_partial")),
        )

        val failures = displayItems.filterIsInstance<ChatDisplayItem.AssistantFailure>()
        assertEquals(1, failures.size)
        assertEquals(assistantMessageId, failures.single().messageId)
        assertTrue(
            "失败占位必须排在用户消息之后",
            displayItems.indexOfFirst { it is ChatDisplayItem.AssistantFailure } >
                displayItems.indexOfFirst { it is ChatDisplayItem.Timeline },
        )
    }

    @Test
    fun streamingEmptyReplyDoesNotRenderFailure() {
        val displayItems = timelineItems().toChatDisplayItems(
            messageBlocksByPayloadRef = mapOf(pendingPayloadRef to emptyPendingNode()),
            messagesById = mapOf(assistantMessageId to messageWithStatus("streaming")),
        )

        assertTrue(displayItems.filterIsInstance<ChatDisplayItem.AssistantFailure>().isEmpty())
    }

    @Test
    fun cancelledEmptyReplyDoesNotRenderFailure() {
        val displayItems = timelineItems().toChatDisplayItems(
            messageBlocksByPayloadRef = mapOf(pendingPayloadRef to emptyPendingNode()),
            messagesById = mapOf(assistantMessageId to messageWithStatus("cancelled")),
        )

        assertTrue(displayItems.filterIsInstance<ChatDisplayItem.AssistantFailure>().isEmpty())
    }
}
