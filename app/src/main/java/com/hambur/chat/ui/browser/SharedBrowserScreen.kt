package com.hambur.chat.ui.browser

import android.view.ViewGroup
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.composables.icons.lucide.Lucide
import com.composables.icons.lucide.RefreshCw
import com.composables.icons.lucide.X
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.ui.components.HamburTopBar
import com.hambur.chat.ui.components.StatusPill

@Composable
fun SharedBrowserScreen(
    state: HamburUiState,
    platformAdapter: AndroidPlatformAdapter,
    onBack: () -> Unit,
) {
    val browserState = state.sharedBrowser

    BackHandler(enabled = browserState.active) {
        // Browser tool actions own the WebView while they are active.
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .statusBarsPadding(),
    ) {
        HamburTopBar(
            title = "Shared Browser",
            subtitle = browserState.url.ifBlank { browserState.status },
            onBack = if (browserState.active) null else onBack,
            actions = {
                IconButton(
                    enabled = !browserState.active,
                    onClick = {
                        platformAdapter.getOrCreateSharedWebView().reload()
                    },
                ) {
                    Icon(imageVector = Lucide.RefreshCw, contentDescription = "Reload")
                }
                IconButton(
                    enabled = !browserState.active,
                    onClick = {
                        platformAdapter.closeSharedWebView()
                        onBack()
                    },
                ) {
                    Icon(imageVector = Lucide.X, contentDescription = "Close browser")
                }
            },
        )
        BrowserStatusStrip(state = state)
        Box(modifier = Modifier.fillMaxSize()) {
            AndroidView(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.White),
                factory = {
                    platformAdapter.getOrCreateSharedWebView().also { webView ->
                        (webView.parent as? ViewGroup)?.removeView(webView)
                    }
                },
                update = {},
            )

            if (browserState.active) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .pointerInput(Unit) {
                            awaitPointerEventScope {
                                while (true) {
                                    val event = awaitPointerEvent()
                                    event.changes.forEach { it.consume() }
                                }
                            }
                        }
                        .background(Color.Black.copy(alpha = 0.22f)),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        CircularProgressIndicator()
                        Text(
                            text = "Browser tool is operating",
                            style = MaterialTheme.typography.bodyMedium,
                            color = Color.White,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun BrowserStatusStrip(state: HamburUiState) {
    val browserState = state.sharedBrowser
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = browserState.action.ifBlank { "No active action" },
                style = MaterialTheme.typography.labelLarge,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = browserState.lastText.ifBlank {
                    browserState.requestId.ifBlank { "Open a browser tool call to populate this tab." }
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        StatusPill(text = browserState.status, active = browserState.active)
    }
}
