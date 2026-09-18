//! Tool registry: every tool the model can call is described once, here.
//!
//! `ToolKind` is the single source of truth for a tool's identity and behavioural
//! metadata (risk, timeout, parallelism, audience, display title). `ToolSpec` adds the
//! model-facing schema. Adding a tool means: add a `ToolKind` variant, fill in the
//! metadata match arms (the compiler enforces exhaustiveness), register its spec in
//! `ToolRegistry::with_builtin_tools`, and handle it in the runtime host dispatch.

use std::collections::{BTreeMap, HashSet};

use hambur_core::{HamburError, HamburResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Which model roles a tool is offered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAudience {
    /// Only the main assistant.
    Main,
    /// Only delegate (child) sessions.
    Delegate,
    /// Both main and delegate sessions.
    Both,
    /// Not exposed to any model; callable only in tests/scripted flows.
    Internal,
}

/// Where a tool actually runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolHost {
    /// Pure function inside this crate (no runtime state needed).
    Builtin,
    /// Needs the runtime engine (database, sandbox, platform bridge, ...).
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ToolKind {
    SkillsList,
    SkillView,
    Terminal,
    Process,
    ReadFile,
    WriteFile,
    Patch,
    SearchFiles,
    HamburConfig,
    WebSearch,
    BrowserUse,
    AndroidCli,
    SessionSearch,
    Memory,
    DelegateTask,
    ViewImage,
    SubmitDelegateResult,
    Echo,
}

impl ToolKind {
    /// Every tool, in the order it is presented to the model.
    pub const ALL: &'static [ToolKind] = &[
        Self::SkillsList,
        Self::SkillView,
        Self::Terminal,
        Self::Process,
        Self::ReadFile,
        Self::WriteFile,
        Self::Patch,
        Self::SearchFiles,
        Self::HamburConfig,
        Self::WebSearch,
        Self::BrowserUse,
        Self::AndroidCli,
        Self::SessionSearch,
        Self::Memory,
        Self::DelegateTask,
        Self::ViewImage,
        Self::SubmitDelegateResult,
        Self::Echo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SkillsList => "skills_list",
            Self::SkillView => "skill_view",
            Self::Terminal => "terminal",
            Self::Process => "process",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::Patch => "patch",
            Self::SearchFiles => "search_files",
            Self::HamburConfig => "hambur_config",
            Self::WebSearch => "web_search",
            Self::BrowserUse => "browser_use",
            Self::AndroidCli => "android_cli",
            Self::SessionSearch => "session_search",
            Self::Memory => "memory",
            Self::DelegateTask => "delegate_task",
            Self::ViewImage => "view_image",
            Self::SubmitDelegateResult => "submit_delegate_result",
            Self::Echo => "echo",
        }
    }

    /// Resolve a model-supplied tool name, accepting known aliases.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        if name == "skill_list" {
            return Some(Self::SkillsList);
        }
        Self::ALL.iter().copied().find(|kind| kind.name() == name)
    }

    pub fn audience(self) -> ToolAudience {
        match self {
            Self::DelegateTask => ToolAudience::Main,
            Self::SubmitDelegateResult => ToolAudience::Delegate,
            Self::Echo => ToolAudience::Internal,
            _ => ToolAudience::Both,
        }
    }

    pub fn host(self) -> ToolHost {
        match self {
            Self::Echo => ToolHost::Builtin,
            _ => ToolHost::Runtime,
        }
    }

    pub fn risk(self) -> RiskLevel {
        match self {
            Self::WriteFile
            | Self::Patch
            | Self::Terminal
            | Self::Process
            | Self::HamburConfig
            | Self::DelegateTask
            | Self::SubmitDelegateResult => RiskLevel::High,
            Self::WebSearch | Self::BrowserUse => RiskLevel::Medium,
            Self::SkillsList
            | Self::SkillView
            | Self::ReadFile
            | Self::SearchFiles
            | Self::AndroidCli
            | Self::SessionSearch
            | Self::Memory
            | Self::ViewImage
            | Self::Echo => RiskLevel::Low,
        }
    }

    pub fn requires_approval(self) -> bool {
        self.risk() == RiskLevel::High
    }

    pub fn timeout_ms(self) -> u64 {
        match self {
            Self::Terminal | Self::Process => 120_000,
            Self::WebSearch | Self::BrowserUse | Self::AndroidCli => 60_000,
            Self::DelegateTask => 600_000,
            _ => 30_000,
        }
    }

    /// Whether the tool may run concurrently with other parallel-safe tools in a batch.
    pub fn is_parallel(self) -> bool {
        matches!(
            self,
            Self::WebSearch
                | Self::ReadFile
                | Self::SearchFiles
                | Self::Terminal
                | Self::SessionSearch
                | Self::SkillsList
                | Self::SkillView
                | Self::Echo
                | Self::ViewImage
        )
    }

    /// Human-readable title for the tool call, derived from its arguments.
    pub fn display_title(self, arguments_json: &str) -> String {
        let argument = |key: &str| -> String {
            serde_json::from_str::<Value>(arguments_json)
                .ok()
                .and_then(|value| value.get(key).and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default()
        };
        let titled = |prefix: &str, fallback: &str, value: String| -> String {
            if value.is_empty() {
                fallback.to_string()
            } else {
                format!("{prefix}: {value}")
            }
        };
        match self {
            Self::SessionSearch => titled("Search sessions", "Search sessions", argument("query")),
            Self::Echo => "Echo".to_string(),
            Self::ViewImage => titled("View image", "View image", argument("path")),
            Self::Terminal => "Run terminal command".to_string(),
            Self::Process => titled("Process", "Process control", argument("action")),
            Self::WebSearch => titled("Search web", "Search web", argument("query")),
            Self::BrowserUse => titled("Browser", "Use browser", argument("action")),
            Self::AndroidCli => titled("Android", "Android CLI", argument("action")),
            Self::DelegateTask => "Delegate task".to_string(),
            Self::SubmitDelegateResult => "Submit delegate result".to_string(),
            Self::SkillsList
            | Self::SkillView
            | Self::ReadFile
            | Self::WriteFile
            | Self::Patch
            | Self::SearchFiles
            | Self::HamburConfig
            | Self::Memory => self.name().replace('_', " "),
        }
    }
}

/// Model-facing definition of a tool: identity plus JSON schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: String,
    pub description: String,
    pub parameters_json_schema: Value,
}

impl ToolSpec {
    pub fn new(kind: ToolKind, description: &str, parameters_json_schema: Value) -> Self {
        Self {
            kind,
            name: kind.name().to_string(),
            description: description.to_string(),
            parameters_json_schema,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolRegistry {
    specs: BTreeMap<ToolKind, ToolSpec>,
}

impl ToolRegistry {
    pub fn register(&mut self, spec: ToolSpec) -> HamburResult<()> {
        if !spec.parameters_json_schema.is_object() {
            return Err(HamburError::InvalidCommand(format!(
                "tool schema must be a JSON object: {}",
                spec.name
            )));
        }
        self.specs.insert(spec.kind, spec);
        Ok(())
    }

    pub fn spec(&self, kind: ToolKind) -> Option<&ToolSpec> {
        self.specs.get(&kind)
    }

    /// Look up a spec by model-supplied name (aliases accepted).
    pub fn schema(&self, name: &str) -> Option<&ToolSpec> {
        ToolKind::from_name(name).and_then(|kind| self.specs.get(&kind))
    }

    pub fn compile_openai_tools_json(&self) -> String {
        self.compile_openai_tools_json_excluding(&HashSet::new())
    }

    pub fn compile_delegate_openai_tools_json(&self) -> String {
        self.compile_delegate_openai_tools_json_excluding(&HashSet::new())
    }

    pub fn compile_openai_tools_json_excluding(&self, disabled: &HashSet<String>) -> String {
        self.compile_for_audience(ToolAudience::Main, disabled)
    }

    pub fn compile_delegate_openai_tools_json_excluding(
        &self,
        disabled: &HashSet<String>,
    ) -> String {
        self.compile_for_audience(ToolAudience::Delegate, disabled)
    }

    pub fn compile_named_openai_tools_json(&self, names: &[&str]) -> String {
        let kinds = names
            .iter()
            .filter_map(|name| ToolKind::from_name(name))
            .collect::<Vec<_>>();
        self.compile_kinds(kinds.into_iter(), &HashSet::new())
    }

    fn compile_for_audience(&self, audience: ToolAudience, disabled: &HashSet<String>) -> String {
        let kinds = ToolKind::ALL.iter().copied().filter(|kind| {
            matches!(
                (kind.audience(), audience),
                (ToolAudience::Both, _)
                    | (ToolAudience::Main, ToolAudience::Main)
                    | (ToolAudience::Delegate, ToolAudience::Delegate)
            )
        });
        self.compile_kinds(kinds, disabled)
    }

    fn compile_kinds(
        &self,
        kinds: impl Iterator<Item = ToolKind>,
        disabled: &HashSet<String>,
    ) -> String {
        let tools = kinds
            .filter_map(|kind| self.specs.get(&kind))
            .filter(|spec| !disabled.contains(&spec.name))
            .map(|spec| {
                json!({
                    "type": "function",
                    "function": {
                        "name": spec.name,
                        "description": spec.description,
                        "parameters": with_tool_call_title(&spec.parameters_json_schema),
                    }
                })
            })
            .collect::<Vec<_>>();
        Value::Array(tools).to_string()
    }

    pub fn validate_arguments(&self, name: &str, arguments: &Value) -> HamburResult<()> {
        let Some(spec) = self.schema(name) else {
            return Err(HamburError::InvalidCommand(format!("unknown tool: {name}")));
        };
        let expected_object = spec
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

        let required = spec
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

    pub fn with_builtin_tools() -> HamburResult<Self> {
        let mut registry = Self::default();
        registry.register(ToolSpec::new(
            ToolKind::SessionSearch,
            "Search past chat sessions stored locally on this phone, or read/scroll inside one. Calling shapes: (1) pass query for discovery; (2) pass session_id + around_message_id to scroll around a message; (3) pass session_id only to read a session; (4) pass no args to browse recent sessions. Use this for questions like what did we discuss about X, where did we leave Y, or find the session where Z.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::Echo,
            "Echo text for deterministic local testing.",
            json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::ViewImage,
            "View a local image file from the sandbox. Returns detail (high/original), width, height, and resolved path. The app attaches the image to the next model request as image_url. Path must be an absolute sandbox path starting with /var/hambur/ (e.g. /var/hambur/workspace/image.png).",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute sandbox path to the image file, starting with /var/hambur/."},
                    "detail": {
                        "type": "string",
                        "enum": ["high", "original"],
                        "description": "Optional. Detail mode for the image. If 'high', resizes to fit max 1024px. If 'original', returns full resolution. Omit to use the user's default setting."
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            })),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::ReadFile,
            "Read a text file with line numbers and pagination. Use this instead of cat/head/tail in terminal. Output format is 'LINE_NUM|CONTENT'. Use offset and limit for large files. Path must be an absolute sandbox path starting with /var/hambur/ (e.g. /var/hambur/workspace/file.txt). Cannot read images or binary files.",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute sandbox path to the file, starting with /var/hambur/."},
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::WriteFile,
            "Write content to a file, completely replacing existing content. Use this instead of echo/cat heredoc in terminal. Creates parent directories automatically. Path must be an absolute sandbox path starting with /var/hambur/ (e.g. /var/hambur/workspace/file.txt).",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute sandbox path to the file, starting with /var/hambur/."},
                    "content": {"type": "string", "description": "Complete content to write to the file."}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            })),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::Patch,
            "Targeted find-and-replace edits in files. Use this instead of sed/awk in terminal. Find a unique old_string and replace it with new_string. Include surrounding context lines to ensure uniqueness.",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["replace"],
                        "description": "Edit mode. Defaults to 'replace'.",
                        "default": "replace"
                    },
                    "path": {"type": "string", "description": "Required. Absolute sandbox path to the file, starting with /var/hambur/."},
                    "old_string": {"type": "string", "description": "Required. Exact text to find and replace. Must be unique unless replace_all=true. Include surrounding context lines."},
                    "new_string": {"type": "string", "description": "Required. Replacement text. Pass empty string to delete the matched text."},
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences instead of requiring a unique match. Defaults to false.",
                        "default": false
                    }
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false
            })),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::SearchFiles,
            "Search file contents or find files by name. Use this instead of grep/rg/find/ls in terminal. Content search (target='content') treats pattern as a regex when possible and returns matching lines with line numbers. File search (target='files') treats pattern as a glob or filename fragment. Path must be an absolute sandbox path starting with /var/hambur/ (defaults to /var/hambur/workspace).",
            object_schema(json!({
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
                        "description": "Absolute sandbox directory or file to search in, starting with /var/hambur/. Defaults to /var/hambur/workspace.",
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::Terminal,
            "Run a shell command inside the user's Linux sandbox. Use this for commands, package managers, builds, tests, and one-off shell work. Prefer read_file/write_file/patch/search_files for file operations. Short commands should run in the foreground with a generous timeout; long-running servers, watchers, and jobs should use background=true and then be managed with the process tool. The default working directory is /var/hambur/workspace.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::Process,
            "Manage background processes started with terminal(background=true). Actions: list, poll, log, wait, kill, write, submit, close.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::HamburConfig,
            "Read and update Hambur app configuration as a native tool. Use this instead of editing app prefs or running shell commands. Supports provider/model/model-group/default/tool/appearance/log/network settings plus audit history and revert.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::WebSearch,
            "Search the web for information. Returns results with titles, URLs, snippets, and full page contents fetched concurrently.",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The search query to look up on the web. You may include backend-supported operators such as site:example.com, filetype:pdf, intitle:word, -term, or \"exact phrase\"."}
                },
                "required": ["query"],
                "additionalProperties": false
            })),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::BrowserUse,
            "Control the shared Android WebView browser. Actions: navigate, screenshot, click, type, get_text, scroll, get_page_info, execute_js, find_elements, hover, get_readable, get_backbone, fetch, get_cookies, scroll_and_collect, wait_for_dom_stable, download. Browser output files are saved under /var/hambur/browser and can be shown with Markdown syntax ![title](/var/hambur/browser/...) or view_image.",
            object_schema(json!({
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
                            "download",
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::AndroidCli,
            "Access native Android system and hardware capabilities on-demand without background daemons. Actions: get_location (GPS/network coordinates; returns standard WGS-84 and converted GCJ-02 coordinates for Chinese map services like Gaode, plus altitude, speed, bearing, accuracy), get_battery (percentage, charging status, temperature), get_device_info (hardware model, brand, Android version, network connectivity), clipboard_get (read system clipboard text), clipboard_set (write text to system clipboard), vibrate (haptic vibration feedback), send_notification (post system status bar notification), torch (turn flashlight on/off).",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "get_location",
                            "get_battery",
                            "get_device_info",
                            "clipboard_get",
                            "clipboard_set",
                            "vibrate",
                            "send_notification",
                            "torch"
                        ],
                        "description": "Android device action to perform."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text content to write into the clipboard for clipboard_set."
                    },
                    "duration_ms": {
                        "type": "integer",
                        "description": "Vibration duration in milliseconds for vibrate (default 200).",
                        "default": 200
                    },
                    "title": {
                        "type": "string",
                        "description": "Notification title for send_notification."
                    },
                    "content": {
                        "type": "string",
                        "description": "Notification body content for send_notification."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Flashlight toggle state (true=on, false=off) for torch."
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            })),
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::DelegateTask,
            "Spawn one or more isolated leaf subagents using the current model. Use this for separable research, code investigation, or long subtasks where an isolated context helps. Batch mode runs up to 3 children in parallel. Each child receives a fresh conversation and an isolated workspace snapshot; child file edits are not applied back to the parent automatically.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::Memory,
            "Save durable information to persistent memory shared across all chats. Use this proactively for stable user preferences, environment facts, recurring corrections, and durable project conventions. Do not save short-lived task progress, completed-work logs, or temporary todos; use session_search for past conversation recall.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::SkillsList,
            "List available skills with minimal metadata. Use skill_view(name) to load full content, tags, and linked files.",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter to narrow results."
                    }
                },
                "additionalProperties": false
            })),
        ))?;
                registry.register(ToolSpec::new(
            ToolKind::SkillView,
            "Skills load information about specific tasks and workflows, plus references, templates, scripts, and assets. Load a skill's main SKILL.md content or a linked file inside the skill directory.",
            object_schema(json!({
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
        ))?;
        registry.register(ToolSpec::new(
            ToolKind::SubmitDelegateResult,
            "Submit the structured result from a delegate session.",
            json!({
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
        ))?;
        Ok(registry)
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
