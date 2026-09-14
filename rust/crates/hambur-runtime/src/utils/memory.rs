use crate::*;

pub(crate) fn read_memory_file_content(path: &Path) -> HamburResult<String> {
    let raw = fs::read_to_string(path).unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if is_managed_memory_file(name) {
        let entries = parse_memory_entries(&raw)
            .into_iter()
            .filter(|entry| !is_review_status_entry(entry))
            .collect::<Vec<_>>();
        Ok(entries.join(MEMORY_ENTRY_DELIMITER))
    } else {
        Ok(raw)
    }
}

pub(crate) fn memory_snapshot_from_root(root: &Path) -> HamburResult<MemorySnapshot> {
    fs::create_dir_all(root)
        .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
    let memory_entries =
        parse_memory_entries(&read_memory_file_content(&root.join(MEMORY_FILE_NAME))?);
    let user_entries = parse_memory_entries(&read_memory_file_content(&root.join(USER_FILE_NAME))?);
    Ok(MemorySnapshot {
        memory_block: render_memory_block("memory", &memory_entries, MEMORY_CHAR_LIMIT),
        user_block: render_memory_block("user", &user_entries, USER_MEMORY_CHAR_LIMIT),
    })
}

pub(crate) fn render_memory_block(target: &str, entries: &[String], limit: usize) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let content = entries.join(MEMORY_ENTRY_DELIMITER);
    let pct = if limit == 0 {
        0
    } else {
        ((content.len() as f32 / limit as f32) * 100.0) as u32
    }
    .min(100);
    let header = if target == "user" {
        format!(
            "USER PROFILE (who the user is) [{pct}% - {}/{} chars]",
            content.len(),
            limit
        )
    } else {
        format!(
            "MEMORY (your personal notes) [{pct}% - {}/{} chars]",
            content.len(),
            limit
        )
    };
    let separator = "=".repeat(46);
    format!("{separator}\n{header}\n{separator}\n{content}")
}

pub(crate) fn format_memory_system_prompt(snapshot: &MemorySnapshot) -> String {
    if snapshot.is_empty() {
        return String::new();
    }
    let mut prompt = String::new();
    prompt.push_str("You have persistent memory across chats. Use it as durable background context, not as a new user message.\n");
    prompt.push_str("Save stable preferences, corrections, environment facts, and recurring conventions with the memory tool. Do not save temporary task progress or short-lived todos.\n");
    if !snapshot.memory_block.trim().is_empty() {
        prompt.push('\n');
        prompt.push_str(&snapshot.memory_block);
        prompt.push('\n');
    }
    if !snapshot.user_block.trim().is_empty() {
        prompt.push('\n');
        prompt.push_str(&snapshot.user_block);
        prompt.push('\n');
    }
    prompt.trim().to_string()
}

pub(crate) fn build_memory_review_messages(
    review: &SessionReviewRecord,
    reason: &str,
    memory_snapshot: &MemorySnapshot,
) -> Vec<ModelMessage> {
    vec![
        ModelMessage {
            role: "system".to_string(),
            content: build_memory_review_system_prompt(),
            ..Default::default()
        },
        ModelMessage {
            role: "user".to_string(),
            content: build_memory_review_user_prompt(review, reason, memory_snapshot),
            ..Default::default()
        },
    ]
}

pub(crate) fn build_memory_review_system_prompt() -> String {
    r#"You are Hambur's background memory curator. The assistant's answer has already been shown to the user, so never answer the user's task, never ask follow-up questions, and never mention that you are reviewing memory.

Your only side effect is the memory tool. Use it to keep durable, future-useful memory accurate and compact.

Save these when they are stable and likely useful later:
- User identity, preferences, standing instructions, corrections, communication style, accessibility needs, and long-term goals.
- Stable project, app, repository, workspace, device, model, or tool conventions that will matter across chats.
- Recurring constraints the user expects the assistant to remember.

Do not save these:
- Temporary task progress, plans, one-off debugging details, transient todos, ephemeral files, branch names, commit hashes, or facts likely to expire soon.
- Secrets, API keys, tokens, passwords, private credentials, or sensitive data that the user did not explicitly ask to remember.
- Inferences about the user that are not directly supported by the transcript.
- Anything already represented well in current memory, even if the wording is not identical.

Target selection:
- Use target="user" for facts about the user as a person or their stable preferences.
- Use target="memory" for durable assistant/workspace/project/app operating notes.

Editing policy:
- Compare the transcript with the provided current memory before writing.
- Do not store duplicate memories. If a fact is already present, make no change for that fact.
- Prefer replace/remove when a current entry is stale, duplicated, or contradicted.
- Prefer add only for new atomic facts. Keep each entry short, declarative, and specific.
- It is allowed and often correct to make no modifications. If nothing durable should change, call no tools and return only: no_changes.
- Never pass "no_changes", review summaries, or JSON containing "memory_review", "changed_targets", or "action_counts" as memory tool content or old_text.
- After tool calls, return a short plain-text summary of changed targets and action counts."#
        .to_string()
}

pub(crate) fn build_memory_review_user_prompt(
    review: &SessionReviewRecord,
    reason: &str,
    memory_snapshot: &MemorySnapshot,
) -> String {
    let memory_block = if memory_snapshot.memory_block.trim().is_empty() {
        "MEMORY (your personal notes): empty".to_string()
    } else {
        memory_snapshot.memory_block.clone()
    };
    let user_block = if memory_snapshot.user_block.trim().is_empty() {
        "USER PROFILE (who the user is): empty".to_string()
    } else {
        memory_snapshot.user_block.clone()
    };
    format!(
        "Review trigger: {reason}.\nSession id: {}\n\nCurrent persistent memory snapshot:\n{memory_block}\n\n{user_block}\n\nRecent conversation transcript and tool traces:\n{}\n\nReview the transcript deeply but write conservatively. Use the memory tool only if the update is durable, clearly supported, and not already present in current memory. Making no changes is acceptable.",
        review.id,
        build_memory_review_transcript(review)
    )
}

pub(crate) fn build_memory_review_transcript(review: &SessionReviewRecord) -> String {
    let mut output = String::new();
    let start = review
        .messages
        .len()
        .saturating_sub(MAX_MEMORY_REVIEW_TRANSCRIPT_MESSAGES);
    for (index, message) in review.messages.iter().skip(start).enumerate() {
        output.push_str(&format!(
            "[{index}] role={} id={} turn={}\n",
            message.role, message.id, message.turn_id
        ));
        if !message.provider_name_snapshot.trim().is_empty()
            || !message.provider_id_snapshot.trim().is_empty()
            || !message.model_id_snapshot.trim().is_empty()
        {
            output.push_str(&format!(
                "model={}/{}\n",
                message
                    .clone()
                    .provider_name_snapshot
                    .if_blank(message.provider_id_snapshot.clone()),
                message
                    .clone()
                    .model_id_snapshot
                    .if_blank(message.model_name_snapshot.clone())
            ));
        }
        if !message.attachments.is_empty() {
            let attachments = message
                .attachments
                .iter()
                .map(|attachment| {
                    format!("{}:{}", attachment.kind, attachment.display_name)
                        .chars()
                        .take(MAX_MEMORY_REVIEW_ATTACHMENT_CHARS)
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str("attachments=");
            output.push_str(&attachments);
            output.push('\n');
        }
        if !message.tool_name.trim().is_empty() {
            output.push_str(&format!(
                "tool_result={} title={}\n",
                message.tool_name,
                message.tool_title.clone().if_blank("(none)".to_string())
            ));
        }
        let content = message.content_text.trim();
        output.push_str("content:\n");
        if content.is_empty() {
            output.push_str("(empty)\n");
        } else {
            output.push_str(
                &content
                    .chars()
                    .take(MAX_MEMORY_REVIEW_MESSAGE_CHARS)
                    .collect::<String>(),
            );
            output.push('\n');
            if content.chars().count() > MAX_MEMORY_REVIEW_MESSAGE_CHARS {
                output.push_str("[message truncated]\n");
            }
        }
        output.push('\n');
    }

    let trace_start = review
        .trace_spans
        .len()
        .saturating_sub(MAX_MEMORY_REVIEW_TRACE_EVENTS);
    let traces = review
        .trace_spans
        .iter()
        .skip(trace_start)
        .collect::<Vec<_>>();
    if !traces.is_empty() {
        output.push_str("Latest turn trace events:\n");
        for (index, event) in traces.iter().enumerate() {
            output.push_str(&format!(
                "[{index}] kind={} status={} title={} tool={}\n",
                event.kind,
                event.status,
                event.title,
                if event.tool_call_id.trim().is_empty() {
                    "(none)"
                } else {
                    event.tool_call_id.as_str()
                }
            ));
            if !event.content.trim().is_empty() {
                output.push_str(
                    &event
                        .content
                        .trim()
                        .chars()
                        .take(MAX_MEMORY_REVIEW_TRACE_CHARS)
                        .collect::<String>(),
                );
                output.push('\n');
            }
        }
    }

    let transcript = output.trim().to_string();
    if transcript.chars().count() <= MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS {
        return transcript;
    }
    let tail = transcript
        .chars()
        .rev()
        .take(MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("[older transcript omitted]\n{tail}")
}

pub(crate) fn parse_memory_entries(raw: &str) -> Vec<String> {
    raw.split(MEMORY_ENTRY_DELIMITER)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn count_memory_entries(raw: &str) -> u32 {
    u32::try_from(parse_memory_entries(raw).len()).unwrap_or(u32::MAX)
}

pub(crate) fn memory_preview(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "§")
        .unwrap_or_default()
        .chars()
        .take(240)
        .collect()
}

pub(crate) fn memory_file_sort_priority(name: &str) -> u8 {
    if name.eq_ignore_ascii_case(MEMORY_FILE_NAME) {
        0
    } else if name.eq_ignore_ascii_case(USER_FILE_NAME) {
        1
    } else {
        2
    }
}

pub(crate) fn is_managed_memory_file(name: &str) -> bool {
    name.eq_ignore_ascii_case(MEMORY_FILE_NAME) || name.eq_ignore_ascii_case(USER_FILE_NAME)
}

pub(crate) fn is_review_status_entry(content: &str) -> bool {
    let compact = content
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return false;
    }
    if compact == "no_changes" || compact == "\"no_changes\"" {
        return true;
    }
    compact.starts_with('{')
        && (compact.contains("\"memory_review\"")
            || compact.contains("\"changed_targets\"")
            || compact.contains("\"action_counts\""))
}

pub(crate) fn write_memory_entries(path: &Path, entries: &[String]) -> HamburResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| HamburError::Internal(format!("create memory parent: {error}")))?;
    }
    fs::write(path, entries.join(MEMORY_ENTRY_DELIMITER)).map_err(|error| {
        HamburError::Internal(format!("write memory file {}: {error}", path.display()))
    })
}

pub(crate) fn memory_response(
    success: bool,
    target: &str,
    entries: &[String],
    message: &str,
    error: &str,
) -> Value {
    let limit = if target == "user" { 1375 } else { 2200 };
    let current = entries.join(MEMORY_ENTRY_DELIMITER).len();
    let pct = if limit == 0 {
        0
    } else {
        ((current as f32 / limit as f32) * 100.0).round() as u32
    }
    .min(100);
    json!({
        "success": success,
        "target": target,
        "entries": entries,
        "usage": format!("{pct}% - {current}/{limit} chars"),
        "entry_count": entries.len(),
        "message": message,
        "error": error,
        "summary": if success { message } else { error }
    })
}

pub(crate) const MEMORY_FILE_NAME: &str = "MEMORY.md";

pub(crate) const USER_FILE_NAME: &str = "USER.md";

pub(crate) const MEMORY_ENTRY_DELIMITER: &str = "\n§\n";

pub(crate) const MEMORY_CHAR_LIMIT: usize = 2200;

pub(crate) const USER_MEMORY_CHAR_LIMIT: usize = 1375;

pub(crate) const MAX_MEMORY_REVIEW_TOOL_ITERATIONS: u32 = 8;

pub(crate) const MAX_MEMORY_REVIEW_TRANSCRIPT_MESSAGES: usize = 40;

pub(crate) const MAX_MEMORY_REVIEW_TRACE_EVENTS: usize = 24;

pub(crate) const MAX_MEMORY_REVIEW_TRANSCRIPT_CHARS: usize = 24_000;

pub(crate) const MAX_MEMORY_REVIEW_MESSAGE_CHARS: usize = 3_000;

pub(crate) const MAX_MEMORY_REVIEW_TRACE_CHARS: usize = 1_000;

pub(crate) const MAX_MEMORY_REVIEW_ATTACHMENT_CHARS: usize = 200;
