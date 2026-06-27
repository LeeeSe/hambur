use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hambur_core::{HamburError, HamburResult, new_id, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub const MAX_TOOL_ITERATIONS_PER_TURN: u32 = 64;
pub const MAX_PARALLEL_TOOL_CALLS: usize = 3;
pub const DEFAULT_LARGE_RESULT_THRESHOLD_BYTES: usize = 16 * 1024;
const PREVIEW_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters_json_schema: Value,
}

const MAIN_OPENAI_TOOL_NAMES: &[&str] = &[
    "get_current_time",
    "skills_list",
    "skill_view",
    "terminal",
    "process",
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "hambur_config",
    "web_search",
    "web_fetch",
    "browser_use",
    "session_search",
    "memory",
    "delegate_task",
    "view_image",
];

const DELEGATE_OPENAI_TOOL_NAMES: &[&str] = &[
    "get_current_time",
    "skills_list",
    "skill_view",
    "terminal",
    "process",
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "hambur_config",
    "web_search",
    "web_fetch",
    "browser_use",
    "session_search",
    "memory",
    "view_image",
    "submit_delegate_result",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSchemaCompiler {
    schemas: BTreeMap<String, ToolSchema>,
}

impl ToolSchemaCompiler {
    pub fn with_builtin_tools() -> HamburResult<Self> {
        let mut compiler = Self::default();
        compiler.register(ToolSchema {
            name: "get_current_time".to_string(),
            description: "Get the current date and time from the user's device.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "session_search".to_string(),
            description: "Search past chat sessions stored locally on this phone, or read/scroll inside one. Calling shapes: (1) pass query for discovery; (2) pass session_id + around_message_id to scroll around a message; (3) pass session_id only to read a session; (4) pass no args to browse recent sessions. Use this for questions like what did we discuss about X, where did we leave Y, or find the session where Z.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query for discovery. Omit to browse recent sessions. Ignored when session_id + around_message_id are set."},
                    "limit": {"type": "integer", "description": "Max sessions to return. Default 3, max 10.", "default": 3},
                    "sort": {"type": "string", "enum": ["newest", "oldest"], "description": "Optional temporal bias for discovery results."},
                    "session_id": {"type": "string", "description": "Session to read or scroll inside. Use a session_id returned from discovery or browse."},
                    "around_message_id": {"type": "string", "description": "Message id to center the scroll window on. To scroll forward pass the last window message id; to scroll backward pass the first."},
                    "window": {"type": "integer", "description": "Messages to return on each side of around_message_id. Default 5, max 20.", "default": 5},
                    "role_filter": {"type": "string", "description": "Optional comma-separated roles to include, e.g. 'user,assistant'."}
                },
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "echo".to_string(),
            description: "Echo text for deterministic local testing.".to_string(),
            parameters_json_schema: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        })?;
        compiler.register(ToolSchema {
            name: "view_image".to_string(),
            description: "View a local image file from the sandbox. Returns detail (high/original), width, height, and resolved path. The app attaches the image to the next model request as image_url. Relative paths resolve under /var/hambur/workspace.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to the image file, absolute or relative to /var/hambur/workspace."},
                    "detail": {
                        "type": "string",
                        "enum": ["high", "original"],
                        "description": "Optional. Detail mode for the image. If 'high', resizes to fit max 1024px. If 'original', returns full resolution. Omit to use the user's default setting."
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "read_file".to_string(),
            description: "Read a text file with line numbers and pagination. Use this instead of cat/head/tail in terminal. Output format is 'LINE_NUM|CONTENT'. Use offset and limit for large files. Relative paths resolve under /var/hambur/workspace. Cannot read images or binary files.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to the file to read, absolute or relative to /var/hambur/workspace."},
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from. 1-indexed, default 1.",
                        "default": 1,
                        "minimum": 1
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read. Default 500, max 2000.",
                        "default": 500,
                        "maximum": 2000
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "write_file".to_string(),
            description: "Write content to a file, completely replacing existing content. Use this instead of echo/cat heredoc in terminal. Creates parent directories automatically. Use patch for targeted edits. Relative paths resolve under /var/hambur/workspace.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to write."},
                    "content": {"type": "string", "description": "Complete content to write to the file."}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "patch".to_string(),
            description: "Targeted find-and-replace edits in files. Use this instead of sed/awk in terminal. REPLACE MODE (mode='replace', default): find a unique old_string and replace it with new_string. Include surrounding context lines to ensure uniqueness. PATCH MODE (mode='patch') is reserved for multi-file patches and is not implemented yet on this phone agent; use replace mode or write_file instead.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["replace", "patch"],
                        "description": "Edit mode. 'replace' requires path + old_string + new_string. 'patch' is not implemented yet in this app.",
                        "default": "replace"
                    },
                    "path": {"type": "string", "description": "Required when mode='replace'. File path to edit."},
                    "old_string": {"type": "string", "description": "Required when mode='replace'. Exact text to find and replace. Must be unique unless replace_all=true. Include surrounding context lines."},
                    "new_string": {"type": "string", "description": "Required when mode='replace'. Replacement text. Pass empty string to delete the matched text."},
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences instead of requiring a unique match. Defaults to false.",
                        "default": false
                    },
                    "patch": {"type": "string", "description": "Reserved for V4A multi-file patch content. Not implemented yet on this phone agent."}
                },
                "required": ["mode"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "search_files".to_string(),
            description: "Search file contents or find files by name. Use this instead of grep/rg/find/ls in terminal. Content search (target='content') treats pattern as a regex when possible and returns matching lines with line numbers. File search (target='files') treats pattern as a glob or filename fragment. Relative paths resolve under /var/hambur/workspace.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern for content search, or glob/filename pattern for file search."},
                    "target": {
                        "type": "string",
                        "enum": ["content", "files"],
                        "description": "'content' searches inside file contents, 'files' searches for files by name.",
                        "default": "content"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search in. Defaults to /var/hambur/workspace.",
                        "default": "/var/hambur/workspace"
                    },
                    "file_glob": {"type": "string", "description": "Filter files by glob pattern in content mode, e.g. '*.kt'."},
                    "limit": {"type": "integer", "description": "Maximum number of results to return. Default 50.", "default": 50},
                    "offset": {"type": "integer", "description": "Skip first N results for pagination. Default 0.", "default": 0},
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files_only", "count"],
                        "description": "For content mode: 'content' shows matching lines, 'files_only' lists file paths, 'count' shows match counts per file.",
                        "default": "content"
                    },
                    "context": {"type": "integer", "description": "Number of context lines before and after each match. Default 0.", "default": 0}
                },
                "required": ["pattern"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "terminal".to_string(),
            description: "Run a shell command inside the user's Linux sandbox. Use this for commands, package managers, builds, tests, and one-off shell work. Prefer read_file/write_file/patch/search_files for file operations. Short commands should run in the foreground with a generous timeout; long-running servers, watchers, and jobs should use background=true and then be managed with the process tool. The default working directory is /var/hambur/workspace.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to execute inside the Linux sandbox."},
                    "background": {
                        "type": "boolean",
                        "description": "Run the command in the background and return a process_session_id. Use process(action='poll'|'log'|'wait'|'kill') afterwards. Use this for servers, watchers, and long-running bounded tasks.",
                        "default": false
                    },
                    "timeout": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 300,
                        "description": "Max seconds to wait for a foreground command. Returns as soon as the command finishes. Defaults to 30; max 300. Use background=true for longer work."
                    },
                    "workdir": {"type": "string", "description": "Absolute working directory inside the sandbox. Defaults to /var/hambur/workspace."}
                },
                "required": ["command"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "process".to_string(),
            description: "Manage background processes started with terminal(background=true). Actions: list, poll, log, wait, kill, write, submit, close.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "poll", "log", "wait", "kill", "write", "submit", "close"],
                        "description": "Action to perform."
                    },
                    "session_id": {"type": "string", "description": "Process session ID returned by terminal(background=true). Required except for list."},
                    "data": {"type": "string", "description": "Text to send to stdin for write or submit."},
                    "timeout": {"type": "integer", "minimum": 1, "description": "Seconds to wait for wait action."},
                    "offset": {"type": "integer", "description": "Line offset for log action. Omit or use 0 for latest lines."},
                    "limit": {"type": "integer", "minimum": 1, "description": "Maximum lines for log action."}
                },
                "required": ["action"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "hambur_config".to_string(),
            description: "Read and update Hambur app configuration as a native tool. Use this instead of editing app prefs or running shell commands. Supports provider/model/model-group/default/tool/appearance/log/network settings plus audit history and revert.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list_topics",
                            "topic_help",
                            "get",
                            "set",
                            "append",
                            "remove",
                            "set_batch",
                            "audit_list",
                            "audit_get",
                            "audit_revert"
                        ],
                        "description": "Configuration action to run."
                    },
                    "topic": {"type": "string", "description": "Topic for topic_help, for example providers, models, model_groups, defaults, appearance, tools, sandbox, network, logs."},
                    "path": {"type": "string", "description": "Config path, for example appearance.theme, providers.<id>.enabled, models.<entry_id>.displayName, model_groups.<id>.models.append."},
                    "value_json": {"type": "string", "description": "New value encoded as a JSON literal. Examples: \"dark\", true, [\"a\"], {\"provider_id\":\"...\",\"model_id\":\"...\"}. Required for set/append/remove unless batch is used."},
                    "batch": {
                        "type": "array",
                        "description": "Batch items for set_batch. Each item has path and value_json. Paths may end with .append or .remove.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string", "description": "Config path."},
                                "value_json": {"type": "string", "description": "JSON literal for the new value."},
                                "caption": {"type": "string", "description": "Optional per-item audit caption."}
                            },
                            "required": ["path", "value_json"]
                        }
                    },
                    "filter": {"type": "string", "description": "Keyword filter for collection reads. Space-separated terms are AND matched case-insensitively."},
                    "page": {"type": "integer", "description": "Collection page number. Default 1.", "default": 1},
                    "page_size": {"type": "integer", "description": "Collection page size. Default 20, max 100.", "default": 20},
                    "limit": {"type": "integer", "description": "Audit list limit. Default 50, max 200.", "default": 50},
                    "scope": {"type": "string", "description": "Optional audit topic filter."},
                    "audit_id": {"type": "string", "description": "Audit id for audit_get or audit_revert."},
                    "actor": {"type": "string", "description": "Audit actor. Default agent."},
                    "caption": {"type": "string", "description": "Human-readable audit caption."}
                },
                "required": ["action"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "web_search".to_string(),
            description: "Search the web for information. Returns results with titles, URLs, snippets, and full page contents fetched concurrently.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The search query to look up on the web. You may include backend-supported operators such as site:example.com, filetype:pdf, intitle:word, -term, or \"exact phrase\"."}
                },
                "required": ["query"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "web_fetch".to_string(),
            description: "Extract readable text from web page URLs. Returns simplified text from HTML or raw text for non-HTML responses. Pass up to 5 URLs per call. PDF conversion is not implemented yet on this phone agent.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of HTTP or HTTPS URLs to extract content from. Max 5 URLs per call.",
                        "maxItems": 5
                    },
                    "max_chars": {"type": "integer", "minimum": 1000, "maximum": 50000}
                },
                "required": ["urls"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "browser_use".to_string(),
            description: "Control the shared Android WebView browser. Actions: navigate, screenshot, click, type, get_text, scroll, get_page_info, execute_js, find_elements, hover, get_readable, get_backbone, fetch, get_cookies, scroll_and_collect, wait_for_dom_stable. Browser output files are saved under /var/hambur/browser and can be shown with hambur:// file links or view_image.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "navigate",
                            "screenshot",
                            "click",
                            "type",
                            "get_text",
                            "scroll",
                            "get_page_info",
                            "execute_js",
                            "find_elements",
                            "hover",
                            "get_readable",
                            "get_backbone",
                            "fetch",
                            "get_cookies",
                            "scroll_and_collect",
                            "wait_for_dom_stable"
                        ],
                        "description": "Browser action to perform."
                    },
                    "url": {"type": "string", "description": "URL to navigate to or download. Supports http, https, hambur://, and minis://workspace|browser|attachments preview URLs."},
                    "selector": {"type": "string", "description": "CSS selector for targeting elements."},
                    "text": {"type": "string", "description": "Text to type into the target element."},
                    "coordinate_x": {"type": "integer", "description": "Viewport X coordinate for coordinate-based click."},
                    "coordinate_y": {"type": "integer", "description": "Viewport Y coordinate for coordinate-based click."},
                    "direction": {"type": "string", "enum": ["up", "down"], "description": "Scroll direction."},
                    "amount": {"type": "integer", "description": "Scroll amount in CSS pixels."},
                    "script": {"type": "string", "description": "JavaScript to execute in an async function wrapper. Supports await and return."},
                    "max_depth": {"type": "integer", "description": "Maximum DOM tree depth for get_backbone."},
                    "scroll_count": {"type": "integer", "description": "Number of scroll steps for scroll_and_collect. Max 20."},
                    "item_selector": {"type": "string", "description": "CSS selector for items collected by scroll_and_collect."},
                    "keywords": {"type": "string", "description": "Cookie name filter. Split multiple keywords with spaces."},
                    "fuzzy": {"type": "boolean", "description": "Cookie keyword matching mode. true means contains match; false means exact match."},
                    "timeout": {"type": "integer", "description": "Timeout in seconds for wait_for_dom_stable or long JavaScript actions."}
                },
                "required": ["action"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "delegate_task".to_string(),
            description: "Spawn one or more isolated leaf subagents using the current model. Use this for separable research, code investigation, or long subtasks where an isolated context helps. Batch mode runs up to 3 children in parallel. Each child receives a fresh conversation and an isolated workspace snapshot; child file edits are not applied back to the parent automatically.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "goal": {"type": "string", "description": "Self-contained task goal for the subagent."},
                    "context": {"type": "string", "description": "Relevant background, paths, constraints, and prior findings."},
                    "toolsets": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Toolsets to enable for this subagent. Currently advisory; unavailable tools are ignored. Common: ['terminal','file'], ['web'], ['terminal','file','web']."
                    },
                    "tasks": {
                        "type": "array",
                        "description": "Optional list of subagent tasks. Use this instead of goal when launching multiple independent subtasks. Max 3 tasks per call; split larger batches across calls.",
                        "maxItems": 3,
                        "items": {
                            "type": "object",
                            "properties": {
                                "goal": {"type": "string", "description": "Task goal"},
                                "context": {"type": "string", "description": "Task-specific context"},
                                "toolsets": {"type": "array", "items": {"type": "string"}},
                                "role": {"type": "string", "enum": ["leaf"], "description": "Currently only leaf subagents are supported."}
                            },
                            "required": ["goal"]
                        }
                    },
                    "role": {"type": "string", "enum": ["leaf"], "description": "Currently only leaf subagents are supported."}
                },
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "memory".to_string(),
            description: "Save durable information to persistent memory shared across all chats. Use this proactively for stable user preferences, environment facts, recurring corrections, and durable project conventions. Do not save short-lived task progress, completed-work logs, or temporary todos; use session_search for past conversation recall.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "replace", "remove", "read"],
                        "description": "Action to perform."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["memory", "user"],
                        "description": "Use user for user profile/preferences. Use memory for assistant notes about environment, conventions, and durable facts."
                    },
                    "content": {
                        "type": "string",
                        "description": "Entry content. Required for add and replace."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Short unique substring identifying the entry to replace or remove."
                    }
                },
                "required": ["action", "target"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "skills_list".to_string(),
            description: "List available skills with minimal metadata. Use skill_view(name) to load full content, tags, and linked files.".to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter to narrow results."
                    }
                },
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "skill_list".to_string(),
            description: "Alias for skills_list. List available skills with minimal metadata."
                .to_string(),
            parameters_json_schema: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter to narrow results."
                    }
                },
                "additionalProperties": false
            }),
        })?;
        compiler.register(ToolSchema {
            name: "skill_view".to_string(),
            description: "Skills load information about specific tasks and workflows, plus references, templates, scripts, and assets. Load a skill's main SKILL.md content or a linked file inside the skill directory."
                .to_string(),
            parameters_json_schema: object_schema(json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Skill name or path. Use skills_list to see available skills. Absolute /var/hambur/skills/... paths are accepted."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional path to a linked file inside the skill, such as references/api.md, templates/config.yaml, or scripts/setup.sh. Omit to get SKILL.md."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            })),
        })?;
        compiler.register(ToolSchema {
            name: "submit_delegate_result".to_string(),
            description: "Submit the structured result from a delegate session.".to_string(),
            parameters_json_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string"},
                    "findings": {"type": "array", "items": {"type": "string"}},
                    "changed_files": {"type": "array", "items": {"type": "string"}},
                    "artifact_paths": {"type": "array", "items": {"type": "string"}},
                    "risks": {"type": "array", "items": {"type": "string"}},
                    "next_steps": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["summary"],
                "additionalProperties": false
            }),
        })?;
        Ok(compiler)
    }

    pub fn register(&mut self, schema: ToolSchema) -> HamburResult<()> {
        let name = schema.name.trim();
        if name.is_empty() {
            return Err(HamburError::InvalidCommand(
                "tool schema name must not be empty".to_string(),
            ));
        }
        if !schema.parameters_json_schema.is_object() {
            return Err(HamburError::InvalidCommand(format!(
                "tool schema must be a JSON object: {name}"
            )));
        }
        self.schemas.insert(name.to_string(), schema);
        Ok(())
    }

    pub fn compile_openai_tools_json(&self) -> String {
        self.compile_named_openai_tools_json(MAIN_OPENAI_TOOL_NAMES)
    }

    pub fn compile_delegate_openai_tools_json(&self) -> String {
        self.compile_named_openai_tools_json(DELEGATE_OPENAI_TOOL_NAMES)
    }

    pub fn compile_named_openai_tools_json(&self, names: &[&str]) -> String {
        let tools = self
            .schemas_for_names(names)
            .map(|schema| {
                json!({
                    "type": "function",
                    "function": {
                        "name": schema.name,
                        "description": schema.description,
                        "parameters": with_tool_call_title(&schema.parameters_json_schema),
                    }
                })
            })
            .collect::<Vec<_>>();
        Value::Array(tools).to_string()
    }

    pub fn schema(&self, name: &str) -> Option<&ToolSchema> {
        self.schemas.get(name)
    }

    fn schemas_for_names<'a>(&'a self, names: &'a [&str]) -> impl Iterator<Item = &'a ToolSchema> {
        names.iter().filter_map(|name| self.schemas.get(*name))
    }

    pub fn validate_arguments(&self, name: &str, arguments: &Value) -> HamburResult<()> {
        let Some(schema) = self.schema(name) else {
            return Err(HamburError::InvalidCommand(format!("unknown tool: {name}")));
        };
        let expected_object = schema
            .parameters_json_schema
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            == "object";
        if expected_object && !arguments.is_object() {
            return Err(HamburError::InvalidCommand(format!(
                "tool arguments must be an object: {name}"
            )));
        }

        let required = schema
            .parameters_json_schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for key in required {
            if arguments.get(key).is_none() {
                return Err(HamburError::InvalidCommand(format!(
                    "tool arguments missing required key {key}: {name}"
                )));
            }
        }
        Ok(())
    }
}

fn object_schema(schema: Value) -> Value {
    schema
}

fn with_tool_call_title(schema: &Value) -> Value {
    let mut schema = schema.clone();
    if let Some(object) = schema.as_object_mut() {
        let properties = object
            .entry("properties".to_string())
            .or_insert_with(|| json!({}));
        if let Some(properties) = properties.as_object_mut() {
            properties.insert(
                "title".to_string(),
                json!({
                    "type": "string",
                    "description": "Short user-visible title describing this specific tool call. Use concise Chinese when the user is chatting in Chinese."
                }),
            );
        }

        let required = object
            .entry("required".to_string())
            .or_insert_with(|| json!([]));
        if let Some(required) = required.as_array_mut() {
            if !required.iter().any(|value| value.as_str() == Some("title")) {
                required.push(json!("title"));
            }
        }
    }
    schema
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub index: u32,
    pub tool_call_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub name: String,
    pub arguments_json: String,
    pub display_title: String,
    pub risk_level: String,
    pub requires_approval: bool,
    pub timeout_ms: u64,
    pub cancellable: bool,
}

impl ToolInvocation {
    pub fn from_model_call(
        index: u32,
        tool_call_id: String,
        turn_id: String,
        session_id: String,
        name: String,
        arguments_json: String,
    ) -> HamburResult<Self> {
        if tool_call_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "tool_call_id must not be empty".to_string(),
            ));
        }
        if name.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "tool name must not be empty".to_string(),
            ));
        }
        serde_json::from_str::<Value>(&arguments_json).map_err(|error| {
            HamburError::InvalidCommand(format!("tool arguments are not valid JSON: {error}"))
        })?;
        Ok(Self {
            index,
            tool_call_id,
            turn_id,
            session_id,
            display_title: display_title(&name, &arguments_json),
            risk_level: risk_level(&name),
            requires_approval: requires_approval(&name),
            timeout_ms: timeout_ms(&name),
            cancellable: true,
            name,
            arguments_json,
        })
    }

    pub fn arguments_value(&self) -> HamburResult<Value> {
        serde_json::from_str::<Value>(&self.arguments_json).map_err(|error| {
            HamburError::InvalidCommand(format!(
                "tool arguments are not valid JSON for {}: {error}",
                self.name
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallBatch {
    pub batch_id: String,
    pub turn_id: String,
    pub assistant_message_id: String,
    pub calls: Vec<ToolInvocation>,
    pub status: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

impl ToolCallBatch {
    pub fn new(
        turn_id: String,
        assistant_message_id: String,
        mut calls: Vec<ToolInvocation>,
    ) -> Self {
        calls.sort_by_key(|call| call.index);
        Self {
            batch_id: new_id("tool_batch"),
            turn_id,
            assistant_message_id,
            calls,
            status: "pending".to_string(),
            started_at_ms: now_ms(),
            ended_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionRecord {
    pub invocation: ToolInvocation,
    pub result: ToolResult,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub is_error: bool,
    pub content_json: String,
    pub summary: String,
    pub artifacts_json: String,
    pub trust_level: String,
    pub truncated: bool,
    pub offloaded_file_id: String,
    pub offloaded_path: String,
    pub context_stub: String,
}

impl ToolResult {
    pub fn failed(tool_call_id: &str, tool_name: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            is_error: true,
            content_json: json!({"error": message}).to_string(),
            summary: message.clone(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToolOutput {
    pub tool_call_id: String,
    pub tool_name: String,
    pub is_error: bool,
    pub content: String,
    pub summary: String,
    pub trust_level: String,
    pub command_or_url: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ToolResultNormalizer {
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
            offload_dir,
            sandbox_offload_dir: "/var/hambur/offloads".to_string(),
            large_result_threshold_bytes: DEFAULT_LARGE_RESULT_THRESHOLD_BYTES,
        })
    }

    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.large_result_threshold_bytes = threshold.max(512);
        self
    }

    pub fn normalize(&self, raw: RawToolOutput) -> HamburResult<ToolResult> {
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
                offloaded_path: host_path.to_string_lossy().to_string(),
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

#[derive(Clone)]
pub struct ToolScheduler {
    schemas: ToolSchemaCompiler,
    normalizer: ToolResultNormalizer,
    max_parallel_tool_calls: usize,
    view_image_handler: Option<ViewImageHandler>,
}

type ViewImageHandler = Arc<dyn Fn(&ToolInvocation, &Value) -> RawToolOutput + Send + Sync>;

impl ToolScheduler {
    pub fn new(offload_dir: impl AsRef<Path>) -> HamburResult<Self> {
        Ok(Self {
            schemas: ToolSchemaCompiler::with_builtin_tools()?,
            normalizer: ToolResultNormalizer::new(offload_dir.as_ref().to_path_buf())?,
            max_parallel_tool_calls: MAX_PARALLEL_TOOL_CALLS,
            view_image_handler: None,
        })
    }

    pub fn schemas(&self) -> &ToolSchemaCompiler {
        &self.schemas
    }

    pub fn with_parallel_limit(mut self, limit: usize) -> Self {
        self.max_parallel_tool_calls = limit.clamp(1, MAX_PARALLEL_TOOL_CALLS);
        self
    }

    pub fn with_view_image_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&ToolInvocation, &Value) -> RawToolOutput + Send + Sync + 'static,
    {
        self.view_image_handler = Some(Arc::new(handler));
        self
    }

    pub fn normalize_raw(&self, raw: RawToolOutput) -> HamburResult<ToolResult> {
        self.normalizer.normalize(raw)
    }

    pub async fn execute_batch(
        &self,
        batch: ToolCallBatch,
    ) -> HamburResult<Vec<ToolExecutionRecord>> {
        let semaphore = Arc::new(Semaphore::new(self.max_parallel_tool_calls));
        let mut queue = VecDeque::from(batch.calls);
        let mut join_set = JoinSet::new();
        let mut results = Vec::new();

        while !queue.is_empty() || !join_set.is_empty() {
            while let Some(invocation) = queue.pop_front() {
                let is_serial = !is_parallel_tool(&invocation.name);
                if is_serial && !join_set.is_empty() {
                    queue.push_front(invocation);
                    break;
                }

                let permit =
                    semaphore.clone().acquire_owned().await.map_err(|error| {
                        HamburError::Internal(format!("tool semaphore: {error}"))
                    })?;
                let schemas = self.schemas.clone();
                let normalizer = self.normalizer.clone();
                let view_image_handler = self.view_image_handler.clone();
                join_set.spawn(async move {
                    let _permit = permit;
                    execute_one_tool(schemas, normalizer, view_image_handler, invocation).await
                });

                if is_serial {
                    break;
                }
            }

            if let Some(joined) = join_set.join_next().await {
                let record = joined
                    .map_err(|error| HamburError::Internal(format!("tool task join: {error}")))??;
                results.push(record);
            }
        }

        results.sort_by_key(|record| record.invocation.index);
        Ok(results)
    }
}

async fn execute_one_tool(
    schemas: ToolSchemaCompiler,
    normalizer: ToolResultNormalizer,
    view_image_handler: Option<ViewImageHandler>,
    invocation: ToolInvocation,
) -> HamburResult<ToolExecutionRecord> {
    let started_at_ms = now_ms();
    let raw = match invocation.arguments_value() {
        Ok(arguments) => {
            if let Err(error) = schemas.validate_arguments(&invocation.name, &arguments) {
                RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: error.to_string(),
                    summary: "Tool validation failed".to_string(),
                    trust_level: "trusted".to_string(),
                    command_or_url: invocation.arguments_json.clone(),
                    status: error.code().as_str().to_string(),
                }
            } else if invocation.name == "view_image" {
                match &view_image_handler {
                    Some(handler) => handler(&invocation, &arguments),
                    None => RawToolOutput {
                        tool_call_id: invocation.tool_call_id.clone(),
                        tool_name: invocation.name.clone(),
                        is_error: true,
                        content: "view_image is unavailable for the active route".to_string(),
                        summary: "Image view unavailable".to_string(),
                        trust_level: "trusted".to_string(),
                        command_or_url: invocation.arguments_json.clone(),
                        status: "CapabilityMismatch".to_string(),
                    },
                }
            } else {
                run_builtin_tool(&invocation, &arguments)
            }
        }
        Err(error) => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: error.to_string(),
            summary: "Tool arguments were invalid".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: error.code().as_str().to_string(),
        },
    };
    let result = normalizer.normalize(raw)?;
    Ok(ToolExecutionRecord {
        invocation,
        result,
        started_at_ms,
        ended_at_ms: now_ms(),
    })
}

fn run_builtin_tool(invocation: &ToolInvocation, arguments: &Value) -> RawToolOutput {
    match invocation.name.as_str() {
        "get_current_time" => {
            let content = json!({
                "epoch_ms": now_ms(),
                "timezone": "UTC"
            })
            .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                content,
                summary: "Current time returned".to_string(),
                trust_level: "trusted".to_string(),
                command_or_url: "clock".to_string(),
                status: "ok".to_string(),
            }
        }
        "session_search" => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(1, 20);
            let content = json!({
                "query": query,
                "limit": limit,
                "matches": []
            })
            .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                content,
                summary: "Session search completed".to_string(),
                trust_level: "trusted".to_string(),
                command_or_url: query.to_string(),
                status: "ok".to_string(),
            }
        }
        "echo" => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: false,
                summary: text.chars().take(120).collect(),
                content: text,
                trust_level: "trusted".to_string(),
                command_or_url: "echo".to_string(),
                status: "ok".to_string(),
            }
        }
        "view_image" => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "view_image requires a runtime file resolver".to_string(),
            summary: "Image view unavailable".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "CapabilityMismatch".to_string(),
        },
        "terminal" | "process" => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: format!(
                "{} requires the runtime SandboxService and rootfs lifecycle",
                invocation.name
            ),
            summary: "Sandbox tool unavailable".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "ToolUnavailable".to_string(),
        },
        "browser_use" => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "browser_use requires an Android platform BrowserAction request".to_string(),
            summary: "Browser platform adapter unavailable".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "PlatformRequestUnavailable".to_string(),
        },
        "web_search" | "web_fetch" => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: "web tools require configured provider credentials".to_string(),
            summary: "Web provider unavailable".to_string(),
            trust_level: "untrusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "ProviderUnavailable".to_string(),
        },
        "delegate_task" | "submit_delegate_result" => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: format!("{} requires DelegateAgentService", invocation.name),
            summary: "Delegate service unavailable".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "ToolUnavailable".to_string(),
        },
        _ => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: format!("unknown tool: {}", invocation.name),
            summary: "Unknown tool".to_string(),
            trust_level: "trusted".to_string(),
            command_or_url: invocation.arguments_json.clone(),
            status: "InvalidCommand".to_string(),
        },
    }
}

fn display_title(name: &str, arguments_json: &str) -> String {
    match name {
        "get_current_time" => "Get current time".to_string(),
        "session_search" => {
            let query = serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("query")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            if query.is_empty() {
                "Search sessions".to_string()
            } else {
                format!("Search sessions: {query}")
            }
        }
        "echo" => "Echo".to_string(),
        "view_image" => {
            let path = serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("path")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            if path.is_empty() {
                "View image".to_string()
            } else {
                format!("View image: {path}")
            }
        }
        "terminal" => "Run terminal command".to_string(),
        "process" => {
            let action = serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("action")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            if action.is_empty() {
                "Process control".to_string()
            } else {
                format!("Process: {action}")
            }
        }
        "web_search" => {
            let query = serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("query")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            if query.is_empty() {
                "Search web".to_string()
            } else {
                format!("Search web: {query}")
            }
        }
        "web_fetch" => "Fetch web URLs".to_string(),
        "browser_use" => {
            let action = serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| {
                    value
                        .get("action")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            if action.is_empty() {
                "Use browser".to_string()
            } else {
                format!("Browser: {action}")
            }
        }
        "delegate_task" => "Delegate task".to_string(),
        "submit_delegate_result" => "Submit delegate result".to_string(),
        _ => name.replace('_', " "),
    }
}

fn risk_level(name: &str) -> String {
    match name {
        "write_file"
        | "patch"
        | "terminal"
        | "process"
        | "hambur_config"
        | "delegate_task"
        | "submit_delegate_result" => "high",
        "web_search" | "web_fetch" | "browser_use" => "medium",
        _ => "low",
    }
    .to_string()
}

fn requires_approval(name: &str) -> bool {
    matches!(risk_level(name).as_str(), "high")
}

fn timeout_ms(name: &str) -> u64 {
    match name {
        "terminal" | "process" => 120_000,
        "web_search" | "web_fetch" | "browser_use" => 60_000,
        "delegate_task" => 600_000,
        _ => 30_000,
    }
}

fn is_parallel_tool(name: &str) -> bool {
    matches!(
        name,
        "web_search"
            | "web_fetch"
            | "read_file"
            | "search_files"
            | "terminal"
            | "session_search"
            | "skill_list"
            | "skills_list"
            | "skill_view"
            | "get_current_time"
            | "echo"
            | "view_image"
    )
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

