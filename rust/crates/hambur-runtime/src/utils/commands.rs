use crate::*;

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
        "DeleteProviderModel" | "DeleteModel" => {
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
    pub(crate) sha256: String,
}

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
        let kind = get_string(&["kind"]).if_blank(if mime_type.starts_with("image/") {
            "image".to_string()
        } else {
            "file".to_string()
        });
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
    pub(crate) search_enabled: bool,
}

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
