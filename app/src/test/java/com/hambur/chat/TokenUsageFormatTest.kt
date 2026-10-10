package com.hambur.chat

import com.hambur.chat.reducer.UiTokenUsageGroup
import com.hambur.chat.reducer.UiTokenUsageSnapshot
import com.hambur.chat.ui.settings.buildTokenUsageGroups
import com.hambur.chat.ui.settings.formatCacheHitRate
import com.hambur.chat.ui.settings.formatTokenCount
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 锁定用量页的纯逻辑：
 * - 数字格式（1.2k / 3.4M / 5.6B，保留一位小数）；
 * - 缓存命中率（含除零）；
 * - 按 provider 分组：已知 provider 在前、未知排最后，组内按总量倒序，
 *   展示名缺失时回退到 id，并标记"模型已不在当前配置中"。
 */
class TokenUsageFormatTest {
    @Test
    fun formatsCountsWithCompactSuffixes() {
        assertEquals("0", formatTokenCount(0UL))
        assertEquals("999", formatTokenCount(999UL))
        assertEquals("1.0k", formatTokenCount(1_000UL))
        assertEquals("12.3k", formatTokenCount(12_345UL))
        assertEquals("1.5M", formatTokenCount(1_500_000UL))
        assertEquals("139.7M", formatTokenCount(139_700_000UL))
        assertEquals("2.0B", formatTokenCount(2_000_000_000UL))
    }

    @Test
    fun computesCacheHitRateSafely() {
        assertEquals("50.0%", formatCacheHitRate(cachedTokens = 5UL, promptTokens = 10UL))
        assertEquals("66.0%", formatCacheHitRate(cachedTokens = 92_200UL, promptTokens = 139_700UL))
        assertEquals("0.0%", formatCacheHitRate(cachedTokens = 0UL, promptTokens = 0UL))
    }

    private fun group(
        providerId: String,
        modelId: String,
        promptTokens: ULong,
        completionTokens: ULong,
    ) = UiTokenUsageGroup(
        providerId = providerId,
        modelId = modelId,
        requestCount = 1UL,
        promptTokens = promptTokens,
        completionTokens = completionTokens,
        totalTokens = promptTokens + completionTokens,
    )

    @Test
    fun groupsKnownProvidersFirstAndSortsRowsByTotal() {
        val usage = UiTokenUsageSnapshot(
            byModel = listOf(
                group("prv_1", "deepseek-flash", 45_700_000UL, 296_700UL),
                group("ghost", "mimo-v2.5-pro", 64_700_000UL, 268_900UL),
                group("prv_1", "legacy-model", 10_200UL, 105UL),
            ),
        )

        val groups = buildTokenUsageGroups(
            usage = usage,
            providerNames = mapOf("prv_1" to "OpenAI"),
            modelNames = mapOf(("prv_1" to "deepseek-flash") to "DeepSeek Flash"),
        )

        assertEquals(2, groups.size)
        // 已知 provider 在前，标题用显示名。
        assertEquals("OpenAI", groups[0].title)
        assertFalse(groups[0].unknownProvider)
        assertEquals(2, groups[0].rows.size)
        // 组内按总量倒序。
        assertEquals("DeepSeek Flash", groups[0].rows[0].title)
        assertEquals("deepseek-flash", groups[0].rows[0].modelId)
        // 已知 provider、但模型不在当前配置里 → 标记 notConfigured 并回退到 model id。
        assertEquals("legacy-model", groups[0].rows[1].title)
        assertTrue(groups[0].rows[1].notConfigured)

        // 未知 provider 排最后，标题回退到 id（UI 层再显示为「未知」）。
        assertEquals("ghost", groups[1].title)
        assertTrue(groups[1].unknownProvider)
    }

    @Test
    fun emptyUsageProducesNoGroups() {
        assertTrue(buildTokenUsageGroups(UiTokenUsageSnapshot(), emptyMap(), emptyMap()).isEmpty())
    }
}
