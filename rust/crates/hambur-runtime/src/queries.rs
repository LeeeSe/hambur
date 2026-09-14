use crate::*;

impl RuntimeEngine {
    pub(crate) fn get_startup_tasks_and_enabled(&self) -> (Vec<hambur_sandbox::StartupTask>, bool) {
        let settings_snap = self.database.settings_snapshot()
            .unwrap_or_default();
        let mut tasks = Vec::new();
        let mut enabled = true;
        for setting in &settings_snap.settings {
            if setting.key == "startupTasksEnabled" {
                enabled = setting.value == "true";
            } else if setting.key.starts_with("startup_task:") {
                if let Ok(task) =
                    serde_json::from_str::<hambur_sandbox::StartupTask>(&setting.value)
                {
                    tasks.push(task);
                }
            }
        }
        (tasks, enabled)
    }
    pub(crate) fn list_process_sessions(&self, invocation: &ToolInvocation) -> ToolResult {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process registry unavailable",
                );
            }
        };
        let mut values = Vec::new();
        for state in sessions
            .values_mut()
            .filter(|state| state.session_id == invocation.session_id)
        {
            refresh_process_exit(state);
            values.push(process_status_json(state, None));
        }
        if let Ok(completed_sessions) = self.completed_process_sessions.lock() {
            values.extend(
                completed_sessions
                    .values()
                    .filter(|state| state.session_id == invocation.session_id)
                    .map(completed_process_status_json),
            );
        }
        let content = json!({ "processes": values });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: format!("{} process sessions", values.len()),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }
    pub fn get_session_list_snapshot(&self, limit: u32, offset: u32) -> RuntimeSessionListSnapshot {
        let sessions = self.database.session_list(limit, offset)
            .unwrap_or_default();
        let selected_session_id = self.database.bootstrap_snapshot()
            .map(|snapshot| snapshot.selected_session_id)
            .unwrap_or_default();
        RuntimeSessionListSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            sessions,
            selected_session_id,
        }
    }

    pub fn get_session_snapshot(&self, session_id: String) -> RuntimeSessionSnapshot {
        let session = self.database.session_summary(&session_id)
            .ok();
        let snapshot = self.database.session_snapshot(&session_id)
            .unwrap_or_default();
        RuntimeSessionSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            session,
            timeline_items: snapshot.timeline_items,
            message_block_payloads: snapshot.message_block_payloads,
        }
    }

    pub fn get_timeline_page(
        &self,
        session_id: String,
        before_cursor: u64,
        limit: u32,
    ) -> RuntimeTimelinePage {
        let page = self.database
                    .timeline_page(&session_id, before_cursor, limit)
            .unwrap_or_default();
        RuntimeTimelinePage {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            session_id,
            items: page.items,
            message_block_payloads: page.message_block_payloads,
            next_before_cursor: page.next_before_cursor,
            has_more: page.has_more,
        }
    }

    pub fn get_message_snapshot(&self, message_id: String) -> RuntimeMessageSnapshot {
        RuntimeMessageSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            message: self.database.message_snapshot(&message_id)
                .unwrap_or_default(),
        }
    }

    pub fn search_sessions(&self, query: String, limit: u32) -> RuntimeSearchSnapshot {
        RuntimeSearchSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            sessions: self.database.search_sessions(&query, limit)
                .unwrap_or_default(),
            query,
        }
    }

    pub fn get_settings_snapshot(&self) -> RuntimeSettingsSnapshot {
        RuntimeSettingsSnapshot {
            snapshot_sequence: self.snapshot_sequence(),
            created_at_ms: now_ms(),
            settings: self.database.settings_snapshot()
                .unwrap_or_default(),
        }
    }

    pub fn resolve_sandbox_file(
        &self,
        session_id: String,
        sandbox_path: String,
    ) -> RuntimeFileResolution {
        let clean_path = normalize_tool_sandbox_path(&sandbox_path);
        let resolved = match self
            .sandbox
            .resolve(&session_id, &clean_path, SandboxAccess::Read)
        {
            Ok(resolved) => resolved,
            Err(_) => return RuntimeFileResolution::default(),
        };
        let metadata = fs::metadata(&resolved.host_path).ok();
        let db_file = self.database
                    .resolve_file_by_sandbox_path(&session_id, &resolved.sandbox_path)
            .ok();
        let mime_type = db_file
            .as_ref()
            .map(|file| file.mime_type.clone())
            .unwrap_or_else(|| detect_mime_type(&resolved.host_path).to_string());
        RuntimeFileResolution {
            sandbox_path: resolved.sandbox_path,
            host_path: resolved.host_path.to_string_lossy().to_string(),
            relative_path: resolved.relative_path,
            root: resolved.root,
            writable: resolved.writable,
            exists: metadata.is_some(),
            is_file: metadata.as_ref().is_some_and(|metadata| metadata.is_file()),
            mime_type,
            byte_size: metadata
                .as_ref()
                .map(|metadata| metadata.len())
                .or_else(|| db_file.as_ref().map(|file| file.byte_size))
                .unwrap_or_default(),
            file_id: db_file.map(|file| file.id).unwrap_or_default(),
        }
    }

    pub fn get_rootfs_status(&self) -> RuntimeRootfsStatus {
        let settings_snap = self.database.settings_snapshot()
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        eprintln!(
            "RootfsDebug get_rootfs_status_start app_files_dir={} requested_backend={}",
            self.app_files_dir(),
            requested_backend
        );

        // Dynamically probe status
        let probed = self.sandbox.probe_rootfs_status(requested_backend);

        let rootfs_installed = self.sandbox.is_rootfs_installed();

        let root_available = self.sandbox.probe_root_available();
        let chroot_available = self.sandbox.probe_chroot_available();
        let proot_available = self.sandbox.probe_proot_available();

        let version = if rootfs_installed {
            let version_file = self.sandbox.rootfs_dir().join(".hambur-rootfs.version");
            fs::read_to_string(&version_file)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let size_bytes = rootfs_storage_size(self.sandbox.rootfs_dir());
        eprintln!(
            "RootfsDebug get_rootfs_status_result installed={} backend={} probed_available={} probed_reason={} root_available={} chroot_available={} proot_available={} version={} size={} rootfs_path={}",
            rootfs_installed,
            probed.backend,
            probed.available,
            probed.reason,
            root_available,
            chroot_available,
            proot_available,
            version,
            size_bytes,
            self.sandbox.rootfs_dir().display()
        );

        RuntimeRootfsStatus {
            rootfs_installed,
            proot_available,
            root_available,
            chroot_available,
            backend: probed.backend,
            version,
            rootfs_size_bytes: size_bytes,
            rootfs_path: self.sandbox.rootfs_dir().to_string_lossy().to_string(),
        }
    }

    pub fn list_skills(&self) -> Vec<RuntimeSkillSummary> {
        self.list_skills_internal().unwrap_or_default()
    }

    pub fn get_skill_detail(&self, identifier: String, file_path: String) -> RuntimeSkillDetail {
        self.get_skill_detail_internal(&identifier, &file_path)
            .unwrap_or_default()
    }

    pub fn list_memory_files(&self) -> Vec<RuntimeMemoryFileSummary> {
        self.list_memory_files_internal().unwrap_or_default()
    }

    pub fn get_memory_file_detail(&self, name: String) -> RuntimeMemoryFileDetail {
        self.get_memory_file_detail_internal(&name)
            .unwrap_or_default()
    }

    pub(crate) fn list_skills_internal(&self) -> HamburResult<Vec<RuntimeSkillSummary>> {
        let disabled = self.disabled_skill_paths();
        self.list_skills_with_disabled(&disabled)
    }
    pub(crate) fn list_skills_with_disabled(
        &self,
        disabled: &HashSet<String>,
    ) -> HamburResult<Vec<RuntimeSkillSummary>> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let mut skill_files = Vec::new();
        collect_named_files(&root, "SKILL.md", &mut skill_files)?;
        let mut skills = skill_files
            .into_iter()
            .filter_map(|path| self.load_skill_from_file(&root, &path, disabled).ok())
            .map(|detail| detail.summary)
            .collect::<Vec<_>>();
        skills.sort_by(|a, b| {
            a.category
                .cmp(&b.category)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(skills)
    }
    pub(crate) fn get_skill_detail_internal(
        &self,
        identifier: &str,
        selected_file_path: &str,
    ) -> HamburResult<RuntimeSkillDetail> {
        let disabled = self.disabled_skill_paths();
        self.get_skill_detail_with_disabled(identifier, selected_file_path, &disabled)
    }
    pub(crate) fn get_skill_detail_with_disabled(
        &self,
        identifier: &str,
        selected_file_path: &str,
        disabled: &HashSet<String>,
    ) -> HamburResult<RuntimeSkillDetail> {
        let root = self.skills_root();
        self.ensure_seeded_skills()?;
        let skill_file = self.resolve_skill_file(&root, identifier, disabled)?;
        let mut detail = self.load_skill_from_file(&root, &skill_file, disabled)?;
        let file_path = selected_file_path.trim().trim_start_matches('/');
        if !file_path.is_empty() {
            let skill_dir = root.join(&detail.skill_dir_path);
            let selected = safe_join(&skill_dir, file_path)?;
            if !selected.is_file() {
                return Err(HamburError::InvalidCommand(format!(
                    "skill file not found: {file_path}"
                )));
            }
            let metadata = fs::metadata(&selected).map_err(|error| {
                HamburError::Internal(format!("read skill file metadata: {error}"))
            })?;
            if metadata.len() > SKILL_MAX_LINKED_FILE_BYTES {
                return Err(HamburError::InvalidCommand(
                    "skill file is too large to load".to_string(),
                ));
            }
            detail.selected_file_path = file_path.to_string();
            detail.selected_file_content = fs::read_to_string(&selected).map_err(|error| {
                HamburError::Internal(format!("read skill file {}: {error}", selected.display()))
            })?;
        }
        Ok(detail)
    }
    pub(crate) fn list_memory_files_internal(&self) -> HamburResult<Vec<RuntimeMemoryFileSummary>> {
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let mut files = Vec::new();
        for entry in fs::read_dir(&root)
            .map_err(|error| HamburError::Internal(format!("read memory root: {error}")))?
        {
            let entry = entry
                .map_err(|error| HamburError::Internal(format!("read memory entry: {error}")))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".md") {
                continue;
            }
            let raw = read_memory_file_content(&path)?;
            let metadata = fs::metadata(&path)
                .map_err(|error| HamburError::Internal(format!("memory metadata: {error}")))?;
            files.push(RuntimeMemoryFileSummary {
                name: name.to_string(),
                size_bytes: metadata.len(),
                modified_at_ms: metadata
                    .modified()
                    .ok()
                    .and_then(system_time_to_ms)
                    .unwrap_or_default(),
                entry_count: count_memory_entries(&raw),
                preview: memory_preview(&raw),
            });
        }
        files.sort_by(|a, b| {
            memory_file_sort_priority(&a.name)
                .cmp(&memory_file_sort_priority(&b.name))
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
        });
        Ok(files)
    }
    pub(crate) fn get_memory_file_detail_internal(
        &self,
        name: &str,
    ) -> HamburResult<RuntimeMemoryFileDetail> {
        let root = self.memory_root();
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create memory root: {error}")))?;
        let clean = name.trim();
        if clean.is_empty()
            || clean.contains('/')
            || clean.contains('\\')
            || !clean.to_ascii_lowercase().ends_with(".md")
        {
            return Err(HamburError::InvalidCommand(format!(
                "invalid memory file name: {name}"
            )));
        }
        let path = safe_join(&root, clean)?;
        if !path.is_file() {
            return Err(HamburError::InvalidCommand(format!(
                "memory file not found: {clean}"
            )));
        }
        let raw = read_memory_file_content(&path)?;
        let metadata = fs::metadata(&path)
            .map_err(|error| HamburError::Internal(format!("memory metadata: {error}")))?;
        Ok(RuntimeMemoryFileDetail {
            name: clean.to_string(),
            size_bytes: metadata.len(),
            modified_at_ms: metadata
                .modified()
                .ok()
                .and_then(system_time_to_ms)
                .unwrap_or_default(),
            entry_count: count_memory_entries(&raw),
            content: raw,
        })
    }
    pub(crate) fn search_sandbox_files(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> HamburResult<Value> {
        let pattern = arguments
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if pattern.is_empty() {
            return Err(HamburError::InvalidCommand(
                "pattern must not be empty".to_string(),
            ));
        }
        let target = arguments
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("content");
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("/var/hambur/workspace");
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        let file_glob = arguments
            .get("file_glob")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let output_mode = arguments
            .get("output_mode")
            .and_then(Value::as_str)
            .unwrap_or("content");
        let context = arguments
            .get("context")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let clean_path = path.trim().trim_end_matches('/');
        let resolved_roots: Vec<hambur_sandbox::SandboxPathResolution> =
            if clean_path == "/var/hambur" || clean_path.is_empty() {
                let root_candidates = [
                    "/var/hambur/workspace",
                    "/var/hambur/shared",
                    "/var/hambur/attachments",
                    "/var/hambur/browser",
                    "/var/hambur/offloads",
                    "/var/hambur/skills",
                    "/var/hambur/download",
                ];
                let mut list = Vec::new();
                for r in root_candidates {
                    if let Ok(res) =
                        self.resolve_tool_sandbox_path(&invocation.session_id, r, SandboxAccess::Read)
                    {
                        if res.host_path.exists() {
                            list.push(res);
                        }
                    }
                }
                list
            } else {
                let res = self.resolve_tool_sandbox_path(
                    &invocation.session_id,
                    path,
                    SandboxAccess::Read,
                )?;
                if !res.host_path.exists() {
                    return Err(HamburError::InvalidCommand(format!(
                        "No such file or directory: {path}"
                    )));
                }
                vec![res]
            };
        let compiled_regex = if target == "content" {
            regex::RegexBuilder::new(pattern).build().ok()
        } else {
            None
        };
        let mut results = Vec::new();
        for resolved in resolved_roots {
            let mut files = Vec::new();
            if resolved.host_path.is_file() {
                files.push(resolved.host_path.clone());
            } else {
                collect_all_files(&resolved.host_path, &mut files)?;
            }
            if target == "files" {
                for file in files {
                    let file_name = file
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default();
                    let relative = file
                        .strip_prefix(&resolved.host_path)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if matches_file_pattern(pattern, file_name, &relative) {
                        results.push(json!({"path": path_for_search_result(&resolved, &file)}));
                    }
                }
            } else {
                for file in files {
                    let file_name = file
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default();
                    let relative = file
                        .strip_prefix(&resolved.host_path)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if let Some(glob) = file_glob {
                        if !matches_file_pattern(glob, file_name, &relative) {
                            continue;
                        }
                    }
                    let Ok(content) = fs::read_to_string(&file) else {
                        continue;
                    };
                    let all_lines: Vec<&str> = content.lines().collect();
                    let mut file_match_count = 0usize;
                    for (index, line) in all_lines.iter().enumerate() {
                        let is_match = if let Some(ref re) = compiled_regex {
                            re.is_match(line)
                        } else {
                            line.contains(pattern)
                        };
                        if is_match {
                            file_match_count += 1;
                            if output_mode == "content" {
                                let mut item = json!({
                                    "path": path_for_search_result(&resolved, &file),
                                    "line": index + 1,
                                    "content": line
                                });
                                if context > 0 {
                                    let start_ctx = index.saturating_sub(context);
                                    let end_ctx = (index + context + 1).min(all_lines.len());
                                    let context_snippet = all_lines[start_ctx..end_ctx]
                                        .iter()
                                        .enumerate()
                                        .map(|(offset, l)| format!("{}|{}", start_ctx + offset + 1, l))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    item["context"] = json!(context_snippet);
                                }
                                results.push(item);
                            }
                        }
                    }
                    if file_match_count > 0 {
                        if output_mode == "files_only" {
                            results.push(json!({"path": path_for_search_result(&resolved, &file)}));
                        } else if output_mode == "count" {
                            results.push(json!({
                                "path": path_for_search_result(&resolved, &file),
                                "count": file_match_count
                            }));
                        }
                    }
                }
            }
        }
        let total = results.len();
        let page = results
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(json!({
            "pattern": pattern,
            "target": target,
            "results": page,
            "total": total,
            "summary": format!("{} matches", total)
        }))
    }
}

fn rootfs_storage_size(root: &std::path::Path) -> u64 {
    rootfs_storage_size_inner(root, root)
}

fn rootfs_storage_size_inner(root: &std::path::Path, path: &std::path::Path) -> u64 {
    if should_skip_rootfs_size_path(root, path) {
        return 0;
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return 0,
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            size += rootfs_storage_size_inner(root, &entry.path());
        }
    }
    size
}

fn should_skip_rootfs_size_path(root: &std::path::Path, path: &std::path::Path) -> bool {
    let relative = match path.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_) => return true,
    };
    let mut components = relative.components();
    match components.next() {
        None => false,
        Some(std::path::Component::Normal(name))
            if name == "proc" || name == "sys" || name == "dev" =>
        {
            true
        }
        Some(std::path::Component::Normal(name)) if name == "var" => {
            matches!(
                components.next(),
                Some(std::path::Component::Normal(next)) if next == "hambur"
            )
        }
        _ => false,
    }
}
