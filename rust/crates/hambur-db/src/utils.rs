use crate::*;

pub(crate) fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        DEFAULT_SESSION_TITLE.to_string()
    } else {
        title.chars().take(120).collect()
    }
}

pub(crate) fn normalize_setting_id(value: &str, prefix: &str) -> String {
    let normalized = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        .take(120)
        .collect::<String>();
    if normalized.is_empty() {
        new_id(prefix)
    } else {
        normalized
    }
}

pub(crate) fn normalize_routing_strategy(value: &str) -> HamburResult<String> {
    let value = value.trim();
    match value {
        "" | "fallback" | "priority" => Ok("fallback".to_string()),
        "load_balance" | "round_robin" => Ok("load_balance".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid routing_strategy: {value}"
        ))),
    }
}

pub(crate) fn normalize_fallback_policy(value: &str) -> HamburResult<String> {
    let value = value.trim();
    match value {
        "" | "default" | "never" => Ok("default".to_string()),
        "always" | "always_before_output" => Ok("always".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid fallback_policy: {value}"
        ))),
    }
}

pub(crate) fn normalize_default_group_key(value: &str) -> HamburResult<String> {
    let key = value.trim();
    match key {
        "primary" | "secondary" | "vision" | "tools" => Ok(key.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid default model group key: {key}"
        ))),
    }
}

pub(crate) fn normalize_app_setting_key(value: &str) -> HamburResult<String> {
    let key = value.trim();
    if key.starts_with("skill_enabled:")
        || key.starts_with("startup_task:")
        || key.starts_with("rootfs_setting:")
    {
        return normalize_prefixed_setting_key(key);
    }

    let allowed = [
        "themeMode",
        "fontScale",
        "startupChatMode",
        "lastSelectedSessionId",
        "loggingEnabled",
        "predictiveBackEnabled",
        "fpsOverlayEnabled",
        "rootfsBackend",
        "webFetchBackend",
        "viewImageScaleMode",
        "defaultDeepThinkingEnabled",
        "startupTasksEnabled",
        "tool_settings",
        "skills",
        "memory_projections",
        "startup_tasks",
        "rootfs_settings",
        "browser_tool_settings",
        "appearance",
        "logs",
        "token_usage",
        "persona",
        "environment_variables",
        "deep_thinking",
        "search",
    ];
    if allowed.contains(&key) {
        Ok(key.to_string())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "invalid app setting key: {key}"
        )))
    }
}

pub(crate) fn normalize_prefixed_setting_key(key: &str) -> HamburResult<String> {
    let mut parts = key.splitn(2, ':');
    let prefix = parts.next().unwrap_or_default();
    let id = parts.next().unwrap_or_default();
    if id.trim().is_empty() {
        return Err(HamburError::InvalidCommand(format!(
            "invalid app setting key: {key}"
        )));
    }
    let id = if prefix == "skill_enabled" {
        normalize_skill_setting_key_suffix(id)?
    } else {
        normalize_setting_key_suffix(id)
    };
    Ok(format!("{prefix}:{id}"))
}

pub(crate) fn normalize_skill_setting_key_suffix(value: &str) -> HamburResult<String> {
    let value = value
        .trim()
        .trim_start_matches("/var/hambur/skills/")
        .trim_start_matches('/');
    if value.is_empty() || value.contains("..") || value.contains('\\') {
        return Err(HamburError::InvalidCommand(
            "invalid skill setting key".to_string(),
        ));
    }
    if !value.ends_with("/SKILL.md") {
        return Ok(normalize_setting_key_suffix(value));
    }
    Ok(value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
        .take(240)
        .collect())
}

pub(crate) fn normalize_setting_key_suffix(value: &str) -> String {
    let mut output = String::new();
    let mut previous_separator = false;
    for ch in value.trim().chars() {
        let next = if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            ch
        } else {
            '-'
        };
        if next == '-' {
            if !previous_separator {
                output.push(next);
            }
            previous_separator = true;
        } else {
            output.push(next);
            previous_separator = false;
        }
        if output.len() >= 120 {
            break;
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        new_id("setting")
    } else {
        output
    }
}

pub(crate) fn normalize_app_setting_value(key: &str, value: &str) -> HamburResult<String> {
    let value = value.trim();
    match key {
        "themeMode" => normalize_enum_setting(key, value, &["system", "light", "dark"]),
        "fontScale" => {
            normalize_enum_setting(key, value, &["small", "default", "large", "extra_large"])
        }
        "startupChatMode" => normalize_enum_setting(key, value, &["new_chat", "last_chat"]),
        "rootfsBackend" => normalize_enum_setting(key, value, &["chroot", "proot"]),
        "webFetchBackend" => normalize_enum_setting(key, value, &["local", "tinyfish"]),
        "viewImageScaleMode" => normalize_enum_setting(key, value, &["original", "resize_fit"]),
        "loggingEnabled"
        | "predictiveBackEnabled"
        | "fpsOverlayEnabled"
        | "defaultDeepThinkingEnabled"
        | "startupTasksEnabled" => normalize_bool_setting(key, value),
        "lastSelectedSessionId" => Ok(value.chars().take(160).collect()),
        "browser_tool_settings" => normalize_browser_tool_settings(value),
        key if key.starts_with("skill_enabled:") => normalize_bool_setting(key, value),
        "tool_settings"
        | "skills"
        | "memory_projections"
        | "startup_tasks"
        | "rootfs_settings"
        | "appearance"
        | "logs"
        | "token_usage"
        | "persona"
        | "environment_variables"
        | "deep_thinking"
        | "search" => normalize_json_or_text_setting(value),
        key if key.starts_with("startup_task:") || key.starts_with("rootfs_setting:") => {
            normalize_json_or_text_setting(value)
        }
        _ => Ok(value.chars().take(8000).collect()),
    }
}

pub(crate) fn normalize_enum_setting(
    key: &str,
    value: &str,
    allowed: &[&str],
) -> HamburResult<String> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(HamburError::InvalidCommand(format!(
            "invalid {key} value: {value}"
        )))
    }
}

pub(crate) fn normalize_bool_setting(key: &str, value: &str) -> HamburResult<String> {
    match value {
        "true" | "false" => Ok(value.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid {key} value: expected true or false"
        ))),
    }
}

pub(crate) fn normalize_json_or_text_setting(value: &str) -> HamburResult<String> {
    if value.len() > 8000 {
        return Err(HamburError::InvalidCommand(
            "setting value must be at most 8000 characters".to_string(),
        ));
    }
    if value.starts_with('{') || value.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(value).map_err(|error| {
            HamburError::InvalidCommand(format!("setting value must be valid JSON: {error}"))
        })?;
    }
    Ok(value.to_string())
}

pub(crate) fn normalize_browser_tool_settings(value: &str) -> HamburResult<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(value).map_err(|error| {
        HamburError::InvalidCommand(format!("browser tool settings must be JSON: {error}"))
    })?;
    let max_fetch_bytes = parsed
        .get("maxFetchBytes")
        .or_else(|| parsed.get("max_fetch_bytes"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2_000_000);
    if !(250_000..=10_000_000).contains(&max_fetch_bytes) {
        return Err(HamburError::InvalidCommand(
            "browser maxFetchBytes must be between 250000 and 10000000".to_string(),
        ));
    }
    let auto_close_minutes = parsed
        .get("autoCloseMinutes")
        .or_else(|| parsed.get("auto_close_minutes"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(15);
    if auto_close_minutes > 240 {
        return Err(HamburError::InvalidCommand(
            "browser autoCloseMinutes must be between 0 and 240".to_string(),
        ));
    }
    let accept_cookies = parsed
        .get("acceptCookies")
        .or_else(|| parsed.get("accept_cookies"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let accept_third_party = parsed
        .get("acceptThirdPartyCookies")
        .or_else(|| parsed.get("accept_third_party_cookies"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if !accept_cookies && accept_third_party {
        return Err(HamburError::InvalidCommand(
            "browser acceptThirdPartyCookies must be false when acceptCookies is false".to_string(),
        ));
    }
    normalize_json_or_text_setting(value)
}

pub(crate) fn normalize_role(role: &str) -> HamburResult<String> {
    let role = role.trim();
    match role {
        "system" | "user" | "assistant" | "tool" => Ok(role.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid message role: {role}"
        ))),
    }
}

pub(crate) fn normalize_status(status: &str) -> HamburResult<String> {
    let status = status.trim();
    if status.is_empty() {
        return Err(HamburError::InvalidCommand(
            "status must not be empty".to_string(),
        ));
    }

    Ok(status.chars().take(80).collect())
}

pub(crate) fn unsigned_ms(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(crate) fn unsigned_count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn clamp_limit(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

pub(crate) fn normalize_provider_id(id: &str) -> String {
    let id = id.trim();
    if id.is_empty() {
        new_id("provider")
    } else {
        id.chars().take(120).collect()
    }
}

pub(crate) fn normalize_base_url(base_url: &str) -> HamburResult<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(HamburError::InvalidCommand(
            "provider base_url must not be empty".to_string(),
        ));
    }
    if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
        return Err(HamburError::InvalidCommand(
            "provider base_url must be http or https".to_string(),
        ));
    }
    Ok(base_url.chars().take(512).collect())
}

pub(crate) fn normalize_secret_ref(secret_ref: &str) -> HamburResult<String> {
    let secret_ref = secret_ref.trim();
    if secret_ref.is_empty() {
        return Err(HamburError::InvalidCommand(
            "provider secret_ref must not be empty".to_string(),
        ));
    }
    let lower = secret_ref.to_ascii_lowercase();
    if secret_ref.starts_with("sk-")
        || lower.starts_with("bearer ")
        || lower.contains("api_key=")
        || lower.contains("apikey=")
    {
        return Err(HamburError::InvalidCommand(
            "provider secret_ref must reference Android Secret Store, not a raw API key"
                .to_string(),
        ));
    }
    Ok(secret_ref.chars().take(256).collect())
}

pub(crate) fn secret_label(secret_ref: &str) -> String {
    let secret_ref = secret_ref.trim();
    if secret_ref.is_empty() {
        "Not configured".to_string()
    } else if secret_ref.starts_with("android-secret://") {
        "Android Secret Store".to_string()
    } else if secret_ref.starts_with("env://") {
        "Environment Secret".to_string()
    } else {
        "Secret reference".to_string()
    }
}

pub(crate) fn normalize_file_scope(scope: &str) -> HamburResult<String> {
    let scope = scope.trim();
    match scope {
        "session" | "global" | "cache" => Ok(scope.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid file scope: {scope}"
        ))),
    }
}

pub(crate) fn normalize_retention_policy(policy: &str) -> HamburResult<String> {
    let policy = policy.trim();
    match policy {
        "keep" | "delete_with_session" | "cache" => Ok(policy.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid retention policy: {policy}"
        ))),
    }
}

pub(crate) fn normalize_db_path(path: &str, label: &str) -> HamburResult<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(HamburError::InvalidCommand(format!(
            "{label} must not be empty"
        )));
    }
    if path.contains('\0') || path.contains("..") {
        return Err(HamburError::InvalidCommand(format!(
            "{label} must not contain traversal"
        )));
    }
    Ok(path.chars().take(1024).collect())
}

pub(crate) fn normalize_mime_type(mime_type: &str) -> String {
    let mime_type = mime_type.trim().to_ascii_lowercase();
    let value = if mime_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime_type
    };
    value.chars().take(160).collect()
}

pub(crate) fn normalize_origin_type(origin_type: &str) -> String {
    let origin_type = origin_type.trim();
    match origin_type {
        "content_uri" | "file" | "camera" | "share" | "sandbox" | "generated" => {
            origin_type.to_string()
        }
        _ => "content_uri".to_string(),
    }
}

pub(crate) fn normalize_display_name(display_name: &str) -> String {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        "attachment".to_string()
    } else {
        display_name.chars().take(160).collect()
    }
}

pub(crate) fn normalize_attachment_kind(kind: &str, mime_type: &str) -> String {
    let kind = kind.trim();
    match kind {
        "image" | "file" | "audio" | "video" | "other" => kind.to_string(),
        _ => {
            let mime_type = mime_type.trim().to_ascii_lowercase();
            if mime_type.starts_with("image/") {
                "image".to_string()
            } else if mime_type.starts_with("audio/") {
                "audio".to_string()
            } else if mime_type.starts_with("video/") {
                "video".to_string()
            } else {
                "file".to_string()
            }
        }
    }
}

pub(crate) fn normalize_attachment_status(status: &str) -> HamburResult<String> {
    let status = status.trim();
    match status {
        "pending" | "attached" | "removed" | "expired" => Ok(status.to_string()),
        "" => Ok("pending".to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid attachment status: {status}"
        ))),
    }
}

pub(crate) fn sql_bool(value: i64) -> bool {
    value != 0
}

pub(crate) fn session_summary_from_row(row: &Row) -> HamburResult<SessionSummary> {
    Ok(SessionSummary {
        id: row.get::<String>(0).map_err(database_error)?,
        title: row.get::<String>(1).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        pinned_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        memory_reviewed: sql_bool(row.get::<i64>(5).map_err(database_error)?),
        message_count: unsigned_count(row.get::<i64>(6).map_err(database_error)?),
        latest_preview: row.get::<String>(7).map_err(database_error)?,
    })
}

pub(crate) fn provider_record_from_row(row: &Row) -> HamburResult<ProviderRecord> {
    Ok(ProviderRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        icon_name: row.get::<String>(2).map_err(database_error)?,
        api_type: row.get::<String>(3).map_err(database_error)?,
        base_url: row.get::<String>(4).map_err(database_error)?,
        secret_ref: row.get::<String>(5).map_err(database_error)?,
        enabled: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
    })
}

pub(crate) fn public_provider_from_row(row: &Row) -> HamburResult<PublicProviderRecord> {
    let secret_ref = row.get::<String>(5).map_err(database_error)?;
    Ok(PublicProviderRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        icon_name: row.get::<String>(2).map_err(database_error)?,
        api_type: row.get::<String>(3).map_err(database_error)?,
        base_url: row.get::<String>(4).map_err(database_error)?,
        secret_label: secret_label(&secret_ref),
        enabled: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
    })
}

pub(crate) fn provider_model_from_row(row: &Row) -> HamburResult<ProviderModelRecord> {
    Ok(ProviderModelRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        provider_id: row.get::<String>(1).map_err(database_error)?,
        model_id: row.get::<String>(2).map_err(database_error)?,
        display_name: row.get::<String>(3).map_err(database_error)?,
        supports_tool_call: sql_bool(row.get::<i64>(4).map_err(database_error)?),
        supports_reasoning: sql_bool(row.get::<i64>(5).map_err(database_error)?),
        supports_image_input: sql_bool(row.get::<i64>(6).map_err(database_error)?),
        supports_structured_output: sql_bool(row.get::<i64>(7).map_err(database_error)?),
        supports_temperature: sql_bool(row.get::<i64>(8).map_err(database_error)?),
        context_limit: unsigned_count(row.get::<i64>(9).map_err(database_error)?),
        output_limit: unsigned_count(row.get::<i64>(10).map_err(database_error)?),
        reasoning_field: row.get::<String>(11).map_err(database_error)?,
        metadata_json: row.get::<String>(12).map_err(database_error)?,
        synced_at_ms: unsigned_ms(row.get::<i64>(13).map_err(database_error)?),
    })
}

pub(crate) fn model_group_from_row(row: &Row) -> HamburResult<ModelGroupRecord> {
    Ok(ModelGroupRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        name: row.get::<String>(1).map_err(database_error)?,
        routing_strategy: row.get::<String>(2).map_err(database_error)?,
        fallback_policy: row.get::<String>(3).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(5).map_err(database_error)?),
    })
}

pub(crate) fn model_group_member_from_row(row: &Row) -> HamburResult<ModelGroupMemberRecord> {
    Ok(ModelGroupMemberRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        group_id: row.get::<String>(1).map_err(database_error)?,
        provider_id: row.get::<String>(2).map_err(database_error)?,
        provider_name: row.get::<String>(3).map_err(database_error)?,
        model_id: row.get::<String>(4).map_err(database_error)?,
        model_display_name: row.get::<String>(5).map_err(database_error)?,
        position: unsigned_count(row.get::<i64>(6).map_err(database_error)?),
        enabled: sql_bool(row.get::<i64>(7).map_err(database_error)?),
    })
}

pub(crate) fn config_audit_from_row(row: &Row) -> HamburResult<ConfigAuditRecord> {
    Ok(ConfigAuditRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        command_id: row.get::<String>(1).map_err(database_error)?,
        actor: row.get::<String>(2).map_err(database_error)?,
        action: row.get::<String>(3).map_err(database_error)?,
        target_kind: row.get::<String>(4).map_err(database_error)?,
        target_id: row.get::<String>(5).map_err(database_error)?,
        redacted_summary: row.get::<String>(6).map_err(database_error)?,
        approval_required: sql_bool(row.get::<i64>(7).map_err(database_error)?),
        approval_token: row.get::<String>(8).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
    })
}

pub(crate) fn model_route_from_row(row: &Row) -> HamburResult<ModelRouteSnapshot> {
    Ok(ModelRouteSnapshot {
        provider_id: row.get::<String>(0).map_err(database_error)?,
        provider_name: row.get::<String>(1).map_err(database_error)?,
        provider_protocol: row.get::<String>(2).map_err(database_error)?,
        base_url: row.get::<String>(3).map_err(database_error)?,
        secret_ref: row.get::<String>(4).map_err(database_error)?,
        model_id: row.get::<String>(5).map_err(database_error)?,
        model_display_name: row.get::<String>(6).map_err(database_error)?,
        model_group_id: row.get::<String>(7).map_err(database_error)?,
        model_group_name: row.get::<String>(8).map_err(database_error)?,
        routing_strategy: row.get::<String>(9).map_err(database_error)?,
        fallback_policy: row.get::<String>(10).map_err(database_error)?,
        position: unsigned_count(row.get::<i64>(11).map_err(database_error)?),
        supports_tool_call: sql_bool(row.get::<i64>(12).map_err(database_error)?),
        supports_reasoning: sql_bool(row.get::<i64>(13).map_err(database_error)?),
        supports_image_input: sql_bool(row.get::<i64>(14).map_err(database_error)?),
        supports_structured_output: sql_bool(row.get::<i64>(15).map_err(database_error)?),
        supports_temperature: sql_bool(row.get::<i64>(16).map_err(database_error)?),
        context_limit: unsigned_count(row.get::<i64>(17).map_err(database_error)?),
        output_limit: unsigned_count(row.get::<i64>(18).map_err(database_error)?),
        reasoning_field: row.get::<String>(19).map_err(database_error)?,
    })
}

pub(crate) fn timeline_item_from_row(row: &Row) -> HamburResult<TimelineItemSnapshot> {
    Ok(TimelineItemSnapshot {
        id: row.get::<String>(0).map_err(database_error)?,
        stable_key: row.get::<String>(1).map_err(database_error)?,
        content_type: row.get::<String>(2).map_err(database_error)?,
        display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        payload_ref: row.get::<String>(5).map_err(database_error)?,
        small_summary: row.get::<String>(6).map_err(database_error)?,
        kind: row.get::<String>(7).map_err(database_error)?,
        trace_title: row.get::<String>(8).map_err(database_error)?,
        trace_content: row.get::<String>(9).map_err(database_error)?,
        trace_status: row.get::<String>(10).map_err(database_error)?,
        tool_call_id: row.get::<String>(11).map_err(database_error)?,
        tool_name: row.get::<String>(12).map_err(database_error)?,
        attachments: Vec::new(),
    })
}

pub(crate) fn markdown_block_from_row(row: &Row) -> HamburResult<MarkdownBlockPayloadRecord> {
    Ok(MarkdownBlockPayloadRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        message_id: row.get::<String>(2).map_err(database_error)?,
        block_id: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        stable_key: row.get::<String>(4).map_err(database_error)?,
        committed: row.get::<i64>(5).map_err(database_error)? != 0,
        payload_json: row.get::<String>(6).map_err(database_error)?,
        raw: row.get::<String>(7).map_err(database_error)?,
        small_summary: row.get::<String>(8).map_err(database_error)?,
        version_sequence: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(10).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(11).map_err(database_error)?),
    })
}

pub(crate) fn markdown_payload_refs(items: &[TimelineItemSnapshot]) -> Vec<String> {
    items
        .iter()
        .filter(|item| {
            item.content_type == "assistant_markdown_block"
                || item.content_type == "assistant_pending_block"
        })
        .map(|item| item.payload_ref.clone())
        .collect()
}

pub fn pending_markdown_stable_key(message_id: &str) -> String {
    format!("{message_id}:pending")
}

pub(crate) fn message_from_row(row: &Row) -> HamburResult<MessageRecord> {
    Ok(MessageRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        role: row.get::<String>(2).map_err(database_error)?,
        content_text: row.get::<String>(3).map_err(database_error)?,
        reasoning_content: row.get::<String>(4).map_err(database_error)?,
        status: row.get::<String>(5).map_err(database_error)?,
        turn_id: row.get::<String>(6).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
        version_sequence: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        provider_id_snapshot: row.get::<String>(9).map_err(database_error)?,
        provider_name_snapshot: row.get::<String>(10).map_err(database_error)?,
        provider_protocol: row.get::<String>(11).map_err(database_error)?,
        model_id_snapshot: row.get::<String>(12).map_err(database_error)?,
        model_name_snapshot: row.get::<String>(13).map_err(database_error)?,
        model_group_id: row.get::<String>(14).map_err(database_error)?,
        finish_reason: row.get::<String>(15).map_err(database_error)?,
        native_finish_reason: row.get::<String>(16).map_err(database_error)?,
        tool_call_id: row.get::<String>(17).map_err(database_error)?,
        tool_name: row.get::<String>(18).map_err(database_error)?,
        tool_title: row.get::<String>(19).map_err(database_error)?,
        attachments: Vec::new(),
    })
}

pub(crate) fn trace_span_from_row(row: &Row) -> HamburResult<TraceSpanRecord> {
    Ok(TraceSpanRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        parent_span_id: row.get::<String>(3).map_err(database_error)?,
        kind: row.get::<String>(4).map_err(database_error)?,
        title: row.get::<String>(5).map_err(database_error)?,
        content: row.get::<String>(6).map_err(database_error)?,
        status: row.get::<String>(7).map_err(database_error)?,
        started_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        ended_at_ms: row
            .get::<Option<i64>>(9)
            .map_err(database_error)?
            .map(unsigned_ms)
            .unwrap_or_default(),
        tool_call_id: row.get::<String>(10).map_err(database_error)?,
        payload_json: row.get::<String>(11).map_err(database_error)?,
    })
}

pub(crate) fn tool_call_from_row(row: &Row) -> HamburResult<ToolCallRecord> {
    Ok(ToolCallRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        assistant_message_id: row.get::<String>(3).map_err(database_error)?,
        name: row.get::<String>(4).map_err(database_error)?,
        arguments_json: row.get::<String>(5).map_err(database_error)?,
        display_title: row.get::<String>(6).map_err(database_error)?,
        status: row.get::<String>(7).map_err(database_error)?,
        requires_approval: sql_bool(row.get::<i64>(8).map_err(database_error)?),
        approval_status: row.get::<String>(9).map_err(database_error)?,
        started_at_ms: unsigned_ms(row.get::<i64>(10).map_err(database_error)?),
        ended_at_ms: row
            .get::<Option<i64>>(11)
            .map_err(database_error)?
            .map(unsigned_ms)
            .unwrap_or_default(),
        result_id: row.get::<String>(12).map_err(database_error)?,
        error_code: row.get::<String>(13).map_err(database_error)?,
        error_message: row.get::<String>(14).map_err(database_error)?,
        call_index: unsigned_count(row.get::<i64>(15).map_err(database_error)?),
    })
}

pub(crate) fn tool_result_from_row(row: &Row) -> HamburResult<ToolResultRecord> {
    Ok(ToolResultRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        turn_id: row.get::<String>(2).map_err(database_error)?,
        tool_call_id: row.get::<String>(3).map_err(database_error)?,
        message_id: row.get::<String>(4).map_err(database_error)?,
        is_error: sql_bool(row.get::<i64>(5).map_err(database_error)?),
        content_json: row.get::<String>(6).map_err(database_error)?,
        summary: row.get::<String>(7).map_err(database_error)?,
        artifacts_json: row.get::<String>(8).map_err(database_error)?,
        trust_level: row.get::<String>(9).map_err(database_error)?,
        truncated: sql_bool(row.get::<i64>(10).map_err(database_error)?),
        offloaded_file_id: row.get::<String>(11).map_err(database_error)?,
        offloaded_path: row.get::<String>(12).map_err(database_error)?,
        context_stub: row.get::<String>(13).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(14).map_err(database_error)?),
    })
}

pub(crate) fn file_from_row(row: &Row) -> HamburResult<FileRecord> {
    Ok(FileRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        scope: row.get::<String>(1).map_err(database_error)?,
        session_id: row.get::<String>(2).map_err(database_error)?,
        relative_path: row.get::<String>(3).map_err(database_error)?,
        sandbox_path: row.get::<String>(4).map_err(database_error)?,
        mime_type: row.get::<String>(5).map_err(database_error)?,
        byte_size: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        sha256: row.get::<String>(7).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(8).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(9).map_err(database_error)?),
        retention_policy: row.get::<String>(10).map_err(database_error)?,
    })
}

pub(crate) fn attachment_from_row(row: &Row) -> HamburResult<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        session_id: row.get::<String>(1).map_err(database_error)?,
        message_id: row.get::<String>(2).map_err(database_error)?,
        kind: row.get::<String>(3).map_err(database_error)?,
        display_name: row.get::<String>(4).map_err(database_error)?,
        mime_type: row.get::<String>(5).map_err(database_error)?,
        byte_size: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        origin_type: row.get::<String>(7).map_err(database_error)?,
        original_uri: row.get::<String>(8).map_err(database_error)?,
        file_id: row.get::<String>(9).map_err(database_error)?,
        sandbox_path: row.get::<String>(10).map_err(database_error)?,
        width: unsigned_count(row.get::<i64>(11).map_err(database_error)?),
        height: unsigned_count(row.get::<i64>(12).map_err(database_error)?),
        sha256: row.get::<String>(13).map_err(database_error)?,
        status: row.get::<String>(14).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(15).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(16).map_err(database_error)?),
    })
}

pub(crate) fn file_cleanup_job_from_row(row: &Row) -> HamburResult<FileCleanupJobRecord> {
    Ok(FileCleanupJobRecord {
        id: row.get::<String>(0).map_err(database_error)?,
        file_id: row.get::<String>(1).map_err(database_error)?,
        relative_path: row.get::<String>(2).map_err(database_error)?,
        reason: row.get::<String>(3).map_err(database_error)?,
        status: row.get::<String>(4).map_err(database_error)?,
        attempts: unsigned_count(row.get::<i64>(5).map_err(database_error)?),
        created_at_ms: unsigned_ms(row.get::<i64>(6).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(7).map_err(database_error)?),
    })
}
