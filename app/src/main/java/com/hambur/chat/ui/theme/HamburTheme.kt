package com.hambur.chat.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

object HamburThemeDefaults {
    const val ThemeMode = "light"
    const val FontScale = "default"
}

object HamburThemeSettingKeys {
    const val ThemeMode = "themeMode"
    const val FontScale = "fontScale"
}

@Immutable
data class HamburChatTokens(
    val headerHeight: Dp = 56.dp,
    val headerHorizontalPadding: Dp = 16.dp,
    val headerTitleHorizontalPadding: Dp = 12.dp,
    val headerIconButtonSize: Dp = 48.dp,
    val headerMenuIconSize: Dp = 24.dp,
    val headerBrowserIconSize: Dp = 23.dp,
    val headerNewChatIconSize: Dp = 24.dp,
    val headerTitleFontSize: TextUnit = 16.sp,
    val timelineBottomGap: Dp = 16.dp,
    val emptyStateHorizontalPadding: Dp = 20.dp,
    val emptyStateIconSize: Dp = 132.dp,
    val emptyStateIconTextGap: Dp = 32.dp,
    val emptyStateTitleFontSize: TextUnit = 16.sp,
    val emptyStateTitleLineHeight: TextUnit = 22.sp,
    val inputOuterStartPadding: Dp = 12.dp,
    val inputOuterEndPadding: Dp = 12.dp,
    val inputOuterTopPadding: Dp = 8.dp,
    val inputOuterBottomPadding: Dp = 10.dp,
    val inputOuterGap: Dp = 10.dp,
    val inputCornerRadius: Dp = 25.dp,
    val inputBorderWidth: Dp = 1.dp,
    val inputBorderAlpha: Float = 0.44f,
    val inputShadowElevation: Dp = 16.dp,
    val inputInnerStartPadding: Dp = 14.dp,
    val inputInnerTopPadding: Dp = 6.dp,
    val inputInnerEndPadding: Dp = 10.dp,
    val inputInnerBottomPadding: Dp = 12.dp,
    val inputContentGap: Dp = 8.dp,
    val inputTextMinHeight: Dp = 44.dp,
    val inputTextMaxHeight: Dp = 128.dp,
    val inputTextStartPadding: Dp = 2.dp,
    val inputPlaceholderAlpha: Float = 0.42f,
    val inputIconButtonSize: Dp = 24.dp,
    val inputIconSize: Dp = 24.dp,
    val inputRightIconGap: Dp = 8.dp,
    val inputLeftIconOffsetX: Dp = 4.dp,
    val inputIconOffsetX: Dp = (-4).dp,
    val inputIconOffsetY: Dp = (-4).dp,
)

@Immutable
data class HamburTokens(
    val chat: HamburChatTokens = HamburChatTokens(),
)

private val LocalHamburTokens = staticCompositionLocalOf { HamburTokens() }

object HamburTheme {
    val tokens: HamburTokens
        @Composable
        @ReadOnlyComposable
        get() = LocalHamburTokens.current
}

@Composable
fun HamburThemeProvider(
    themeMode: String,
    fontScale: String,
    content: @Composable () -> Unit,
) {
    val darkTheme = when (themeMode) {
        "light" -> false
        "system" -> isSystemInDarkTheme()
        else -> true
    }
    val density = LocalDensity.current
    CompositionLocalProvider(
        LocalDensity provides Density(
            density = density.density,
            fontScale = density.fontScale * hamburFontScaleFactor(fontScale),
        ),
        LocalHamburTokens provides HamburTokens(),
    ) {
        MaterialTheme(
            colorScheme = if (darkTheme) hamburDarkColorScheme() else hamburLightColorScheme(),
            content = content,
        )
    }
}

fun hamburFontScaleFactor(fontScale: String): Float {
    return when (fontScale) {
        "small" -> 0.90f
        "large" -> 1.15f
        "extra_large" -> 1.30f
        else -> 1.00f
    }
}

private fun hamburDarkColorScheme(): ColorScheme = darkColorScheme(
    primary = Color(0xFF7DD3C7),
    onPrimary = Color(0xFF06201D),
    primaryContainer = Color(0xFF143F39),
    onPrimaryContainer = Color(0xFFD3F8F1),
    secondary = Color(0xFFB8C8C3),
    tertiary = Color(0xFFF6C56B),
    background = Color(0xFF111312),
    surface = Color(0xFF181B1A),
    surfaceVariant = Color(0xFF252A28),
    onSurface = Color(0xFFE6E9E7),
    onSurfaceVariant = Color(0xFFB9C1BE),
    outline = Color(0xFF717A76),
    outlineVariant = Color(0xFF343B38),
    error = Color(0xFFFFB4AB),
)

private fun hamburLightColorScheme(): ColorScheme = lightColorScheme(
    primary = Color(0xFF000000),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFE5E5EA),
    onPrimaryContainer = Color(0xFF000000),
    secondary = Color(0xFF5F6368),
    tertiary = Color(0xFF3F8CFF),
    background = Color.White,
    surface = Color.White,
    surfaceVariant = Color(0xFFF2F2F7),
    onSurface = Color.Black,
    onSurfaceVariant = Color(0xFF8E8E93),
    outline = Color(0xFFE5E5EA),
    outlineVariant = Color(0xFFE5E5EA),
    error = Color(0xFFBA1A1A),
)
