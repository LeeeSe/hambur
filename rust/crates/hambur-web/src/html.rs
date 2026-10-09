//! A dependency-free HTML to Markdown converter for fetched pages.
//!
//! DeepSeek Harness renders fetched HTML with turndown plus its GFM plugin. The Android
//! build carries no DOM engine, so this module implements the same idea directly: drop
//! non-content elements, walk tags, and emit the block and inline Markdown a model reads
//! well. It deliberately favors robustness over spec completeness — unknown tags simply
//! fall through as containers instead of failing the fetch.

/// Convert an HTML document or fragment to Markdown-ish plain text.
pub fn html_to_markdown(html: &str) -> String {
    let cleaned = strip_comments(html);
    let cleaned = strip_elements(&cleaned, "script");
    let cleaned = strip_elements(&cleaned, "style");
    let cleaned = strip_elements(&cleaned, "noscript");
    let cleaned = strip_elements(&cleaned, "template");
    let cleaned = strip_elements(&cleaned, "svg");
    let cleaned = strip_elements(&cleaned, "canvas");
    let cleaned = strip_elements(&cleaned, "head");

    let mut converter = Converter::default();
    converter.run(&cleaned);
    converter.finish()
}

#[derive(Default)]
struct Converter {
    out: String,
    /// Depth of open `<pre>` elements: inside them text is kept verbatim.
    pre_depth: usize,
    /// Open lists: `(ordered, next index)`.
    lists: Vec<(bool, usize)>,
    /// Open `<blockquote>` output offsets, so their finished range can be quoted.
    quotes: Vec<usize>,
    /// Cells already emitted in the current table row, so a cell after an empty one
    /// still gets its separator.
    row_cells: usize,
    /// Open `<a>` elements: `(output offset of the link text, href)`.
    links: Vec<(usize, Option<String>)>,
}

impl Converter {
    fn run(&mut self, html: &str) {
        let mut index = 0;
        let length = html.len();
        while index < length {
            if html.as_bytes()[index] == b'<' {
                match tag_end(html, index) {
                    Some(end) => {
                        self.tag(&html[index + 1..end]);
                        index = end + 1;
                    }
                    None => {
                        self.text(&html[index..]);
                        break;
                    }
                }
                continue;
            }
            let next = html[index..]
                .find('<')
                .map(|offset| index + offset)
                .unwrap_or(length);
            self.text(&html[index..next]);
            index = next;
        }
    }

    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let decoded = decode_entities(raw);
        if self.pre_depth > 0 {
            self.out.push_str(&decoded);
            return;
        }
        let collapsed = collapse_spaces(&decoded);
        if collapsed.is_empty() {
            return;
        }
        // Keep a single separating space between inline runs without stacking them.
        if collapsed.starts_with(' ') && (self.out.is_empty() || self.out.ends_with([' ', '\n'])) {
            self.out.push_str(collapsed.trim_start());
        } else {
            self.out.push_str(&collapsed);
        }
    }

    fn tag(&mut self, raw: &str) {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('!') || raw.starts_with('?') {
            return;
        }
        let closing = raw.starts_with('/');
        let body = raw.trim_start_matches('/').trim_end_matches('/').trim();
        let name_end = body
            .find(|character: char| character.is_whitespace())
            .unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attributes = &body[name_end..];

        match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if closing {
                    self.blank_line();
                } else {
                    self.blank_line();
                    let level = name[1..].parse::<usize>().unwrap_or(1).clamp(1, 6);
                    self.out.push_str(&"#".repeat(level));
                    self.out.push(' ');
                }
            }
            "p" | "div" | "section" | "article" | "main" | "header" | "footer" | "aside"
            | "figure" | "figcaption" | "nav" | "form" | "fieldset" | "dl" | "dt" | "dd"
            | "details" | "summary" | "table" | "tbody" | "thead" | "tfoot" | "address" => {
                self.blank_line();
            }
            "br" => {
                self.out.push('\n');
            }
            "hr" => {
                self.blank_line();
                self.out.push_str("---");
                self.blank_line();
            }
            "pre" => {
                if closing {
                    self.pre_depth = self.pre_depth.saturating_sub(1);
                    while self.out.ends_with('\n') {
                        self.out.pop();
                    }
                    self.out.push('\n');
                    self.out.push_str("```");
                    self.blank_line();
                } else {
                    self.blank_line();
                    self.out.push_str("```\n");
                    self.pre_depth += 1;
                }
            }
            "code" => {
                if self.pre_depth == 0 {
                    self.out.push('`');
                }
            }
            "strong" | "b" => self.out.push_str("**"),
            "em" | "i" => self.out.push('*'),
            "del" | "s" | "strike" => self.out.push_str("~~"),
            "blockquote" => {
                if closing {
                    if let Some(start) = self.quotes.pop() {
                        let quoted = quote_lines(&self.out[start..]);
                        self.out.truncate(start);
                        self.out.push_str(&quoted);
                    }
                    self.blank_line();
                } else {
                    self.blank_line();
                    self.quotes.push(self.out.len());
                }
            }
            "ul" | "ol" => {
                if closing {
                    self.lists.pop();
                    self.blank_line();
                } else {
                    self.blank_line();
                    self.lists.push((name == "ol", 1));
                }
            }
            "li" => {
                // The opening tag owns the line break, so closing adds nothing and items
                // stay one line apart.
                if !closing {
                    self.out.push('\n');
                    let marker = match self.lists.last_mut() {
                        Some((true, next)) => {
                            let marker = format!("{next}. ");
                            *next += 1;
                            marker
                        }
                        _ => "- ".to_string(),
                    };
                    self.out.push_str(&marker);
                }
            }
            "tr" => {
                if !closing {
                    self.row_cells = 0;
                }
                self.out.push('\n');
            }
            "td" | "th" => {
                if !closing {
                    // Every cell after the first gets a separator, even when the
                    // preceding cell rendered nothing.
                    if self.row_cells > 0 {
                        self.out.push_str(" | ");
                    }
                    self.row_cells += 1;
                }
            }
            "a" => {
                if closing {
                    if let Some((start, href)) = self.links.pop()
                        && let Some(href) = href.filter(|href| !href.is_empty())
                        && self.out.len() > start
                    {
                        self.out.insert(start, '[');
                        self.out.push_str(&format!("]({href})"));
                    }
                } else {
                    let href = attribute(attributes, "href").filter(|href| is_linkable(href));
                    self.links.push((self.out.len(), href));
                }
            }
            "img" => {
                if !closing {
                    let source = attribute(attributes, "src")
                        .or_else(|| attribute(attributes, "data-src"))
                        .filter(|source| is_linkable(source));
                    if let Some(source) = source {
                        let alt = attribute(attributes, "alt").unwrap_or_default();
                        self.out.push_str(&format!("![{}]({source})", alt.trim()));
                    }
                }
            }
            _ => {}
        }
    }

    /// Force a paragraph boundary, collapsing repeated blank lines.
    fn blank_line(&mut self) {
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        if self.out.is_empty() {
            return;
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn finish(mut self) -> String {
        // Close anything the document left open so the tail is not lost.
        while let Some((start, href)) = self.links.pop() {
            if let Some(href) = href.filter(|href| !href.is_empty())
                && self.out.len() > start
            {
                self.out.insert(start, '[');
                self.out.push_str(&format!("]({href})"));
            }
        }
        while let Some(start) = self.quotes.pop() {
            let quoted = quote_lines(&self.out[start..]);
            self.out.truncate(start);
            self.out.push_str(&quoted);
        }

        let mut lines: Vec<String> = Vec::new();
        let mut blank_run = 0;
        for line in self.out.lines() {
            let trimmed = line.trim_end();
            if trimmed.trim().is_empty() || trimmed.trim() == "|" {
                blank_run += 1;
                if blank_run > 1 {
                    continue;
                }
                lines.push(String::new());
                continue;
            }
            blank_run = 0;
            lines.push(trimmed.to_string());
        }
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        while lines.first().is_some_and(|line| line.is_empty()) {
            lines.remove(0);
        }
        lines.join("\n")
    }
}

/// Prefix every non-empty line of a finished range with `> `.
fn quote_lines(text: &str) -> String {
    let mut quoted = String::new();
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            quoted.push('\n');
        }
        if line.trim().is_empty() {
            quoted.push('>');
        } else {
            quoted.push_str("> ");
            quoted.push_str(line.trim_start());
        }
    }
    quoted.push('\n');
    quoted
}

/// Collapse whitespace runs to single spaces, keeping at most one leading and one trailing
/// space: they separate inline runs (`<strong>a</strong> &amp; b`) and must survive.
fn collapse_spaces(text: &str) -> String {
    let leading = text.starts_with(char::is_whitespace);
    let trailing = text.ends_with(char::is_whitespace);
    let mut core = String::new();
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !core.is_empty() {
            core.push(' ');
        }
        pending_space = false;
        core.push(character);
    }
    if core.is_empty() {
        return String::new();
    }
    let mut collapsed = String::with_capacity(core.len() + 2);
    if leading {
        collapsed.push(' ');
    }
    collapsed.push_str(&core);
    if trailing {
        collapsed.push(' ');
    }
    collapsed
}

fn is_linkable(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.starts_with('#')
        && !value.to_ascii_lowercase().starts_with("javascript:")
        && !value.to_ascii_lowercase().starts_with("data:")
}

/// Find the end of the tag starting at `start` (`<`), honoring quoted attributes.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, character) in html[start..].char_indices() {
        if start + offset == start {
            continue;
        }
        match quote {
            Some(active) => {
                if character == active {
                    quote = None;
                }
            }
            None => match character {
                '"' | '\'' => quote = Some(character),
                '>' => return Some(start + offset),
                _ => {}
            },
        }
    }
    None
}

/// Read one attribute value out of a tag's attribute text.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let lower = attributes.to_ascii_lowercase();
    let mut search = 0;
    while let Some(relative) = lower[search..].find(name) {
        let position = search + relative;
        let before_ok = position == 0
            || !lower[..position]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '-');
        let after = &attributes[position + name.len()..];
        if before_ok {
            let rest = after.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                let mut characters = rest.chars();
                let value = match characters.next() {
                    Some(quote @ ('"' | '\'')) => rest[1..]
                        .split(quote)
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    _ => rest
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                };
                return Some(decode_entities(&value).trim().to_string());
            }
        }
        search = position + name.len();
    }
    None
}

/// Remove HTML comments.
fn strip_comments(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Remove every `<tag ...>…</tag>` element, including its content.
fn strip_elements(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::new();
    let mut rest = html;
    loop {
        let Some(start) = find_ignore_case(rest, &open) else {
            out.push_str(rest);
            return out;
        };
        let after_name = rest[start + open.len()..].chars().next();
        if after_name.is_some_and(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == ':'
        }) {
            out.push_str(&rest[..start + open.len()]);
            rest = &rest[start + open.len()..];
            continue;
        }
        out.push_str(&rest[..start]);
        // Honor quoted attributes when finding the end of the opening tag; a plain
        // `find('>')` would stop at a `>` inside an attribute value.
        let Some(open_end) = tag_end(rest, start) else {
            return out;
        };
        let content_start = open_end + 1;
        let Some(close_offset) = find_ignore_case(&rest[content_start..], &close) else {
            // HTML allows omitting end tags (`<head>…<body>…`). Never discard the rest
            // of the document: drop only the unclosed opening tag and keep the content.
            out.push_str(&rest[content_start..]);
            return out;
        };
        let close_start = content_start + close_offset;
        let Some(close_end) = tag_end(rest, close_start) else {
            return out;
        };
        rest = &rest[close_end + 1..];
    }
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    // ASCII case-insensitive search without allocating a lowercased copy of the whole
    // haystack on every call.
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&start| {
        haystack[start..start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(haystack, needle)| haystack.eq_ignore_ascii_case(needle))
    })
}

/// Decode the HTML entities a page actually uses, plus numeric forms.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        // The entity name is short, but slicing at a fixed byte offset can land inside a
        // multi-byte character in page text; shrink to the nearest char boundary.
        let mut boundary = tail.len().min(12);
        while boundary > 0 && !tail.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let Some(semicolon) = tail[..boundary].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semicolon];
        match entity_value(entity) {
            Some(value) => {
                out.push_str(&value);
                rest = &tail[semicolon + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity_value(entity: &str) -> Option<String> {
    if let Some(digits) = entity.strip_prefix('#') {
        let code = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            digits.parse::<u32>().ok()?
        };
        return char::from_u32(code).map(String::from);
    }
    let character = match entity.to_ascii_lowercase().as_str() {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" | "ensp" | "emsp" | "thinsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "middot" => '·',
        "bull" => '•',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "sect" => '§',
        "para" => '¶',
        "dagger" => '†',
        "permil" => '‰',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "micro" => 'µ',
        "ne" => '≠',
        "le" => '≤',
        "ge" => '≥',
        "larr" => '←',
        "rarr" => '→',
        "harr" => '↔',
        "infinity" => '∞',
        "check" => '✓',
        "cross" => '✗',
        "star" => '★',
        _ => return None,
    };
    Some(character.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_headings_links_and_lists() {
        let html = r#"
            <html><head><title>ignored</title><style>p{color:red}</style></head>
            <body>
              <h1>Title</h1>
              <p>Hello <strong>world</strong> &amp; <a href="https://example.com/a">friends</a>.</p>
              <ul><li>one</li><li>two</li></ul>
              <ol><li>first</li><li>second</li></ol>
              <script>var x = "<p>not content</p>";</script>
            </body></html>"#;
        let markdown = html_to_markdown(html);
        assert!(markdown.contains("# Title"), "{markdown}");
        assert!(
            markdown.contains("Hello **world** & [friends](https://example.com/a)."),
            "{markdown}"
        );
        assert!(markdown.contains("- one"), "{markdown}");
        assert!(markdown.contains("1. first"), "{markdown}");
        assert!(markdown.contains("2. second"), "{markdown}");
        assert!(!markdown.contains("not content"), "{markdown}");
        assert!(!markdown.contains("color:red"), "{markdown}");
        assert!(!markdown.contains("ignored"), "{markdown}");
    }

    #[test]
    fn keeps_preformatted_blocks_verbatim() {
        let html = "<p>before</p><pre><code>fn main() {\n    run();\n}</code></pre><p>after</p>";
        let markdown = html_to_markdown(html);
        assert!(
            markdown.contains("```\nfn main() {\n    run();\n}\n```"),
            "{markdown}"
        );
    }

    #[test]
    fn renders_images_quotes_and_tables() {
        let html = r#"<blockquote><p>quoted words</p></blockquote>
            <table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>
            <img src="/logo.png" alt="Logo">"#;
        let markdown = html_to_markdown(html);
        assert!(markdown.contains("> quoted words"), "{markdown}");
        assert!(markdown.contains("a | b"), "{markdown}");
        assert!(markdown.contains("![Logo](/logo.png)"), "{markdown}");
    }

    #[test]
    fn decodes_numeric_and_named_entities() {
        assert_eq!(
            decode_entities("a &lt; b &#65; &#x42; &nbsp;c"),
            "a < b A B  c"
        );
        assert_eq!(
            decode_entities("100% &notanentity; ok"),
            "100% &notanentity; ok"
        );
    }

    #[test]
    fn tolerates_broken_markup() {
        let markdown = html_to_markdown("<p>unclosed <b>bold <a href=\"https://x.example\">link");
        assert!(markdown.contains("bold"), "{markdown}");
        assert!(markdown.contains("[link](https://x.example)"), "{markdown}");
    }

    #[test]
    fn decodes_entities_that_straddle_a_byte_boundary() {
        // The 12-byte window used to slice inside the third byte of a CJK character
        // and panic on attacker-controlled page text.
        let decoded = decode_entities("&你好世界你好世界");
        assert!(decoded.starts_with('&'), "{decoded}");
        assert!(decoded.contains("你好世界"), "{decoded}");
    }

    #[test]
    fn keeps_a_column_for_an_empty_cell() {
        let markdown = html_to_markdown("<table><tr><td></td><td>1</td></tr></table>");
        assert!(markdown.contains(" | 1"), "{markdown}");
    }

    #[test]
    fn keeps_the_document_after_an_omitted_end_tag() {
        // HTML allows `<head>…<body>…` without `</head>`; the body must survive.
        let markdown =
            html_to_markdown("<html><head><title>t</title><body>BODY TEXT</body></html>");
        assert!(markdown.contains("BODY TEXT"), "{markdown}");
    }

    #[test]
    fn strips_an_element_with_quoted_gt_and_close_prefix_in_attributes() {
        let markdown =
            html_to_markdown(r#"<p>a</p><script data-x="a></script>b">evil()</script><p>b</p>"#);
        assert!(!markdown.contains("evil()"), "{markdown}");
        assert!(markdown.contains('a'), "{markdown}");
        assert!(markdown.contains('b'), "{markdown}");
    }

    #[test]
    fn strips_an_element_whose_attribute_contains_a_gt() {
        let markdown = html_to_markdown(r#"<p>a</p><script data-x="a>b">evil()</script><p>b</p>"#);
        assert!(!markdown.contains("evil()"), "{markdown}");
    }
}
