package com.hambur.chat

import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.thinkingEnabledForSession
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 思考开关的取值来源。每次启动都会新建一个「新对话」会话，它没有自己的记录，
 * 所以必须能回落到用户上次手动切换的值，否则重开 APP 后按钮永远是关闭。
 */
class ThinkingStateFallbackTest {
    @Test
    fun neverToggledFollowsSettingDefault() {
        assertTrue(HamburUiState(defaultThinkingEnabled = true).thinkingEnabledForSession("ses_new"))
        assertFalse(HamburUiState(defaultThinkingEnabled = false).thinkingEnabledForSession("ses_new"))
    }

    @Test
    fun lastToggledValueWinsForSessionsWithoutOwnRecord() {
        val state = HamburUiState(defaultThinkingEnabled = false, lastThinkingEnabled = true)
        assertTrue(state.thinkingEnabledForSession("ses_startup_new_chat"))
        assertTrue(state.thinkingEnabledForSession(""))
    }

    @Test
    fun sessionRecordOverridesLastToggledValue() {
        val state = HamburUiState(
            thinkingEnabledBySession = mapOf("ses_a" to false, "__new_session__" to false),
            lastThinkingEnabled = true,
        )
        assertFalse(state.thinkingEnabledForSession("ses_a"))
        assertFalse(state.thinkingEnabledForSession(""))
        assertTrue(state.thinkingEnabledForSession("ses_b"))
    }

    @Test
    fun lastToggledValueOverridesEnabledSettingDefault() {
        val state = HamburUiState(defaultThinkingEnabled = true, lastThinkingEnabled = false)
        assertFalse(state.thinkingEnabledForSession("ses_new"))
    }
}
