package com.hambur.chat.platform

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.location.Location
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Base64
import android.view.View
import android.webkit.CookieManager
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.hambur.chat.uniffi.PlatformRequestDto
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
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
    private val browserMutex = Mutex()
    private val secretStore = AndroidSecretStore(appContext)
    private var sharedWebView: WebView? = null

    fun saveSecret(secretRef: String, value: String) {
        secretStore.put(secretRef, value)
    }

    fun getSecret(secretRef: String): String? {
        return secretStore.get(secretRef)
    }

    fun getOrCreateSharedWebView(): WebView {
        return browserView()
    }

    fun closeSharedWebView() {
        sharedWebView?.destroy()
        sharedWebView = null
    }

    suspend fun handle(request: PlatformRequestDto): PlatformResult {
        return when (request.kind) {
            "BrowserAction" -> handleBrowserAction(request)
            "ResolveSecret" -> handleResolveSecret(request)
            "AndroidCliAction" -> handleAndroidCliAction(request)
            else -> PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "UnsupportedPlatformRequest",
                message = "Unsupported platform request kind: ${request.kind}",
            )
        }
    }

    private fun handleResolveSecret(request: PlatformRequestDto): PlatformResult {
        return runCatching {
            val payload = JSONObject(request.payloadJson)
            val secretRef = payload.optString("secretRef")
            val value = secretStore.get(secretRef)
                ?: return PlatformResult(
                    requestId = request.requestId,
                    isError = true,
                    errorCode = "SecretNotFound",
                    message = "Secret ref is not available",
                )
            PlatformResult(
                requestId = request.requestId,
                isError = false,
                payloadJson = JSONObject().put("apiKey", value).toString(),
            )
        }.getOrElse { error ->
            PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "ResolveSecretFailed",
                message = error.message ?: "ResolveSecret failed",
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
                "fetch" -> fetchUrl(action, request.timeoutMs.toLong())
                "navigate",
                "get_text",
                "get_page_info",
                "get_readable",
                "get_backbone",
                "click",
                "type",
                "scroll",
                "execute_js",
                "find_elements",
                "hover",
                "screenshot",
                "get_cookies",
                "scroll_and_collect",
                "wait_for_dom_stable",
                -> runWebViewAction(actionName, action, request.timeoutMs.toLong())
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

    private suspend fun runWebViewAction(
        actionName: String,
        action: JSONObject,
        timeoutMs: Long,
    ): JSONObject = browserMutex.withLock {
        withContext(Dispatchers.Main.immediate) {
            withTimeout(timeoutMs.coerceIn(1_000L, 120_000L)) {
                val webView = browserView()
                navigateIfRequested(webView, action)
                when (actionName) {
                    "navigate",
                    "get_text",
                    "get_page_info",
                    "get_readable",
                    "wait_for_dom_stable",
                    -> pageSnapshot(webView, actionName)
                    "get_backbone" -> pageBackbone(webView)
                    "click" -> clickElement(webView, action)
                    "type" -> typeElement(webView, action)
                    "scroll" -> scrollPage(webView, action)
                    "execute_js" -> executeJs(webView, action)
                    "find_elements" -> findElements(webView, action)
                    "hover" -> hoverElement(webView, action)
                    "screenshot" -> screenshot(webView)
                    "get_cookies" -> cookies(webView)
                    "scroll_and_collect" -> scrollAndCollect(webView)
                    else -> throw IllegalArgumentException("Unsupported browser action: $actionName")
                }
            }
        }
    }

    private fun browserView(): WebView {
        sharedWebView?.let { return it }
        return WebView(appContext).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.loadWithOverviewMode = true
            settings.useWideViewPort = true
        }.also { sharedWebView = it }
    }

    private suspend fun navigateIfRequested(webView: WebView, action: JSONObject) {
        val url = action.optString("url")
        if (url.isBlank()) {
            if (webView.url.isNullOrBlank()) {
                throw IllegalArgumentException("browser action url must not be empty")
            }
            return
        }
        if (webView.url == url) return
        suspendCancellableCoroutine { continuation ->
            webView.webViewClient = object : WebViewClient() {
                override fun onPageFinished(view: WebView, finishedUrl: String) {
                    if (continuation.isActive) {
                        continuation.resume(Unit)
                    }
                }
            }
            continuation.invokeOnCancellation {
                webView.stopLoading()
            }
            webView.loadUrl(url)
        }
    }

    private suspend fun pageSnapshot(webView: WebView, action: String): JSONObject {
        return evaluateJson(
            webView,
            """
            const body = document.body;
            const root = document.documentElement;
            return {
              action: ${JSONObject.quote(action)},
              url: location.href,
              title: document.title || "",
              text: (body && body.innerText || "").slice(0, 20000),
              htmlLength: root ? root.outerHTML.length : 0,
              viewport: {
                width: window.innerWidth || 0,
                height: window.innerHeight || 0,
                scrollX: window.scrollX || 0,
                scrollY: window.scrollY || 0,
                scrollHeight: root ? root.scrollHeight : 0
              }
            };
            """.trimIndent(),
        )
    }

    private suspend fun pageBackbone(webView: WebView): JSONObject {
        return evaluateJson(
            webView,
            """
            const textOf = (el) => (el.innerText || el.textContent || el.value || "").trim().slice(0, 500);
            const attrs = (el) => ({
              tag: el.tagName,
              id: el.id || "",
              name: el.getAttribute("name") || "",
              href: el.getAttribute("href") || "",
              role: el.getAttribute("role") || "",
              text: textOf(el)
            });
            return {
              url: location.href,
              title: document.title || "",
              headings: Array.from(document.querySelectorAll("h1,h2,h3")).slice(0, 80).map(attrs),
              links: Array.from(document.querySelectorAll("a[href]")).slice(0, 120).map(attrs),
              controls: Array.from(document.querySelectorAll("button,input,textarea,select,[role=button]")).slice(0, 120).map(attrs)
            };
            """.trimIndent(),
        )
    }

    private suspend fun clickElement(webView: WebView, action: JSONObject): JSONObject {
        val selector = requiredSelector(action)
        return evaluateJson(
            webView,
            """
            const el = document.querySelector(${JSONObject.quote(selector)});
            if (!el) return { url: location.href, clicked: false, selector: ${JSONObject.quote(selector)}, error: "ElementNotFound" };
            el.scrollIntoView({ block: "center", inline: "center" });
            el.click();
            return {
              url: location.href,
              clicked: true,
              selector: ${JSONObject.quote(selector)},
              tag: el.tagName,
              text: (el.innerText || el.textContent || el.value || "").slice(0, 1000)
            };
            """.trimIndent(),
        )
    }

    private suspend fun hoverElement(webView: WebView, action: JSONObject): JSONObject {
        val selector = requiredSelector(action)
        return evaluateJson(
            webView,
            """
            const el = document.querySelector(${JSONObject.quote(selector)});
            if (!el) return { url: location.href, hovered: false, selector: ${JSONObject.quote(selector)}, error: "ElementNotFound" };
            el.scrollIntoView({ block: "center", inline: "center" });
            for (const type of ["mouseover", "mouseenter", "mousemove"]) {
              el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, view: window }));
            }
            return { url: location.href, hovered: true, selector: ${JSONObject.quote(selector)}, tag: el.tagName };
            """.trimIndent(),
        )
    }

    private suspend fun typeElement(webView: WebView, action: JSONObject): JSONObject {
        val selector = requiredSelector(action)
        val text = action.optString("text")
        return evaluateJson(
            webView,
            """
            const el = document.querySelector(${JSONObject.quote(selector)});
            if (!el) return { url: location.href, typed: false, selector: ${JSONObject.quote(selector)}, error: "ElementNotFound" };
            el.scrollIntoView({ block: "center", inline: "center" });
            el.focus();
            el.value = ${JSONObject.quote(text)};
            el.dispatchEvent(new Event("input", { bubbles: true }));
            el.dispatchEvent(new Event("change", { bubbles: true }));
            return { url: location.href, typed: true, selector: ${JSONObject.quote(selector)}, length: ${text.length} };
            """.trimIndent(),
        )
    }

    private suspend fun scrollPage(webView: WebView, action: JSONObject): JSONObject {
        val text = action.optString("text").lowercase()
        val amount = text.toIntOrNull()
            ?: when {
                text.contains("up") -> -900
                text.contains("top") -> Int.MIN_VALUE
                text.contains("bottom") -> Int.MAX_VALUE
                else -> 900
            }
        return evaluateJson(
            webView,
            """
            const amount = $amount;
            if (amount === ${Int.MIN_VALUE}) window.scrollTo(0, 0);
            else if (amount === ${Int.MAX_VALUE}) window.scrollTo(0, document.documentElement.scrollHeight || document.body.scrollHeight || 0);
            else window.scrollBy(0, amount);
            return {
              url: location.href,
              scrollX: window.scrollX || 0,
              scrollY: window.scrollY || 0,
              scrollHeight: document.documentElement ? document.documentElement.scrollHeight : 0,
              text: (document.body && document.body.innerText || "").slice(0, 20000)
            };
            """.trimIndent(),
        )
    }

    private suspend fun executeJs(webView: WebView, action: JSONObject): JSONObject {
        val script = action.optString("script").ifBlank {
            throw IllegalArgumentException("browser execute_js script must not be empty")
        }
        return evaluateJson(
            webView,
            """
            const source = ${JSONObject.quote(script)};
            let value;
            try {
              value = new Function("return (" + source + ");")();
            } catch (expressionError) {
              value = new Function(source)();
            }
            return {
              url: location.href,
              value: value === undefined ? null : value
            };
            """.trimIndent(),
        )
    }

    private suspend fun findElements(webView: WebView, action: JSONObject): JSONObject {
        val selector = action.optString("selector").ifBlank { "a,button,input,textarea,select,[role=button]" }
        return evaluateJson(
            webView,
            """
            const selector = ${JSONObject.quote(selector)};
            const box = (el) => {
              const rect = el.getBoundingClientRect();
              return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
            };
            const textOf = (el) => (el.innerText || el.textContent || el.value || "").trim().slice(0, 1000);
            return {
              url: location.href,
              selector,
              elements: Array.from(document.querySelectorAll(selector)).slice(0, 200).map((el, index) => ({
                index,
                tag: el.tagName,
                id: el.id || "",
                name: el.getAttribute("name") || "",
                href: el.getAttribute("href") || "",
                role: el.getAttribute("role") || "",
                text: textOf(el),
                box: box(el)
              }))
            };
            """.trimIndent(),
        )
    }

    private suspend fun scrollAndCollect(webView: WebView): JSONObject {
        return evaluateJson(
            webView,
            """
            const root = document.documentElement || document.body;
            const originalY = window.scrollY || 0;
            const height = window.innerHeight || 900;
            const maxY = root ? root.scrollHeight : 0;
            const chunks = [];
            for (let y = 0; y <= maxY && chunks.length < 8; y += height) {
              window.scrollTo(0, y);
              chunks.push({
                scrollY: window.scrollY || y,
                text: (document.body && document.body.innerText || "").slice(0, 12000)
              });
            }
            window.scrollTo(0, originalY);
            return { url: location.href, chunks };
            """.trimIndent(),
        )
    }

    private fun screenshot(webView: WebView): JSONObject {
        val width = if (webView.width > 0) webView.width else 1080
        val height = if (webView.height > 0) webView.height else 1920
        webView.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        webView.layout(0, 0, width, height)
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        webView.draw(canvas)
        val output = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
        bitmap.recycle()
        val bytes = output.toByteArray()
        return JSONObject()
            .put("url", webView.url.orEmpty())
            .put("mimeType", "image/png")
            .put("width", width)
            .put("height", height)
            .put("byteSize", bytes.size)
            .put("base64", Base64.encodeToString(bytes, Base64.NO_WRAP))
    }

    private fun cookies(webView: WebView): JSONObject {
        val url = webView.url.orEmpty()
        return JSONObject()
            .put("url", url)
            .put("cookies", CookieManager.getInstance().getCookie(url).orEmpty())
    }

    private suspend fun evaluateJson(webView: WebView, body: String): JSONObject {
        val script = """
            (() => {
              try {
                const value = (() => {
                  $body
                })();
                return JSON.stringify(value && typeof value === "object" ? value : { value });
              } catch (error) {
                return JSON.stringify({ url: location.href, error: String(error && error.message || error) });
              }
            })()
        """.trimIndent()
        return suspendCancellableCoroutine { continuation ->
            webView.evaluateJavascript(script) { encoded ->
                val result = runCatching {
                    val jsonText = JSONObject("""{"value":${encoded ?: "null"}}""")
                        .optString("value")
                    JSONObject(jsonText.ifBlank { "{}" })
                }.getOrElse { error ->
                    JSONObject()
                        .put("url", webView.url.orEmpty())
                        .put("title", webView.title.orEmpty())
                        .put("error", error.message.orEmpty())
                }
                if (continuation.isActive) {
                    continuation.resume(result)
                }
            }
        }
    }

    private fun requiredSelector(action: JSONObject): String {
        return action.optString("selector").ifBlank {
            throw IllegalArgumentException("browser action selector must not be empty")
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

    private suspend fun handleAndroidCliAction(request: PlatformRequestDto): PlatformResult {
        return runCatching {
            val root = JSONObject(request.payloadJson)
            val actionObj = root.optJSONObject("action")
                ?: throw IllegalArgumentException("Missing action object in payload")
            val action = actionObj.optString("action").ifBlank {
                throw IllegalArgumentException("Missing action name in action payload")
            }

            val resultPayload: JSONObject = when (action) {
                "get_location" -> getLocation()
                "get_battery" -> getBattery()
                "get_device_info" -> getDeviceInfo()
                "clipboard_get" -> getClipboard()
                "clipboard_set" -> setClipboard(actionObj.optString("text"))
                "vibrate" -> triggerVibrate(actionObj.optLong("duration_ms", 200L))
                "send_notification" -> sendNotification(
                    actionObj.optString("title", "Hambur Notification"),
                    actionObj.optString("content", "")
                )
                "torch" -> setTorch(actionObj.optBoolean("enabled", false))
                else -> throw IllegalArgumentException("Unsupported android_cli action: $action")
            }

            PlatformResult(
                requestId = request.requestId,
                isError = false,
                payloadJson = resultPayload.toString(),
            )
        }.getOrElse { error ->
            PlatformResult(
                requestId = request.requestId,
                isError = true,
                errorCode = "AndroidCliActionFailed",
                message = error.message ?: "android_cli action failed",
            )
        }
    }

    private suspend fun getLocation(): JSONObject {
        val hasFine = ContextCompat.checkSelfPermission(
            appContext,
            Manifest.permission.ACCESS_FINE_LOCATION
        ) == PackageManager.PERMISSION_GRANTED
        val hasCoarse = ContextCompat.checkSelfPermission(
            appContext,
            Manifest.permission.ACCESS_COARSE_LOCATION
        ) == PackageManager.PERMISSION_GRANTED

        if (!hasFine && !hasCoarse) {
            return JSONObject()
                .put("status", "PermissionDenied")
                .put("message", "Location permission (ACCESS_FINE_LOCATION or ACCESS_COARSE_LOCATION) is not granted")
        }

        val lm = appContext.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
            ?: return JSONObject().put("status", "Unavailable").put("message", "LocationManager unavailable")

        var bestLocation: Location? = null
        val providers = listOf(
            LocationManager.GPS_PROVIDER,
            LocationManager.NETWORK_PROVIDER,
            LocationManager.PASSIVE_PROVIDER
        )
        for (provider in providers) {
            try {
                if (lm.isProviderEnabled(provider)) {
                    val loc = lm.getLastKnownLocation(provider)
                    if (loc != null && (bestLocation == null || loc.time > bestLocation.time)) {
                        bestLocation = loc
                    }
                }
            } catch (_: SecurityException) {
            }
        }

        if (bestLocation != null) {
            return JSONObject()
                .put("status", "Success")
                .put("latitude", bestLocation.latitude)
                .put("longitude", bestLocation.longitude)
                .put("altitude", if (bestLocation.hasAltitude()) bestLocation.altitude else null)
                .put("accuracy", if (bestLocation.hasAccuracy()) bestLocation.accuracy else null)
                .put("provider", bestLocation.provider)
                .put("timestamp", bestLocation.time)
        }

        return JSONObject()
            .put("status", "LocationUnavailable")
            .put("message", "No recent location cached; please ensure GPS/location services are enabled on the device.")
    }

    private fun getBattery(): JSONObject {
        val filter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
        val batteryStatus = appContext.registerReceiver(null, filter)

        val level = batteryStatus?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = batteryStatus?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val batteryPct = if (level >= 0 && scale > 0) (level * 100 / scale.toFloat()) else -1f

        val status = batteryStatus?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val isCharging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
                status == BatteryManager.BATTERY_STATUS_FULL
        val chargePlug = batteryStatus?.getIntExtra(BatteryManager.EXTRA_PLUGGED, -1) ?: -1
        val plugType = when (chargePlug) {
            BatteryManager.BATTERY_PLUGGED_AC -> "AC"
            BatteryManager.BATTERY_PLUGGED_USB -> "USB"
            BatteryManager.BATTERY_PLUGGED_WIRELESS -> "Wireless"
            else -> "Unplugged"
        }
        val tempRaw = batteryStatus?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, 0) ?: 0
        val tempCelsius = tempRaw / 10.0

        return JSONObject()
            .put("level_percent", batteryPct)
            .put("is_charging", isCharging)
            .put("plug_type", plugType)
            .put("temperature_celsius", tempCelsius)
    }

    private fun getDeviceInfo(): JSONObject {
        val cm = appContext.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        val activeNet = cm?.activeNetwork
        val caps = cm?.getNetworkCapabilities(activeNet)

        val isWifi = caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
        val isCellular = caps?.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) == true
        val isEthernet = caps?.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) == true

        val netType = when {
            isWifi -> "WiFi"
            isCellular -> "Cellular"
            isEthernet -> "Ethernet"
            activeNet != null -> "Other"
            else -> "Disconnected"
        }

        return JSONObject()
            .put("brand", Build.BRAND)
            .put("manufacturer", Build.MANUFACTURER)
            .put("model", Build.MODEL)
            .put("device", Build.DEVICE)
            .put("android_release", Build.VERSION.RELEASE)
            .put("sdk_int", Build.VERSION.SDK_INT)
            .put("network_status", netType)
            .put("internet_connected", caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true)
    }

    private suspend fun getClipboard(): JSONObject {
        return withContext(Dispatchers.Main) {
            val cm = appContext.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            val item = cm?.primaryClip?.getItemAt(0)
            val text = item?.text?.toString().orEmpty()
            JSONObject()
                .put("status", "Success")
                .put("text", text)
                .put("length", text.length)
        }
    }

    private suspend fun setClipboard(text: String): JSONObject {
        return withContext(Dispatchers.Main) {
            val cm = appContext.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            val clip = ClipData.newPlainText("hambur_cli", text)
            cm?.setPrimaryClip(clip)
            JSONObject()
                .put("status", "Success")
                .put("length", text.length)
        }
    }

    private fun triggerVibrate(durationMs: Long): JSONObject {
        val clampedDuration = durationMs.coerceIn(10L, 5000L)
        val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val vm = appContext.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
            vm?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            appContext.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

        return if (vibrator != null && vibrator.hasVibrator()) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vibrator.vibrate(
                    VibrationEffect.createOneShot(
                        clampedDuration,
                        VibrationEffect.DEFAULT_AMPLITUDE
                    )
                )
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(clampedDuration)
            }
            JSONObject().put("status", "Success").put("duration_ms", clampedDuration)
        } else {
            JSONObject().put("status", "Unavailable").put("message", "Device has no vibrator")
        }
    }

    private fun sendNotification(title: String, content: String): JSONObject {
        val nm = appContext.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
            ?: return JSONObject().put("status", "Unavailable").put("message", "NotificationManager unavailable")

        val channelId = "hambur_cli_channel"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                channelId,
                "Hambur Assistant",
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = "Notifications from Hambur CLI tools"
            }
            nm.createNotificationChannel(channel)
        }

        val notification = NotificationCompat.Builder(appContext, channelId)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(content)
            .setStyle(NotificationCompat.BigTextStyle().bigText(content))
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .build()

        val notificationId = (System.currentTimeMillis() % 100000).toInt()
        nm.notify(notificationId, notification)

        return JSONObject()
            .put("status", "Success")
            .put("notification_id", notificationId)
            .put("title", title)
    }

    private fun setTorch(enabled: Boolean): JSONObject {
        val cm = appContext.getSystemService(Context.CAMERA_SERVICE) as? CameraManager
            ?: return JSONObject().put("status", "Unavailable").put("message", "CameraManager unavailable")

        return try {
            val cameraId = cm.cameraIdList.firstOrNull { id ->
                cm.getCameraCharacteristics(id)
                    .get(CameraCharacteristics.FLASH_INFO_AVAILABLE) == true
            }
            if (cameraId != null) {
                cm.setTorchMode(cameraId, enabled)
                JSONObject().put("status", "Success").put("torch_enabled", enabled)
            } else {
                JSONObject().put("status", "Unavailable").put("message", "No camera with flash found")
            }
        } catch (e: Exception) {
            JSONObject().put("status", "Error").put("message", e.message ?: "Failed to set torch mode")
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
