package com.hambur.chat.ui.components

import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutLinearInEasing
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import coil3.compose.SubcomposeAsyncImage
import com.composables.icons.lucide.FileText
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.RefreshCw
import com.composables.icons.lucide.X
import java.io.File
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

data class HamburZoomImageState(
    val model: Any,
    val title: String = "",
    val subtitle: String = "",
)

/**
 * 沉浸式图片放大沙箱 (Image Zoom Lightbox Sandbox)
 *
 * 特性：
 * 1. 双指捏合自由缩放 (1.0x - 5.0x，带弹性阻尼)
 * 2. 放大后单指拖拽平移与视口边界保护
 * 3. 双击平滑聚焦放大 (2.5x) 或复位 (1.0x)
 * 4. 1.0x 下拉拖拽退出，背景实时半透明渐变
 * 5. 单指点按唤起/隐藏顶底栏，体验纯净全屏
 * 6. 顶栏包含缩放百分比胶囊，支持一键复位
 * 7. 优雅平滑的进出场淡入淡出动画，不破坏原有页面体验
 */
@Composable
fun HamburImageZoomSandbox(
    state: HamburZoomImageState,
    onResolveHostPath: ((String) -> String)? = null,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    val resolvedModel = remember(state.model, onResolveHostPath) {
        resolveZoomImageModel(state.model, onResolveHostPath)
    }

    val scale = remember { Animatable(1.0f) }
    val offsetX = remember { Animatable(0f) }
    val offsetY = remember { Animatable(0f) }
    val dismissDragY = remember { Animatable(0f) }
    val backdropAlpha = remember { Animatable(0f) }

    var isDismissing by remember { mutableStateOf(false) }
    var showChrome by remember { mutableStateOf(true) }

    val displayTitle = remember(state.title, state.model) {
        if (state.title.isNotBlank()) state.title
        else when (val m = state.model) {
            is String -> m.substringBefore('?').substringAfterLast('/').ifBlank { "图片预览" }
            is File -> m.name
            else -> "图片预览"
        }
    }

    fun requestDismiss() {
        if (isDismissing) return
        isDismissing = true
        scope.launch {
            launch { backdropAlpha.animateTo(0f, tween(180, easing = FastOutLinearInEasing)) }
            launch { dismissDragY.animateTo(dismissDragY.value + 100f, tween(180, easing = FastOutLinearInEasing)) }
            onDismiss()
        }
    }

    BackHandler {
        requestDismiss()
    }

    LaunchedEffect(Unit) {
        backdropAlpha.animateTo(1f, tween(200, easing = FastOutSlowInEasing))
    }

    val effectiveAlpha = (backdropAlpha.value * (1f - (dismissDragY.value / 600f).coerceIn(0f, 0.75f))).coerceIn(0f, 0.97f)

    BoxWithConstraints(
        modifier = modifier
            .fillMaxSize()
            .zIndex(999f)
            .background(Color.Black.copy(alpha = effectiveAlpha)),
    ) {
        val viewWidth = constraints.maxWidth.toFloat().coerceAtLeast(1f)
        val viewHeight = constraints.maxHeight.toFloat().coerceAtLeast(1f)

        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(Unit) {
                    detectTapGestures(
                        onDoubleTap = { tapOffset ->
                            if (isDismissing) return@detectTapGestures
                            scope.launch {
                                if (scale.value > 1.05f) {
                                    launch { scale.animateTo(1f, tween(200, easing = FastOutSlowInEasing)) }
                                    launch { offsetX.animateTo(0f, tween(200, easing = FastOutSlowInEasing)) }
                                    launch { offsetY.animateTo(0f, tween(200, easing = FastOutSlowInEasing)) }
                                } else {
                                    val targetScale = 2.5f
                                    val center = Offset(viewWidth / 2f, viewHeight / 2f)
                                    val maxOx = (viewWidth * (targetScale - 1f)) / 2f
                                    val maxOy = (viewHeight * (targetScale - 1f)) / 2f
                                    val targetOx = ((center.x - tapOffset.x) * (targetScale - 1f)).coerceIn(-maxOx, maxOx)
                                    val targetOy = ((center.y - tapOffset.y) * (targetScale - 1f)).coerceIn(-maxOy, maxOy)

                                    launch { scale.animateTo(targetScale, tween(200, easing = FastOutSlowInEasing)) }
                                    launch { offsetX.animateTo(targetOx, tween(200, easing = FastOutSlowInEasing)) }
                                    launch { offsetY.animateTo(targetOy, tween(200, easing = FastOutSlowInEasing)) }
                                }
                            }
                        },
                        onTap = {
                            if (!isDismissing) {
                                showChrome = !showChrome
                            }
                        },
                    )
                }
                .pointerInput(Unit) {
                    detectTransformGestures(panZoomLock = false) { centroid, pan, zoom, _ ->
                        if (isDismissing) return@detectTransformGestures
                        scope.launch {
                            if (zoom != 1f) {
                                // 严格钳位在 1.0f - 5.0f，无过度缩放，无回弹
                                val newScale = (scale.value * zoom).coerceIn(1.0f, 5.0f)
                                scale.snapTo(newScale)
                                val maxOx = (viewWidth * (newScale - 1f).coerceAtLeast(0f)) / 2f
                                val maxOy = (viewHeight * (newScale - 1f).coerceAtLeast(0f)) / 2f
                                offsetX.snapTo((offsetX.value + pan.x).coerceIn(-maxOx, maxOx))
                                offsetY.snapTo((offsetY.value + pan.y).coerceIn(-maxOy, maxOy))
                            } else {
                                if (scale.value > 1.05f) {
                                    val maxOx = (viewWidth * (scale.value - 1f)) / 2f
                                    val maxOy = (viewHeight * (scale.value - 1f)) / 2f
                                    offsetX.snapTo((offsetX.value + pan.x).coerceIn(-maxOx, maxOx))
                                    offsetY.snapTo((offsetY.value + pan.y).coerceIn(-maxOy, maxOy))
                                } else {
                                    if (pan.y > 0 || dismissDragY.value > 0) {
                                        val newDragY = (dismissDragY.value + pan.y).coerceAtLeast(0f)
                                        dismissDragY.snapTo(newDragY)
                                    }
                                }
                            }
                        }
                    }
                }
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false)
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Main)
                        } while (event.changes.any { it.pressed })

                        if (isDismissing) return@awaitEachGesture
                        scope.launch {
                            if (dismissDragY.value > 120f) {
                                isDismissing = true
                                launch { backdropAlpha.animateTo(0f, tween(160, easing = FastOutLinearInEasing)) }
                                launch { dismissDragY.animateTo(viewHeight, tween(160, easing = FastOutLinearInEasing)) }
                                onDismiss()
                            } else if (dismissDragY.value > 0f) {
                                // 平滑归零，无回弹物理震荡
                                dismissDragY.animateTo(0f, tween(160, easing = FastOutSlowInEasing))
                            }
                        }
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            SubcomposeAsyncImage(
                model = resolvedModel,
                contentDescription = displayTitle,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        scaleX = scale.value
                        scaleY = scale.value
                        translationX = offsetX.value
                        translationY = offsetY.value + dismissDragY.value
                    },
                loading = {
                    Box(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center,
                    ) {
                        Surface(
                            shape = CircleShape,
                            color = Color.Black.copy(alpha = 0.5f),
                            modifier = Modifier.size(56.dp),
                        ) {
                            Box(
                                modifier = Modifier.fillMaxSize(),
                                contentAlignment = Alignment.Center,
                            ) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(24.dp),
                                    strokeWidth = 2.5.dp,
                                    color = Color.White,
                                )
                            }
                        }
                    }
                },
                error = {
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Icon(
                            imageVector = Lucide.FileText,
                            contentDescription = null,
                            tint = Color.White.copy(alpha = 0.6f),
                            modifier = Modifier.size(36.dp),
                        )
                        Text(
                            text = "图片加载失败",
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            color = Color.White.copy(alpha = 0.9f),
                        )
                        Text(
                            text = state.model.toString(),
                            style = MaterialTheme.typography.bodySmall,
                            color = Color.White.copy(alpha = 0.5f),
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            textAlign = TextAlign.Center,
                        )
                    }
                },
            )
        }

        // 顶栏控件 (带过渡动画)
        AnimatedVisibility(
            visible = showChrome && !isDismissing,
            enter = fadeIn(tween(180)) + slideInVertically(tween(180)) { -it },
            exit = fadeOut(tween(180)) + slideOutVertically(tween(180)) { -it },
            modifier = Modifier.align(Alignment.TopCenter),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .statusBarsPadding()
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                // 关闭按钮
                Box(
                    modifier = Modifier
                        .size(38.dp)
                        .clip(CircleShape)
                        .background(Color.White.copy(alpha = 0.18f))
                        .border(0.5.dp, Color.White.copy(alpha = 0.25f), CircleShape)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                        ) { requestDismiss() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Lucide.X,
                        contentDescription = "Close",
                        tint = Color.White,
                        modifier = Modifier.size(20.dp),
                    )
                }

                // 标题
                Text(
                    text = displayTitle,
                    style = MaterialTheme.typography.titleSmall,
                    color = Color.White.copy(alpha = 0.92f),
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .weight(1f)
                        .padding(horizontal = 16.dp),
                )

                // 缩放百分比药丸与复位按钮 (当放大时显示)
                val currentPercent = (scale.value * 100f).roundToInt()
                if (currentPercent != 100) {
                    Row(
                        modifier = Modifier
                            .height(34.dp)
                            .clip(RoundedCornerShape(17.dp))
                            .background(Color.White.copy(alpha = 0.18f))
                            .border(0.5.dp, Color.White.copy(alpha = 0.25f), RoundedCornerShape(17.dp))
                            .clickable(
                                interactionSource = remember { MutableInteractionSource() },
                                indication = null,
                            ) {
                                scope.launch {
                                    launch { scale.animateTo(1f, tween(240, easing = FastOutSlowInEasing)) }
                                    launch { offsetX.animateTo(0f, tween(240, easing = FastOutSlowInEasing)) }
                                    launch { offsetY.animateTo(0f, tween(240, easing = FastOutSlowInEasing)) }
                                }
                            }
                            .padding(horizontal = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Icon(
                            imageVector = Lucide.RefreshCw,
                            contentDescription = "Reset Zoom",
                            tint = Color.White.copy(alpha = 0.85f),
                            modifier = Modifier.size(13.dp),
                        )
                        Text(
                            text = "$currentPercent%",
                            color = Color.White,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                } else {
                    Spacer(modifier = Modifier.size(38.dp))
                }
            }
        }

        // 底部提示 (轻触全屏，双击放大，下拉关闭)
        AnimatedVisibility(
            visible = showChrome && !isDismissing && scale.value <= 1.05f && dismissDragY.value == 0f,
            enter = fadeIn(tween(200)) + slideInVertically(tween(200)) { it },
            exit = fadeOut(tween(200)) + slideOutVertically(tween(200)) { it },
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Box(
                modifier = Modifier
                    .navigationBarsPadding()
                    .padding(bottom = 20.dp),
                contentAlignment = Alignment.Center,
            ) {
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = Color.Black.copy(alpha = 0.45f),
                    border = androidx.compose.foundation.BorderStroke(0.5.dp, Color.White.copy(alpha = 0.15f)),
                ) {
                    Text(
                        text = "双指缩放 · 双击放大 · 下拉退出",
                        color = Color.White.copy(alpha = 0.65f),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Normal,
                        modifier = Modifier.padding(horizontal = 14.dp, vertical = 6.dp),
                    )
                }
            }
        }
    }
}

private fun resolveZoomImageModel(
    model: Any,
    onResolveHostPath: ((String) -> String)? = null,
): Any {
    if (model !is String) return model
    val str = model.trim()
    val cleanPath = when {
        str.startsWith("hambur://") -> str.removePrefix("hambur://")
        str.startsWith("hambur:") -> str.removePrefix("hambur:")
        else -> str
    }
    val hostPath = if (cleanPath.startsWith("/") && File(cleanPath).exists()) {
        cleanPath
    } else {
        onResolveHostPath?.invoke(str).orEmpty()
    }
    return when {
        str.startsWith("http://") || str.startsWith("https://") -> str
        str.startsWith("content://") -> Uri.parse(str)
        str.startsWith("file://") -> Uri.parse(str)
        hostPath.isNotBlank() && File(hostPath).exists() -> File(hostPath)
        cleanPath.isNotBlank() && File(cleanPath).exists() -> File(cleanPath)
        else -> str
    }
}
