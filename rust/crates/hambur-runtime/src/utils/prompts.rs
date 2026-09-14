use crate::*;

pub(crate) const HAMBUR_FILE_LINK_SYSTEM_PROMPT: &str = r#"Hambur can render local file links sent in Markdown image syntax.
To send any file to the user, write `![title](/var/hambur/PATH)`, where the destination is the absolute sandbox path, for example `![report.pdf](/var/hambur/workspace/report.pdf)` or `![chart.png](/var/hambur/workspace/chart.png)`.
Images, audio, and video render inline in the conversation. Other file types open a preview page with share and download actions.
Always use Markdown image syntax with the leading exclamation mark: `![title](/var/hambur/...)`.
Use a clear title as the visible link text. Do not use this for normal web links.

Unified sandbox paths:
- All tools (read_file, write_file, patch, view_image, search_files) strictly require absolute sandbox paths starting with `/var/hambur/` (e.g. `/var/hambur/workspace/`, `/var/hambur/download/`, `/var/hambur/attachments/uploads/`, `/var/hambur/browser/`, `/var/hambur/shared/`). Do not use relative paths or custom URL schemes like hambur://.
- User uploaded files and images are stored under `/var/hambur/attachments/uploads/`.
- The directory `/var/hambur/shared/` is a shared folder that can be read and written across chat sessions.
- When the user uploads attachments/images, their sandbox paths are supplied at the end of the user message inside a `<user_attach_files>path1,path2,...</user_attach_files>` tag. Use these paths with `view_image` or other tools to access the files."#;

pub(crate) const HAMBUR_CONFIG_SYSTEM_PROMPT: &str = r#"Use the `hambur_config` tool to inspect or change Hambur app settings, providers, models, model groups, default routing, startup tasks, tool options, network toggles, sandbox backend, and logging.
Do not edit Android preference files or use terminal commands for Hambur app configuration. Start with `action=list_topics` or `action=topic_help` if you need to discover available config paths."#;

pub(crate) fn format_beijing_timestamp_with_weekday(now_ms: u64) -> String {
    // Beijing Time is UTC+8: add 8 hours (8 * 3600 seconds = 28,800 seconds)
    let beijing_secs = (now_ms / 1000).saturating_add(8 * 3600);
    let day_secs = beijing_secs % 86400;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = beijing_secs % 60;

    let days = beijing_secs / 86400;
    let (weekday_en, weekday_zh) = match (days + 4) % 7 {
        0 => ("Sunday", "星期日"),
        1 => ("Monday", "星期一"),
        2 => ("Tuesday", "星期二"),
        3 => ("Wednesday", "星期三"),
        4 => ("Thursday", "星期四"),
        5 => ("Friday", "星期五"),
        _ => ("Saturday", "星期六"),
    };

    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {hour:02}:{minute:02}:{second:02} (UTC+8 / 北京时间), {weekday_en} ({weekday_zh})")
}

pub(crate) fn format_user_content_with_prefix(
    prompt_prefix: &str,
    content: &str,
    attachments: &[AttachmentRecord],
) -> String {
    let trimmed = content.trim();
    let mut formatted = if prompt_prefix.trim().is_empty() {
        trimmed.to_string()
    } else {
        format!("{prompt_prefix}\n\n{trimmed}")
    };
    if !attachments.is_empty() {
        formatted.push_str("\n\nAttachments:");
        for attachment in attachments {
            if attachment.kind == "image" {
                formatted.push_str(&format!(
                    "\n- 用户附加了一张图片：{}，大小：{} bytes。路径：{}。需要查看时请调用 view_image。",
                    attachment.display_name, attachment.byte_size, attachment.sandbox_path
                ));
            } else {
                formatted.push_str(&format!(
                    "\n- 用户附加了文件：{}，大小：{} bytes。路径：{}。需要查看时请使用文件工具读取。",
                    attachment.display_name, attachment.byte_size, attachment.sandbox_path
                ));
            }
        }
    }
    formatted
}

pub(crate) fn format_user_content_for_model(
    content: &str,
    attachments: &[AttachmentRecord],
    now_ms: u64,
) -> String {
    let time_str = format_beijing_timestamp_with_weekday(now_ms);
    let prefix = format!("[Current Time: {time_str}]");
    format_user_content_with_prefix(&prefix, content, attachments)
}

#[allow(dead_code)]
pub(crate) fn format_user_content_with_attachments(
    content: &str,
    attachments: &[AttachmentRecord],
) -> String {
    format_user_content_for_model(content, attachments, now_ms())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_beijing_timestamp_with_weekday() {
        assert_eq!(
            format_beijing_timestamp_with_weekday(0),
            "1970-01-01 08:00:00 (UTC+8 / 北京时间), Thursday (星期四)"
        );
        assert_eq!(
            format_beijing_timestamp_with_weekday(1704067200000),
            "2024-01-01 08:00:00 (UTC+8 / 北京时间), Monday (星期一)"
        );
    }

    #[test]
    fn test_format_user_content_for_model() {
        let content = "Hello world";
        let formatted = format_user_content_for_model(content, &[], 0);
        assert_eq!(
            formatted,
            "[Current Time: 1970-01-01 08:00:00 (UTC+8 / 北京时间), Thursday (星期四)]\n\nHello world"
        );
    }
}
