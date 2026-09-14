use crate::*;

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
    pub(crate) topic: &'static str,
}

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

pub(crate) fn app_setting_for_hambur_config_path(
    path: &str,
    value_json: &str,
) -> Option<(String, String)> {
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

pub(crate) fn normalize_view_image_scale_value(value: &str) -> String {
    match value {
        "resizeFit" | "RESIZE_FIT" => "resize_fit".to_string(),
        "ORIGINAL" => "original".to_string(),
        _ => value.to_string(),
    }
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
