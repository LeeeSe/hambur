package com.hambur.chat.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.hambur.chat.uniffi.MarkdownBlockNodeDto
import com.hambur.chat.uniffi.MarkdownInlineNodeDto

@Composable
fun MarkdownBlock(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    when (node.nodeKind) {
        "Heading" -> MarkdownInlineText(
            inlines = node.inlines,
            style = when (node.level.toInt()) {
                1 -> MaterialTheme.typography.headlineSmall
                2 -> MaterialTheme.typography.titleLarge
                else -> MaterialTheme.typography.titleMedium
            }.copy(fontWeight = FontWeight.SemiBold),
            modifier = modifier.fillMaxWidth(),
        )
        "Paragraph" -> MarkdownInlineText(
            inlines = node.inlines,
            style = MaterialTheme.typography.bodyMedium,
            modifier = modifier.fillMaxWidth(),
        )
        "CodeBlock" -> MarkdownCodeBlock(node = node, modifier = modifier)
        "BlockQuote" -> MarkdownBlockQuote(node = node, modifier = modifier)
        "List" -> MarkdownList(node = node, modifier = modifier)
        "Table" -> MarkdownTable(node = node, modifier = modifier)
        "ThematicBreak" -> HorizontalDivider(modifier = modifier.fillMaxWidth())
        "HamburFileBlock" -> MarkdownFileBlock(node = node, modifier = modifier)
        "HtmlBlock", "MathBlock" -> MarkdownPlainBlock(node = node, modifier = modifier)
        else -> MarkdownPlainBlock(node = node, modifier = modifier)
    }
}

@Composable
private fun MarkdownInlineText(
    inlines: List<MarkdownInlineNodeDto>,
    style: TextStyle,
    modifier: Modifier = Modifier,
) {
    val colorScheme = MaterialTheme.colorScheme
    val annotated = remember(inlines, colorScheme.primary, colorScheme.surfaceVariant) {
        buildAnnotatedString {
            appendInlineNodes(
                inlines = inlines,
                linkColor = colorScheme.primary,
                inlineCodeBackground = colorScheme.surfaceVariant,
            )
        }
    }

    Text(
        text = annotated,
        style = style,
        modifier = modifier,
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
            "Link" -> withStyle(
                SpanStyle(
                    color = linkColor,
                    textDecoration = TextDecoration.Underline,
                ),
            ) {
                appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
            }
            "Image" -> append(inline.alt.ifBlank { inline.destination })
            else -> appendInlineNodes(inline.children, linkColor, inlineCodeBackground)
        }
    }
}

@Composable
private fun MarkdownCodeBlock(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (node.language.isNotBlank()) {
                Text(
                    text = node.language,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                text = node.text,
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            )
        }
    }
}

@Composable
private fun MarkdownBlockQuote(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Surface(
            modifier = Modifier.width(3.dp),
            color = MaterialTheme.colorScheme.primary,
            shape = RoundedCornerShape(2.dp),
        ) {}
        MarkdownInlineText(
            inlines = node.inlines,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun MarkdownList(
    node: MarkdownBlockNodeDto,
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
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
    }
}

@Composable
private fun MarkdownTable(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    val rows = remember(node.tableHeader, node.tableRows) {
        buildList {
            if (node.tableHeader.isNotEmpty()) add(node.tableHeader)
            addAll(node.tableRows.map { it.cells })
        }
    }
    val scrollState = rememberScrollState()

    Column(
        modifier = modifier
            .fillMaxWidth()
            .horizontalScroll(scrollState),
    ) {
        rows.forEachIndexed { index, cells ->
            Row {
                cells.forEach { cell ->
                    Text(
                        text = cell,
                        style = if (index == 0) {
                            MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold)
                        } else {
                            MaterialTheme.typography.bodySmall
                        },
                        modifier = Modifier
                            .width(132.dp)
                            .border(
                                width = 1.dp,
                                color = MaterialTheme.colorScheme.outlineVariant,
                            )
                            .padding(horizontal = 8.dp, vertical = 6.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun MarkdownFileBlock(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = node.text.ifBlank { node.fileKind.ifBlank { "file" } },
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = node.path,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MarkdownPlainBlock(
    node: MarkdownBlockNodeDto,
    modifier: Modifier = Modifier,
) {
    Text(
        text = node.text.ifBlank { node.raw.trim() },
        style = MaterialTheme.typography.bodyMedium,
        modifier = modifier.fillMaxWidth(),
    )
}
