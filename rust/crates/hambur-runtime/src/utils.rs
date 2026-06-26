use crate::*;

pub(crate) fn safe_join(root: &Path, relative: &str) -> HamburResult<PathBuf> {
    if relative.contains('\0')
        || relative.contains("..")
        || relative.contains('\\')
        || Path::new(relative).is_absolute()
    {
        return Err(HamburError::InvalidCommand(
            "path must not escape root".to_string(),
        ));
    }
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)?;
            root.canonicalize()
        })
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let candidate = root.join(relative);
    let check_path = if candidate.exists() {
        candidate.canonicalize().map_err(|error| {
            HamburError::Internal(format!(
                "canonicalize path {}: {error}",
                candidate.display()
            ))
        })?
    } else {
        candidate
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .unwrap_or_else(|| root.clone())
    };
    if !check_path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(candidate)
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> HamburResult<String> {
    let root = root
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let path = path
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize path: {error}")))?;
    if !path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/"))
}

pub(crate) fn collect_named_files(root: &Path, name: &str, output: &mut Vec<PathBuf>) -> HamburResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, name, output)?;
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
    Ok(())
}

pub(crate) fn list_relative_files(root: &Path) -> HamburResult<Vec<String>> {
    let mut output = Vec::new();
    collect_relative_files(root, root, &mut output)?;
    output.sort();
    Ok(output)
}

pub(crate) fn collect_relative_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> HamburResult<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else if path.is_file() {
            output.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

pub(crate) fn parse_frontmatter(raw: &str) -> HashMap<String, Vec<String>> {
    if !raw.starts_with("---\n") {
        return HashMap::new();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return HashMap::new();
    };
    let yaml = &raw[4..4 + end];
    yaml.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains(':') {
                return None;
            }
            let key = trimmed.split_once(':')?.0.trim().to_string();
            let value = trimmed.split_once(':')?.1.trim();
            Some((key, parse_frontmatter_value(value)))
        })
        .collect()
}

pub(crate) fn parse_frontmatter_value(value: &str) -> Vec<String> {
    let unquoted = value.trim().trim_matches('"').trim_matches('\'');
    if unquoted.starts_with('[') && unquoted.ends_with(']') {
        return unquoted
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|item| !item.is_empty())
            .collect();
    }
    if unquoted.is_empty() {
        Vec::new()
    } else {
        vec![unquoted.to_string()]
    }
}

pub(crate) fn strip_frontmatter(raw: &str) -> String {
    if !raw.starts_with("---\n") {
        return raw.to_string();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return raw.to_string();
    };
    raw[4 + end + 4..].trim_start_matches('\n').to_string()
}

pub(crate) fn seed_bundled_skills(root: &Path) -> HamburResult<()> {
    fs::create_dir_all(root)
        .map_err(|error| HamburError::Internal(format!("create skills root: {error}")))?;
    for bundled in BUNDLED_SKILLS {
        let destination = safe_join(root, bundled.relative_path)?;
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| HamburError::Internal(format!("create skill dir: {error}")))?;
        }
        fs::write(&destination, bundled.content)
            .map_err(|error| HamburError::Internal(format!("seed skill: {error}")))?;
    }
    Ok(())
}

pub(crate) fn is_bundled_skill_path(path: &str) -> bool {
    BUNDLED_SKILLS
        .iter()
        .any(|bundled| bundled.relative_path == path)
}

pub(crate) fn linked_skill_files_json(skill_dir: &Path) -> String {
    let mut groups = serde_json::Map::new();
    for child in ["references", "templates", "scripts", "assets"] {
        let dir = skill_dir.join(child);
        if !dir.is_dir() {
            continue;
        }
        let files = list_relative_files(&dir)
            .unwrap_or_default()
            .into_iter()
            .map(|path| Value::String(format!("{child}/{path}")))
            .collect::<Vec<_>>();
        if !files.is_empty() {
            groups.insert(child.to_string(), Value::Array(files));
        }
    }
    Value::Object(groups).to_string()
}

pub(crate) fn system_time_to_ms(value: std::time::SystemTime) -> Option<u64> {
    value
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

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

pub(crate) fn normalize_tool_sandbox_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/var/hambur/workspace".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/var/hambur/workspace/{trimmed}")
    }
}

pub(crate) fn collect_all_files(root: &Path, output: &mut Vec<PathBuf>) -> HamburResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_all_files(&path, output)?;
        } else if path.is_file() {
            output.push(path);
        }
    }
    Ok(())
}

pub(crate) fn path_for_search_result(resolved: &hambur_sandbox::SandboxPathResolution, file: &Path) -> String {
    if resolved.host_path.is_file() {
        return resolved.sandbox_path.clone();
    }
    let relative = file
        .strip_prefix(&resolved.host_path)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    if relative.is_empty() {
        resolved.sandbox_path.clone()
    } else {
        format!(
            "{}/{}",
            resolved.sandbox_path.trim_end_matches('/'),
            relative.trim_start_matches('/')
        )
    }
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

pub(crate) fn skill_summary_json(skill: RuntimeSkillSummary) -> Value {
    json!({
        "name": skill.name,
        "description": skill.description,
        "category": skill.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", skill.path),
        "tags": skill.tags
    })
}

pub(crate) fn skill_detail_json(detail: RuntimeSkillDetail, selected_file: bool) -> Value {
    if selected_file {
        return json!({
            "success": true,
            "name": detail.summary.name,
            "file_path": detail.selected_file_path,
            "path": format!("{SANDBOX_SKILLS_PATH}/{}/{}", detail.skill_dir_path, detail.selected_file_path),
            "content": detail.selected_file_content,
            "summary": "skill file loaded"
        });
    }
    let linked_files =
        serde_json::from_str::<Value>(&detail.linked_files_json).unwrap_or_else(|_| json!({}));
    json!({
        "success": true,
        "name": detail.summary.name,
        "description": detail.summary.description,
        "category": detail.summary.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", detail.summary.path),
        "skill_dir": format!("{SANDBOX_SKILLS_PATH}/{}", detail.skill_dir_path),
        "linked_files": linked_files,
        "content": detail.content,
        "hint": "To view linked files, call skill_view(name, file_path) where file_path is e.g. references/api.md, templates/config.yaml, or scripts/setup.sh."
    })
}

pub(crate) fn normalize_knowledge_tool_name(name: &str) -> &str {
    match name {
        "skill_list" => "skills_list",
        other => other,
    }
}

pub(crate) fn disabled_skill_paths_from_snapshot(snapshot: SettingsSnapshot) -> HashSet<String> {
    snapshot
        .settings
        .into_iter()
        .filter_map(|setting| {
            setting
                .key
                .strip_prefix("skill_enabled:")
                .filter(|_| setting.value == "false")
                .map(ToString::to_string)
        })
        .collect()
}

pub(crate) fn format_skills_index_prompt(skills: Vec<RuntimeSkillSummary>) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut prompt = String::new();
    prompt.push_str("Hambur has a local Skills system at /var/hambur/skills. This directory is shared by all chat sessions and visible inside the Linux sandbox.\n");
    prompt.push_str("Skills are reusable task instructions, references, scripts, and templates. They are loaded through progressive disclosure: use `skills_list` for compact metadata, then `skill_view` to inspect a skill before following its detailed workflow.\n");
    prompt.push_str("Available skills:\n");
    for skill in skills {
        prompt.push_str("- ");
        prompt.push_str(&skill.name);
        if !skill.category.is_empty() {
            prompt.push_str(" [");
            prompt.push_str(&skill.category);
            prompt.push(']');
        }
        prompt.push_str(": ");
        prompt.push_str(
            &skill
                .description
                .chars()
                .take(SKILL_MAX_DESCRIPTION_CHARS)
                .collect::<String>(),
        );
        prompt.push('\n');
    }
    prompt.trim().to_string()
}

pub(crate) fn database_path(bootstrap: &AppBootstrap) -> PathBuf {
    PathBuf::from(&bootstrap.app_files_dir).join("hambur.db")
}

pub(crate) const SANDBOX_SKILLS_PATH: &str = "/var/hambur/skills";
pub(crate) const SKILL_MAX_LINKED_FILE_BYTES: u64 = 512_000;
pub(crate) const SKILL_MAX_DESCRIPTION_CHARS: usize = 320;
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
pub(crate) const BUNDLED_SKILLS: &[BundledSkillFile] = &[BundledSkillFile {
    relative_path: "system/skill-creator/SKILL.md",
    content: include_str!("../assets/skills/system/skill-creator/SKILL.md"),
}];

pub(crate) struct BundledSkillFile {
    pub(crate) relative_path: &'static str,
    pub(crate) content: &'static str,}

pub(crate) fn normalize_command(mut command: RuntimeCommand) -> RuntimeCommand {
    if command.command_id.trim().is_empty() {
        command.command_id = new_id("cmd");
    }
    if command.created_at_ms == 0 {
        command.created_at_ms = now_ms();
    }
    command.kind = command.kind.trim().to_string();
    command.idempotency_key = command.idempotency_key.trim().to_string();
    command.session_id = command.session_id.trim().to_string();
    command.turn_id = command.turn_id.trim().to_string();
    command.message_id = command.message_id.trim().to_string();
    command.provider_id = command.provider_id.trim().to_string();
    command.model_id = command.model_id.trim().to_string();
    command.source_message_id = command.source_message_id.trim().to_string();
    command
}

pub(crate) fn validate_command(command: &RuntimeCommand) -> HamburResult<()> {
    if command.kind.is_empty() {
        return Err(HamburError::InvalidCommand(
            "command kind must not be empty".to_string(),
        ));
    }
    if command.idempotency_key.is_empty() {
        return Err(HamburError::InvalidCommand(
            "idempotency_key must not be empty".to_string(),
        ));
    }

    match command.kind.as_str() {
        "Initialize" | "Shutdown" | "CreateSession" => Ok(()),
        "OpenSession" | "DeleteSession" | "SoftDeleteSession" | "HardPurgeSession"
        | "SetSessionPinned" | "PinSession" | "UnpinSession" => require_session_id(command),
        "RenameSession" | "UpdateSessionTitle" => {
            require_session_id(command)?;
            if command.title.trim().is_empty()
                && command.content.trim().is_empty()
                && command.chunk.trim().is_empty()
                && config_payload_string(&command.payload_json, "title").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "session title must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateProvider" => {
            if command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider base_url must not be empty".to_string(),
                ));
            }
            if command.payload_json.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider secret_ref must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteProvider" => {
            if command.provider_id.is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RefreshProviderModels" => {
            if command.provider_id.is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateModelOverride" | "UpdateModelDetail" => {
            if command.provider_id.is_empty()
                && command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "providerId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "provider_id must not be empty".to_string(),
                ));
            }
            if command.model_id.is_empty()
                && config_payload_string(&command.payload_json, "modelId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "model_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "UpdateModelGroup" => Ok(()),
        "UpdateModelGroupMember" => Ok(()),
        "SetDefaultModelGroup" => Ok(()),
        "UpdateDefaultModelGroups" => {
            if command.payload_json.trim().is_empty() && command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "default model group payload must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteModelGroup" => {
            if command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "groupId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "group_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteModelGroupMember" => {
            if command.message_id.is_empty()
                || command.provider_id.is_empty()
                || command.model_id.is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "group_id (message_id), provider_id, and model_id must not be empty"
                        .to_string(),
                ));
            }
            Ok(())
        }
        "UpdateToolSettings"
        | "UpdateSkills"
        | "UpdateMemoryProjections"
        | "UpdateStartupTasks"
        | "UpdateRootfsSettings"
        | "UpdateAppearance"
        | "UpdateLogs"
        | "UpdateTokenUsage"
        | "UpdatePersona"
        | "UpdateEnvironmentVariables"
        | "UpdateAppSetting"
        | "UpdateBrowserToolSettings"
        | "UpdateSkillEnabled"
        | "UpdateStartupTask"
        | "DeleteStartupTask"
        | "UpdateRootfsSetting" => {
            if command.payload_json.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "setting payload_json must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "DeleteSkill" => {
            if command.message_id.trim().is_empty()
                && config_payload_string(&command.payload_json, "skillId").is_empty()
                && config_payload_string(&command.payload_json, "skillPath").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "skill id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "ImportAttachmentFromUri" => {
            require_session_id(command)?;
            Ok(())
        }
        "RemovePendingAttachment" => {
            require_session_id(command)?;
            if command.message_id.is_empty() && command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "attachment_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "ClearPendingAttachments" => {
            require_session_id(command)?;
            Ok(())
        }
        "SendMessage" | "EditMessage" => {
            require_session_id(command)?;
            if command.content.trim().is_empty() && command.chunk.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "message content must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RetryTurn" | "RegenerateMessage" => {
            require_session_id(command)?;
            if command.content.trim().is_empty()
                && command.chunk.trim().is_empty()
                && command.source_message_id.is_empty()
                && command.message_id.is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "source_message_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "CancelTurn" => {
            if command.session_id.is_empty() && command.turn_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "CancelTurn requires session_id or turn_id".to_string(),
                ));
            }
            Ok(())
        }
        "SubmitPlatformResult" => {
            if command.message_id.is_empty()
                && config_payload_string(&command.payload_json, "requestId").is_empty()
            {
                return Err(HamburError::InvalidCommand(
                    "request_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        "RunRootfsWarmup" => Ok(()),
        "ResetRootfs" => require_approval(command, "rootfs_reset"),
        "AppendMarkdownDelta" | "MarkdownRenderUpdate" => {
            require_session_id(command)?;
            if command.message_id.is_empty() {
                return Err(HamburError::InvalidCommand(
                    "message_id must not be empty".to_string(),
                ));
            }
            Ok(())
        }
        _ => Err(HamburError::InvalidCommand(format!(
            "unsupported command kind: {}",
            command.kind
        ))),
    }
}

pub(crate) fn require_session_id(command: &RuntimeCommand) -> HamburResult<()> {
    if command.session_id.is_empty() {
        Err(HamburError::InvalidCommand(
            "session_id must not be empty".to_string(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn route_plan_from_records(records: Vec<ModelRouteSnapshot>) -> RoutePlan {
    let first = records.first().cloned().unwrap_or_default();
    RoutePlan {
        group_id: first.model_group_id,
        routing_strategy: RoutingStrategy::parse(&first.routing_strategy),
        fallback_policy: FallbackPolicy::parse(&first.fallback_policy),
        targets: records
            .into_iter()
            .map(provider_target_from_route)
            .collect(),
    }
}

pub(crate) fn provider_target_from_route(route: ModelRouteSnapshot) -> ProviderTarget {
    ProviderTarget {
        provider: ProviderConfig {
            id: route.provider_id.clone(),
            name: route.provider_name.clone(),
            protocol: route.provider_protocol.clone(),
            base_url: route.base_url.clone(),
            secret_ref: route.secret_ref.clone(),
            enabled: true,
        },
        model: ProviderModel {
            provider_id: route.provider_id.clone(),
            model_id: route.model_id.clone(),
            display_name: route.model_display_name.clone(),
            capabilities: ModelCapabilities {
                supports_tool_call: route.supports_tool_call,
                supports_reasoning: route.supports_reasoning,
                supports_image_input: route.supports_image_input,
                supports_structured_output: route.supports_structured_output,
                supports_temperature: route.supports_temperature,
                context_limit: route.context_limit,
                output_limit: route.output_limit,
                reasoning_field: route.reasoning_field.clone(),
            },
            metadata_json: "{}".to_string(),
        },
        model_group_id: route.model_group_id,
        model_group_name: route.model_group_name,
        position: route.position,
    }
}

pub(crate) fn route_snapshot_from_target(target: &ProviderTarget) -> ModelRouteSnapshot {
    ModelRouteSnapshot {
        provider_id: target.provider.id.clone(),
        provider_name: target.provider.name.clone(),
        provider_protocol: target.provider.protocol.clone(),
        base_url: target.provider.base_url.clone(),
        secret_ref: target.provider.secret_ref.clone(),
        model_id: target.model.model_id.clone(),
        model_display_name: target.model.display_name.clone(),
        model_group_id: target.model_group_id.clone(),
        model_group_name: target.model_group_name.clone(),
        routing_strategy: RoutingStrategy::Fallback.as_str().to_string(),
        fallback_policy: FallbackPolicy::Default.as_str().to_string(),
        position: target.position,
        supports_tool_call: target.model.capabilities.supports_tool_call,
        supports_reasoning: target.model.capabilities.supports_reasoning,
        supports_image_input: target.model.capabilities.supports_image_input,
        supports_structured_output: target.model.capabilities.supports_structured_output,
        supports_temperature: target.model.capabilities.supports_temperature,
        context_limit: target.model.capabilities.context_limit,
        output_limit: target.model.capabilities.output_limit,
        reasoning_field: target.model.capabilities.reasoning_field.clone(),
    }
}

pub(crate) fn append_transcript_entry_to_context(
    messages: &mut Vec<ModelMessage>,
    open_tool_call_ids: &mut HashSet<String>,
    entry: hambur_db::ChatTranscriptEntry,
) -> HamburResult<()> {
    let message = entry.message;
    match message.role.as_str() {
        "user" => {
            if message.status == "completed" && !message.content_text.trim().is_empty() {
                messages.push(ModelMessage {
                    role: "user".to_string(),
                    content: message.content_text,
                    ..Default::default()
                });
            }
        }
        "assistant" => {
            if !assistant_message_is_context_eligible(&message, &entry.tool_calls) {
                return Ok(());
            }
            let tool_calls_json = if entry.tool_calls.is_empty() {
                String::new()
            } else {
                tool_calls_json_from_records(&entry.tool_calls)?
            };
            for call in &entry.tool_calls {
                open_tool_call_ids.insert(call.id.clone());
            }
            messages.push(ModelMessage {
                role: "assistant".to_string(),
                content: message.content_text,
                tool_calls_json,
                tool_call_id: String::new(),
            });
        }
        "tool" => {
            if message.status != "completed"
                || message.tool_call_id.trim().is_empty()
                || !open_tool_call_ids.remove(&message.tool_call_id)
            {
                return Ok(());
            }
            messages.push(ModelMessage {
                role: "tool".to_string(),
                content: message.content_text,
                tool_calls_json: String::new(),
                tool_call_id: message.tool_call_id,
            });
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn assistant_message_is_context_eligible(
    message: &MessageRecord,
    tool_calls: &[hambur_db::ToolCallRecord],
) -> bool {
    if matches!(
        message.status.as_str(),
        "failed" | "failed_partial" | "cancelled" | "deleted"
    ) {
        return false;
    }
    !message.content_text.trim().is_empty() || !tool_calls.is_empty()
}

pub(crate) fn tool_calls_json_from_records(calls: &[hambur_db::ToolCallRecord]) -> HamburResult<String> {
    let values = calls
        .iter()
        .map(|call| {
            if call.id.trim().is_empty() || call.name.trim().is_empty() {
                return Err(HamburError::InvalidCommand(
                    "stored tool call is missing id or name".to_string(),
                ));
            }
            let arguments = if call.arguments_json.trim().is_empty() {
                "{}"
            } else {
                call.arguments_json.trim()
            };
            Ok(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments,
                }
            }))
        })
        .collect::<HamburResult<Vec<_>>>()?;
    serde_json::to_string(&values)
        .map_err(|error| HamburError::Internal(format!("serialize stored tool calls: {error}")))
}

pub(crate) fn stream_source_for_command(
    command: &RuntimeCommand,
    turn_id: &str,
    content: &str,
    messages: Vec<ModelMessage>,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let provider_source = provider_stream_source(
        &command.session_id,
        turn_id,
        messages,
        route,
        tools_json,
        skills_index_prompt,
        memory_system_prompt,
        deep_thinking_enabled,
        search_enabled,
    );
    let request = match &provider_source {
        RouteStreamSource::Provider(request) => request.clone(),
        RouteStreamSource::Scripted { request, .. } => request.clone(),
    };
    let payload = command.payload_json.trim();
    if payload.starts_with("data:") {
        return scripted_stream_source(request, payload.to_string(), Vec::new());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Some(route_value) = scripted_route_value(&value, route) {
            let continuation_sse = scripted_continuation_sse(route_value);
            if let Some(sse) = route_value.get("sse").and_then(serde_json::Value::as_str) {
                return scripted_stream_source(request, sse.to_string(), continuation_sse);
            }
            let response = route_value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = route_value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
        let continuation_sse = scripted_continuation_sse(&value);
        if let Some(sse) = value.get("sse").and_then(serde_json::Value::as_str) {
            return scripted_stream_source(request, sse.to_string(), continuation_sse);
        }
        if value.get("content").is_some() || value.get("reasoning").is_some() {
            let response = value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| content.trim());
            let reasoning = value
                .get("reasoning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(command.reasoning.as_str());
            return RouteStreamSource::Scripted {
                request,
                chunks: scripted_openai_sse_chunks(response, reasoning),
                continuation_sse,
            };
        }
    }

    provider_source
}

pub(crate) const HAMBUR_FILE_LINK_SYSTEM_PROMPT: &str = r#"Hambur can render local file links sent in Markdown image syntax.
To send any file to the user, write `![title](hambur://PATH)`, where PATH is the absolute file path, for example `![report.pdf](hambur:///var/hambur/report.pdf)`.
Images, audio, and video render inline in the conversation. Other file types open a preview page with share and download actions.
Always use Markdown image syntax with the leading exclamation mark for hambur file links; do not write `[title](hambur://PATH)`.
Use a clear title as the visible link text. Do not use this for normal web links.

Directory structure and attachments:
- User uploaded files and images are stored under `/var/hambur/attachments/uploads/`.
- The directory `/var/hambur/shared/` is a shared folder that can be read and written across chat sessions.
- When the user uploads attachments/images, their sandbox paths are supplied at the end of the user message inside a `<user_attach_files>path1,path2,...</user_attach_files>` tag. Use these paths with `view_image` or other tools to access the files."#;

pub(crate) const HAMBUR_CONFIG_SYSTEM_PROMPT: &str = r#"Use the `hambur_config` tool to inspect or change Hambur app settings, providers, models, model groups, default routing, startup tasks, tool options, network toggles, sandbox backend, and logging.
Do not edit Android preference files or use terminal commands for Hambur app configuration. Start with `action=list_topics` or `action=topic_help` if you need to discover available config paths."#;

pub(crate) fn scripted_stream_source(
    request: ModelRequest,
    sse: String,
    continuation_sse: Vec<String>,
) -> RouteStreamSource {
    RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&sse),
        continuation_sse,
    }
}

pub(crate) fn scripted_continuation_sse(value: &Value) -> Vec<String> {
    if let Some(items) = value
        .get("sse_sequence")
        .or_else(|| value.get("continuation_sse"))
        .and_then(Value::as_array)
    {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    value
        .get("continuationSse")
        .and_then(Value::as_str)
        .map(|sse| vec![sse.to_string()])
        .unwrap_or_default()
}

pub(crate) fn provider_stream_source(
    session_id: &str,
    turn_id: &str,
    messages: Vec<ModelMessage>,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    skills_index_prompt: &str,
    memory_system_prompt: &str,
    deep_thinking_enabled: bool,
    search_enabled: bool,
) -> RouteStreamSource {
    let mut system_blocks = vec![
        "You are Hambur, a concise assistant.".to_string(),
        HAMBUR_FILE_LINK_SYSTEM_PROMPT.to_string(),
        HAMBUR_CONFIG_SYSTEM_PROMPT.to_string(),
    ];
    if !skills_index_prompt.trim().is_empty() {
        system_blocks.push(skills_index_prompt.to_string());
    }
    if !memory_system_prompt.trim().is_empty() {
        system_blocks.push(memory_system_prompt.to_string());
    }
    if search_enabled {
        system_blocks.push(
            "Web/search assistance is enabled for this turn. Use available search or fetch tools when current external information is needed."
                .to_string(),
        );
    }
    RouteStreamSource::Provider(ModelRequest {
        request_id: new_id("llm_req"),
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        purpose: "chat".to_string(),
        stream: true,
        system_blocks,
        messages,
        reasoning_mode: if route.supports_reasoning && deep_thinking_enabled {
            ReasoningMode::Enabled
        } else {
            ReasoningMode::Disabled
        },
        max_output_tokens: route.output_limit,
        temperature: Some(0.7),
        tools_json: if route.supports_tool_call {
            tools_json.to_string()
        } else {
            String::new()
        },
    })
}

pub(crate) fn tool_continuation_stream_source(
    mut request: ModelRequest,
    route: &ModelRouteSnapshot,
    tools_json: &str,
    assistant_content: &str,
    tool_calls: Vec<CompleteToolCall>,
    tool_result_messages: Vec<ModelMessage>,
    mut continuation_sse: Vec<String>,
    scripted_source: bool,
) -> HamburResult<RouteStreamSource> {
    request.request_id = new_id("llm_req");
    request.max_output_tokens = route.output_limit;
    request.tools_json = if route.supports_tool_call {
        tools_json.to_string()
    } else {
        String::new()
    };
    request.messages.push(ModelMessage {
        role: "assistant".to_string(),
        content: assistant_content.to_string(),
        tool_calls_json: complete_tool_calls_json(&tool_calls)?,
        tool_call_id: String::new(),
    });
    request.messages.extend(tool_result_messages);

    if continuation_sse.is_empty() {
        if scripted_source {
            return Err(HamburError::InvalidCommand(
                "scripted tool loop requires a continuation SSE".to_string(),
            ));
        }
        return Ok(RouteStreamSource::Provider(request));
    }
    let next_sse = continuation_sse.remove(0);
    Ok(RouteStreamSource::Scripted {
        request,
        chunks: split_scripted_sse(&next_sse),
        continuation_sse,
    })
}

pub(crate) fn complete_tool_calls_json(calls: &[CompleteToolCall]) -> HamburResult<String> {
    let values = calls
        .iter()
        .map(|call| {
            let arguments_value: Value =
                serde_json::from_str(&call.arguments_json).unwrap_or_else(|_| json!({}));
            let arguments_json = serde_json::to_string(&arguments_value).map_err(|error| {
                HamburError::Internal(format!("serialize tool call arguments: {error}"))
            })?;
            Ok(json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments_json
                }
            }))
        })
        .collect::<HamburResult<Vec<_>>>()?;
    serde_json::to_string(&values)
        .map_err(|error| HamburError::Internal(format!("serialize tool calls: {error}")))
}

pub(crate) fn openai_non_stream_request(
    request: &ModelRequest,
    target: &ProviderTarget,
    api_key: &str,
) -> HamburResult<hambur_llm::HttpRequestSpec> {
    let mut spec = OpenAiCompatibleAdapter::build_stream_request(request, target, api_key)?;
    let mut body: Value = serde_json::from_str(&spec.body_json)
        .map_err(|error| HamburError::InvalidCommand(format!("invalid request body: {error}")))?;
    body["stream"] = json!(false);
    if request.reasoning_mode == ReasoningMode::Disabled {
        body["thinking"] = json!({"type": "disabled"});
    }
    spec.body_json = body.to_string();
    Ok(spec)
}

pub(crate) async fn reqwest_json(spec: hambur_llm::HttpRequestSpec) -> HamburResult<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_provider_http_status(status));
    }
    response
        .text()
        .await
        .map_err(|error| HamburError::ProviderUnavailable(format!("NetworkError: {error}")))
}

pub(crate) fn parse_openai_non_stream_message(body: &str) -> HamburResult<MemoryReviewAssistantMessage> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| HamburError::SseParse(format!("parse chat completion JSON: {error}")))?;
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("provider error");
        return Err(HamburError::ProviderUnavailable(message.to_string()));
    }
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| HamburError::SseParse("chat completion missing message".to_string()))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(items) = message.get("tool_calls").and_then(Value::as_array) {
        for (fallback_index, item) in items.iter().enumerate() {
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(fallback_index as u32);
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let function = item.get("function").unwrap_or(&Value::Null);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments_json = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            if !id.is_empty() && !name.is_empty() {
                tool_calls.push(CompleteToolCall {
                    index,
                    id,
                    name,
                    arguments_json,
                });
            }
        }
    }
    Ok(MemoryReviewAssistantMessage {
        content,
        tool_calls,
    })
}

pub(crate) fn compile_named_tools_json(tools_json: &str, names: &[&str]) -> HamburResult<String> {
    let allowed = names.iter().copied().collect::<HashSet<_>>();
    let value: Value = serde_json::from_str(tools_json.trim()).map_err(|error| {
        HamburError::InvalidCommand(format!("invalid OpenAI tools JSON: {error}"))
    })?;
    let Value::Array(items) = value else {
        return Err(HamburError::InvalidCommand(
            "OpenAI tools JSON must be an array".to_string(),
        ));
    };
    let filtered = items
        .into_iter()
        .filter(|item| {
            item.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| allowed.contains(name))
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(filtered).to_string())
}

pub(crate) async fn reqwest_stream(spec: hambur_llm::HttpRequestSpec) -> HamburResult<reqwest::Response> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| {
            HamburError::ProviderUnavailable(format!("NetworkError: build HTTP client: {error}"))
        })?;
    let method = reqwest::Method::from_bytes(spec.method.as_bytes()).map_err(|error| {
        HamburError::ProviderUnavailable(format!("NetworkError: invalid HTTP method: {error}"))
    })?;
    let mut request = client.request(method, &spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.body(spec.body_json).send().await.map_err(|error| {
        if error.is_timeout() {
            HamburError::ProviderUnavailable(format!("NetworkTimeout: {error}"))
        } else {
            HamburError::ProviderUnavailable(format!("NetworkError: {error}"))
        }
    })?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        Err(map_provider_http_status(status))
    }
}

pub(crate) fn map_provider_http_status(status: StatusCode) -> HamburError {
    if status == StatusCode::TOO_MANY_REQUESTS {
        HamburError::ProviderUnavailable(format!("Http429: HTTP {}", status.as_u16()))
    } else if status.is_server_error() {
        HamburError::ProviderUnavailable(format!("Http5xx: HTTP {}", status.as_u16()))
    } else {
        HamburError::ProviderUnavailable(format!(
            "Http{}: HTTP {}",
            status.as_u16(),
            status.as_u16()
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AttachmentImportPayload {
    pub(crate) display_name: String,
    pub(crate) mime_type: String,
    pub(crate) byte_size: u64,
    pub(crate) origin_type: String,
    pub(crate) original_uri: String,
    pub(crate) source_path: String,
    pub(crate) bytes_base64: String,
    pub(crate) kind: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sha256: String,}

impl AttachmentImportPayload {
    pub(crate) fn parse(payload_json: &str) -> HamburResult<Self> {
        let value = if payload_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str::<Value>(payload_json).map_err(|error| {
                HamburError::InvalidCommand(format!(
                    "attachment import payload must be JSON: {error}"
                ))
            })?
        };
        let get_string = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| value.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .to_string()
        };
        let mime_type = get_string(&["mimeType", "mime_type"]);
        let kind = get_string(&["kind"]);
        Ok(Self {
            display_name: get_string(&["displayName", "display_name", "name"]),
            mime_type: if mime_type.trim().is_empty() {
                "application/octet-stream".to_string()
            } else {
                mime_type
            },
            byte_size: value
                .get("byteSize")
                .or_else(|| value.get("byte_size"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            origin_type: get_string(&["originType", "origin_type"])
                .if_blank("content_uri".to_string()),
            original_uri: get_string(&["originalUri", "original_uri", "uri"]),
            source_path: get_string(&["sourcePath", "source_path", "path"]),
            bytes_base64: get_string(&["bytesBase64", "bytes_base64", "base64"]),
            kind,
            width: value
                .get("width")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default(),
            height: value
                .get("height")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default(),
            sha256: get_string(&["sha256"]),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SendOptions {
    pub(crate) attachment_ids: Vec<String>,
    pub(crate) deep_thinking_enabled: bool,
    pub(crate) search_enabled: bool,}

impl SendOptions {
    pub(crate) fn parse(payload_json: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload_json) else {
            return Self::default();
        };
        let attachment_ids = value
            .get("attachmentIds")
            .or_else(|| value.get("attachment_ids"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        Self {
            attachment_ids,
            deep_thinking_enabled: value
                .get("deepThinkingEnabled")
                .or_else(|| value.get("deep_thinking_enabled"))
                .or_else(|| value.get("deepThinking"))
                .or_else(|| value.get("deep_thinking"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            search_enabled: value
                .get("searchEnabled")
                .or_else(|| value.get("search_enabled"))
                .or_else(|| value.get("search"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

pub(crate) fn format_user_content_with_attachments(content: &str, attachments: &[AttachmentRecord]) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }
    let mut formatted = content.trim().to_string();
    formatted.push_str("\n\nAttachments:");
    for attachment in attachments {
        if attachment.kind == "image" {
            formatted.push_str(&format!(
                "\n- ImagePart(fileId={}, sandboxPath={}, mimeType={}, detail=auto)",
                attachment.file_id, attachment.sandbox_path, attachment.mime_type
            ));
        } else {
            formatted.push_str(&format!(
                "\n- FileReferencePart(fileId={}, sandboxPath={}, name={}, size={}, mimeType={})",
                attachment.file_id,
                attachment.sandbox_path,
                attachment.display_name,
                attachment.byte_size,
                attachment.mime_type
            ));
        }
    }
    formatted
}

pub(crate) fn format_synthetic_view_image_message(context_stubs: &[String]) -> String {
    let image_parts = context_stubs
        .iter()
        .filter(|stub| stub.contains("ImagePart(fileId="))
        .map(|stub| stub.trim())
        .collect::<Vec<_>>();
    if image_parts.is_empty() {
        "Image returned by view_image.".to_string()
    } else {
        format!(
            "Synthetic multimodal continuation for view_image.\n{}",
            image_parts.join("\n")
        )
    }
}

#[allow(dead_code)]
pub(crate) fn run_terminal_command(
    invocation: &ToolInvocation,
    command: &str,
    cwd: &std::path::Path,
    timeout_ms: u64,
) -> RawToolOutput {
    let shell = platform_shell();
    let mut child = match Command::new(shell)
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: error.to_string(),
                summary: "terminal execution failed".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: "spawn_failed".to_string(),
            };
        }
    };
    let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(StdDuration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: json!({
                        "command": command,
                        "cwd": cwd.to_string_lossy(),
                        "timeoutMs": timeout_ms,
                        "timedOut": true
                    })
                    .to_string(),
                    summary: "terminal timed out".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "timeout".to_string(),
                };
            }
            Err(error) => {
                let _ = child.kill();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: error.to_string(),
                    summary: "terminal wait failed".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "wait_failed".to_string(),
                };
            }
        }
    }

    match child.wait_with_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            let content = json!({
                "command": command,
                "cwd": cwd.to_string_lossy(),
                "timeoutMs": timeout_ms,
                "exitCode": exit_code,
                "stdout": stdout,
                "stderr": stderr
            })
            .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !output.status.success(),
                summary: if output.status.success() {
                    format!(
                        "terminal exited 0 (stdout {} bytes, stderr {} bytes)",
                        output.stdout.len(),
                        output.stderr.len()
                    )
                } else {
                    format!("terminal exited {exit_code}")
                },
                content,
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: exit_code.to_string(),
            }
        }
        Err(error) => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: error.to_string(),
            summary: "terminal execution failed".to_string(),
            trust_level: "untrusted".to_string(),
            command_or_url: command.to_string(),
            status: "spawn_failed".to_string(),
        },
    }
}

#[allow(dead_code)]
pub(crate) fn platform_shell() -> &'static str {
    if cfg!(target_os = "android") {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    }
}

pub(crate) fn spawn_process_pipe_reader(
    mut reader: impl Read + Send + 'static,
    output: Arc<Mutex<ProcessOutputBuffer>>,
    stdout: bool,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if let Ok(mut output) = output.lock() {
                        if stdout {
                            output.push_stdout(&buffer[..size]);
                        } else {
                            output.push_stderr(&buffer[..size]);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub(crate) fn push_ring(buffer: &mut VecDeque<u8>, bytes: &[u8]) {
    const PROCESS_OUTPUT_RING_BYTES: usize = 64 * 1024;
    for byte in bytes {
        if buffer.len() >= PROCESS_OUTPUT_RING_BYTES {
            buffer.pop_front();
        }
        buffer.push_back(*byte);
    }
}

pub(crate) fn refresh_process_exit(state: &mut BackgroundProcessSession) {
    if state.exit_code.is_some() {
        return;
    }
    if let Ok(Some(status)) = state.child.try_wait() {
        state.exit_code = Some(status.code().unwrap_or(-1));
        state.finished_at_ms = now_ms();
    }
}

pub(crate) fn terminate_background_process_wrapper(state: &BackgroundProcessSession) {
    let Some(pid_file) = &state.pid_file else {
        return;
    };
    let Ok(pid) = fs::read_to_string(pid_file) else {
        return;
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return;
    };
    let _ = Command::new("su")
        .arg("-c")
        .arg(format!("kill -TERM {pid} 2>/dev/null || true"))
        .output();
}

pub(crate) fn process_status_json(
    state: &BackgroundProcessSession,
    output: Option<ProcessOutputSnapshot>,
) -> Value {
    let mut value = json!({
        "processSessionId": state.process_session_id,
        "backend": state.backend,
        "command": state.command,
        "cwd": state.cwd,
        "startedAt": state.started_at_ms,
        "pid": state.pid,
        "running": state.exit_code.is_none(),
        "exitCode": state.exit_code,
        "finishedAt": state.finished_at_ms
    });
    if let Some(output) = output
        && let Some(object) = value.as_object_mut()
    {
        object.insert("stdout".to_string(), json!(output.stdout));
        object.insert("stderr".to_string(), json!(output.stderr));
        object.insert(
            "stdoutTotalBytes".to_string(),
            json!(output.stdout_total_bytes),
        );
        object.insert(
            "stderrTotalBytes".to_string(),
            json!(output.stderr_total_bytes),
        );
    }
    value
}

pub(crate) fn completed_process_session_from_state(
    state: &BackgroundProcessSession,
    process_session_id: &str,
) -> Option<CompletedProcessSession> {
    let output = state.output.lock().ok().map(|buffer| buffer.snapshot())?;
    Some(CompletedProcessSession {
        session_id: state.session_id.clone(),
        process_session_id: process_session_id.to_string(),
        backend: state.backend.clone(),
        command: state.command.clone(),
        cwd: state.cwd.clone(),
        started_at_ms: state.started_at_ms,
        pid: state.pid,
        output,
        exit_code: state.exit_code,
        finished_at_ms: state.finished_at_ms,
    })
}

pub(crate) fn completed_process_status_json(state: &CompletedProcessSession) -> Value {
    json!({
        "processSessionId": state.process_session_id,
        "backend": state.backend,
        "command": state.command,
        "cwd": state.cwd,
        "startedAt": state.started_at_ms,
        "pid": state.pid,
        "running": false,
        "exitCode": state.exit_code,
        "finishedAt": state.finished_at_ms,
        "stdout": state.output.stdout,
        "stderr": state.output.stderr,
        "stdoutTotalBytes": state.output.stdout_total_bytes,
        "stderrTotalBytes": state.output.stderr_total_bytes
    })
}

pub(crate) fn markdown_block_summary(node: &hambur_markdown::MarkdownBlockNode) -> String {
    node.text
        .trim()
        .to_string()
        .if_blank(node.raw.trim().to_string())
        .chars()
        .take(160)
        .collect()
}

pub(crate) const TINYFISH_API_KEY: &str = "sk-tinyfish-nOfH8Vi9QMLd88_lfB0MKZbWg_O23YN-";

pub(crate) fn run_web_fetch(invocation: &ToolInvocation, arguments: &Value, backend: &str) -> RawToolOutput {
    let urls = arguments
        .get("urls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    if urls.is_empty() {
        return RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "web_fetch urls must not be empty".to_string(),
            summary: "No URLs to fetch".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: arguments.to_string(),
            status: "InvalidCommand".to_string(),
        };
    }
    if urls.len() > 5 {
        return RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "web_fetch supports at most 5 URLs per call".to_string(),
            summary: "Too many URLs".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: arguments.to_string(),
            status: "InvalidCommand".to_string(),
        };
    }
    let max_chars = arguments
        .get("max_chars")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(20_000)
        .clamp(1_000, 50_000);
    let max_bytes = arguments
        .get("max_bytes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(max_chars.saturating_mul(4))
        .clamp(1_024, 10_000_000);
    let mut fetched = Vec::new();
    let mut errors = Vec::new();
    for url in urls {
        let fetch_url = url.clone();
        let backend = backend.to_string();
        let result = thread::spawn(move || {
            if backend == "tinyfish" {
                fetch_tinyfish_url(&fetch_url)
            } else {
                fetch_http_url(&fetch_url, max_bytes)
            }
        })
        .join()
        .unwrap_or_else(|_| Err("web_fetch worker panicked".to_string()));
        match result {
            Ok(mut value) => {
                let content_key = if value.get("content").is_some() {
                    "content"
                } else {
                    "text"
                };
                if let Some(content) = value.get(content_key).and_then(Value::as_str) {
                    let truncated = content.chars().count() > max_chars;
                    value["content"] = Value::String(content.chars().take(max_chars).collect());
                    value["truncated"] = Value::Bool(truncated);
                    if content_key == "text" {
                        value.as_object_mut().map(|object| object.remove("text"));
                    }
                }
                fetched.push(value);
            }
            Err(error) => errors.push(json!({
                "url": url,
                "error": error
            })),
        }
    }
    let fetched_count = fetched.len();
    let error_count = errors.len();
    let content = json!({
        "fetched": fetched,
        "errors": errors
    });
    RawToolOutput {
        tool_call_id: invocation.tool_call_id.clone(),
        tool_name: invocation.name.clone(),
        is_error: error_count > 0 && fetched_count == 0,
        content: content.to_string(),
        summary: format!("Fetched {fetched_count} URLs, {error_count} failed"),
        trust_level: "untrusted".to_string(),
        command_or_url: arguments.to_string(),
        status: if error_count == 0 { "ok" } else { "partial" }.to_string(),
    }
}

pub(crate) fn fetch_tinyfish_url(url: &str) -> Result<Value, String> {
    validate_web_fetch_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .build()
        .map_err(|error| format!("build TinyFish client failed: {error}"))?;
    let body = serde_json::to_string(&json!({ "urls": [url] }))
        .map_err(|error| format!("serialize TinyFish request failed: {error}"))?;
    let response = client
        .post("https://api.fetch.tinyfish.ai")
        .header("X-API-Key", TINYFISH_API_KEY)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(|error| format!("TinyFish request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("TinyFish HTTP {}", status.as_u16()));
    }
    let body = response
        .text()
        .map_err(|error| format!("TinyFish response read failed: {error}"))?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("TinyFish response JSON parse failed: {error}"))?;
    let text = value
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("Empty result from TinyFish".to_string());
    }
    Ok(json!({
        "url": url,
        "transport": "tinyfish",
        "content_type": "text/html",
        "content": text,
        "truncated": false
    }))
}

pub(crate) fn fetch_http_url(url: &str, max_bytes: usize) -> Result<Value, String> {
    validate_web_fetch_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| format!("build web client failed: {error}"))?;
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/*, application/json;q=0.9, */*;q=0.1",
        )
        .header(reqwest::header::USER_AGENT, "Hambur/0.1")
        .send()
        .map_err(|error| format!("fetch failed: {error}"))?;
    let status = response.status().as_u16();
    let headers = response_headers_json(response.headers());
    let mut bytes = Vec::new();
    response
        .copy_to(&mut LimitedWrite::new(&mut bytes, max_bytes))
        .map_err(|error| format!("read response failed: {error}"))?;
    let truncated = bytes.len() >= max_bytes;
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(json!({
        "url": url,
        "status": status,
        "headers": headers,
        "text": body,
        "truncated": truncated
    }))
}

pub(crate) fn validate_web_fetch_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("web_fetch URL must start with http:// or https://".to_string());
    }
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    if parsed.host_str().unwrap_or_default().trim().is_empty() {
        return Err("web_fetch host must not be empty".to_string());
    }
    Ok(())
}

pub(crate) fn response_headers_json(headers: &reqwest::header::HeaderMap) -> Value {
    let values = headers
        .iter()
        .map(|(name, value)| {
            json!({
                "name": name.as_str(),
                "value": value.to_str().unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    Value::Array(values)
}

pub(crate) struct LimitedWrite<'a> {
    target: &'a mut Vec<u8>,
    limit: usize,
}

impl<'a> LimitedWrite<'a> {
    fn new(target: &'a mut Vec<u8>, limit: usize) -> Self {
        Self { target, limit }
    }
}

impl Write for LimitedWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.target.len() >= self.limit {
            return Ok(buf.len());
        }
        let remaining = self.limit - self.target.len();
        let take = remaining.min(buf.len());
        self.target.extend_from_slice(&buf[..take]);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn normalize_image_detail(detail: &str) -> &'static str {
    match detail {
        "original" => "original",
        _ => "high",
    }
}

pub(crate) fn vision_handoff_target(
    route_candidates: &[ModelRouteSnapshot],
    active_route: &ModelRouteSnapshot,
) -> Option<ModelRouteSnapshot> {
    route_candidates
        .iter()
        .find(|candidate| {
            candidate.supports_image_input
                && (candidate.provider_id != active_route.provider_id
                    || candidate.model_id != active_route.model_id)
        })
        .cloned()
}

pub(crate) fn select_view_image_handoff_route(
    active_route: &ModelRouteSnapshot,
    route_candidates: &[ModelRouteSnapshot],
    records: &[ToolExecutionRecord],
) -> Option<ModelRouteSnapshot> {
    if active_route.supports_image_input {
        return None;
    }
    let needs_handoff = records.iter().any(|record| {
        record.invocation.name == "view_image"
            && !record.result.is_error
            && serde_json::from_str::<Value>(&record.result.content_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("imageAttachedToNextRequest")
                        .and_then(Value::as_bool)
                })
                .unwrap_or(false)
    });
    if needs_handoff {
        vision_handoff_target(route_candidates, active_route)
    } else {
        None
    }
}

pub(crate) fn scripted_route_value<'a>(
    value: &'a serde_json::Value,
    route: &ModelRouteSnapshot,
) -> Option<&'a serde_json::Value> {
    let routes = value.get("routes")?.as_object()?;
    routes
        .get(&route.provider_id)
        .or_else(|| routes.get(&route.model_id))
        .or_else(|| routes.get(&route.position.to_string()))
}

pub(crate) fn split_scripted_sse(sse: &str) -> Vec<Vec<u8>> {
    sse.as_bytes()
        .chunks(13)
        .map(|chunk| chunk.to_vec())
        .collect()
}

pub(crate) fn stream_error_from_provider(code: String, message: String) -> HamburError {
    match code.as_str() {
        "Http429" | "Http5xx" | "NetworkTimeout" | "NetworkError" => {
            HamburError::ProviderUnavailable(format!("{code}: {message}"))
        }
        "CapabilityMismatch" => HamburError::CapabilityMismatch(message),
        "ModelUnavailable" => HamburError::ModelUnavailable(message),
        "SseParseError" => HamburError::SseParse(message),
        _ => HamburError::Internal(format!("{code}: {message}")),
    }
}

pub(crate) fn fallback_error_code(error: &HamburError) -> &'static str {
    let message = error.to_string();
    if message.contains("Http429:") {
        "Http429"
    } else if message.contains("Http5xx:") {
        "Http5xx"
    } else if message.contains("NetworkTimeout:") {
        "NetworkTimeout"
    } else if message.contains("NetworkError:") {
        "NetworkError"
    } else {
        error.code().as_str()
    }
}

pub(crate) fn default_models_response(model_id: &str) -> String {
    let model_id = if model_id.trim().is_empty() {
        "hambur-openai-compatible-text"
    } else {
        model_id.trim()
    };
    let supports_image_input = model_id.to_ascii_lowercase().contains("vision");
    format!(
        r#"{{"data":[{{"id":"{model_id}","display_name":"{model_id}","supports_reasoning":true,"supports_tool_call":true,"supports_image_input":{supports_image_input},"supports_structured_output":false,"supports_temperature":true,"context_limit":32000,"output_limit":4096}}]}}"#
    )
}

pub(crate) fn config_payload_value(payload_json: &str) -> Value {
    serde_json::from_str::<Value>(payload_json)
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigOperation {
    Set,
    Append,
    Remove,
}

impl ConfigOperation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Append => "append",
            Self::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HamburConfigFieldSpec {
    pub(crate) path: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) schema: &'static str,
    pub(crate) access: &'static str,
    pub(crate) risk: &'static str,
    pub(crate) revertable: bool,
    pub(crate) topic: &'static str,}

impl HamburConfigFieldSpec {
    pub(crate) fn to_json(self) -> Value {
        json!({
            "path": self.path,
            "display_name": self.display_name,
            "description": self.description,
            "schema": self.schema,
            "access": self.access,
            "risk": self.risk,
            "revertable": self.revertable
        })
    }
}

pub(crate) fn hambur_config_fields() -> Vec<HamburConfigFieldSpec> {
    const RAW: &[(&str, &str, &str, &str, &str, &str, bool)] = &[
        (
            "appearance.theme",
            "Theme",
            "App color theme.",
            "one of: system, light, dark",
            "readwrite",
            "normal",
            true,
        ),
        (
            "appearance.fontScale",
            "Font scale",
            "App text scale.",
            "one of: small, default, large, extraLarge",
            "readwrite",
            "normal",
            true,
        ),
        (
            "defaults.primaryModelGroup",
            "Primary model group",
            "Default model group used for normal chat.",
            "string model group id",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "defaults.secondaryModelGroup",
            "Secondary model group",
            "Default model group used for title generation and memory review.",
            "string model group id",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "defaults.deepThinking",
            "Deep thinking default",
            "Default deep-thinking state for new chats.",
            "bool",
            "readwrite",
            "normal",
            true,
        ),
        (
            "defaults.startupChatMode",
            "Startup chat mode",
            "Which chat to open on app start.",
            "one of: newChat, lastChat",
            "readwrite",
            "normal",
            true,
        ),
        (
            "logs.enabled",
            "Logging enabled",
            "Hambur logcat logging switch.",
            "bool",
            "readwrite",
            "normal",
            true,
        ),
        (
            "permissions.hamburConfig.enabled",
            "Allow hambur_config",
            "Native config tool availability. Currently always enabled.",
            "bool",
            "readonly",
            "destructive",
            false,
        ),
        (
            "providers",
            "LLM providers",
            "Provider summary collection. Supports append/remove.",
            "json",
            "readwrite",
            "sensitive",
            false,
        ),
        (
            "providers.<provider_id>.name",
            "Provider name",
            "User-visible provider name.",
            "string max 200 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "providers.<provider_id>.iconName",
            "Provider icon",
            "Provider icon key.",
            "string max 64 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "providers.<provider_id>.apiType",
            "Provider API type",
            "Provider API protocol.",
            "one of: openAI, gemini, anthropic",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "providers.<provider_id>.baseUrl",
            "Provider base URL",
            "API base URL.",
            "string max 1000 chars",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "providers.<provider_id>.apiKey",
            "Provider API key",
            "Provider credential. Write-only and redacted in audit.",
            "string max 10000 chars",
            "write-only",
            "destructive",
            false,
        ),
        (
            "providers.<provider_id>.enabled",
            "Provider enabled",
            "Whether provider can be used for routing.",
            "bool",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "providers.<provider_id>.selectedModel",
            "Provider selected model",
            "Provider default selected model.",
            "string",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "providers.<provider_id>.models",
            "Provider models",
            "Model ids available on this provider.",
            "[string]",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "models",
            "Model entries",
            "Flattened provider model collection. Supports append/remove.",
            "json",
            "readwrite",
            "sensitive",
            false,
        ),
        (
            "models.<entry_id>.displayName",
            "Model display name",
            "Custom display name for a provider model.",
            "string max 200 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "models.<entry_id>.notes",
            "Model notes",
            "Custom notes for a provider model.",
            "string max 1000 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "models.<entry_id>.modelId",
            "Model id",
            "API model id.",
            "string",
            "readonly",
            "normal",
            false,
        ),
        (
            "models.<entry_id>.providerId",
            "Provider id",
            "Owning provider id.",
            "string",
            "readonly",
            "normal",
            false,
        ),
        (
            "models.<entry_id>.contextWindow",
            "Context window",
            "Catalog context window when known.",
            "int|null",
            "readonly",
            "normal",
            false,
        ),
        (
            "models.<entry_id>.maxOutputTokens",
            "Max output tokens",
            "Catalog output limit when known.",
            "int|null",
            "readonly",
            "normal",
            false,
        ),
        (
            "models.<entry_id>.supportsTools",
            "Supports tools",
            "Catalog tool-call support when known.",
            "bool",
            "readonly",
            "normal",
            false,
        ),
        (
            "models.<entry_id>.supportsVision",
            "Supports vision",
            "Catalog image input support when known.",
            "bool",
            "readonly",
            "normal",
            false,
        ),
        (
            "model_groups",
            "Model groups",
            "Model routing group collection. Supports append/remove.",
            "json",
            "readwrite",
            "sensitive",
            false,
        ),
        (
            "model_groups.<group_id>.name",
            "Model group name",
            "User-visible group name.",
            "string max 200 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "model_groups.<group_id>.routingStrategy",
            "Routing strategy",
            "How to choose among group models.",
            "one of: fallback, loadBalance",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "model_groups.<group_id>.fallbackPolicy",
            "Fallback policy",
            "When to fall back to another model.",
            "one of: default, always",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "model_groups.<group_id>.models",
            "Group models",
            "Group model entries. Supports append/remove.",
            "[{provider_id, model_id}]",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "sandbox.rootfsBackend",
            "Linux sandbox backend",
            "Rootfs execution backend.",
            "one of: chroot, proot",
            "readwrite",
            "sensitive",
            true,
        ),
        (
            "startup_tasks",
            "Startup tasks",
            "App-start shell task collection. Supports append/remove.",
            "json; append object {name, script, enabled}; remove string task id",
            "readwrite",
            "destructive",
            false,
        ),
        (
            "startup_tasks.enabled",
            "Startup tasks enabled",
            "Master switch for all App-start shell tasks.",
            "bool",
            "readwrite",
            "destructive",
            true,
        ),
        (
            "startup_tasks.<task_id>.name",
            "Startup task name",
            "User-visible startup task name.",
            "string max 200 chars",
            "readwrite",
            "normal",
            true,
        ),
        (
            "startup_tasks.<task_id>.script",
            "Startup task script",
            "Shell script content executed from /var/minis/autostart on sandbox initialization.",
            "string max 200000 chars",
            "readwrite",
            "destructive",
            true,
        ),
        (
            "startup_tasks.<task_id>.enabled",
            "Startup task enabled",
            "Whether this startup task runs when the master switch is enabled.",
            "bool",
            "readwrite",
            "destructive",
            true,
        ),
        (
            "startup_tasks.<task_id>.createdAt",
            "Startup task created at",
            "Creation timestamp in epoch milliseconds.",
            "long",
            "readonly",
            "normal",
            false,
        ),
        (
            "startup_tasks.<task_id>.updatedAt",
            "Startup task updated at",
            "Update timestamp in epoch milliseconds.",
            "long",
            "readonly",
            "normal",
            false,
        ),
        (
            "startup_tasks.<task_id>.path",
            "Startup task path",
            "Sandbox .sh path.",
            "string",
            "readonly",
            "normal",
            false,
        ),
        (
            "tools.webFetchBackend",
            "Web fetch backend",
            "Backend used by web_fetch.",
            "one of: local, tinyfish",
            "readwrite",
            "normal",
            true,
        ),
        (
            "tools.viewImageScaleMode",
            "View image scale mode",
            "Image preprocessing mode for view_image.",
            "one of: resizeFit, original",
            "readwrite",
            "normal",
            true,
        ),
    ];
    RAW.iter()
        .map(
            |(path, display_name, description, schema, access, risk, revertable)| {
                HamburConfigFieldSpec {
                    path,
                    display_name,
                    description,
                    schema,
                    access,
                    risk,
                    revertable: *revertable,
                    topic: path.split('.').next().unwrap_or(""),
                }
            },
        )
        .collect()
}

pub(crate) fn hambur_config_topics() -> Vec<&'static str> {
    let mut topics = hambur_config_fields()
        .into_iter()
        .map(|field| field.topic)
        .collect::<Vec<_>>();
    topics.sort();
    topics.dedup();
    topics
}

pub(crate) fn hambur_config_field_for(path: &str) -> Option<HamburConfigFieldSpec> {
    let normalized = normalize_hambur_config_path(path);
    for field in hambur_config_fields() {
        if field.path == normalized || config_path_matches(field.path, &normalized) {
            return Some(field);
        }
    }
    None
}

pub(crate) fn config_path_matches(pattern: &str, path: &str) -> bool {
    let pattern_parts = pattern.split('.').collect::<Vec<_>>();
    let path_parts = path.split('.').collect::<Vec<_>>();
    pattern_parts.len() == path_parts.len()
        && pattern_parts
            .iter()
            .zip(path_parts.iter())
            .all(|(pattern, actual)| {
                (pattern.starts_with('<') && pattern.ends_with('>')) || pattern == actual
            })
}

pub(crate) fn normalize_hambur_config_topic(topic: &str) -> String {
    topic.trim().trim_matches('.').to_ascii_lowercase()
}

pub(crate) fn normalize_hambur_config_path(path: &str) -> String {
    path.trim().trim_matches('.').to_string()
}

pub(crate) fn parse_hambur_config_operation(path: &str) -> (String, ConfigOperation) {
    let path = normalize_hambur_config_path(path);
    if let Some(base) = path.strip_suffix(".append") {
        return (base.to_string(), ConfigOperation::Append);
    }
    if let Some(base) = path.strip_suffix(".remove") {
        return (base.to_string(), ConfigOperation::Remove);
    }
    (path, ConfigOperation::Set)
}

pub(crate) fn config_error(error: &str, reason: &str) -> Value {
    json!({
        "ok": false,
        "error": error,
        "reason": reason
    })
}

pub(crate) fn read_hambur_config_path(
    snapshot: &SettingsSnapshot,
    path: &str,
    field: &HamburConfigFieldSpec,
    filter: &str,
    page: u32,
    page_size: u32,
) -> Value {
    match path {
        "providers" => config_collection_response(
            field,
            snapshot
                .providers
                .iter()
                .map(provider_config_json)
                .collect(),
            filter,
            page,
            page_size,
        ),
        "models" => config_collection_response(
            field,
            snapshot
                .provider_models
                .iter()
                .map(model_config_json)
                .collect(),
            filter,
            page,
            page_size,
        ),
        "model_groups" => config_collection_response(
            field,
            model_group_config_json(snapshot),
            filter,
            page,
            page_size,
        ),
        "startup_tasks" => config_collection_response(
            field,
            startup_task_config_json(snapshot),
            filter,
            page,
            page_size,
        ),
        _ => {
            let value = read_hambur_config_value(snapshot, path);
            if value.is_null() {
                config_error("unknown_path", &format!("No registered field at '{path}'."))
            } else {
                json!({
                    "ok": true,
                    "value": value.to_string(),
                    "schema": field.schema,
                    "display_name": field.display_name
                })
            }
        }
    }
}

pub(crate) fn config_collection_response(
    field: &HamburConfigFieldSpec,
    items: Vec<Value>,
    filter: &str,
    page: u32,
    page_size: u32,
) -> Value {
    let terms = filter
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    let filtered = if terms.is_empty() {
        items.clone()
    } else {
        items
            .iter()
            .filter(|item| {
                let text = item.to_string().to_ascii_lowercase();
                terms.iter().all(|term| text.contains(term))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let page_size = page_size.clamp(1, 100) as usize;
    let page = page.max(1) as usize;
    let total_pages = filtered.len().div_ceil(page_size).max(1);
    let from = ((page - 1) * page_size).min(filtered.len());
    let to = (from + page_size).min(filtered.len());
    let page_items = filtered[from..to].to_vec();
    json!({
        "ok": true,
        "value": Value::Array(page_items.clone()).to_string(),
        "schema": field.schema,
        "display_name": field.display_name,
        "filtered": !terms.is_empty(),
        "filter": if terms.is_empty() { Value::Null } else { json!(filter) },
        "total": items.len(),
        "matched": filtered.len(),
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": filtered.len(),
            "total_pages": total_pages,
            "has_next": page < total_pages,
            "has_prev": page > 1
        },
        "agent_hint": if page < total_pages {
            format!("Showing page {page} of {total_pages}. To get more, use action=get path={} page={} page_size={page_size}.", field.path, page + 1)
        } else {
            format!("Showing all {} item(s) on page {page}.", page_items.len())
        }
    })
}

pub(crate) fn read_hambur_config_value(snapshot: &SettingsSnapshot, path: &str) -> Value {
    match path {
        "appearance.theme" => json!(setting_value(snapshot, "themeMode", "light")),
        "appearance.fontScale" => json!(setting_value(snapshot, "fontScale", "default")),
        "defaults.primaryModelGroup" => json!(default_group(snapshot, "primary")),
        "defaults.secondaryModelGroup" => json!(default_group(snapshot, "secondary")),
        "defaults.deepThinking" => {
            json!(setting_bool(snapshot, "defaultDeepThinkingEnabled", false))
        }
        "defaults.startupChatMode" => json!(setting_value(snapshot, "startupChatMode", "new_chat")),
        "logs.enabled" => json!(setting_bool(snapshot, "loggingEnabled", true)),
        "permissions.hamburConfig.enabled" => json!(true),
        "sandbox.rootfsBackend" => json!(setting_value(snapshot, "rootfsBackend", "chroot")),
        "startup_tasks.enabled" => json!(setting_bool(snapshot, "startupTasksEnabled", true)),
        "tools.webFetchBackend" => json!(setting_value(snapshot, "webFetchBackend", "local")),
        "tools.viewImageScaleMode" => {
            json!(setting_value(snapshot, "viewImageScaleMode", "resize_fit"))
        }
        _ => dynamic_hambur_config_value(snapshot, path),
    }
}

pub(crate) fn read_hambur_config_raw_value(snapshot: &SettingsSnapshot, path: &str) -> String {
    read_hambur_config_value(snapshot, path).to_string()
}

pub(crate) fn dynamic_hambur_config_value(snapshot: &SettingsSnapshot, path: &str) -> Value {
    let parts = path.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        ["providers", provider_id, field] => snapshot
            .providers
            .iter()
            .find(|provider| provider.id == *provider_id)
            .map(|provider| match *field {
                "name" => json!(provider.name),
                "iconName" => json!(provider.icon_name),
                "apiType" => json!(provider.api_type),
                "baseUrl" => json!(provider.base_url),
                "enabled" => json!(provider.enabled),
                "apiKey" => Value::Null,
                _ => Value::Null,
            })
            .unwrap_or(Value::Null),
        ["models", entry_id, field] => decode_model_entry_id(entry_id)
            .and_then(|(provider_id, model_id)| {
                snapshot
                    .provider_models
                    .iter()
                    .find(|model| model.provider_id == provider_id && model.model_id == model_id)
            })
            .map(|model| match *field {
                "displayName" => json!(model.display_name),
                "modelId" => json!(model.model_id),
                "providerId" => json!(model.provider_id),
                "contextWindow" => json!(model.context_limit),
                "maxOutputTokens" => json!(model.output_limit),
                "supportsTools" => json!(model.supports_tool_call),
                "supportsVision" => json!(model.supports_image_input),
                "notes" => json!(""),
                _ => Value::Null,
            })
            .unwrap_or(Value::Null),
        ["model_groups", group_id, field] => snapshot
            .model_groups
            .iter()
            .find(|group| group.id == *group_id)
            .map(|group| match *field {
                "name" => json!(group.name),
                "routingStrategy" => json!(group.routing_strategy),
                "fallbackPolicy" => json!(group.fallback_policy),
                "models" => json!(
                    snapshot
                        .model_group_members
                        .iter()
                        .filter(|member| member.group_id == *group_id)
                        .map(group_member_config_json)
                        .collect::<Vec<_>>()
                ),
                _ => Value::Null,
            })
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub(crate) fn setting_value(snapshot: &SettingsSnapshot, key: &str, fallback: &str) -> String {
    snapshot
        .settings
        .iter()
        .find(|setting| setting.key == key)
        .map(|setting| setting.value.clone())
        .unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn setting_bool(snapshot: &SettingsSnapshot, key: &str, fallback: bool) -> bool {
    setting_value(snapshot, key, if fallback { "true" } else { "false" }) == "true"
}

pub(crate) fn default_group(snapshot: &SettingsSnapshot, key: &str) -> String {
    snapshot
        .default_model_groups
        .iter()
        .find(|default| default.key == key)
        .map(|default| default.group_id.clone())
        .unwrap_or_default()
}

pub(crate) fn app_setting_for_hambur_config_path(path: &str, value_json: &str) -> Option<(String, String)> {
    let value = parse_config_literal(value_json);
    let string_value = config_literal_string(&value);
    let mapped = match path {
        "appearance.theme" => ("themeMode", normalize_theme_value(&string_value)),
        "appearance.fontScale" => ("fontScale", normalize_font_scale_value(&string_value)),
        "defaults.deepThinking" => ("defaultDeepThinkingEnabled", string_value),
        "defaults.startupChatMode" => (
            "startupChatMode",
            normalize_startup_chat_value(&string_value),
        ),
        "logs.enabled" => ("loggingEnabled", string_value),
        "sandbox.rootfsBackend" => ("rootfsBackend", string_value),
        "startup_tasks.enabled" => ("startupTasksEnabled", string_value),
        "tools.webFetchBackend" => (
            "webFetchBackend",
            normalize_web_fetch_backend_value(&string_value),
        ),
        "tools.viewImageScaleMode" => (
            "viewImageScaleMode",
            normalize_view_image_scale_value(&string_value),
        ),
        _ => return None,
    };
    Some((mapped.0.to_string(), mapped.1))
}

pub(crate) fn parse_config_literal(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw.trim()).unwrap_or_else(|_| json!(raw.trim()))
}

pub(crate) fn config_literal_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

pub(crate) fn normalize_theme_value(value: &str) -> String {
    match value {
        "system" | "light" | "dark" => value.to_string(),
        "SYSTEM" => "system".to_string(),
        "LIGHT" => "light".to_string(),
        "DARK" => "dark".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn normalize_font_scale_value(value: &str) -> String {
    match value {
        "extraLarge" => "extra_large".to_string(),
        "SMALL" => "small".to_string(),
        "DEFAULT" => "default".to_string(),
        "LARGE" => "large".to_string(),
        "EXTRA_LARGE" => "extra_large".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn normalize_startup_chat_value(value: &str) -> String {
    match value {
        "newChat" => "new_chat".to_string(),
        "lastChat" => "last_chat".to_string(),
        "NEW_CHAT" => "new_chat".to_string(),
        "LAST_CHAT" => "last_chat".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn normalize_web_fetch_backend_value(value: &str) -> String {
    match value {
        "LOCAL" => "local".to_string(),
        "TINYFISH" => "tinyfish".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn normalize_view_image_scale_value(value: &str) -> String {
    match value {
        "resizeFit" | "RESIZE_FIT" => "resize_fit".to_string(),
        "ORIGINAL" => "original".to_string(),
        _ => value.to_string(),
    }
}

pub(crate) fn delegate_task_prompt(
    arguments: &Value,
    task_value: Option<&Value>,
    task_index: usize,
    task_count: usize,
) -> String {
    let source = task_value.unwrap_or(arguments);
    let explicit_task = source
        .get("task")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("task").and_then(Value::as_str))
        .unwrap_or_default()
        .trim();
    if !explicit_task.is_empty() {
        return explicit_task.to_string();
    }

    let goal = source
        .get("goal")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("goal").and_then(Value::as_str))
        .unwrap_or_default()
        .trim();
    let context = source
        .get("context")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("context").and_then(Value::as_str))
        .unwrap_or_default()
        .trim();
    if goal.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    if task_count > 1 {
        lines.push(format!("Batch task {} of {}", task_index + 1, task_count));
        lines.push(String::new());
    }
    lines.push("Goal:".to_string());
    lines.push(goal.to_string());
    if !context.is_empty() {
        lines.push(String::new());
        lines.push("Context:".to_string());
        lines.push(context.to_string());
    }
    lines.join("\n")
}

pub(crate) fn provider_config_json(provider: &hambur_db::PublicProviderRecord) -> Value {
    json!({
        "id": provider.id,
        "name": provider.name,
        "label": provider.name,
        "api_type": provider.api_type,
        "providerType": provider.api_type,
        "base_url": provider.base_url,
        "enabled": provider.enabled,
        "isEnabled": provider.enabled,
        "selected_model": "",
        "model_count": 0
    })
}

pub(crate) fn model_config_json(model: &hambur_db::ProviderModelRecord) -> Value {
    json!({
        "entry_id": encode_model_entry_id(&model.provider_id, &model.model_id),
        "display_name": model.display_name,
        "model_id": model.model_id,
        "provider_id": model.provider_id,
        "provider_label": "",
        "provider_type": "",
        "context_window": model.context_limit,
        "max_output_tokens": model.output_limit,
        "supports_tools": model.supports_tool_call,
        "supports_vision": model.supports_image_input,
        "input_modalities": if model.supports_image_input { json!(["text", "image"]) } else { json!(["text"]) },
        "output_modalities": json!(["text"])
    })
}

pub(crate) fn model_group_config_json(snapshot: &SettingsSnapshot) -> Vec<Value> {
    snapshot
        .model_groups
        .iter()
        .map(|group| {
            json!({
                "id": group.id,
                "name": group.name,
                "routing_strategy": group.routing_strategy,
                "fallback_policy": group.fallback_policy,
                "models": snapshot
                    .model_group_members
                    .iter()
                    .filter(|member| member.group_id == group.id)
                    .map(group_member_config_json)
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

pub(crate) fn group_member_config_json(member: &hambur_db::ModelGroupMemberRecord) -> Value {
    json!({
        "id": member.id,
        "provider_id": member.provider_id,
        "provider_label": member.provider_name,
        "model_id": member.model_id,
        "missing": false
    })
}

pub(crate) fn startup_task_config_json(snapshot: &SettingsSnapshot) -> Vec<Value> {
    snapshot
        .settings
        .iter()
        .filter(|setting| setting.key.starts_with("startup_task:"))
        .map(|setting| {
            json!({
                "id": setting.key.trim_start_matches("startup_task:"),
                "name": setting.key.trim_start_matches("startup_task:"),
                "enabled": true,
                "created_at": setting.updated_at_ms,
                "updated_at": setting.updated_at_ms,
                "path": format!("/var/minis/autostart/{}.sh", setting.key.trim_start_matches("startup_task:")),
                "script_preview": setting.value.lines().take(4).collect::<Vec<_>>().join("\n").chars().take(400).collect::<String>(),
                "script_size": setting.value.len()
            })
        })
        .collect()
}

pub(crate) fn config_audit_json(entry: &hambur_db::ConfigAuditRecord) -> Value {
    json!({
        "id": entry.id,
        "at": entry.created_at_ms,
        "actor": entry.actor,
        "action": entry.action,
        "scope": normalize_hambur_config_topic(&entry.target_kind),
        "key": entry.target_id,
        "old": "",
        "new": entry.redacted_summary,
        "status": "applied",
        "confirmed_at": entry.created_at_ms,
        "caption": entry.redacted_summary
    })
}

pub(crate) fn encode_model_entry_id(provider_id: &str, model_id: &str) -> String {
    format!(
        "{}__{}",
        provider_id.replace('_', "_u").replace('/', "_s"),
        model_id.replace('_', "_u").replace('/', "_s")
    )
}

pub(crate) fn decode_model_entry_id(entry_id: &str) -> Option<(String, String)> {
    let (provider_id, model_id) = entry_id.split_once("__")?;
    Some((
        provider_id.replace("_s", "/").replace("_u", "_"),
        model_id.replace("_s", "/").replace("_u", "_"),
    ))
}

pub(crate) fn config_payload_string(payload_json: &str, key: &str) -> String {
    let value = config_payload_value(payload_json);
    config_string(&value, key)
}

pub(crate) fn config_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub(crate) fn config_value_string(value: &Value, key: &str) -> String {
    let Some(child) = value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
    else {
        return String::new();
    };
    match child {
        Value::String(text) => text.trim().to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => child.to_string(),
    }
}

pub(crate) fn config_bool(value: &Value, key: &str, fallback: bool) -> bool {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_bool)
        .unwrap_or(fallback)
}

pub(crate) fn config_payload_bool(payload_json: &str, key: &str, fallback: bool) -> bool {
    let value = config_payload_value(payload_json);
    config_bool(&value, key, fallback)
}

pub(crate) fn config_u32(value: &Value, key: &str, fallback: u32) -> u32 {
    value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback)
}

pub(crate) fn argument_seconds_or_ms(
    value: &Value,
    seconds_key: &str,
    millis_key: &str,
    fallback_ms: u64,
) -> u64 {
    if let Some(seconds) = value.get(seconds_key).and_then(Value::as_u64) {
        return seconds.saturating_mul(1_000);
    }
    value
        .get(millis_key)
        .and_then(Value::as_u64)
        .unwrap_or(fallback_ms)
}

pub(crate) fn config_object_string(value: &Value, key: &str) -> String {
    let Some(child) = value
        .get(key)
        .or_else(|| value.get(to_snake_key(key).as_str()))
    else {
        return "{}".to_string();
    };
    if let Some(text) = child.as_str() {
        if text.trim().is_empty() {
            "{}".to_string()
        } else {
            text.to_string()
        }
    } else {
        child.to_string()
    }
}

pub(crate) fn to_snake_key(key: &str) -> String {
    let mut output = String::new();
    for (index, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

pub(crate) fn provider_secret_ref_from_payload(payload_json: &str) -> String {
    let parsed = config_payload_value(payload_json);
    config_string(&parsed, "secretRef")
        .if_blank(config_string(&parsed, "secret_ref"))
        .if_blank(payload_json.trim().to_string())
}

pub(crate) fn redacted_secret_label(secret_ref: &str) -> &'static str {
    if secret_ref.starts_with("android-secret://") {
        "android-secret"
    } else if secret_ref.starts_with("env://") {
        "env"
    } else {
        "secret-ref"
    }
}

pub(crate) fn approval_token_from_payload(payload_json: &str) -> String {
    config_payload_string(payload_json, "approvalToken")
}

pub(crate) fn require_approval(command: &RuntimeCommand, scope: &str) -> HamburResult<()> {
    let token = approval_token_from_payload(&command.payload_json);
    let expected = approval_tokens_for_scope(scope);
    if expected.contains(&token) {
        Ok(())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "{scope} requires approval token: {}",
            expected.join(" or ")
        )))
    }
}

pub(crate) fn approval_tokens_for_scope(scope: &str) -> Vec<String> {
    let mut tokens = vec![format!("approve:{scope}")];
    if scope.starts_with("rootfs_setting:") {
        tokens.push("approve:rootfs_settings".to_string());
    }
    if scope.starts_with("startup_task:") {
        tokens.push("approve:startup_tasks".to_string());
    }
    tokens
}

pub(crate) fn setting_key_for_command(command: &RuntimeCommand) -> HamburResult<String> {
    let payload = config_payload_value(&command.payload_json);
    let key = match command.kind.as_str() {
        "UpdateToolSettings" => "tool_settings".to_string(),
        "UpdateSkills" => "skills".to_string(),
        "UpdateMemoryProjections" => "memory_projections".to_string(),
        "UpdateStartupTasks" => "startup_tasks".to_string(),
        "UpdateRootfsSettings" => "rootfs_settings".to_string(),
        "UpdateAppearance" => "appearance".to_string(),
        "UpdateLogs" => "logs".to_string(),
        "UpdateTokenUsage" => "token_usage".to_string(),
        "UpdatePersona" => "persona".to_string(),
        "UpdateEnvironmentVariables" => "environment_variables".to_string(),
        "UpdateBrowserToolSettings" => "browser_tool_settings".to_string(),
        "UpdateAppSetting" => command
            .chunk
            .clone()
            .if_blank(config_string(&payload, "settingKey"))
            .if_blank(config_string(&payload, "key")),
        "UpdateSkillEnabled" => {
            let skill_id = command
                .message_id
                .clone()
                .if_blank(config_string(&payload, "skillId"))
                .if_blank(config_string(&payload, "skillPath"));
            format!("skill_enabled:{skill_id}")
        }
        "UpdateStartupTask" | "DeleteStartupTask" => {
            let task_id = command
                .message_id
                .clone()
                .if_blank(config_string(&payload, "startupTaskId"))
                .if_blank(config_string(&payload, "taskId"))
                .if_blank(config_string(&payload, "id"));
            format!("startup_task:{task_id}")
        }
        "UpdateRootfsSetting" => {
            let rootfs_key = command
                .chunk
                .clone()
                .if_blank(config_string(&payload, "settingKey"))
                .if_blank(config_string(&payload, "key"));
            format!("rootfs_setting:{rootfs_key}")
        }
        _ => {
            return Err(HamburError::InvalidCommand(format!(
                "unsupported setting command kind: {}",
                command.kind
            )));
        }
    };
    if key.trim().is_empty()
        || key.ends_with(':')
        || matches!(
            key.as_str(),
            "skill_enabled:" | "startup_task:" | "rootfs_setting:"
        )
    {
        return Err(HamburError::InvalidCommand(
            "setting key must not be empty".to_string(),
        ));
    }
    Ok(key)
}

pub(crate) fn setting_value_for_command(command: &RuntimeCommand) -> String {
    let payload = config_payload_value(&command.payload_json);
    match command.kind.as_str() {
        "UpdateAppSetting" => command
            .content
            .clone()
            .if_blank(config_value_string(&payload, "value"))
            .if_blank(command.payload_json.clone()),
        "UpdateSkillEnabled" => config_bool(&payload, "enabled", true).to_string(),
        "DeleteStartupTask" => String::new(),
        "UpdateRootfsSetting" => command
            .content
            .clone()
            .if_blank(config_value_string(&payload, "value"))
            .if_blank(command.payload_json.clone()),
        _ => command.payload_json.clone(),
    }
}

pub(crate) fn setting_audit_summary(command_kind: &str, setting_key: &str) -> String {
    match command_kind {
        "DeleteStartupTask" => format!("Setting '{setting_key}' deleted"),
        _ => format!("Setting '{setting_key}' updated"),
    }
}

pub(crate) fn setting_requires_approval(setting_key: &str) -> bool {
    setting_key == "rootfs_settings"
        || setting_key == "startup_tasks"
        || setting_key.starts_with("startup_task:")
        || setting_key.starts_with("rootfs_setting:")
}

pub(crate) trait IfBlank {
    fn if_blank(self, fallback: String) -> String;
}

impl IfBlank for String {
    fn if_blank(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

pub(crate) fn is_default_session_title(title: &str) -> bool {
    matches!(
        title.trim(),
        "" | "New chat" | "新对话" | "Untitled session"
    )
}

pub(crate) fn title_from_first_user_message(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(32)
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn accepted_ack(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
) -> RuntimeCommandAck {
    RuntimeCommandAck {
        command_id: command_id.into(),
        idempotency_key: idempotency_key.into(),
        accepted: true,
        duplicate: false,
        rejection_code: String::new(),
        message: String::new(),
    }
}

pub(crate) fn rejected_ack(
    command_id: impl Into<String>,
    idempotency_key: impl Into<String>,
    error: HamburError,
) -> RuntimeCommandAck {
    RuntimeCommandAck {
        command_id: command_id.into(),
        idempotency_key: idempotency_key.into(),
        accepted: false,
        duplicate: false,
        rejection_code: error.code().as_str().to_string(),
        message: error.to_string(),
    }
}

pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            size += dir_size(&entry.path());
        }
    }
    size
}

pub(crate) fn get_rootfs_backend(settings: &[hambur_db::AppSettingRecord]) -> &str {
    for s in settings {
        if s.key == "rootfsBackend" || s.key == "rootfs_setting:rootfsBackend" {
            let val = s.value.trim_matches('"');
            if val == "chroot" || val == "proot" {
                return val;
            }
        }
    }
    "proot"
}
