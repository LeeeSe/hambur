package com.hambur.chat.ui.preview

import android.graphics.Bitmap
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.widget.VideoView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import coil3.compose.AsyncImage
import coil3.compose.SubcomposeAsyncImage
import com.composables.icons.lucide.FileText
import com.composables.icons.lucide.Lucide
import com.hambur.chat.reducer.HamburUiState
import com.hambur.chat.reducer.HamburUiStore
import com.hambur.chat.ui.components.HamburSection
import com.hambur.chat.ui.components.HamburTopBar
import com.hambur.chat.ui.components.HamburZoomImageState
import com.hambur.chat.ui.markdown.MarkdownBlock
import com.hambur.chat.ui.markdown.rememberMarkdownRenderCache
import com.hambur.chat.ui.markdown.rememberMarkdownStyle
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import java.io.File
import java.io.FileInputStream
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private const val TextPreviewMaxBytes = 512 * 1024

private data class ResolvedPreviewFile(
    val originalPath: String,
    val file: File? = null,
    val uri: Uri? = null,
    val webUrl: String? = null,
    val displayName: String,
    val kind: PreviewFileKind,
    val sizeLabel: String,
)

private enum class PreviewFileKind {
    Image,
    Audio,
    Video,
    Markdown,
    Text,
    Pdf,
    Other,
}

private data class TextPreview(
    val text: String,
    val truncated: Boolean,
)

private data class PdfPageBitmap(
    val bitmap: Bitmap,
    val pageNumber: Int,
    val pageCount: Int,
)

@Composable
fun HamburFilePreviewScreen(
    path: String,
    state: HamburUiState,
    store: HamburUiStore,
    onBack: () -> Unit,
    onOpenFile: (String) -> Unit,
    onOpenZoomImage: ((HamburZoomImageState) -> Unit)? = null,
) {
    val resolved = remember(path, state.selectedSessionId) {
        val cleanPath = when {
            path.startsWith("file://") -> Uri.parse(path).path.orEmpty()
            path.startsWith("hambur://") -> {
                val parsed = Uri.parse(path)
                val p = parsed.path.orEmpty()
                if (p.isNotBlank()) p else "/" + path.removePrefix("hambur://").trimStart('/')
            }
            else -> path
        }
        val hostPath = when {
            cleanPath.startsWith("/var/hambur/") -> {
                store.resolveSandboxHostPath(state.selectedSessionId, cleanPath).ifBlank {
                    if (File(cleanPath).exists()) cleanPath else ""
                }
            }
            cleanPath.isNotBlank() && File(cleanPath).exists() -> cleanPath
            cleanPath.isNotBlank() -> {
                store.resolveSandboxHostPath(state.selectedSessionId, cleanPath).ifBlank {
                    if (File(cleanPath).exists()) cleanPath else ""
                }
            }
            else -> ""
        }
        resolvePreviewFile(
            path = path,
            sandboxHostPath = hostPath,
        )
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .statusBarsPadding(),
    ) {
        HamburTopBar(
            title = resolved?.displayName ?: "File Preview",
            subtitle = path,
            onBack = onBack,
        )
        when {
            resolved == null -> FilePreviewMessage(
                title = "File is not available",
                message = path.ifBlank { "No file selected" },
                modifier = Modifier.fillMaxSize(),
            )
            resolved.file != null && !resolved.file.exists() -> FilePreviewMessage(
                title = "File not found",
                message = resolved.file.absolutePath,
                modifier = Modifier.fillMaxSize(),
            )
            resolved.file != null && !resolved.file.isFile -> FilePreviewMessage(
                title = "Unsupported target",
                message = "${resolved.file.absolutePath} is not a regular file.",
                modifier = Modifier.fillMaxSize(),
            )
            else -> FilePreviewBody(
                resolved = resolved,
                state = state,
                store = store,
                onOpenFile = onOpenFile,
                onOpenZoomImage = onOpenZoomImage,
            )
        }
    }
}

@Composable
private fun FilePreviewBody(
    resolved: ResolvedPreviewFile,
    state: HamburUiState,
    store: HamburUiStore,
    onOpenFile: (String) -> Unit,
    onOpenZoomImage: ((HamburZoomImageState) -> Unit)? = null,
) {
    when (resolved.kind) {
        PreviewFileKind.Image -> ImagePreview(
            resolved = resolved,
            onOpenZoomImage = onOpenZoomImage,
        )
        PreviewFileKind.Audio -> AudioPreview(resolved)
        PreviewFileKind.Video -> VideoPreview(resolved)
        PreviewFileKind.Markdown -> MarkdownFilePreview(
            resolved = resolved,
            state = state,
            store = store,
            onOpenFile = onOpenFile,
            onOpenZoomImage = onOpenZoomImage,
        )
        PreviewFileKind.Text -> TextFilePreview(resolved)
        PreviewFileKind.Pdf -> PdfPreview(resolved)
        PreviewFileKind.Other -> FileInfoPreview(resolved)
    }
}

@Composable
private fun ImagePreview(
    resolved: ResolvedPreviewFile,
    onOpenZoomImage: ((HamburZoomImageState) -> Unit)? = null,
) {
    val model: Any? = when {
        resolved.webUrl != null -> resolved.webUrl
        resolved.uri != null -> resolved.uri
        resolved.file != null -> resolved.file
        else -> resolved.originalPath
    }
    val cardShape = RoundedCornerShape(14.dp)
    Box(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        contentAlignment = Alignment.TopCenter,
    ) {
        SubcomposeAsyncImage(
            model = model,
            contentDescription = resolved.displayName,
            modifier = Modifier
                .fillMaxWidth()
                .clip(cardShape)
                .border(0.5.dp, MaterialTheme.colorScheme.outlineVariant, cardShape)
                .clickable {
                    val m = model ?: resolved.originalPath
                    onOpenZoomImage?.invoke(
                        HamburZoomImageState(
                            model = m,
                            title = resolved.displayName,
                        )
                    )
                },
            contentScale = ContentScale.FillWidth,
            loading = {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(240.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(28.dp),
                        strokeWidth = 2.5.dp,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            },
            error = {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Icon(
                        imageVector = Lucide.FileText,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                        modifier = Modifier.size(32.dp),
                    )
                    Text(
                        text = "图片加载失败",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        text = resolved.webUrl ?: resolved.originalPath,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        textAlign = TextAlign.Center,
                    )
                }
            },
        )
    }
}

@Composable
private fun AudioPreview(resolved: ResolvedPreviewFile) {
    MediaInfoPreview(
        title = resolved.displayName,
        message = "Audio preview uses the system player when opened from Android. Inline audio controls are not wired in the new UI yet.",
        resolved = resolved,
    )
}

@Composable
private fun VideoPreview(resolved: ResolvedPreviewFile) {
    val file = resolved.file ?: return
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .height(240.dp),
            shape = RoundedCornerShape(8.dp),
            color = MaterialTheme.colorScheme.surface,
            border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        ) {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { context ->
                    VideoView(context).apply {
                        setVideoURI(Uri.fromFile(file))
                        setOnPreparedListener { player ->
                            player.isLooping = false
                            start()
                        }
                    }
                },
                update = {},
            )
        }
        FileMetadataSection(resolved)
    }
}

@Composable
private fun MarkdownFilePreview(
    resolved: ResolvedPreviewFile,
    state: HamburUiState,
    store: HamburUiStore,
    onOpenFile: (String) -> Unit,
    onOpenZoomImage: ((HamburZoomImageState) -> Unit)? = null,
) {
    val file = resolved.file ?: return
    val preview by produceState<Result<TextPreview>?>(initialValue = null, file) {
        value = runCatching { readTextPreview(file) }
    }
    val messageId = remember(file.absolutePath, file.lastModified()) {
        "file-preview:${file.absolutePath.hashCode()}:${file.lastModified()}"
    }
    val markdownStyle = rememberMarkdownStyle()
    val markdownCache = rememberMarkdownRenderCache()

    val content = preview?.getOrNull()?.let {
        if (it.truncated) {
            it.text + "\n\n... preview truncated"
        } else {
            it.text
        }
    }.orEmpty()
    val blocks by produceState<List<MarkdownBlockNodeDto>>(
        initialValue = emptyList(),
        messageId,
        content,
    ) {
        value = if (content.isBlank()) {
            emptyList()
        } else {
            withContext(Dispatchers.IO) {
                store.renderMarkdownDocument(messageId, content)
            }
        }
    }

    when {
        preview == null -> LoadingPreview()
        preview?.isFailure == true -> FilePreviewMessage(
            title = "Read failed",
            message = preview?.exceptionOrNull()?.message ?: file.absolutePath,
            modifier = Modifier.fillMaxSize(),
        )
        blocks.isEmpty() -> LoadingPreview()
        else -> LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(blocks, key = { it.stableKey }) { block ->
                MarkdownBlock(
                    node = block,
                    style = markdownStyle,
                    renderCache = markdownCache,
                    onOpenDestination = onOpenFile,
                    onResolveHostPath = { path ->
                        store.resolveSandboxHostPath(state.selectedSessionId, path)
                    },
                    onOpenImage = { destination, alt ->
                        onOpenZoomImage?.invoke(
                            HamburZoomImageState(
                                model = destination,
                                title = alt,
                            )
                        )
                    },
                )
            }
        }
    }
}

@Composable
private fun TextFilePreview(resolved: ResolvedPreviewFile) {
    val file = resolved.file ?: return
    val preview by produceState<Result<TextPreview>?>(initialValue = null, file) {
        value = runCatching { readTextPreview(file) }
    }

    when {
        preview == null -> LoadingPreview()
        preview?.isFailure == true -> FilePreviewMessage(
            title = "Read failed",
            message = preview?.exceptionOrNull()?.message ?: file.absolutePath,
            modifier = Modifier.fillMaxSize(),
        )
        else -> {
            val result = preview!!.getOrThrow()
            val text = if (result.truncated) {
                result.text + "\n\n... preview truncated"
            } else {
                result.text
            }
            SelectionContainer {
                Text(
                    text = text,
                    style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(16.dp)
                        .horizontalScroll(rememberScrollState())
                        .verticalScroll(rememberScrollState()),
                )
            }
        }
    }
}

@Composable
private fun PdfPreview(resolved: ResolvedPreviewFile) {
    val file = resolved.file ?: return
    val pageCount by produceState<Result<Int>?>(initialValue = null, file) {
        value = runCatching { readPdfPageCount(file) }
    }

    when {
        pageCount == null -> LoadingPreview()
        pageCount?.isFailure == true -> FilePreviewMessage(
            title = "PDF read failed",
            message = pageCount?.exceptionOrNull()?.message ?: file.absolutePath,
            modifier = Modifier.fillMaxSize(),
        )
        pageCount!!.getOrThrow() <= 0 -> FilePreviewMessage(
            title = "Empty PDF",
            message = file.absolutePath,
            modifier = Modifier.fillMaxSize(),
        )
        else -> BoxWithConstraints(
            modifier = Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background),
        ) {
            val density = LocalDensity.current
            val targetWidthPx = remember(maxWidth, density) {
                with(density) {
                    (maxWidth - 32.dp)
                        .coerceAtLeast(1.dp)
                        .toPx()
                        .roundToInt()
                }
            }
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items((0 until pageCount!!.getOrThrow()).toList()) { pageIndex ->
                    PdfPage(
                        file = file,
                        pageIndex = pageIndex,
                        targetWidthPx = targetWidthPx,
                    )
                }
            }
        }
    }
}

@Composable
private fun PdfPage(
    file: File,
    pageIndex: Int,
    targetWidthPx: Int,
) {
    val pageBitmap by produceState<Result<PdfPageBitmap>?>(initialValue = null, file, pageIndex, targetWidthPx) {
        value = runCatching { renderPdfPage(file, pageIndex, targetWidthPx) }
    }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = androidx.compose.ui.graphics.Color.White,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        when {
            pageBitmap == null -> Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(220.dp),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp))
            }
            pageBitmap?.isFailure == true -> Text(
                text = "Page ${pageIndex + 1} render failed",
                modifier = Modifier.padding(24.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
            else -> {
                val rendered = pageBitmap!!.getOrThrow()
                Box(modifier = Modifier.fillMaxWidth()) {
                    Image(
                        bitmap = rendered.bitmap.asImageBitmap(),
                        contentDescription = "Page ${rendered.pageNumber}",
                        modifier = Modifier.fillMaxWidth(),
                        contentScale = ContentScale.FillWidth,
                    )
                    Text(
                        text = "${rendered.pageNumber} / ${rendered.pageCount}",
                        style = MaterialTheme.typography.labelSmall,
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(8.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(MaterialTheme.colorScheme.surface.copy(alpha = 0.86f))
                            .padding(horizontal = 8.dp, vertical = 3.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun FileInfoPreview(resolved: ResolvedPreviewFile) {
    MediaInfoPreview(
        title = resolved.displayName,
        message = "This file type does not have an inline preview yet.",
        resolved = resolved,
    )
}

@Composable
private fun MediaInfoPreview(
    title: String,
    message: String,
    resolved: ResolvedPreviewFile,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        FilePreviewMessage(
            title = title,
            message = message,
            modifier = Modifier.fillMaxWidth(),
        )
        FileMetadataSection(resolved)
    }
}

@Composable
private fun FileMetadataSection(resolved: ResolvedPreviewFile) {
    HamburSection(title = "File") {
        MetadataRow(label = "Path", value = resolved.file?.absolutePath ?: resolved.originalPath)
        MetadataRow(label = "Size", value = resolved.sizeLabel)
        MetadataRow(label = "Kind", value = resolved.kind.name)
    }
}

@Composable
private fun MetadataRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(72.dp),
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun LoadingPreview() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        CircularProgressIndicator(modifier = Modifier.size(28.dp))
    }
}

@Composable
private fun FilePreviewMessage(
    title: String,
    message: String,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            imageVector = Lucide.FileText,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(36.dp),
        )
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

private fun resolvePreviewFile(path: String, sandboxHostPath: String = ""): ResolvedPreviewFile? {
    if (path.isBlank()) return null
    if (path.startsWith("http://", ignoreCase = true) || path.startsWith("https://", ignoreCase = true)) {
        val cleanUrl = path.substringBefore('?')
        val fileName = cleanUrl.substringAfterLast('/').ifBlank { "web_file" }
        val ext = fileName.substringAfterLast('.', "").lowercase()
        val isImage = ext in setOf("png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif", "svg", "ico") ||
            cleanUrl.contains("image", ignoreCase = true)
        return ResolvedPreviewFile(
            originalPath = path,
            file = null,
            uri = null,
            webUrl = path,
            displayName = fileName,
            kind = if (isImage) PreviewFileKind.Image else PreviewFileKind.Other,
            sizeLabel = if (isImage) "网络图片" else "网络文件",
        )
    }
    if (path.startsWith("content://", ignoreCase = true)) {
        val parsedUri = Uri.parse(path)
        return ResolvedPreviewFile(
            originalPath = path,
            file = null,
            uri = parsedUri,
            webUrl = null,
            displayName = "图片",
            kind = PreviewFileKind.Image,
            sizeLabel = "本地图片",
        )
    }
    val normalized = when {
        path.startsWith("file://") -> Uri.parse(path).path.orEmpty()
        path.startsWith("hambur://") -> {
            val parsed = Uri.parse(path)
            val p = parsed.path.orEmpty()
            if (p.isNotBlank()) p else "/" + path.removePrefix("hambur://").trimStart('/')
        }
        else -> path
    }
    if (normalized.isBlank()) return null
    val targetPath = if (sandboxHostPath.isNotBlank()) sandboxHostPath else normalized
    val file = File(targetPath)
    val extension = file.extension.lowercase()
    val kind = when {
        extension in setOf("png", "jpg", "jpeg", "webp", "gif", "bmp", "heic", "heif", "svg") -> PreviewFileKind.Image
        extension in setOf("mp3", "m4a", "aac", "wav", "ogg", "flac") -> PreviewFileKind.Audio
        extension in setOf("mp4", "webm", "mov", "mkv", "avi") -> PreviewFileKind.Video
        extension in setOf("md", "markdown", "mdown") -> PreviewFileKind.Markdown
        extension == "pdf" -> PreviewFileKind.Pdf
        extension in setOf("txt", "json", "jsonl", "log", "csv", "tsv", "xml", "html", "css", "js", "ts", "kt", "java", "rs", "toml", "yaml", "yml", "sh", "py") -> PreviewFileKind.Text
        else -> PreviewFileKind.Other
    }
    return ResolvedPreviewFile(
        originalPath = path,
        file = file,
        displayName = file.name.ifBlank { path },
        kind = kind,
        sizeLabel = sizeLabel(file.length()),
    )
}

private suspend fun readTextPreview(file: File): TextPreview = withContext(Dispatchers.IO) {
    val buffer = ByteArray(TextPreviewMaxBytes)
    var offset = 0
    FileInputStream(file).use { input ->
        while (offset < TextPreviewMaxBytes) {
            val read = input.read(buffer, offset, TextPreviewMaxBytes - offset)
            if (read <= 0) break
            offset += read
        }
    }
    TextPreview(
        text = buffer.copyOf(offset).toString(Charsets.UTF_8),
        truncated = file.length() > TextPreviewMaxBytes,
    )
}

private suspend fun readPdfPageCount(file: File): Int = withContext(Dispatchers.IO) {
    ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY).use { descriptor ->
        PdfRenderer(descriptor).use { renderer ->
            renderer.pageCount
        }
    }
}

private suspend fun renderPdfPage(
    file: File,
    pageIndex: Int,
    targetWidthPx: Int,
): PdfPageBitmap = withContext(Dispatchers.IO) {
    ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY).use { descriptor ->
        PdfRenderer(descriptor).use { renderer ->
            renderer.openPage(pageIndex).use { page ->
                val width = targetWidthPx.coerceAtLeast(1)
                val scale = width.toFloat() / page.width.toFloat().coerceAtLeast(1f)
                val height = (page.height * scale).roundToInt().coerceAtLeast(1)
                val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                bitmap.eraseColor(android.graphics.Color.WHITE)
                page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                PdfPageBitmap(
                    bitmap = bitmap,
                    pageNumber = pageIndex + 1,
                    pageCount = renderer.pageCount,
                )
            }
        }
    }
}

private fun sizeLabel(bytes: Long): String {
    val units = listOf("B", "KB", "MB", "GB")
    var value = bytes.toDouble().coerceAtLeast(0.0)
    var unit = 0
    while (value >= 1024.0 && unit < units.lastIndex) {
        value /= 1024.0
        unit += 1
    }
    return if (unit == 0) {
        "${value.toLong()} ${units[unit]}"
    } else {
        "%.1f %s".format(value, units[unit])
    }
}
