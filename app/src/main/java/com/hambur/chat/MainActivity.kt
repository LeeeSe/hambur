package com.hambur.chat

import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.ui.app.HamburApp
import java.io.File
import java.util.UUID
import kotlin.coroutines.resume
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

import androidx.compose.foundation.ComposeFoundationFlags
import androidx.compose.foundation.ExperimentalFoundationApi
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit

@OptIn(ExperimentalFoundationApi::class)
class MainActivity : ComponentActivity() {
    companion object {
        init {
            ComposeFoundationFlags.isNewContextMenuEnabled = false
        }
    }
    private var pendingPickedAttachment: ((String, String, ULong, String, String) -> Unit)? = null
    private val imagePicker = registerForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        handlePickedAttachment(uri)
    }
    private val filePicker = registerForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        handlePickedAttachment(uri)
    }

    private val permissionMutex = Mutex()
    private var pendingPermissionContinuation: CancellableContinuation<Map<String, Boolean>>? = null
    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { results ->
        val cont = pendingPermissionContinuation
        pendingPermissionContinuation = null
        cont?.resume(results)
    }

    private suspend fun requestPermissionsInternal(permissions: Array<String>): Map<String, Boolean> {
        val results = mutableMapOf<String, Boolean>()
        val needed = mutableListOf<String>()
        for (perm in permissions) {
            if (ContextCompat.checkSelfPermission(this, perm) == PackageManager.PERMISSION_GRANTED) {
                results[perm] = true
            } else {
                needed.add(perm)
            }
        }
        if (needed.isEmpty()) {
            return results
        }

        val requestedResults = permissionMutex.withLock {
            withContext(Dispatchers.Main.immediate) {
                suspendCancellableCoroutine { continuation ->
                    pendingPermissionContinuation = continuation
                    continuation.invokeOnCancellation {
                        pendingPermissionContinuation = null
                    }
                    permissionLauncher.launch(needed.toTypedArray())
                }
            }
        }
        results.putAll(requestedResults)
        return results
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        SingletonImageLoader.setSafe { context ->
            val okHttpClient = OkHttpClient.Builder()
                .connectTimeout(15, TimeUnit.SECONDS)
                .readTimeout(20, TimeUnit.SECONDS)
                .addInterceptor { chain ->
                    val request = chain.request().newBuilder()
                        .header(
                            "User-Agent",
                            "Mozilla/5.0 (Linux; Android 14; Mobile) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36",
                        )
                        .build()
                    chain.proceed(request)
                }
                .build()

            ImageLoader.Builder(context)
                .components {
                    add(OkHttpNetworkFetcherFactory(callFactory = { okHttpClient }))
                }
                .build()
        }

        val platformAdapter = AndroidPlatformAdapter(
            appContext = applicationContext,
            permissionRequester = { permissions -> requestPermissionsInternal(permissions) },
        )

        setContent {
            HamburApp(
                appFilesDir = filesDir.absolutePath,
                nativeLibraryDir = applicationInfo.nativeLibraryDir,
                platformAdapter = platformAdapter,
                onPickImage = { onPicked ->
                    pendingPickedAttachment = onPicked
                    imagePicker.launch("image/*")
                },
                onPickFile = { onPicked ->
                    pendingPickedAttachment = onPicked
                    filePicker.launch("*/*")
                },
            )
        }
    }

    private fun handlePickedAttachment(uri: Uri?) {
        val callback = pendingPickedAttachment ?: return
        pendingPickedAttachment = null
        if (uri == null) return
        callback(
            displayNameFor(uri),
            contentResolver.getType(uri).orEmpty(),
            sizeFor(uri),
            uri.toString(),
            copyUriToCache(uri, displayNameFor(uri)),
        )
    }

    private fun displayNameFor(uri: Uri): String {
        val cursor = contentResolver.query(
            uri,
            arrayOf(android.provider.OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )
        cursor?.use {
            if (it.moveToFirst()) {
                val index = it.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                if (index >= 0) {
                    return it.getString(index).orEmpty().ifBlank {
                        uri.lastPathSegment ?: "attachment"
                    }
                }
            }
        }
        return uri.lastPathSegment ?: "attachment"
    }

    private fun sizeFor(uri: Uri): ULong {
        val cursor = contentResolver.query(
            uri,
            arrayOf(android.provider.OpenableColumns.SIZE),
            null,
            null,
            null,
        )
        cursor?.use {
            if (it.moveToFirst()) {
                val index = it.getColumnIndex(android.provider.OpenableColumns.SIZE)
                if (index >= 0 && !it.isNull(index)) {
                    return it.getLong(index).coerceAtLeast(0L).toULong()
                }
            }
        }
        return 0UL
    }

    private fun copyUriToCache(uri: Uri, displayName: String): String {
        return runCatching {
            val dir = File(cacheDir, "hambur_attachments").also { it.mkdirs() }
            val safeName = displayName
                .ifBlank { uri.lastPathSegment ?: "attachment" }
                .map { ch ->
                    if (ch.isLetterOrDigit() || ch == '.' || ch == '-' || ch == '_') ch else '_'
                }
                .joinToString("")
                .ifBlank { "attachment" }
                .take(96)
            val file = File(dir, "${UUID.randomUUID()}-$safeName")
            contentResolver.openInputStream(uri)?.use { input ->
                file.outputStream().use { output -> input.copyTo(output) }
            } ?: return@runCatching ""
            file.absolutePath
        }.getOrDefault("")
    }
}
