package com.hambur.chat.ui.markdown

import android.net.Uri
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.layout.ContentScale
import coil3.compose.AsyncImage
import java.io.File
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.hambur.chat.perf.ChatJankTracer
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.MarkdownInlineNodeDto
import kotlin.math.max

private const val DestinationAnnotation = "hambur_destination"

@Stable
class MarkdownRenderCache(private val maxEntries: Int = 512) {
    private data class AnnotatedStringKey(
        val messageId: String,
        val blockId: ULong,
        val styleVersion: Int,
    )

    private data class AnnotatedStringEntry(
        val contentFingerprint: Int,
        val styleFingerprint: Int,
        val annotatedString: AnnotatedString,
    )

    private val annotatedStrings = object :
        LinkedHashMap<AnnotatedStringKey, AnnotatedStringEntry>(maxEntries, 0.75f, true) {
        override fun removeEldestEntry(
            eldest: MutableMap.MutableEntry<AnnotatedStringKey, AnnotatedStringEntry>,
        ): Boolean = size > maxEntries
    }

    fun annotatedStringFor(
        node: MarkdownBlockNodeDto,
        style: MarkdownStyle,
    ): AnnotatedString {
        val key = AnnotatedStringKey(
            messageId = node.messageId,
            blockId = node.blockId,
            styleVersion = style.styleVersion,
        )
        val contentFingerprint = node.contentFingerprint()
        val styleFingerprint = style.visualFingerprint()
        val cached = annotatedStrings[key]
        if (
            cached != null &&
            cached.contentFingerprint == contentFingerprint &&
            cached.styleFingerprint == styleFingerprint
        ) {
            return cached.annotatedString
        }

        val annotatedString = buildAnnotatedString {
            appendInlineNodes(
                inlines = node.inlines,
                linkColor = style.linkColor,
                inlineCodeBackground = style.inlineCodeBackground,
            )
        }
        annotatedStrings[key] = AnnotatedStringEntry(
            contentFingerprint = contentFingerprint,
            styleFingerprint = styleFingerprint,
            annotatedString = annotatedString,
        )
        return annotatedString
    }

    fun clear() {
        annotatedStrings.clear()
    }
}

@Composable
fun rememberMarkdownRenderCache(maxEntries: Int = 512): MarkdownRenderCache {
    return remember(maxEntries) { MarkdownRenderCache(maxEntries) }
}

@Stable
data class MarkdownStyle(
    val styleVersion: Int,
    val paragraphTextStyle: TextStyle,
    val heading1TextStyle: TextStyle,
    val heading2TextStyle: TextStyle,
    val headingTextStyle: TextStyle,
    val labelTextStyle: TextStyle,
    val codeTextStyle: TextStyle,
    val smallTextStyle: TextStyle,
    val linkColor: Color,
    val inlineCodeBackground: Color,
    val codeBlockColor: Color,
    val blockSurfaceColor: Color,
    val quoteRailColor: Color,
    val outlineColor: Color,
    val onSurfaceVariantColor: Color,
    val tableHeaderBackgroundColor: Color,
    val tableBodyBackgroundColor: Color,
    val tableHeaderTextStyle: TextStyle,
    val tableBodyTextStyle: TextStyle,
    val tableMinColumnWidth: Dp,
    val tableMaxColumnWidth: Dp,
    val tableCellHorizontalPadding: Dp,
    val tableCellVerticalPadding: Dp,
    val cornerRadius: Dp,
    val blockPadding: Dp,
)

@Composable
fun rememberMarkdownStyle(styleVersion: Int = 1): MarkdownStyle {
    val colors = MaterialTheme.colorScheme
    val typography = MaterialTheme.typography
    return remember(colors, typography, styleVersion) {
        MarkdownStyle(
            styleVersion = styleVersion,
            paragraphTextStyle = typography.bodyLarge,
            heading1TextStyle = typography.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
            heading2TextStyle = typography.titleLarge.copy(fontWeight = FontWeight.SemiBold),
            headingTextStyle = typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
            labelTextStyle = typography.labelSmall,
            codeTextStyle = typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            smallTextStyle = typography.bodySmall,
            linkColor = colors.primary,
            inlineCodeBackground = colors.surfaceVariant,
            codeBlockColor = colors.surfaceVariant,
            blockSurfaceColor = colors.surface,
            quoteRailColor = colors.primary,
            outlineColor = colors.outlineVariant,
            onSurfaceVariantColor = colors.onSurfaceVariant,
            tableHeaderBackgroundColor = colors.surfaceVariant.copy(alpha = 0.72f),
            tableBodyBackgroundColor = colors.surface,
            tableHeaderTextStyle = typography.labelLarge.copy(fontWeight = FontWeight.SemiBold),
            tableBodyTextStyle = typography.bodyMedium,
            tableMinColumnWidth = 92.dp,
            tableMaxColumnWidth = 224.dp,
            tableCellHorizontalPadding = 12.dp,
            tableCellVerticalPadding = 8.dp,
            cornerRadius = 8.dp,
            blockPadding = 12.dp,
        )
    }
}

@Composable
fun MarkdownBlock(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
    style: MarkdownStyle? = null,
    renderCache: MarkdownRenderCache? = null,
    onOpenDestination: (String) -> Unit = {},
    onResolveHostPath: ((String) -> String)? = null,
) {
    val markdownStyle = style ?: rememberMarkdownStyle()
    SideEffect {
        ChatJankTracer.markSessionSwitchOnce(
            phase = "markdown_block_seen",
            key = "markdown_seen:${node.messageId}:${node.blockId}",
            extra = "kind=${node.nodeKind} message=${traceShortId(node.messageId)} block=${node.blockId} raw=${node.raw.length} text=${node.text.length}",
        )
    }
    when (node.nodeKind) {
        "Heading" -> MarkdownInlineText(
            node = node,
            textStyle = markdownStyle.headingStyle(node.level.toInt()),
            markdownStyle = markdownStyle,
            renderCache = renderCache,
            onOpenDestination = onOpenDestination,
            modifier = modifier.fillMaxWidth(),
        )
        "Paragraph" -> MarkdownInlineText(
            node = node,
            textStyle = markdownStyle.paragraphTextStyle,
            markdownStyle = markdownStyle,
            renderCache = renderCache,
            onOpenDestination = onOpenDestination,
            modifier = modifier.fillMaxWidth(),
        )
        "CodeBlock" -> MarkdownCodeBlock(
            node = node,
            markdownStyle = markdownStyle,
            modifier = modifier,
        )
        "BlockQuote" -> MarkdownBlockQuote(
            node = node,
            markdownStyle = markdownStyle,
            renderCache = renderCache,
            onOpenDestination = onOpenDestination,
            modifier = modifier,
        )
        "List" -> MarkdownList(node = node, markdownStyle = markdownStyle, modifier = modifier)
        "Table" -> MarkdownTable(node = node, markdownStyle = markdownStyle, modifier = modifier)
        "ThematicBreak" -> HorizontalDivider(modifier = modifier.fillMaxWidth())
        "HamburFileBlock" -> MarkdownFileBlock(
            node = node,
            markdownStyle = markdownStyle,
            onOpenDestination = onOpenDestination,
            onResolveHostPath = onResolveHostPath,
            modifier = modifier,
        )
        "HtmlBlock", "MathBlock" -> MarkdownPlainBlock(
            node = node,
            markdownStyle = markdownStyle,
            modifier = modifier,
        )
        else -> MarkdownPlainBlock(
            node = node,
            markdownStyle = markdownStyle,
            modifier = modifier,
        )
    }
}

private fun MarkdownStyle.headingStyle(level: Int): TextStyle {
    return when (level) {
        1 -> heading1TextStyle
        2 -> heading2TextStyle
        else -> headingTextStyle
    }
}

private fun MarkdownStyle.visualFingerprint(): Int {
    var result = styleVersion
    result = 31 * result + paragraphTextStyle.hashCode()
    result = 31 * result + heading1TextStyle.hashCode()
    result = 31 * result + heading2TextStyle.hashCode()
    result = 31 * result + headingTextStyle.hashCode()
    result = 31 * result + codeTextStyle.hashCode()
    result = 31 * result + linkColor.hashCode()
    result = 31 * result + inlineCodeBackground.hashCode()
    return result
}

private fun MarkdownBlockNodeDto.contentFingerprint(): Int {
    var result = raw.hashCode()
    result = 31 * result + text.hashCode()
    result = 31 * result + inlines.hashCode()
    return result
}

@Composable
private fun MarkdownInlineText(
    node: MarkdownBlockNodeDto,
    textStyle: TextStyle,
    markdownStyle: MarkdownStyle,
    renderCache: MarkdownRenderCache?,
    onOpenDestination: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val annotated = remember(
        node.messageId,
        node.blockId,
        node.raw,
        node.inlines,
        markdownStyle.styleVersion,
        markdownStyle.visualFingerprint(),
        renderCache,
    ) {
        ChatJankTracer.timeSessionSwitch(
            phase = "markdown_annotated_build",
            warnAtMs = 2.0,
            extra = "kind=${node.nodeKind} message=${traceShortId(node.messageId)} block=${node.blockId} inlineNodes=${node.inlines.size} raw=${node.raw.length}",
        ) {
            renderCache?.annotatedStringFor(node, markdownStyle) ?: buildAnnotatedString {
                appendInlineNodes(
                    inlines = node.inlines,
                    linkColor = markdownStyle.linkColor,
                    inlineCodeBackground = markdownStyle.inlineCodeBackground,
                )
            }
        }
    }
    var layoutResult by remember(annotated) { mutableStateOf<TextLayoutResult?>(null) }

    Text(
        text = annotated,
        style = textStyle,
        onTextLayout = { layoutResult = it },
        modifier = modifier.pointerInput(annotated, onOpenDestination) {
            detectTapGestures { position ->
                val offset = layoutResult?.getOffsetForPosition(position) ?: return@detectTapGestures
                val destination = annotated
                    .getStringAnnotations(DestinationAnnotation, offset, offset)
                    .firstOrNull()
                    ?.item
                    .orEmpty()
                if (destination.isNotBlank()) {
                    onOpenDestination(destination)
                }
            }
        },
    )
}

private fun AnnotatedString.Builder.appendInlineNodes(
    inlines: List<MarkdownInlineNodeDto>,
    linkColor: Color,
    inlineCodeBackground: Color,
) {
    inlines.forEach { inline ->
        when (inline.kind) {
            "Text" -> append(inline.text)
            "SoftBreak", "HardBreak" -> append("\n")
            "InlineCode" -> withStyle(
                SpanStyle(
                    fontFamily = FontFamily.Monospace,
                    background = inlineCodeBackground,
                ),
            ) {
                append(inline.text)
            }
            "Emphasis" -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) {
                appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
            }
            "Strong" -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) {
                appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
            }
            "Strikethrough" -> withStyle(
                SpanStyle(textDecoration = TextDecoration.LineThrough),
            ) {
                appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
            }
            "Link" -> appendDestinationInline(inline, linkColor, inlineCodeBackground)
            "Image" -> appendImageInline(inline, linkColor, inlineCodeBackground)
            else -> appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
        }
    }
}

private fun AnnotatedString.Builder.appendDestinationInline(
    inline: MarkdownInlineNodeDto,
    linkColor: Color,
    inlineCodeBackground: Color,
) {
    val destination = inline.destination
    if (destination.isBlank()) {
        appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
        return
    }

    pushStringAnnotation(DestinationAnnotation, destination)
    withStyle(
        SpanStyle(
            color = linkColor,
            textDecoration = TextDecoration.Underline,
        ),
    ) {
        appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
    }
    pop()
}

private fun AnnotatedString.Builder.appendImageInline(
    inline: MarkdownInlineNodeDto,
    linkColor: Color,
    inlineCodeBackground: Color,
) {
    val label = inline.alt.ifBlank { inline.destination }
    if (inline.destination.isBlank()) {
        append(label)
        return
    }

    pushStringAnnotation(DestinationAnnotation, inline.destination)
    withStyle(
        SpanStyle(
            color = linkColor,
            textDecoration = TextDecoration.Underline,
        ),
    ) {
        append(label)
        if (label.isBlank()) {
            appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
        }
    }
    pop()
}

@Composable
private fun MarkdownCodeBlock(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(markdownStyle.cornerRadius),
        color = markdownStyle.codeBlockColor,
    ) {
        Column(
            modifier = Modifier.padding(markdownStyle.blockPadding),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (node.language.isNotBlank()) {
                Text(
                    text = node.language,
                    style = markdownStyle.labelTextStyle,
                    color = markdownStyle.onSurfaceVariantColor,
                )
            }
            Text(
                text = node.text,
                style = markdownStyle.codeTextStyle,
            )
        }
    }
}

@Composable
private fun MarkdownBlockQuote(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    renderCache: MarkdownRenderCache?,
    onOpenDestination: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Surface(
            modifier = Modifier.width(3.dp),
            color = markdownStyle.quoteRailColor,
            shape = RoundedCornerShape(2.dp),
        ) {}
        MarkdownInlineText(
            node = node,
            textStyle = markdownStyle.paragraphTextStyle,
            markdownStyle = markdownStyle,
            renderCache = renderCache,
            onOpenDestination = onOpenDestination,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun MarkdownList(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        node.text
            .lines()
            .filter { it.isNotBlank() }
            .forEach { line ->
                Text(
                    text = line,
                    style = markdownStyle.paragraphTextStyle,
                )
            }
    }
}

@Composable
private fun MarkdownTable(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    modifier: Modifier = Modifier,
) {
    val hasHeader = node.tableHeader.isNotEmpty()
    val rows = remember(node.tableHeader, node.tableRows) {
        ChatJankTracer.timeSessionSwitch(
            phase = "markdown_table_rows_build",
            warnAtMs = 2.0,
            extra = "message=${traceShortId(node.messageId)} block=${node.blockId} header=${node.tableHeader.size} rows=${node.tableRows.size}",
        ) {
            buildList<List<String>> {
                if (hasHeader) add(node.tableHeader)
                addAll(node.tableRows.map { it.cells })
            }.normalizedTableRows()
        }
    }
    val scrollState = rememberScrollState()

    if (rows.isEmpty()) return

    Box(
        modifier = modifier
            .fillMaxWidth()
            .horizontalScroll(scrollState),
    ) {
        MarkdownTableLayout(
            rows = rows,
            hasHeader = hasHeader,
            alignments = node.tableAlignments,
            markdownStyle = markdownStyle,
        )
    }
}

private fun List<List<String>>.normalizedTableRows(): List<List<String>> {
    val columnCount = maxOfOrNull { it.size } ?: return emptyList()
    if (columnCount == 0) return emptyList()
    return map { row ->
        if (row.size == columnCount) {
            row
        } else {
            row + List(columnCount - row.size) { "" }
        }
    }
}

@Composable
private fun MarkdownTableLayout(
    rows: List<List<String>>,
    hasHeader: Boolean,
    alignments: List<String>,
    markdownStyle: MarkdownStyle,
) {
    val rowCount = rows.size
    val columnCount = rows.firstOrNull()?.size ?: return
    val gridMetrics = remember(rowCount, columnCount) { MarkdownTableGridMetrics() }
    val tableShape = RoundedCornerShape(markdownStyle.cornerRadius)
    val cellPadding = Modifier.padding(
        horizontal = markdownStyle.tableCellHorizontalPadding,
        vertical = markdownStyle.tableCellVerticalPadding,
    )

    Layout(
        modifier = Modifier
            .clip(tableShape)
            .drawWithContent {
                drawRect(markdownStyle.tableBodyBackgroundColor)
                if (hasHeader && gridMetrics.rowHeights.isNotEmpty()) {
                    drawRect(
                        color = markdownStyle.tableHeaderBackgroundColor,
                        size = Size(
                            width = size.width,
                            height = gridMetrics.rowHeights[0].toFloat().coerceAtMost(size.height),
                        ),
                    )
                }
                drawContent()

                val strokeWidth = 1.dp.toPx()
                val halfStroke = strokeWidth / 2f
                val gridWidth = gridMetrics.columnWidths.sum().toFloat()
                    .coerceAtMost(size.width)
                val gridHeight = gridMetrics.rowHeights.sum().toFloat()
                    .coerceAtMost(size.height)
                if (gridWidth <= 0f || gridHeight <= 0f) {
                    return@drawWithContent
                }

                var x = halfStroke
                drawLine(
                    color = markdownStyle.outlineColor,
                    start = Offset(x, 0f),
                    end = Offset(x, gridHeight),
                    strokeWidth = strokeWidth,
                )
                gridMetrics.columnWidths.forEach { columnWidth ->
                    x += columnWidth
                    val lineX = x.coerceAtMost(gridWidth - halfStroke)
                    drawLine(
                        color = markdownStyle.outlineColor,
                        start = Offset(lineX, 0f),
                        end = Offset(lineX, gridHeight),
                        strokeWidth = strokeWidth,
                    )
                }

                var y = halfStroke
                drawLine(
                    color = markdownStyle.outlineColor,
                    start = Offset(0f, y),
                    end = Offset(gridWidth, y),
                    strokeWidth = strokeWidth,
                )
                gridMetrics.rowHeights.forEach { rowHeight ->
                    y += rowHeight
                    val lineY = y.coerceAtMost(gridHeight - halfStroke)
                    drawLine(
                        color = markdownStyle.outlineColor,
                        start = Offset(0f, lineY),
                        end = Offset(gridWidth, lineY),
                        strokeWidth = strokeWidth,
                    )
                }
            },
        content = {
            rows.forEachIndexed { rowIndex, cells ->
                val textStyle = if (hasHeader && rowIndex == 0) {
                    markdownStyle.tableHeaderTextStyle
                } else {
                    markdownStyle.tableBodyTextStyle
                }
                cells.forEachIndexed { columnIndex, cell ->
                    Text(
                        text = cell,
                        style = textStyle,
                        textAlign = alignments.textAlignAt(columnIndex),
                        modifier = cellPadding,
                    )
                }
            }
        },
    ) { measurables, constraints ->
        val measureStartNs = ChatJankTracer.nowNs()
        val minColumnWidth = markdownStyle.tableMinColumnWidth.roundToPx()
        val maxColumnWidth = markdownStyle.tableMaxColumnWidth.roundToPx()
            .coerceAtLeast(minColumnWidth)
        val horizontalPaddingPx = markdownStyle.tableCellHorizontalPadding.roundToPx() * 2
        val columnWidths = rows.estimatedTableColumnWidths(
            columnCount = columnCount,
            minColumnWidth = minColumnWidth,
            maxColumnWidth = maxColumnWidth,
            horizontalPaddingPx = horizontalPaddingPx,
        )

        val placeables = measurables.mapIndexed { index, measurable ->
            val columnIndex = index % columnCount
            measurable.measure(
                Constraints(
                    minWidth = columnWidths[columnIndex],
                    maxWidth = columnWidths[columnIndex],
                    minHeight = 0,
                    maxHeight = constraints.maxHeight,
                ),
            )
        }
        val rowHeights = IntArray(rowCount)
        placeables.forEachIndexed { index, placeable ->
            val rowIndex = index / columnCount
            rowHeights[rowIndex] = max(rowHeights[rowIndex], placeable.height)
        }

        val tableWidth = columnWidths.sum()
        val tableHeight = rowHeights.sum()
        val layoutWidth = if (constraints.hasBoundedWidth) {
            tableWidth.coerceIn(constraints.minWidth, constraints.maxWidth)
        } else {
            max(tableWidth, constraints.minWidth)
        }
        val layoutHeight = if (constraints.hasBoundedHeight) {
            tableHeight.coerceIn(constraints.minHeight, constraints.maxHeight)
        } else {
            max(tableHeight, constraints.minHeight)
        }
        gridMetrics.columnWidths = columnWidths
        gridMetrics.rowHeights = rowHeights
        ChatJankTracer.markDuration(
            phase = "markdown_table_measure",
            startNs = measureStartNs,
            warnAtMs = 3.0,
            extra = "rows=$rowCount columns=$columnCount cells=${measurables.size} width=$tableWidth height=$tableHeight",
        )

        layout(layoutWidth, layoutHeight) {
            var y = 0
            repeat(rowCount) { rowIndex ->
                var x = 0
                repeat(columnCount) { columnIndex ->
                    val placeable = placeables[rowIndex * columnCount + columnIndex]
                    placeable.placeRelative(x, y)
                    x += columnWidths[columnIndex]
                }
                y += rowHeights[rowIndex]
            }
        }
    }
}

private fun List<List<String>>.estimatedTableColumnWidths(
    columnCount: Int,
    minColumnWidth: Int,
    maxColumnWidth: Int,
    horizontalPaddingPx: Int,
): IntArray {
    val narrowCharWidth = ((maxColumnWidth - horizontalPaddingPx) / 28)
        .coerceAtLeast(6)
    val wideCharWidth = (narrowCharWidth * 1.65f).toInt()
        .coerceAtLeast(narrowCharWidth + 2)
    val whitespaceCharWidth = (narrowCharWidth / 2).coerceAtLeast(3)
    val widths = IntArray(columnCount) { minColumnWidth }

    forEach { row ->
        row.forEachIndexed { columnIndex, cell ->
            if (columnIndex >= columnCount) return@forEachIndexed
            widths[columnIndex] = max(
                widths[columnIndex],
                cell.estimatedTableCellWidth(
                    minColumnWidth = minColumnWidth,
                    maxColumnWidth = maxColumnWidth,
                    horizontalPaddingPx = horizontalPaddingPx,
                    narrowCharWidth = narrowCharWidth,
                    wideCharWidth = wideCharWidth,
                    whitespaceCharWidth = whitespaceCharWidth,
                ),
            )
        }
    }

    return widths
}

private fun String.estimatedTableCellWidth(
    minColumnWidth: Int,
    maxColumnWidth: Int,
    horizontalPaddingPx: Int,
    narrowCharWidth: Int,
    wideCharWidth: Int,
    whitespaceCharWidth: Int,
): Int {
    if (isBlank()) return minColumnWidth
    var lineWidth = horizontalPaddingPx
    var widestLineWidth = minColumnWidth
    for (char in this) {
        if (char == '\n') {
            widestLineWidth = max(widestLineWidth, lineWidth)
            if (widestLineWidth >= maxColumnWidth) return maxColumnWidth
            lineWidth = horizontalPaddingPx
            continue
        }
        lineWidth += when {
            char.isWhitespace() -> whitespaceCharWidth
            char.isWideTableChar() -> wideCharWidth
            else -> narrowCharWidth
        }
        if (lineWidth >= maxColumnWidth) return maxColumnWidth
    }
    return max(widestLineWidth, lineWidth).coerceIn(minColumnWidth, maxColumnWidth)
}

private fun Char.isWideTableChar(): Boolean {
    val code = code
    return code in 0x1100..0x115F ||
        code in 0x2E80..0xA4CF ||
        code in 0xAC00..0xD7A3 ||
        code in 0xF900..0xFAFF ||
        code in 0xFE10..0xFE19 ||
        code in 0xFE30..0xFE6F ||
        code in 0xFF00..0xFF60 ||
        code in 0xFFE0..0xFFE6
}

private class MarkdownTableGridMetrics {
    var columnWidths: IntArray = IntArray(0)
    var rowHeights: IntArray = IntArray(0)
}

private fun List<String>.textAlignAt(index: Int): TextAlign {
    return when (getOrNull(index)) {
        "center" -> TextAlign.Center
        "right" -> TextAlign.End
        else -> TextAlign.Start
    }
}

private fun traceShortId(id: String): String {
    if (id.isBlank()) return "-"
    return if (id.length <= 10) id else id.take(4) + ".." + id.takeLast(6)
}

@Composable
private fun MarkdownFileBlock(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    onOpenDestination: (String) -> Unit,
    onResolveHostPath: ((String) -> String)? = null,
    modifier: Modifier = Modifier,
) {
    val clickableModifier = if (node.path.isNotBlank()) {
        Modifier.clickable { onOpenDestination(node.path) }
    } else {
        Modifier
    }

    val cleanPath = when {
        node.path.startsWith("hambur://") -> node.path.removePrefix("hambur://")
        node.path.startsWith("hambur:") -> node.path.removePrefix("hambur:")
        else -> node.path
    }
    val hostPath = remember(node.path, onResolveHostPath) {
        if (cleanPath.startsWith("/") && File(cleanPath).exists()) {
            cleanPath
        } else {
            onResolveHostPath?.invoke(node.path).orEmpty()
        }
    }
    val imageFile = remember(hostPath) {
        if (hostPath.isNotBlank()) File(hostPath) else null
    }
    val isImage = remember(node.fileKind, node.path) {
        node.fileKind.equals("image", ignoreCase = true) ||
            listOf(".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg").any {
                node.path.substringBefore('?').endsWith(it, ignoreCase = true)
            }
    }
    val imageModel: Any? = remember(node.path, hostPath, imageFile) {
        when {
            node.path.startsWith("http://") || node.path.startsWith("https://") -> node.path
            node.path.startsWith("content://") -> Uri.parse(node.path)
            imageFile?.exists() == true -> imageFile
            else -> null
        }
    }

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .then(clickableModifier),
        shape = RoundedCornerShape(markdownStyle.cornerRadius),
        color = markdownStyle.blockSurfaceColor,
        border = BorderStroke(1.dp, markdownStyle.outlineColor),
    ) {
        Column(
            modifier = Modifier.padding(markdownStyle.blockPadding),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (isImage && imageModel != null) {
                AsyncImage(
                    model = imageModel,
                    contentDescription = node.text.ifBlank { node.path },
                    contentScale = ContentScale.Fit,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(markdownStyle.cornerRadius)),
                )
            }
            Column(
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    text = node.text.ifBlank { node.fileKind.ifBlank { "file" } },
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = node.path,
                    style = markdownStyle.smallTextStyle,
                    color = markdownStyle.onSurfaceVariantColor,
                )
            }
        }
    }
}

@Composable
private fun MarkdownPlainBlock(
    node: MarkdownBlockNodeDto,
    markdownStyle: MarkdownStyle,
    modifier: Modifier = Modifier,
) {
    Text(
        text = node.text.ifBlank { node.raw.trim() },
        style = markdownStyle.paragraphTextStyle,
        modifier = modifier.fillMaxWidth(),
    )
}
