package com.hambur.chat.platform

import android.content.Context
import android.webkit.WebView
import android.webkit.WebViewClient
import com.hambur.chat.uniffi.PlatformRequestDto
import java.net.HttpURLConnection
import java.net.URL
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.json.JSONObject

data class PlatformResult(
    val requestId: String,
    val isError: Boolean,
    val payloadJson: String = "",
    val errorCode: String = "",
    val message: String = "",
)

class AndroidPlatformAdapter(
    private val appContext: Context,
) {
    suspend fun handle(request: PlatformRequestDto): PlatformResult {
        return when (request.kind) {
            "BrowserAction" -> handleBrowserAction(request)
            else -> PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "UnsupportedPlatformRequest",
                message = "Unsupported platform request kind: ${request.kind}",
            )
        }
    }

    private suspend fun handleBrowserAction(request: PlatformRequestDto): PlatformResult {
        val action = runCatching {
            JSONObject(request.payloadJson)
                .getJSONObject("action")
        }.getOrElse { error ->
            return PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "InvalidPlatformPayload",
                message = error.message ?: "BrowserAction payload was invalid",
            )
        }
        val actionName = action.optString("action")
        return runCatching {
            val payload = when (actionName) {
                "navigate",
                "get_text",
                "get_page_info",
                "get_readable",
                "get_backbone",
                "wait_for_dom_stable",
                -> loadPageWithWebView(action, request.timeoutMs.toLong())
                "fetch" -> fetchUrl(action, request.timeoutMs.toLong())
                else -> throw IllegalArgumentException("Unsupported browser action: $actionName")
            }
            PlatformResult(
                requestId = request.requestId,
                isError = false,
                payloadJson = payload.toString(),
            )
        }.getOrElse { error ->
            PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "BrowserActionFailed",
                message = error.message ?: "Browser action failed",
            )
        }
    }

    private suspend fun loadPageWithWebView(
        action: JSONObject,
        timeoutMs: Long,
    ): JSONObject = withContext(Dispatchers.Main.immediate) {
        withTimeout(timeoutMs.coerceIn(1_000L, 120_000L)) {
            suspendCancellableCoroutine { continuation ->
                val webView = WebView(appContext)
                webView.settings.javaScriptEnabled = true
                webView.settings.domStorageEnabled = true
                val url = action.optString("url").ifBlank {
                    throw IllegalArgumentException("browser action url must not be empty")
                }
                webView.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(view: WebView, finishedUrl: String) {
                        view.evaluateJavascript(
                            """
                            (() => JSON.stringify({
                              url: location.href,
                              title: document.title || "",
                              text: (document.body && document.body.innerText || "").slice(0, 20000),
                              htmlLength: document.documentElement ? document.documentElement.outerHTML.length : 0
                            }))()
                            """.trimIndent(),
                        ) { encoded ->
                            val result = runCatching {
                                val jsonText = JSONObject("""{"value":$encoded}""")
                                    .optString("value")
                                JSONObject(jsonText)
                            }.getOrElse { error ->
                                JSONObject()
                                    .put("url", finishedUrl)
                                    .put("title", view.title.orEmpty())
                                    .put("text", "")
                                    .put("error", error.message.orEmpty())
                            }
                            if (continuation.isActive) {
                                continuation.resume(result)
                            }
                            view.destroy()
                        }
                    }
                }
                continuation.invokeOnCancellation {
                    webView.stopLoading()
                    webView.destroy()
                }
                webView.loadUrl(url)
            }
        }
    }

    private suspend fun fetchUrl(action: JSONObject, timeoutMs: Long): JSONObject {
        return withContext(Dispatchers.IO) {
            val url = action.optString("url").ifBlank {
                throw IllegalArgumentException("browser fetch url must not be empty")
            }
            val maxBytes = action.optLong("max_bytes", 1_000_000L)
                .coerceIn(1_024L, 10_000_000L)
                .toInt()
            val connection = (URL(url).openConnection() as HttpURLConnection).apply {
                connectTimeout = timeoutMs.coerceIn(1_000L, 120_000L).toInt()
                readTimeout = timeoutMs.coerceIn(1_000L, 120_000L).toInt()
                requestMethod = "GET"
                instanceFollowRedirects = true
            }
            try {
                val status = connection.responseCode
                val stream = if (status >= 400) connection.errorStream else connection.inputStream
                val bytes = stream?.use { it.readBytes(maxBytes) } ?: ByteArray(0)
                JSONObject()
                    .put("url", url)
                    .put("status", status)
                    .put("contentType", connection.contentType.orEmpty())
                    .put("text", bytes.toString(Charsets.UTF_8))
                    .put("truncated", bytes.size >= maxBytes)
            } finally {
                connection.disconnect()
            }
        }
    }
}

private fun java.io.InputStream.readBytes(maxBytes: Int): ByteArray {
    val buffer = ByteArray(8 * 1024)
    val output = java.io.ByteArrayOutputStream()
    var remaining = maxBytes
    while (remaining > 0) {
        val read = read(buffer, 0, minOf(buffer.size, remaining))
        if (read <= 0) break
        output.write(buffer, 0, read)
        remaining -= read
    }
    return output.toByteArray()
}
