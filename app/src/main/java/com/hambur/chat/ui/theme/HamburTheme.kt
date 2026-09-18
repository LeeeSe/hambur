package com.hambur.chat.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.remember
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
    val textSelectionColors = remember(darkTheme) {
        if (darkTheme) {
            TextSelectionColors(
                handleColor = Color(0xFF7DD3C7),
                backgroundColor = Color(0xFF7DD3C7).copy(alpha = 0.30f),
            )
        } else {
            TextSelectionColors(
                handleColor = Color(0xFF3F8CFF),
                backgroundColor = Color(0xFF3F8CFF).copy(alpha = 0.22f),
            )
        }
    }

    CompositionLocalProvider(
        LocalDensity provides Density(
            density = density.density,
            fontScale = density.fontScale * hamburFontScaleFactor(fontScale),
        ),
        LocalHamburTokens provides HamburTokens(),
    ) {
        MaterialTheme(
            colorScheme = if (darkTheme) hamburDarkColorScheme() else hamburLightColorScheme(),
        ) {
            CompositionLocalProvider(
                LocalTextSelectionColors provides textSelectionColors,
                content = content,
            )
        }
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
    inversePrimary = Color(0xFF143F39),
    secondary = Color(0xFFB8C8C3),
    onSecondary = Color(0xFF23322E),
    secondaryContainer = Color(0xFF252A28),
    onSecondaryContainer = Color(0xFFE6E9E7),
    tertiary = Color(0xFFF6C56B),
    onTertiary = Color(0xFF412D00),
    tertiaryContainer = Color(0xFF3D3219),
    onTertiaryContainer = Color(0xFFFFDEA3),
    background = Color(0xFF111312),
    onBackground = Color(0xFFE6E9E7),
    surface = Color(0xFF181B1A),
    onSurface = Color(0xFFE6E9E7),
    surfaceVariant = Color(0xFF252A28),
    onSurfaceVariant = Color(0xFFB9C1BE),
    surfaceTint = Color.Transparent,
    inverseSurface = Color(0xFFE6E9E7),
    inverseOnSurface = Color(0xFF181B1A),
    outline = Color(0xFF717A76),
    outlineVariant = Color(0xFF343B38),
    error = Color(0xFFFFB4AB),
    onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A),
    onErrorContainer = Color(0xFFFFDAD6),
    surfaceDim = Color(0xFF111312),
    surfaceBright = Color(0xFF343B38),
    surfaceContainerLowest = Color(0xFF0C0E0D),
    surfaceContainerLow = Color(0xFF141716),
    surfaceContainer = Color(0xFF181B1A),
    surfaceContainerHigh = Color(0xFF202422),
    surfaceContainerHighest = Color(0xFF2A2F2D),
)

private fun hamburLightColorScheme(): ColorScheme = lightColorScheme(
    primary = Color(0xFF000000),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFE5E5EA),
    onPrimaryContainer = Color(0xFF000000),
    inversePrimary = Color(0xFFFFFFFF),
    secondary = Color(0xFF5F6368),
    onSecondary = Color(0xFFFFFFFF),
    secondaryContainer = Color(0xFFF2F2F7),
    onSecondaryContainer = Color(0xFF1C1C1E),
    tertiary = Color(0xFF3F8CFF),
    onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFFE8F0FE),
    onTertiaryContainer = Color(0xFF1967D2),
    background = Color.White,
    onBackground = Color(0xFF1C1C1E),
    surface = Color.White,
    onSurface = Color(0xFF1C1C1E),
    surfaceVariant = Color(0xFFF2F2F7),
    onSurfaceVariant = Color(0xFF8E8E93),
    surfaceTint = Color.Transparent,
    inverseSurface = Color(0xFF2C2C2E),
    inverseOnSurface = Color(0xFFF2F2F7),
    outline = Color(0xFFE5E5EA),
    outlineVariant = Color(0xFFE5E5EA),
    error = Color(0xFFBA1A1A),
    onError = Color(0xFFFFFFFF),
    errorContainer = Color(0xFFFFDAD6),
    onErrorContainer = Color(0xFF410002),
    surfaceDim = Color(0xFFE5E5EA),
    surfaceBright = Color.White,
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFF7F7F8),
    surfaceContainer = Color(0xFFF2F2F7),
    surfaceContainerHigh = Color.White,
    surfaceContainerHighest = Color(0xFFE5E5EA),
)
