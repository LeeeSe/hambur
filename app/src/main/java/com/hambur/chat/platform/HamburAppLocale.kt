package com.hambur.chat.platform

import androidx.appcompat.app.AppCompatDelegate
import androidx.core.os.LocaleListCompat

/**
 * 应用内语言（per-app language），Appearance 页「显示语言」的后端。
 *
 * 只用 AndroidX AppCompat 那套官方 API，不自己拼 Configuration：
 * - Android 13（API 33）及以上：调用被转发给系统 LocaleManager，语言由系统保存，
 *   系统「设置 → 应用 → Hambur → 语言」里也能改，两边天然同步；
 * - Android 12/12L（API 31/32）：AppCompat 接管，语言写进它自己的 SharedPreferences
 *   （靠 AndroidManifest 里 autoStoreLocales 那份元数据开启），下次冷启动在
 *   attachBaseContext 阶段就生效——不会先画一帧错语言再跳。
 *
 * 为什么不像 themeMode / fontScale 那样存进 app_settings（Rust 那份设置库）：
 * 设置快照是异步回来的，等它到达首帧早画完了，只能再切一次语言（伴随重建/闪屏）；
 * 而且 Android 13+ 的系统设置页也能改语言，两份真值必然会对不上。
 * 所以唯一真值就是 AppCompat/系统这里的按应用语言，[current] 直接读回它。
 */
object HamburAppLocale {
    /** 跟随系统：等价于「清空按应用语言」，让 Resources 回落设备语言。 */
    const val System = "system"
    const val English = "en"
    const val SimplifiedChinese = "zh-CN"

    /** Appearance 页给的选项（顺序即展示顺序，标签见 appearance_language_* 字符串）。 */
    val options = listOf(System, English, SimplifiedChinese)

    /** 当前选项：跟随系统时是 [System]，否则是归一化后的语言标签。 */
    fun current(): String {
        val locales = AppCompatDelegate.getApplicationLocales()
        if (locales.isEmpty) return System
        val selected = locales.get(0) ?: return System
        return when (selected.language) {
            // 只按语言归一化：系统可能回 "zh-Hans-CN"、"zh-CN" 这些写法，界面上都算「简体中文」。
            "zh" -> SimplifiedChinese
            "en" -> English
            else -> selected.toLanguageTag()
        }
    }

    /** 切到 [tag]（[System] 表示跟随系统）。Activity 不重建，见 AndroidManifest 的 configChanges。 */
    fun apply(tag: String) {
        val target = if (tag == System) {
            LocaleListCompat.getEmptyLocaleList()
        } else {
            LocaleListCompat.forLanguageTags(tag)
        }
        // 目标与当前一致就别再调了：AppCompat 每次调用都会重新套一遍 Configuration，
        // 白跑一趟还会让整棵树多重组一次。
        if (target == AppCompatDelegate.getApplicationLocales()) return
        AppCompatDelegate.setApplicationLocales(target)
    }
}
