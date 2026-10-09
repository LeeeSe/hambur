package com.hambur.chat

import com.hambur.chat.ui.chat.toolCallDetail
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * 工具调用详情行（ToolCallDetailTest）。
 *
 * 折叠块展开后，每个工具行第二行显示"这次到底动了什么"（命令 / URL / 路径 / 查询词）。
 * 这些参数来自 tool_calls.arguments_json：trace_spans.content 在工具跑完后会被结果摘要
 * 覆盖，所以详情行必须走 traceArguments，否则只有执行中的那几百毫秒能看到。
 */
class ToolCallDetailTest {

    @Test
    fun terminalShowsTheCommand() {
        assertEquals(
            "pip install numpy",
            toolCallDetail("terminal", """{"command":"pip install numpy","title":"安装依赖"}"""),
        )
    }

    @Test
    fun webToolsShowTheQueryOrUrl() {
        assertEquals(
            "rust 打洞工具",
            toolCallDetail("web_search", """{"queries":["rust 打洞工具","rust nat traversal"]}"""),
        )
        assertEquals(
            "https://example.com/a",
            toolCallDetail("web_fetch", """{"urls":["https://example.com/a","https://example.com/b"]}"""),
        )
        assertEquals(
            "https://example.com/x",
            toolCallDetail("web_fetch", """{"url":"https://example.com/x"}"""),
        )
    }

    @Test
    fun fileAndDeviceToolsShowTheirTarget() {
        assertEquals(
            "/var/hambur/workspace/app/build.gradle.kts",
            toolCallDetail("read_file", """{"path":"/var/hambur/workspace/app/build.gradle.kts"}"""),
        )
        assertEquals(
            "get_device_info",
            toolCallDetail("android_cli", """{"action":"get_device_info","title":"读取机型"}"""),
        )
        assertEquals(
            "get_location",
            toolCallDetail("android_cli", """{"action":"get_location"}"""),
        )
    }

    @Test
    fun missingOrUnparsableArgumentsYieldNoDetail() {
        assertEquals("", toolCallDetail("terminal", ""))
        assertEquals("", toolCallDetail("terminal", "not json"))
        assertEquals("", toolCallDetail("skills_list", """{"title":"列出技能"}"""))
        assertEquals("", toolCallDetail("unknown_tool", """{"command":"ls"}"""))
    }
}
