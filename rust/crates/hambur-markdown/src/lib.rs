use std::collections::HashMap;

use mdstream::{Block, BlockId, MdStream, Update};
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options as PulldownOptions, Parser, Tag, TagEnd,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownInlineNode {
    pub kind: String,
    pub text: String,
    pub destination: String,
    pub title: String,
    pub alt: String,
    pub children: Vec<MarkdownInlineNode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownTableRow {
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownBlockNode {
    pub message_id: String,
    pub block_id: u64,
    pub stable_key: String,
    pub source_kind: String,
    pub node_kind: String,
    pub committed: bool,
    pub level: u8,
    pub inlines: Vec<MarkdownInlineNode>,
    pub language: String,
    pub text: String,
    pub raw: String,
    pub children_json: String,
    pub items_json: String,
    pub table_header: Vec<String>,
    pub table_rows: Vec<MarkdownTableRow>,
    pub path: String,
    pub file_kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownRenderUpdate {
    pub message_id: String,
    pub reset: bool,
    pub committed_nodes: Vec<MarkdownBlockNode>,
    pub pending_node: Option<MarkdownBlockNode>,
    pub invalidated_block_ids: Vec<u64>,
}

pub struct MarkdownPipeline {
    message_id: String,
    stream: MdStream,
    committed_blocks: HashMap<BlockId, Block>,
    committed_nodes: HashMap<BlockId, MarkdownBlockNode>,
}

impl MarkdownPipeline {
    pub fn new(message_id: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            stream: MdStream::streamdown_defaults(),
            committed_blocks: HashMap::new(),
            committed_nodes: HashMap::new(),
        }
    }

    pub fn append(&mut self, chunk: &str) -> MarkdownRenderUpdate {
        let update = self.stream.append(chunk);
        self.render_update(update)
    }

    pub fn finalize(&mut self) -> MarkdownRenderUpdate {
        let update = self.stream.finalize();
        self.render_update(update)
    }

    fn render_update(&mut self, update: Update) -> MarkdownRenderUpdate {
        if update.reset {
            self.committed_blocks.clear();
            self.committed_nodes.clear();
        }

        let mut committed_nodes = Vec::new();
        for block in &update.committed {
            self.committed_blocks.insert(block.id, block.clone());
            let node = build_block_node(&self.message_id, block, true);
            self.committed_nodes.insert(block.id, node.clone());
            committed_nodes.push(node);
        }

        for block_id in &update.invalidated {
            let Some(block) = self.committed_blocks.get(block_id) else {
                continue;
            };
            let node = build_block_node(&self.message_id, block, true);
            self.committed_nodes.insert(*block_id, node.clone());
            committed_nodes.push(node);
        }

        let pending_node = update
            .pending
            .as_ref()
            .map(|block| build_block_node(&self.message_id, block, false));

        MarkdownRenderUpdate {
            message_id: self.message_id.clone(),
            reset: update.reset,
            committed_nodes,
            pending_node,
            invalidated_block_ids: update.invalidated.iter().map(|id| id.0).collect(),
        }
    }
}

pub fn render_markdown_to_nodes(
    message_id: impl Into<String>,
    markdown: &str,
) -> Vec<MarkdownBlockNode> {
    let mut pipeline = MarkdownPipeline::new(message_id);
    let mut nodes = pipeline.append(markdown).committed_nodes;
    let final_update = pipeline.finalize();
    nodes.extend(final_update.committed_nodes);
    nodes
}

fn build_block_node(message_id: &str, block: &Block, committed: bool) -> MarkdownBlockNode {
    let input = if committed {
        block.raw.as_str()
    } else {
        block.display_or_raw()
    };
    let raw = if committed {
        block.raw.clone()
    } else {
        input.to_string()
    };
    let source_kind = format!("{:?}", block.kind);
    let mut node = MarkdownBlockNode {
        message_id: message_id.to_string(),
        block_id: block.id.0,
        stable_key: stable_key(message_id, block.id),
        source_kind,
        committed,
        raw,
        ..Default::default()
    };

    let trimmed = input.trim();
    if trimmed.starts_with("$$") {
        node.node_kind = "MathBlock".to_string();
        node.text = trimmed.to_string();
        return node;
    }

    let options = pulldown_options();
    let events = Parser::new_ext(input, options).collect::<Vec<_>>();

    if let Some(file_node) = hambur_file_block(message_id, block, committed, &events, input) {
        return file_node;
    }

    if events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::Table(_))))
    {
        fill_table_node(&mut node, &events);
        return node;
    }

    if let Some((level, inlines)) = heading_from_events(&events) {
        node.node_kind = "Heading".to_string();
        node.level = level;
        node.text = plain_text(&inlines);
        node.inlines = inlines;
        return node;
    }

    if let Some((language, text)) = code_block_from_events(&events) {
        node.node_kind = "CodeBlock".to_string();
        node.language = language;
        node.text = text;
        return node;
    }

    if events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::BlockQuote(_))))
    {
        let inlines = collect_inlines(&events);
        node.node_kind = "BlockQuote".to_string();
        node.text = plain_text(&inlines);
        node.inlines = inlines.clone();
        let children = vec![paragraph_child(message_id, &inlines)];
        node.children_json = serde_json::to_string(&children).unwrap_or_else(|_| "[]".to_string());
        return node;
    }

    if events
        .iter()
        .any(|event| matches!(event, Event::Start(Tag::List(_))))
    {
        fill_list_node(&mut node, &events);
        return node;
    }

    if events.iter().any(|event| matches!(event, Event::Rule)) {
        node.node_kind = "ThematicBreak".to_string();
        return node;
    }

    if events.iter().any(|event| {
        matches!(
            event,
            Event::Html(_) | Event::InlineHtml(_) | Event::Start(Tag::HtmlBlock)
        )
    }) {
        node.node_kind = "HtmlBlock".to_string();
        node.text = input.to_string();
        return node;
    }

    let inlines = collect_inlines(&events);
    node.node_kind = "Paragraph".to_string();
    node.text = plain_text(&inlines);
    node.inlines = inlines;
    node
}

fn pulldown_options() -> PulldownOptions {
    PulldownOptions::ENABLE_TABLES
        | PulldownOptions::ENABLE_STRIKETHROUGH
        | PulldownOptions::ENABLE_TASKLISTS
}

fn stable_key(message_id: &str, block_id: BlockId) -> String {
    format!("{message_id}:{}", block_id.0)
}

fn heading_from_events(events: &[Event<'_>]) -> Option<(u8, Vec<MarkdownInlineNode>)> {
    for event in events {
        if let Event::Start(Tag::Heading { level, .. }) = event {
            return Some((heading_level(*level), collect_inlines(events)));
        }
    }
    None
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn code_block_from_events(events: &[Event<'_>]) -> Option<(String, String)> {
    let mut language = String::new();
    let mut text = String::new();
    let mut in_code = false;

    for event in events {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = true;
                language = match kind {
                    CodeBlockKind::Fenced(value) => value.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Event::Text(value) if in_code => text.push_str(value),
            Event::End(TagEnd::CodeBlock) if in_code => return Some((language, text)),
            _ => {}
        }
    }

    None
}

fn hambur_file_block(
    message_id: &str,
    block: &Block,
    committed: bool,
    events: &[Event<'_>],
    input: &str,
) -> Option<MarkdownBlockNode> {
    let inlines = collect_inlines(events);
    if inlines.len() != 1 {
        return None;
    }

    let inline = &inlines[0];
    let destination = inline.destination.trim();
    if !is_hambur_file_destination(destination) {
        return None;
    }

    let file_kind = if inline.kind == "Image"
        || destination.starts_with("hambur://image")
        || destination.starts_with("hambur://media/image")
    {
        "image"
    } else if destination.starts_with("hambur://video")
        || destination.starts_with("hambur://media/video")
    {
        "video"
    } else {
        "file"
    };

    Some(MarkdownBlockNode {
        message_id: message_id.to_string(),
        block_id: block.id.0,
        stable_key: stable_key(message_id, block.id),
        source_kind: format!("{:?}", block.kind),
        node_kind: "HamburFileBlock".to_string(),
        committed,
        text: if inline.alt.is_empty() {
            plain_text(&inline.children)
        } else {
            inline.alt.clone()
        },
        raw: input.to_string(),
        path: destination.to_string(),
        file_kind: file_kind.to_string(),
        inlines,
        ..Default::default()
    })
}

fn is_hambur_file_destination(destination: &str) -> bool {
    destination.starts_with("hambur://") || destination.starts_with("file://")
}

fn fill_list_node(node: &mut MarkdownBlockNode, events: &[Event<'_>]) {
    let mut item_events = Vec::new();
    let mut item_depth = 0usize;
    let mut item_nodes = Vec::<MarkdownBlockNode>::new();
    let mut text_lines = Vec::<String>::new();

    for event in events {
        match event {
            Event::Start(Tag::Item) => {
                item_depth += 1;
                if item_depth > 1 {
                    item_events.push(event.clone());
                }
            }
            Event::End(TagEnd::Item) => {
                if item_depth > 1 {
                    item_events.push(event.clone());
                } else {
                    let inlines = collect_inlines(&item_events);
                    let text = plain_text(&inlines);
                    text_lines.push(format!("- {text}"));
                    item_nodes.push(paragraph_child(&node.message_id, &inlines));
                    item_events.clear();
                }
                item_depth = item_depth.saturating_sub(1);
            }
            _ if item_depth > 0 => item_events.push(event.clone()),
            _ => {}
        }
    }

    node.node_kind = "List".to_string();
    node.text = text_lines.join("\n");
    node.items_json = serde_json::to_string(&item_nodes).unwrap_or_else(|_| "[]".to_string());
}

fn fill_table_node(node: &mut MarkdownBlockNode, events: &[Event<'_>]) {
    let mut in_head = false;
    let mut in_cell = false;
    let mut current_cell_events = Vec::new();
    let mut current_row = Vec::<String>::new();
    let mut header = Vec::<String>::new();
    let mut rows = Vec::<MarkdownTableRow>::new();

    for event in events {
        match event {
            Event::Start(Tag::TableHead) => {
                in_head = true;
            }
            Event::End(TagEnd::TableHead) => {
                if !current_row.is_empty() {
                    header = current_row.clone();
                    current_row.clear();
                }
                in_head = false;
            }
            Event::Start(Tag::TableRow) => {
                current_row.clear();
            }
            Event::End(TagEnd::TableRow) => {
                if in_head {
                    header = current_row.clone();
                } else if !current_row.is_empty() {
                    rows.push(MarkdownTableRow {
                        cells: current_row.clone(),
                    });
                }
                current_row.clear();
            }
            Event::Start(Tag::TableCell) => {
                in_cell = true;
                current_cell_events.clear();
            }
            Event::End(TagEnd::TableCell) => {
                let inlines = collect_inlines(&current_cell_events);
                current_row.push(plain_text(&inlines));
                current_cell_events.clear();
                in_cell = false;
            }
            _ if in_cell => current_cell_events.push(event.clone()),
            _ => {}
        }
    }

    node.node_kind = "Table".to_string();
    node.table_header = header;
    node.table_rows = rows;
}

fn paragraph_child(message_id: &str, inlines: &[MarkdownInlineNode]) -> MarkdownBlockNode {
    MarkdownBlockNode {
        message_id: message_id.to_string(),
        node_kind: "Paragraph".to_string(),
        inlines: inlines.to_vec(),
        text: plain_text(inlines),
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
struct InlineFrame {
    kind: String,
    destination: String,
    title: String,
    children: Vec<MarkdownInlineNode>,
}

fn collect_inlines(events: &[Event<'_>]) -> Vec<MarkdownInlineNode> {
    let mut out = Vec::new();
    let mut stack = Vec::<InlineFrame>::new();

    for event in events {
        match event {
            Event::Text(value) => push_inline(&mut stack, &mut out, text_node(value)),
            Event::Code(value) => push_inline(
                &mut stack,
                &mut out,
                MarkdownInlineNode {
                    kind: "InlineCode".to_string(),
                    text: value.to_string(),
                    ..Default::default()
                },
            ),
            Event::SoftBreak => push_inline(
                &mut stack,
                &mut out,
                MarkdownInlineNode {
                    kind: "SoftBreak".to_string(),
                    ..Default::default()
                },
            ),
            Event::HardBreak => push_inline(
                &mut stack,
                &mut out,
                MarkdownInlineNode {
                    kind: "HardBreak".to_string(),
                    ..Default::default()
                },
            ),
            Event::Html(value) | Event::InlineHtml(value) => {
                push_inline(&mut stack, &mut out, text_node(value))
            }
            Event::FootnoteReference(value) => push_inline(
                &mut stack,
                &mut out,
                text_node(format!("[{value}]").as_str()),
            ),
            Event::TaskListMarker(checked) => push_inline(
                &mut stack,
                &mut out,
                text_node(if *checked { "[x] " } else { "[ ] " }),
            ),
            Event::Start(tag) => start_inline(tag, &mut stack),
            Event::End(tag) => end_inline(tag, &mut stack, &mut out),
            Event::InlineMath(value) | Event::DisplayMath(value) => push_inline(
                &mut stack,
                &mut out,
                MarkdownInlineNode {
                    kind: "InlineCode".to_string(),
                    text: value.to_string(),
                    ..Default::default()
                },
            ),
            _ => {}
        }
    }

    while let Some(frame) = stack.pop() {
        let node = frame.into_node();
        push_inline(&mut stack, &mut out, node);
    }

    out
}

fn text_node(text: &str) -> MarkdownInlineNode {
    MarkdownInlineNode {
        kind: "Text".to_string(),
        text: text.to_string(),
        ..Default::default()
    }
}

fn push_inline(
    stack: &mut [InlineFrame],
    out: &mut Vec<MarkdownInlineNode>,
    node: MarkdownInlineNode,
) {
    if let Some(frame) = stack.last_mut() {
        frame.children.push(node);
    } else {
        out.push(node);
    }
}

fn start_inline(tag: &Tag<'_>, stack: &mut Vec<InlineFrame>) {
    match tag {
        Tag::Emphasis => stack.push(InlineFrame::new("Emphasis")),
        Tag::Strong => stack.push(InlineFrame::new("Strong")),
        Tag::Strikethrough => stack.push(InlineFrame::new("Strikethrough")),
        Tag::Link {
            dest_url, title, ..
        } => stack.push(InlineFrame::link(dest_url, title)),
        Tag::Image {
            dest_url, title, ..
        } => stack.push(InlineFrame::image(dest_url, title)),
        _ => {}
    }
}

fn end_inline(tag: &TagEnd, stack: &mut Vec<InlineFrame>, out: &mut Vec<MarkdownInlineNode>) {
    let expected = match tag {
        TagEnd::Emphasis => Some("Emphasis"),
        TagEnd::Strong => Some("Strong"),
        TagEnd::Strikethrough => Some("Strikethrough"),
        TagEnd::Link => Some("Link"),
        TagEnd::Image => Some("Image"),
        _ => None,
    };

    let Some(expected) = expected else {
        return;
    };

    let Some(index) = stack.iter().rposition(|frame| frame.kind == expected) else {
        return;
    };
    while stack.len() > index + 1 {
        let node = stack.pop().expect("inline frame").into_node();
        push_inline(stack, out, node);
    }
    if let Some(frame) = stack.pop() {
        let node = frame.into_node();
        push_inline(stack, out, node);
    }
}

impl InlineFrame {
    fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            destination: String::new(),
            title: String::new(),
            children: Vec::new(),
        }
    }

    fn link(destination: &str, title: &str) -> Self {
        Self {
            kind: "Link".to_string(),
            destination: destination.to_string(),
            title: title.to_string(),
            children: Vec::new(),
        }
    }

    fn image(destination: &str, title: &str) -> Self {
        Self {
            kind: "Image".to_string(),
            destination: destination.to_string(),
            title: title.to_string(),
            children: Vec::new(),
        }
    }

    fn into_node(self) -> MarkdownInlineNode {
        let alt = if self.kind == "Image" {
            plain_text(&self.children)
        } else {
            String::new()
        };

        MarkdownInlineNode {
            kind: self.kind,
            text: String::new(),
            destination: self.destination,
            title: self.title,
            alt,
            children: self.children,
        }
    }
}

fn plain_text(inlines: &[MarkdownInlineNode]) -> String {
    let mut text = String::new();
    append_plain_text(inlines, &mut text);
    text.trim().to_string()
}

fn append_plain_text(inlines: &[MarkdownInlineNode], out: &mut String) {
    for inline in inlines {
        match inline.kind.as_str() {
            "Text" | "InlineCode" => out.push_str(&inline.text),
            "SoftBreak" | "HardBreak" => out.push('\n'),
            "Image" if !inline.alt.is_empty() => out.push_str(&inline.alt),
            _ => append_plain_text(&inline.children, out),
        }
    }
}
