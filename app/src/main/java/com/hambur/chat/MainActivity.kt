package com.hambur.chat

import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import com.hambur.chat.platform.AndroidPlatformAdapter
import com.hambur.chat.ui.app.HamburApp
import java.io.File
import java.util.UUID

class MainActivity : ComponentActivity() {
    private var pendingPickedAttachment: ((String, String, ULong, String, String) -> Unit)? = null
    private val imagePicker = registerForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        handlePickedAttachment(uri)
    }
    private val filePicker = registerForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        handlePickedAttachment(uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val platformAdapter = AndroidPlatformAdapter(applicationContext)

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
