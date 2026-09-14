use crate::*;

impl RuntimeEngine {
    pub(crate) fn resolve_hambur_config_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let value = match self.hambur_config_response(arguments) {
            Ok(value) => value,
            Err(error) => json!({
                "ok": false,
                "error": "validation_failed",
                "reason": error.to_string()
            }),
        };
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: value
                .get("ok")
                .and_then(Value::as_bool)
                .map(|ok| !ok)
                .unwrap_or(false),
            summary: value
                .get("user_message")
                .or_else(|| value.get("reason"))
                .or_else(|| value.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or("hambur_config completed")
                .to_string(),
            content_json: value.to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: value.to_string(),
        }
    }

    pub(crate) fn hambur_config_response(&self, arguments: &Value) -> HamburResult<Value> {
        let action = config_string(arguments, "action");
        match action.as_str() {
            "list_topics" => Ok(json!({
                "ok": true,
                "data": {
                    "topics": hambur_config_topics()
                }
            })),
            "topic_help" => {
                let topic = normalize_hambur_config_topic(
                    &config_string(arguments, "topic").if_blank(config_string(arguments, "path")),
                );
                if topic.is_empty() {
                    return Ok(config_error("validation_failed", "topic is required."));
                }
                let fields = hambur_config_fields()
                    .into_iter()
                    .filter(|field| field.topic == topic)
                    .map(HamburConfigFieldSpec::to_json)
                    .collect::<Vec<_>>();
                if fields.is_empty() {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No registered topic '{topic}'."),
                    ));
                }
                Ok(json!({
                    "ok": true,
                    "data": {
                        "topic": topic,
                        "fields": fields
                    }
                }))
            }
            "get" => {
                let path = normalize_hambur_config_path(&config_string(arguments, "path"));
                if path.is_empty() {
                    return Ok(config_error("validation_failed", "path is required."));
                }
                let field = hambur_config_field_for(&path);
                let Some(field) = field else {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No registered field at '{path}'."),
                    ));
                };
                if field.access == "write-only" {
                    return Ok(config_error(
                        "permission_denied",
                        &format!(
                            "permission_denied: {} is write-only and cannot be read back.",
                            field.path
                        ),
                    ));
                }
                let snapshot = self.database.settings_snapshot()?;
                Ok(read_hambur_config_path(
                    &snapshot,
                    &path,
                    &field,
                    &config_string(arguments, "filter"),
                    config_u32(arguments, "page", 1),
                    config_u32(arguments, "page_size", 20),
                ))
            }
            "set" | "append" | "remove" => {
                let mut path = config_string(arguments, "path");
                if action == "append" && !path.ends_with(".append") {
                    path.push_str(".append");
                }
                if action == "remove" && !path.ends_with(".remove") {
                    path.push_str(".remove");
                }
                let result = self
                    .apply_hambur_config_mutation(
                        &path,
                        &config_string(arguments, "value_json"),
                        &config_string(arguments, "actor").if_blank("agent".to_string()),
                        &config_string(arguments, "caption"),
                    )?;
                Ok(result)
            }
            "set_batch" => {
                let Some(items) = arguments.get("batch").and_then(Value::as_array) else {
                    return Ok(config_error(
                        "validation_failed",
                        "batch is required and cannot be empty.",
                    ));
                };
                if items.is_empty() {
                    return Ok(config_error(
                        "validation_failed",
                        "batch is required and cannot be empty.",
                    ));
                }
                let actor = config_string(arguments, "actor").if_blank("agent".to_string());
                let caption = config_string(arguments, "caption");
                let mut results = Vec::new();
                for item in items {
                    let item_path = config_string(item, "path");
                    let item_caption = config_string(item, "caption").if_blank(caption.clone());
                    let outcome = self
                        .apply_hambur_config_mutation(
                            &item_path,
                            &config_string(item, "value_json"),
                            &actor,
                            &item_caption,
                        )?;
                    if !outcome.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                        return Ok(json!({
                            "ok": false,
                            "error": outcome.get("error").cloned().unwrap_or_else(|| json!("validation_failed")),
                            "reason": format!("Batch failed at '{}': {}", item_path, outcome.get("reason").and_then(Value::as_str).unwrap_or("unknown error")),
                            "results": results
                        }));
                    }
                    results.push(outcome);
                }
                Ok(json!({
                    "ok": true,
                    "count": results.len(),
                    "results": results,
                    "user_message": format!("已更新 {} 项 Hambur 配置。", results.len())
                }))
            }
            "audit_list" => {
                let snapshot = self.database.settings_snapshot()?;
                let scope = normalize_hambur_config_topic(&config_string(arguments, "scope"));
                let limit = config_u32(arguments, "limit", 50).clamp(1, 200) as usize;
                let entries = snapshot
                    .config_audits
                    .iter()
                    .filter(|entry| {
                        scope.is_empty()
                            || normalize_hambur_config_topic(&entry.target_kind) == scope
                    })
                    .take(limit)
                    .map(config_audit_json)
                    .collect::<Vec<_>>();
                Ok(json!({
                    "ok": true,
                    "count": entries.len(),
                    "capacity": 40,
                    "total_used": snapshot.config_audits.len(),
                    "entries": entries
                }))
            }
            "audit_get" => {
                let audit_id = config_string(arguments, "audit_id");
                if audit_id.is_empty() {
                    return Ok(config_error("validation_failed", "audit_id is required."));
                }
                let snapshot = self.database.settings_snapshot()?;
                let Some(entry) = snapshot
                    .config_audits
                    .iter()
                    .find(|entry| entry.id == audit_id)
                else {
                    return Ok(config_error(
                        "unknown_path",
                        &format!("No audit entry '{audit_id}'."),
                    ));
                };
                Ok(json!({
                    "ok": true,
                    "entry": config_audit_json(entry)
                }))
            }
            "audit_revert" => Ok(config_error(
                "permission_denied",
                "audit_revert is not implemented in the Rust settings backend yet.",
            )),
            _ => Ok(config_error(
                "validation_failed",
                &format!("Unknown hambur_config action: {action}"),
            )),
        }
    }

    pub(crate) fn apply_hambur_config_mutation(
        &self,
        raw_path: &str,
        value_json: &str,
        actor: &str,
        caption: &str,
    ) -> HamburResult<Value> {
        let (path, operation) = parse_hambur_config_operation(raw_path);
        if path.is_empty() {
            return Ok(config_error("validation_failed", "path is required."));
        }
        let Some(field) = hambur_config_field_for(&path) else {
            return Ok(config_error(
                "unknown_path",
                &format!("No registered field at '{raw_path}'."),
            ));
        };
        if field.access == "readonly" {
            return Ok(config_error(
                "permission_denied",
                &format!("Field '{path}' is readonly."),
            ));
        }
        if operation != ConfigOperation::Set {
            return Ok(config_error(
                "validation_failed",
                &format!("'{path}' does not support {}.", operation.as_str()),
            ));
        }

        let snapshot = self.database.settings_snapshot()?;
        let old = read_hambur_config_raw_value(&snapshot, &path);
        let setting = match app_setting_for_hambur_config_path(&path, value_json) {
            Some(setting) => setting,
            None => {
                return Ok(config_error(
                    "unknown_path",
                    &format!("No writable field at '{path}'."),
                ));
            }
        };
        let record = self
            .database
            .upsert_app_setting(&setting.0, &setting.1)?;
        let new_value = serde_json::to_string(&record.value).unwrap_or_else(|_| record.value);
        let summary = format!(
            "{} {}\nPath: {}\nOld: {}\nNew: {}",
            operation.as_str(),
            field.display_name,
            path,
            old.chars().take(240).collect::<String>(),
            new_value.chars().take(240).collect::<String>()
        );
        let audit = self
            .database
            .insert_config_audit(
                &new_id("config_tool"),
                if actor.trim().is_empty() {
                    "agent"
                } else {
                    actor
                },
                "HamburConfigTool",
                field.topic,
                &path,
                if caption.trim().is_empty() {
                    &summary
                } else {
                    caption
                },
                false,
                "",
            )?;
                let _ = self.emit_plain(
            RuntimeEventKind::SettingsChanged,
            String::new(),
            String::new(),
            format!("Setting updated: {}", path),
        );
        Ok(json!({
            "ok": true,
            "path": path,
            "old": old,
            "new": new_value,
            "audit_id": audit.id,
            "user_message": format!("已更新 Hambur 配置：{}", path)
        }))
    }
}
