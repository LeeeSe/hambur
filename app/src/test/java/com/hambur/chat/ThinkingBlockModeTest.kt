package com.hambur.chat

import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_AUTO_EXPAND
import com.hambur.chat.reducer.THINKING_BLOCK_DISPLAY_COLLAPSED
import com.hambur.chat.ui.chat.initialThinkingBlockExpanded
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ThinkingBlockModeTest {
    @Test
    fun autoExpandStartsExpandedRegardlessOfGeneration() {
        assertTrue(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_AUTO_EXPAND, isGenerating = true))
        assertTrue(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_AUTO_EXPAND, isGenerating = false))
    }

    @Test
    fun collapsedStartsCollapsedRegardlessOfGeneration() {
        assertFalse(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_COLLAPSED, isGenerating = true))
        assertFalse(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_COLLAPSED, isGenerating = false))
    }

    @Test
    fun autoCollapseFollowsGenerationState() {
        assertTrue(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE, isGenerating = true))
        assertFalse(initialThinkingBlockExpanded(THINKING_BLOCK_DISPLAY_AUTO_COLLAPSE, isGenerating = false))
    }

    @Test
    fun unknownModeFallsBackToExpanded() {
        assertTrue(initialThinkingBlockExpanded("", isGenerating = false))
        assertTrue(initialThinkingBlockExpanded("unexpected_value", isGenerating = false))
    }
}
