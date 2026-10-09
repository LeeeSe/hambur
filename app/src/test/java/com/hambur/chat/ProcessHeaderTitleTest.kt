package com.hambur.chat

import com.hambur.chat.ui.chat.ProcessStep
import com.hambur.chat.ui.chat.resolveProcessHeaderTitle
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * 折叠态思考块主标题（ProcessHeaderTitleTest）。
 *
 * 背景：思考块折叠时主标题恒为"正在思考..."，工具执行期间也一样，
 * 用户看不出模型当前在干什么。现在标题会跟随最后一个步骤：
 * 正在跑工具时显示该工具调用的描述（模型传入的 `title`），
 * 纯思考时回退"正在思考..."，回合结束后仍显示用时摘要。
 */
class ProcessHeaderTitleTest {

    private fun reasoning(text: String, seq: ULong = 1UL) =
        ProcessStep.Reasoning(id = "r-$seq", text = text, displaySequence = seq)

    private fun toolCall(
        toolName: String,
        title: String,
        seq: ULong = 2UL,
        status: String = "running",
        argumentsJson: String = "",
    ) = ProcessStep.ToolCall(
        id = "t-$seq",
        toolName = toolName,
        title = title,
        argumentsJson = argumentsJson,
        status = status,
        displaySequence = seq,
    )

    @Test
    fun runningToolCallShowsTheModelDescription() {
        val steps = listOf(
            reasoning("先看一下构建脚本"),
            toolCall("terminal", "安装调试包到手机"),
        )
        assertEquals(
            "安装调试包到手机",
            resolveProcessHeaderTitle(steps, isGenerating = true, durationSeconds = null),
        )
    }

    @Test
    fun theLatestStepWins() {
        val steps = listOf(
            toolCall("terminal", "编译 Rust 库", seq = 1UL),
            reasoning("编译好了", seq = 2UL),
            toolCall("read_file", "读取构建脚本", seq = 3UL),
        )
        assertEquals(
            "读取构建脚本",
            resolveProcessHeaderTitle(steps, isGenerating = true, durationSeconds = null),
        )
    }

    @Test
    fun reasoningOnlyFallsBackToThinking() {
        val steps = listOf(reasoning("在想事情"))
        assertEquals(
            "正在思考...",
            resolveProcessHeaderTitle(steps, isGenerating = true, durationSeconds = null),
        )
        assertEquals(
            "正在思考...",
            resolveProcessHeaderTitle(emptyList(), isGenerating = true, durationSeconds = null),
        )
    }

    @Test
    fun toolCallWithoutDescriptionUsesTheDerivedTitle() {
        val steps = listOf(toolCall("terminal", ""))
        assertEquals(
            "执行终端命令",
            resolveProcessHeaderTitle(steps, isGenerating = true, durationSeconds = null),
        )
    }

    @Test
    fun aDescriptionlessStepWithNoToolNameKeepsTheThinkingLabel() {
        val steps = listOf(toolCall("", ""))
        assertEquals(
            "正在思考...",
            resolveProcessHeaderTitle(steps, isGenerating = true, durationSeconds = null),
        )
    }

    @Test
    fun finishedTurnKeepsTheDurationSummary() {
        val steps = listOf(toolCall("terminal", "安装调试包到手机"))
        assertEquals(
            "已思考（用时 7 秒）",
            resolveProcessHeaderTitle(steps, isGenerating = false, durationSeconds = 7),
        )
        assertEquals(
            "已思考",
            resolveProcessHeaderTitle(steps, isGenerating = false, durationSeconds = null),
        )
        assertEquals(
            "已思考",
            resolveProcessHeaderTitle(steps, isGenerating = false, durationSeconds = 0),
        )
    }
}
