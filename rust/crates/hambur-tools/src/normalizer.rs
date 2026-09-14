//! Turns raw tool output into a bounded, trust-labelled `ToolResult`, offloading large
//! payloads to files inside the session sandbox.

use std::fs;
use std::path::PathBuf;

use hambur_core::{HamburError, HamburResult, new_id};
use serde_json::json;

use crate::invocation::{RawToolOutput, ToolResult};

pub const DEFAULT_LARGE_RESULT_THRESHOLD_BYTES: usize = 16 * 1024;
const PREVIEW_BYTES: usize = 2048;

#[derive(Debug, Clone)]
pub struct ToolResultNormalizer {
    app_files_dir: Option<PathBuf>,
    offload_dir: PathBuf,
    sandbox_offload_dir: String,
    large_result_threshold_bytes: usize,
}

impl ToolResultNormalizer {
    pub fn new(offload_dir: PathBuf) -> HamburResult<Self> {
        fs::create_dir_all(&offload_dir).map_err(|error| {
            HamburError::Internal(format!("create tool offload directory: {error}"))
        })?;
        Ok(Self {
            app_files_dir: None,
            offload_dir,
            sandbox_offload_dir: "/var/hambur/offloads".to_string(),
            large_result_threshold_bytes: DEFAULT_LARGE_RESULT_THRESHOLD_BYTES,
        })
    }

    pub fn with_app_files_dir(mut self, dir: PathBuf) -> Self {
        self.app_files_dir = Some(dir);
        self
    }

    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.large_result_threshold_bytes = threshold.max(512);
        self
    }

    pub fn normalize(&self, raw: RawToolOutput) -> HamburResult<ToolResult> {
        self.normalize_with_session(raw, None)
    }

    pub fn normalize_with_session(
        &self,
        raw: RawToolOutput,
        session_id: Option<&str>,
    ) -> HamburResult<ToolResult> {
        let bytes = raw.content.len();
        let untrusted = raw.trust_level == "untrusted";
        let wrapped_content = if untrusted {
            wrap_untrusted(&raw.tool_name, &raw.tool_call_id, &raw.content)
        } else {
            raw.content.clone()
        };

        if bytes > self.large_result_threshold_bytes {
            let offload_file_id = new_id("offload");
            let file_name = format!("{offload_file_id}.txt");
            let host_path = self.offload_dir.join(&file_name);
            fs::write(&host_path, raw.content.as_bytes()).map_err(|error| {
                HamburError::Internal(format!("write tool offload file: {error}"))
            })?;

            let mut final_host_path = host_path.clone();
            if let (Some(app_files), Some(sid)) = (&self.app_files_dir, session_id) {
                let target_dirs = [
                    app_files.join("sandbox").join("sessions").join(sid).join("offloads"),
                    app_files.join("sessions").join(sid).join("offloads"),
                    app_files.join("sandbox").join("offloads"),
                ];
                for dir in &target_dirs {
                    if fs::create_dir_all(dir).is_ok() {
                        let file = dir.join(&file_name);
                        if fs::write(&file, raw.content.as_bytes()).is_ok() && dir.to_string_lossy().contains("sandbox") {
                            final_host_path = file;
                        }
                    }
                }
            }

            let sandbox_path = format!("{}/{}", self.sandbox_offload_dir, file_name);
            let context_stub = large_result_stub(
                &raw.tool_name,
                &raw.command_or_url,
                &raw.status,
                &sandbox_path,
                bytes,
                &raw.content,
            );
            Ok(ToolResult {
                tool_call_id: raw.tool_call_id,
                tool_name: raw.tool_name,
                is_error: raw.is_error,
                content_json: json!({
                    "summary": raw.summary,
                    "offloaded_path": sandbox_path,
                    "bytes": bytes
                })
                .to_string(),
                summary: raw.summary,
                artifacts_json: "[]".to_string(),
                trust_level: raw.trust_level,
                truncated: true,
                offloaded_file_id: offload_file_id,
                offloaded_path: final_host_path.to_string_lossy().to_string(),
                context_stub,
            })
        } else {
            Ok(ToolResult {
                tool_call_id: raw.tool_call_id,
                tool_name: raw.tool_name,
                is_error: raw.is_error,
                content_json: json!({"text": raw.content, "bytes": bytes}).to_string(),
                summary: raw.summary,
                artifacts_json: "[]".to_string(),
                trust_level: raw.trust_level,
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: wrapped_content,
            })
        }
    }
}

fn wrap_untrusted(tool_name: &str, tool_call_id: &str, content: &str) -> String {
    for _ in 0..16 {
        let nonce = new_id("nonce").replace('-', "_");
        let begin = format!("BEGIN_UNTRUSTED_DATA_{nonce}");
        let end = format!("END_UNTRUSTED_DATA_{nonce}");
        if !content.contains(&begin) && !content.contains(&end) {
            return format!(
                "<untrusted_tool_result source=\"{tool_name}\" tool_call_id=\"{tool_call_id}\">\n{begin}\n{content}\n{end}\n</untrusted_tool_result>"
            );
        }
    }
    format!(
        "<untrusted_tool_result source=\"{tool_name}\" tool_call_id=\"{tool_call_id}\">\n{content}\n</untrusted_tool_result>"
    )
}

fn large_result_stub(
    tool_name: &str,
    command_or_url: &str,
    status: &str,
    sandbox_path: &str,
    bytes: usize,
    content: &str,
) -> String {
    let head = preview_head(content, PREVIEW_BYTES);
    let tail = preview_tail(content, PREVIEW_BYTES);
    format!(
        "[Tool output truncated. Full output saved to: {sandbox_path}]\n\
tool={tool_name}\n\
status={status}\n\
action={command_or_url}\n\
bytes={bytes}\n\
truncated=true\n\n\
--- HEAD ---\n{head}\n\n\
--- TAIL ---\n{tail}"
    )
}

fn preview_head(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    trim_to_char_boundary(content, max_bytes).to_string()
}

fn preview_tail(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let mut start = content.len().saturating_sub(max_bytes);
    while !content.is_char_boundary(start) && start < content.len() {
        start += 1;
    }
    content[start..].to_string()
}

fn trim_to_char_boundary(content: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(content.len());
    while !content.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &content[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use hambur_core::now_ms;

    #[test]
    fn test_normalizer_session_offload() {
        let temp_dir = std::env::temp_dir().join(format!("hambur_test_{}", now_ms()));
        let offload_dir = temp_dir.join("offloads");
        let normalizer = ToolResultNormalizer::new(offload_dir.clone())
            .unwrap()
            .with_app_files_dir(temp_dir.clone())
            .with_threshold(100);

        let large_content = "A".repeat(1000);
        let raw = RawToolOutput {
            tool_call_id: "call_123".to_string(),
            tool_name: "web_search".to_string(),
            is_error: false,
            content: large_content.clone(),
            summary: "Fetched large url".to_string(),
            trust_level: "untrusted".to_string(),
            command_or_url: "http://example.com".to_string(),
            status: "ok".to_string(),
        };

        let result = normalizer.normalize_with_session(raw, Some("test_session")).unwrap();
        assert!(result.truncated);
        assert!(!result.offloaded_file_id.is_empty());

        let expected_sandbox_offload = temp_dir
            .join("sandbox")
            .join("sessions")
            .join("test_session")
            .join("offloads")
            .join(format!("{}.txt", result.offloaded_file_id));

        assert!(expected_sandbox_offload.exists(), "File must exist in sandbox session offload dir");
        let saved_content = fs::read_to_string(&expected_sandbox_offload).unwrap();
        assert_eq!(saved_content, large_content);

        // cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
