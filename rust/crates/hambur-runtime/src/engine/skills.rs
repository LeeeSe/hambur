use crate::*;

impl RuntimeEngine {
    pub(crate) fn resolve_knowledge_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let tool_name = invocation
            .kind
            .map(ToolKind::name)
            .unwrap_or(invocation.name.as_str());
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(tool_name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }
        let result = match tool_name {
            "skills_list" => {
                let category = arguments
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let disabled = self.disabled_skill_paths();
                let all_skills = self
                    .list_skills_with_disabled(&disabled)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|skill| skill.enabled)
                    .collect::<Vec<_>>();
                let mut categories = all_skills
                    .iter()
                    .filter_map(|skill| {
                        (!skill.category.is_empty()).then(|| skill.category.clone())
                    })
                    .collect::<Vec<_>>();
                categories.sort();
                categories.dedup();
                let skills = all_skills
                    .into_iter()
                    .filter(|skill| category.is_empty() || skill.category == category)
                    .map(skill_summary_json)
                    .collect::<Vec<_>>();
                let count = skills.len();
                Ok(json!({
                    "success": true,
                    "skills": skills,
                    "categories": categories,
                    "count": count,
                    "hint": "Use skill_view(name) to see full content, tags, and linked files.",
                    "summary": format!("{count} skills")
                }))
            }
            "skill_view" => {
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let file_path = arguments
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let disabled = self.disabled_skill_paths();
                self.get_skill_detail_with_disabled(name, file_path, &disabled)
                    .map(|detail| skill_detail_json(detail, !file_path.trim().is_empty()))
            }
            "memory" => self.memory_tool_result(arguments),
            _ => Err(HamburError::InvalidCommand(format!(
                "unknown knowledge tool: {}",
                invocation.name
            ))),
        };
        match result {
            Ok(value) => ToolResult {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !value
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                summary: value
                    .get("summary")
                    .or_else(|| value.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("knowledge tool completed")
                    .to_string(),
                content_json: value.to_string(),
                artifacts_json: "[]".to_string(),
                trust_level: "trusted".to_string(),
                truncated: false,
                offloaded_file_id: String::new(),
                offloaded_path: String::new(),
                context_stub: value.to_string(),
            },
            Err(error) => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            ),
        }
    }

    pub fn delete_skill(&self, identifier: String) -> RuntimeCommandAck {
        let command = RuntimeCommand {
            kind: "DeleteSkill".to_string(),
            message_id: identifier.clone(),
            idempotency_key: format!("skill:{identifier}:delete:{}", new_id("attempt")),
            ..RuntimeCommand::default()
        };
        self.dispatch(command)
    }

    pub(crate) fn skills_root(&self) -> PathBuf {
        PathBuf::from(&self.bootstrap.app_files_dir)
            .join("sandbox")
            .join("global")
            .join("skills")
    }

    pub(crate) fn ensure_seeded_skills(&self) -> HamburResult<()> {
        seed_bundled_skills(&self.skills_root())
    }

    pub(crate) fn delete_skill_internal(&self, identifier: &str) -> HamburResult<String> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let disabled = self.disabled_skill_paths();
        let skill_file = self.resolve_skill_file(&root, identifier, &disabled)?;
        let skill_dir = skill_file.parent().ok_or_else(|| {
            HamburError::InvalidCommand(format!("skill directory not found: {identifier}"))
        })?;
        if !skill_dir.join("SKILL.md").is_file() {
            return Err(HamburError::InvalidCommand(format!(
                "skill not found: {identifier}"
            )));
        }
        let relative = relative_path(&root, &skill_file)?.replace('\\', "/");
        fs::remove_dir_all(skill_dir)
            .map_err(|error| HamburError::Internal(format!("delete skill directory: {error}")))?;
        Ok(relative)
    }

    pub(crate) fn resolve_skill_file(
        &self,
        root: &PathBuf,
        identifier: &str,
        disabled: &HashSet<String>,
    ) -> HamburResult<PathBuf> {
        let raw = identifier.trim();
        let normalized = raw
            .strip_prefix(SANDBOX_SKILLS_PATH)
            .unwrap_or(raw)
            .trim_start_matches('/')
            .strip_prefix("skills/")
            .unwrap_or_else(|| {
                raw.strip_prefix(SANDBOX_SKILLS_PATH)
                    .unwrap_or(raw)
                    .trim_start_matches('/')
            });
        if normalized.is_empty()
            || normalized.contains("..")
            || normalized.contains('\\')
            || normalized.starts_with('/')
        {
            return Err(HamburError::InvalidCommand(
                "invalid skill identifier".to_string(),
            ));
        }
        let candidates = [
            normalized.to_string(),
            format!("{normalized}/SKILL.md"),
            format!("{normalized}.md"),
        ];
        for candidate in candidates {
            let path = safe_join(root, &candidate)?;
            if path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            {
                return Ok(path);
            }
        }
        let lowered = normalized.to_ascii_lowercase();
        for skill in self.list_skills_with_disabled(disabled)? {
            let path_without_file = skill.path.trim_end_matches("/SKILL.md");
            if skill.name.eq_ignore_ascii_case(&lowered)
                || skill.name.eq_ignore_ascii_case(normalized)
                || path_without_file.eq_ignore_ascii_case(normalized)
                || path_without_file
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(normalized))
            {
                return safe_join(root, &skill.path);
            }
        }
        Err(HamburError::InvalidCommand(format!(
            "skill not found: {identifier}"
        )))
    }

    pub(crate) fn load_skill_from_file(
        &self,
        root: &PathBuf,
        skill_file: &PathBuf,
        disabled: &HashSet<String>,
    ) -> HamburResult<RuntimeSkillDetail> {
        let raw = fs::read_to_string(skill_file).map_err(|error| {
            HamburError::Internal(format!("read skill {}: {error}", skill_file.display()))
        })?;
        let path = relative_path(root, skill_file)?.replace('\\', "/");
        let skill_dir_path = path.trim_end_matches("/SKILL.md").to_string();
        let skill_dir = root.join(&skill_dir_path);
        let frontmatter = parse_frontmatter(&raw);
        let body = strip_frontmatter(&raw);
        let name = frontmatter
            .get("name")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_else(|| {
                skill_dir_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("skill")
                    .to_string()
            });
        let description = frontmatter
            .get("description")
            .and_then(|values| values.first())
            .cloned()
            .unwrap_or_default();
        let tags = frontmatter.get("tags").cloned().unwrap_or_default();
        let category = skill_dir_path
            .rsplit_once('/')
            .map(|(category, _)| category.to_string())
            .unwrap_or_default();
        let files = list_relative_files(&skill_dir)?;
        let modified_at_ms = files
            .iter()
            .filter_map(|file| fs::metadata(skill_dir.join(file)).ok())
            .filter_map(|metadata| metadata.modified().ok())
            .filter_map(system_time_to_ms)
            .max()
            .unwrap_or_default();
        let created_at_ms = fs::metadata(&skill_dir)
            .ok()
            .and_then(|metadata| metadata.created().ok())
            .and_then(system_time_to_ms)
            .unwrap_or(modified_at_ms);
        let linked_files_json = linked_skill_files_json(&skill_dir);
        Ok(RuntimeSkillDetail {
            summary: RuntimeSkillSummary {
                name,
                description: description
                    .chars()
                    .take(SKILL_MAX_DESCRIPTION_CHARS)
                    .collect(),
                path: path.clone(),
                category,
                tags,
                built_in: is_bundled_skill_path(&path),
                enabled: !disabled.contains(&path),
                created_at_ms,
                modified_at_ms,
                files,
            },
            content: body,
            raw_content: raw,
            skill_dir_path,
            linked_files_json,
            selected_file_path: String::new(),
            selected_file_content: String::new(),
        })
    }

    pub(crate) fn disabled_skill_paths(&self) -> HashSet<String> {
        self.database.settings_snapshot()
            .map(disabled_skill_paths_from_snapshot)
            .unwrap_or_default()
    }

    pub(crate) fn disabled_tool_names(&self) -> HashSet<String> {
        self.database.settings_snapshot()
            .map(|snapshot| disabled_tool_names_from_snapshot(&snapshot))
            .unwrap_or_default()
    }

    pub(crate) fn compile_enabled_main_tools_json(&self) -> String {
        let disabled = self.disabled_tool_names();
        self.tools
            .schemas()
            .compile_openai_tools_json_excluding(&disabled)
    }

    pub(crate) fn compile_enabled_delegate_tools_json(&self) -> String {
        let disabled = self.disabled_tool_names();
        self.tools
            .schemas()
            .compile_delegate_openai_tools_json_excluding(&disabled)
    }

    pub(crate) fn build_skills_index_prompt(&self) -> String {
        let disabled = self.disabled_skill_paths();
        let skills = self
            .list_skills_with_disabled(&disabled)
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect::<Vec<_>>();
        format_skills_index_prompt(skills)
    }
}
