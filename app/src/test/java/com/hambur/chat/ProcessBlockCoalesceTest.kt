package com.hambur.chat

import com.hambur.chat.reducer.UiMessageSnapshot
import com.hambur.chat.reducer.UiTimelineItem
import com.hambur.chat.ui.chat.ChatDisplayItem
import com.hambur.chat.ui.chat.ProcessStep
import com.hambur.chat.ui.chat.toChatDisplayItems
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ProcessBlockCoalesceTest {

    private fun createNode(messageId: String, text: String, raw: String = text): MarkdownBlockNodeDto {
        return MarkdownBlockNodeDto(
            messageId = messageId,
            blockId = 0UL,
            stableKey = "node:",
            sourceKind = "assistant",
            nodeKind = "paragraph",
            committed = true,
            level = 0.toUByte(),
            inlines = emptyList(),
            language = "",
            text = text,
            raw = raw,
            childrenJson = "",
            itemsJson = "",
            tableHeader = emptyList(),
            tableRows = emptyList(),
            tableAlignments = emptyList(),
            path = "",
            fileKind = "",
        )
    }

    @Test
    fun multiStepToolCallsCoalesceIntoSingleProcessBlock() {
        val userMsg = UiTimelineItem(
            id = "item-user-1",
            stableKey = "msg-user-1",
            contentType = "user_message",
            versionSequence = 1UL,
            payloadRef = "msg-user-1",
            smallSummary = "Check weather and find activities",
            kind = "UserMessage",
            displaySequence = 1000UL,
        )

        val reasoning1 = UiTimelineItem(
            id = "item-reason-1",
            stableKey = "reason-1",
            contentType = "assistant_reasoning_block",
            versionSequence = 1UL,
            payloadRef = "reason-node-1",
            smallSummary = "First I will search for the weather in Beijing",
            kind = "AssistantReasoningBlock",
            displaySequence = 1001UL,
        )

        val pending1 = UiTimelineItem(
            id = "item-pending-1",
            stableKey = "pending-1",
            contentType = "assistant_pending_block",
            versionSequence = 1UL,
            payloadRef = "pending-node-1",
            smallSummary = "",
            kind = "AssistantPendingBlock",
            displaySequence = 1002UL,
        )

        val tool1 = UiTimelineItem(
            id = "item-tool-1",
            stableKey = "tool-1",
            contentType = "trace",
            versionSequence = 1UL,
            payloadRef = "tool-1",
            smallSummary = "search_weather",
            kind = "ToolTrace",
            traceTitle = "Search weather: Beijing",
            traceContent = "Sunny, 22C",
            traceStatus = "completed",
            toolName = "search_weather",
            displaySequence = 1003UL,
        )

        val reasoning2 = UiTimelineItem(
            id = "item-reason-2",
            stableKey = "reason-2",
            contentType = "assistant_reasoning_block",
            versionSequence = 1UL,
            payloadRef = "reason-node-2",
            smallSummary = "Weather is sunny, now searching for outdoor activities",
            kind = "AssistantReasoningBlock",
            displaySequence = 1004UL,
        )

        val pending2 = UiTimelineItem(
            id = "item-pending-2",
            stableKey = "pending-2",
            contentType = "assistant_pending_block",
            versionSequence = 1UL,
            payloadRef = "pending-node-2",
            smallSummary = "",
            kind = "AssistantPendingBlock",
            displaySequence = 1005UL,
        )

        val tool2 = UiTimelineItem(
            id = "item-tool-2",
            stableKey = "tool-2",
            contentType = "trace",
            versionSequence = 1UL,
            payloadRef = "tool-2",
            smallSummary = "search_activities",
            kind = "ToolTrace",
            traceTitle = "Search activities: Beijing",
            traceContent = "Hiking at Xiangshan",
            traceStatus = "completed",
            toolName = "search_activities",
            displaySequence = 1006UL,
        )

        val reasoning3 = UiTimelineItem(
            id = "item-reason-3",
            stableKey = "reason-3",
            contentType = "assistant_reasoning_block",
            versionSequence = 1UL,
            payloadRef = "reason-node-3",
            smallSummary = "I have enough info to formulate the final answer",
            kind = "AssistantReasoningBlock",
            displaySequence = 1007UL,
        )

        val finalAnswer = UiTimelineItem(
            id = "item-final-1",
            stableKey = "final-1",
            contentType = "assistant_markdown_block",
            versionSequence = 1UL,
            payloadRef = "final-node-1",
            smallSummary = "Beijing is sunny today. Xiangshan is recommended!",
            kind = "AssistantMarkdownBlock",
            displaySequence = 1008UL,
        )

        val nodes = mapOf(
            "reason-node-1" to createNode("msg-asst-1", "First I will search for the weather in Beijing"),
            "pending-node-1" to createNode("msg-asst-1", ""),
            "reason-node-2" to createNode("msg-asst-2", "Weather is sunny, now searching for outdoor activities"),
            "pending-node-2" to createNode("msg-asst-2", ""),
            "reason-node-3" to createNode("msg-asst-3", "I have enough info to formulate the final answer"),
            "final-node-1" to createNode("msg-asst-3", "Beijing is sunny today. Xiangshan is recommended!"),
        )

        val messages = mapOf(
            "msg-user-1" to UiMessageSnapshot(
                id = "msg-user-1",
                sessionId = "s1",
                role = "user",
                contentText = "Check weather and find activities",
                reasoningContent = "",
                status = "completed",
                turnId = "turn-1",
                providerName = "",
                modelName = "",
                finishReason = "",
                nativeFinishReason = "",
                versionSequence = 1UL,
            ),
            "msg-asst-1" to UiMessageSnapshot(
                id = "msg-asst-1",
                sessionId = "s1",
                role = "assistant",
                contentText = "",
                reasoningContent = "First I will search for the weather in Beijing",
                status = "completed",
                turnId = "turn-1",
                providerName = "",
                modelName = "",
                finishReason = "",
                nativeFinishReason = "",
                versionSequence = 1UL,
            ),
            "msg-asst-2" to UiMessageSnapshot(
                id = "msg-asst-2",
                sessionId = "s1",
                role = "assistant",
                contentText = "",
                reasoningContent = "Weather is sunny, now searching for outdoor activities",
                status = "completed",
                turnId = "turn-1",
                providerName = "",
                modelName = "",
                finishReason = "",
                nativeFinishReason = "",
                versionSequence = 1UL,
            ),
            "msg-asst-3" to UiMessageSnapshot(
                id = "msg-asst-3",
                sessionId = "s1",
                role = "assistant",
                contentText = "Beijing is sunny today. Xiangshan is recommended!",
                reasoningContent = "I have enough info to formulate the final answer",
                status = "completed",
                turnId = "turn-1",
                providerName = "",
                modelName = "",
                finishReason = "",
                nativeFinishReason = "",
                versionSequence = 1UL,
            ),
        )

        val timelineItems = listOf(
            userMsg,
            reasoning1,
            pending1,
            tool1,
            reasoning2,
            pending2,
            tool2,
            reasoning3,
            finalAnswer,
        )

        val displayItems = timelineItems.toChatDisplayItems(nodes, messages)

        val processBlocks = displayItems.filterIsInstance<ChatDisplayItem.AssistantProcessBlock>()
        assertEquals(1, processBlocks.size)

        val processBlock = processBlocks.first()
        assertEquals(5, processBlock.steps.size)
        assertTrue(processBlock.steps[0] is ProcessStep.Reasoning)
        assertEquals("First I will search for the weather in Beijing", (processBlock.steps[0] as ProcessStep.Reasoning).text)
        assertTrue(processBlock.steps[1] is ProcessStep.ToolCall)
        assertEquals("search_weather", (processBlock.steps[1] as ProcessStep.ToolCall).toolName)
        assertTrue(processBlock.steps[2] is ProcessStep.Reasoning)
        assertEquals("Weather is sunny, now searching for outdoor activities", (processBlock.steps[2] as ProcessStep.Reasoning).text)
        assertTrue(processBlock.steps[3] is ProcessStep.ToolCall)
        assertEquals("search_activities", (processBlock.steps[3] as ProcessStep.ToolCall).toolName)
        assertTrue(processBlock.steps[4] is ProcessStep.Reasoning)
        assertEquals("I have enough info to formulate the final answer", (processBlock.steps[4] as ProcessStep.Reasoning).text)

        val markdownBlocks = displayItems.filterIsInstance<ChatDisplayItem.AssistantMarkdownBlock>()
        assertEquals(1, markdownBlocks.size)
        assertEquals("Beijing is sunny today. Xiangshan is recommended!", markdownBlocks.first().assistantText)

        val actions = displayItems.filterIsInstance<ChatDisplayItem.AssistantActions>()
        assertEquals(1, actions.size)
    }

    @Test
    fun multipleTurnsHaveIndependentSingleProcessBlocks() {
        val user1 = UiTimelineItem(
            id = "u1",
            stableKey = "u1",
            contentType = "user_message",
            versionSequence = 1UL,
            payloadRef = "u1",
            smallSummary = "Hello",
            kind = "UserMessage",
            displaySequence = 100UL,
        )
        val r1 = UiTimelineItem(
            id = "r1",
            stableKey = "r1",
            contentType = "assistant_reasoning_block",
            versionSequence = 1UL,
            payloadRef = "r1",
            smallSummary = "Thinking about greeting",
            kind = "AssistantReasoningBlock",
            displaySequence = 101UL,
        )
        val ans1 = UiTimelineItem(
            id = "ans1",
            stableKey = "ans1",
            contentType = "assistant_markdown_block",
            versionSequence = 1UL,
            payloadRef = "ans1",
            smallSummary = "Hi there!",
            kind = "AssistantMarkdownBlock",
            displaySequence = 102UL,
        )

        val user2 = UiTimelineItem(
            id = "u2",
            stableKey = "u2",
            contentType = "user_message",
            versionSequence = 1UL,
            payloadRef = "u2",
            smallSummary = "What is 2+2?",
            kind = "UserMessage",
            displaySequence = 200UL,
        )
        val r2 = UiTimelineItem(
            id = "r2",
            stableKey = "r2",
            contentType = "assistant_reasoning_block",
            versionSequence = 1UL,
            payloadRef = "r2",
            smallSummary = "Calculating math",
            kind = "AssistantReasoningBlock",
            displaySequence = 201UL,
        )
        val ans2 = UiTimelineItem(
            id = "ans2",
            stableKey = "ans2",
            contentType = "assistant_markdown_block",
            versionSequence = 1UL,
            payloadRef = "ans2",
            smallSummary = "4",
            kind = "AssistantMarkdownBlock",
            displaySequence = 202UL,
        )

        val nodes = mapOf(
            "r1" to createNode("m1", "Thinking about greeting"),
            "ans1" to createNode("m1", "Hi there!"),
            "r2" to createNode("m2", "Calculating math"),
            "ans2" to createNode("m2", "4"),
        )
        val messages = mapOf(
            "u1" to UiMessageSnapshot(
                id = "u1", sessionId = "s1", role = "user", contentText = "Hello",
                reasoningContent = "", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
            "m1" to UiMessageSnapshot(
                id = "m1", sessionId = "s1", role = "assistant", contentText = "Hi there!",
                reasoningContent = "Thinking about greeting", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
            "u2" to UiMessageSnapshot(
                id = "u2", sessionId = "s1", role = "user", contentText = "What is 2+2?",
                reasoningContent = "", status = "completed", turnId = "t2",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
            "m2" to UiMessageSnapshot(
                id = "m2", sessionId = "s1", role = "assistant", contentText = "4",
                reasoningContent = "Calculating math", status = "completed", turnId = "t2",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
        )

        val displayItems = listOf(user1, r1, ans1, user2, r2, ans2).toChatDisplayItems(nodes, messages)

        val processBlocks = displayItems.filterIsInstance<ChatDisplayItem.AssistantProcessBlock>()
        assertEquals(2, processBlocks.size)
        assertEquals("Thinking about greeting", (processBlocks[0].steps.first() as ProcessStep.Reasoning).text)
        assertEquals("Calculating math", (processBlocks[1].steps.first() as ProcessStep.Reasoning).text)
    }

    @Test
    fun processStepsStrictlySortedByDisplaySequence() {
        val user = UiTimelineItem(
            id = "u1", stableKey = "u1", contentType = "user_message", versionSequence = 1UL,
            payloadRef = "u1", smallSummary = "Test sequence", kind = "UserMessage", displaySequence = 100UL,
        )
        // Step 1: Reasoning 1 (seq = 101)
        val r1 = UiTimelineItem(
            id = "r1", stableKey = "r1-key", contentType = "assistant_reasoning_block", versionSequence = 1UL,
            payloadRef = "r1-node", smallSummary = "Reasoning step 1", kind = "AssistantReasoningBlock", displaySequence = 101UL,
        )
        // Step 2: Tool 1 (seq = 105)
        val t1 = UiTimelineItem(
            id = "t1", stableKey = "t1-key", contentType = "trace", versionSequence = 1UL,
            payloadRef = "t1-ref", smallSummary = "Tool step 1", kind = "ToolTrace",
            traceTitle = "Running tool 1", traceContent = "result 1", traceStatus = "completed",
            toolName = "tool_1", displaySequence = 105UL,
        )
        // Step 3: Reasoning 2 (seq = 110)
        val r2 = UiTimelineItem(
            id = "r2", stableKey = "r2-key", contentType = "assistant_reasoning_block", versionSequence = 1UL,
            payloadRef = "r2-node", smallSummary = "Reasoning step 2", kind = "AssistantReasoningBlock", displaySequence = 110UL,
        )
        // Step 4: Tool 2 (seq = 115)
        val t2 = UiTimelineItem(
            id = "t2", stableKey = "t2-key", contentType = "trace", versionSequence = 1UL,
            payloadRef = "t2-ref", smallSummary = "Tool step 2", kind = "ToolTrace",
            traceTitle = "Running tool 2", traceContent = "result 2", traceStatus = "completed",
            toolName = "tool_2", displaySequence = 115UL,
        )
        // Step 5: Final markdown (seq = 120)
        val ans = UiTimelineItem(
            id = "ans", stableKey = "ans-key", contentType = "assistant_markdown_block", versionSequence = 1UL,
            payloadRef = "ans-node", smallSummary = "Final answer", kind = "AssistantMarkdownBlock", displaySequence = 120UL,
        )

        val nodes = mapOf(
            "r1-node" to createNode("m1", "Reasoning step 1"),
            "r2-node" to createNode("m2", "Reasoning step 2"),
            "ans-node" to createNode("m2", "Final answer"),
        )
        val messages = mapOf(
            "u1" to UiMessageSnapshot(
                id = "u1", sessionId = "s1", role = "user", contentText = "Test sequence",
                reasoningContent = "", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
            "m1" to UiMessageSnapshot(
                id = "m1", sessionId = "s1", role = "assistant", contentText = "",
                reasoningContent = "Reasoning step 1", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
            "m2" to UiMessageSnapshot(
                id = "m2", sessionId = "s1", role = "assistant", contentText = "Final answer",
                reasoningContent = "Reasoning step 2", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
        )

        // Pass items out of order to verify sorting by displaySequence
        val displayItems = listOf(user, r2, t1, r1, t2, ans).toChatDisplayItems(nodes, messages)
        val processBlock = displayItems.filterIsInstance<ChatDisplayItem.AssistantProcessBlock>().first()

        assertEquals(4, processBlock.steps.size)
        // Step 0 must be r1 (displaySequence = 101)
        assertTrue(processBlock.steps[0] is ProcessStep.Reasoning)
        assertEquals("Reasoning step 1", (processBlock.steps[0] as ProcessStep.Reasoning).text)
        assertEquals(101UL, processBlock.steps[0].displaySequence)

        // Step 1 must be t1 (displaySequence = 105)
        assertTrue(processBlock.steps[1] is ProcessStep.ToolCall)
        assertEquals("tool_1", (processBlock.steps[1] as ProcessStep.ToolCall).toolName)
        assertEquals(105UL, processBlock.steps[1].displaySequence)

        // Step 2 must be r2 (displaySequence = 110)
        assertTrue(processBlock.steps[2] is ProcessStep.Reasoning)
        assertEquals("Reasoning step 2", (processBlock.steps[2] as ProcessStep.Reasoning).text)
        assertEquals(110UL, processBlock.steps[2].displaySequence)

        // Step 3 must be t2 (displaySequence = 115)
        assertTrue(processBlock.steps[3] is ProcessStep.ToolCall)
        assertEquals("tool_2", (processBlock.steps[3] as ProcessStep.ToolCall).toolName)
        assertEquals(115UL, processBlock.steps[3].displaySequence)
    }

    @Test
    fun updatingStepWithStableKeyUpdatesInPlaceWithoutDuplicatingOrReordering() {
        val user = UiTimelineItem(
            id = "u1", stableKey = "u1", contentType = "user_message", versionSequence = 1UL,
            payloadRef = "u1", smallSummary = "Hi", kind = "UserMessage", displaySequence = 100UL,
        )
        val r1 = UiTimelineItem(
            id = "item-r1", stableKey = "msg1:reasoning", contentType = "assistant_reasoning_block", versionSequence = 1UL,
            payloadRef = "r1-payload", smallSummary = "Initial thought", kind = "AssistantReasoningBlock", displaySequence = 101UL,
        )
        val t1 = UiTimelineItem(
            id = "item-t1", stableKey = "trace-1", contentType = "trace", versionSequence = 1UL,
            payloadRef = "t1-payload", smallSummary = "tool_a", kind = "ToolTrace",
            traceTitle = "Tool A", traceContent = "ok", traceStatus = "completed",
            toolName = "tool_a", displaySequence = 105UL,
        )

        val nodesInitial = mapOf(
            "r1-payload" to createNode("msg1", "Initial thought"),
        )
        val messages = mapOf(
            "u1" to UiMessageSnapshot(
                id = "u1", sessionId = "s1", role = "user", contentText = "Hi",
                reasoningContent = "", status = "completed", turnId = "t1",
                providerName = "", modelName = "", finishReason = "", nativeFinishReason = "", versionSequence = 1UL,
            ),
        )

        val itemsBefore = listOf(user, r1, t1).toChatDisplayItems(nodesInitial, messages)
        val pbBefore = itemsBefore.filterIsInstance<ChatDisplayItem.AssistantProcessBlock>().first()
        assertEquals(2, pbBefore.steps.size)
        assertEquals("Initial thought", (pbBefore.steps[0] as ProcessStep.Reasoning).text)

        // Streaming update arrives: node updated with delta
        val nodesUpdated = mapOf(
            "r1-payload" to createNode("msg1", "Initial thought and further elaboration"),
        )
        val itemsAfter = listOf(user, r1, t1).toChatDisplayItems(nodesUpdated, messages)
        val pbAfter = itemsAfter.filterIsInstance<ChatDisplayItem.AssistantProcessBlock>().first()
        assertEquals(2, pbAfter.steps.size)
        // Step 0 must be updated in place, still at index 0!
        assertEquals("Initial thought and further elaboration", (pbAfter.steps[0] as ProcessStep.Reasoning).text)
        assertEquals("tool_a", (pbAfter.steps[1] as ProcessStep.ToolCall).toolName)
    }
}
