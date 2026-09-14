use crate::*;

impl RuntimeEngine {
    pub(crate) fn start_background_process(
        &self,
        invocation: &ToolInvocation,
        command: &str,
        sandbox_cwd: &str,
    ) -> ToolResult {
        let (tasks, enabled) = self.get_startup_tasks_and_enabled();
        let settings_snap = self.database.settings_snapshot()
            .unwrap_or_default();
        let requested_backend = get_rootfs_backend(&settings_snap.settings);
        if let Err(error) = self
            .sandbox
            .ensure_initialized(&tasks, enabled, requested_backend)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("sandbox initialization failed: {error}"),
            );
        }
        self.sandbox.update_rootfs_status(requested_backend);
        self.sandbox.prewarm_chroot_if_available();
        let process_session_id = new_id("proc");
        let backend = self.sandbox.rootfs_status().backend;
        let pid_file = if backend == "chroot" {
            match self.sandbox.chroot_process_pid_file(&process_session_id) {
                Ok(path) => Some(path),
                Err(error) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("build execution command failed: {error}"),
                    );
                }
            }
        } else {
            None
        };

        let (program, args, envs) = match self.sandbox.build_execution_command(
            &invocation.session_id,
            command,
            sandbox_cwd,
            &process_session_id,
        ) {
            Ok(res) => res,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("build execution command failed: {error}"),
                );
            }
        };

        let mut cmd = Command::new(&program);
        cmd.args(&args);
        for (k, v) in envs {
            cmd.env(k, v);
        }

        let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(error) => {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    format!("terminal background spawn failed: {error}"),
                );
            }
        };
        let started_at_ms = now_ms();
        let pid = child.id();
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::default()));
        if let Some(stdout) = child.stdout.take() {
            spawn_process_pipe_reader(stdout, output.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_process_pipe_reader(stderr, output.clone(), false);
        }

        let state = BackgroundProcessSession {
            session_id: invocation.session_id.clone(),
            process_session_id: process_session_id.clone(),
            backend: self.sandbox.rootfs_status().backend.clone(),
            command: command.to_string(),
            cwd: sandbox_cwd.to_string(),
            started_at_ms,
            pid,
            pid_file,
            child,
            output,
            exit_code: None,
            finished_at_ms: 0,
        };
        if let Ok(mut sessions) = self.process_sessions.lock() {
            sessions.insert(process_session_id.clone(), state);
        } else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process registry unavailable",
            );
        }

        let content = json!({
            "processSessionId": process_session_id,
            "backend": backend,
            "command": command,
            "cwd": sandbox_cwd,
            "startedAt": started_at_ms,
            "pid": pid
        });
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "background process started".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    pub(crate) fn resolve_process_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let process_session_id = arguments
            .get("session_id")
            .or_else(|| arguments.get("process_session_id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        match action {
            "list" => self.list_process_sessions(invocation),
            "poll" | "log" => self.snapshot_process_session(invocation, process_session_id, false),
            "wait" => {
                let timeout_ms = argument_seconds_or_ms(arguments, "timeout", "timeout_ms", 30_000)
                    .clamp(1_000, 300_000);
                self.wait_process_session(invocation, process_session_id, timeout_ms)
            }
            "kill" | "close" => self.kill_process_session(invocation, process_session_id),
            "write" | "submit" => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process stdin is not attached for this backend",
            ),
            _ => ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("unsupported process action: {action}"),
            ),
        }
    }

    pub(crate) fn snapshot_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        remove_finished: bool,
    ) -> ToolResult {
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
        let Some(state) = sessions.get_mut(process_session_id) else {
            drop(sessions);
            if let Some(completed) = self.completed_process_snapshot(invocation, process_session_id)
            {
                return completed;
            }
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        refresh_process_exit(state);
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(state, Some(output));
        if remove_finished && state.exit_code.is_some() {
            sessions.remove(process_session_id);
        }
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session snapshot".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    pub(crate) fn wait_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
        timeout_ms: u64,
    ) -> ToolResult {
        let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
        loop {
            {
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
                let Some(state) = sessions.get_mut(process_session_id) else {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session not found",
                    );
                };
                if state.session_id != invocation.session_id {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        "process session belongs to a different session",
                    );
                }
                refresh_process_exit(state);
                if state.exit_code.is_some() {
                    let output = state
                        .output
                        .lock()
                        .map(|buffer| buffer.snapshot())
                        .unwrap_or_default();
                    let content = process_status_json(state, Some(output));
                    if let Some(completed) =
                        completed_process_session_from_state(state, process_session_id)
                    {
                        if let Ok(mut completed_sessions) = self.completed_process_sessions.lock() {
                            completed_sessions.insert(process_session_id.to_string(), completed);
                        }
                    }
                    sessions.remove(process_session_id);
                    return ToolResult {
                        tool_call_id: invocation.tool_call_id.clone(),
                        tool_name: invocation.name.clone(),
                        is_error: false,
                        content_json: content.to_string(),
                        summary: "process session finished".to_string(),
                        artifacts_json: "[]".to_string(),
                        trust_level: "untrusted".to_string(),
                        truncated: false,
                        offloaded_file_id: String::new(),
                        offloaded_path: String::new(),
                        context_stub: content.to_string(),
                    };
                }
            }
            if Instant::now() >= deadline {
                return ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    "process wait timed out",
                );
            }
            thread::sleep(StdDuration::from_millis(20));
        }
    }

    pub(crate) fn completed_process_snapshot(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
    ) -> Option<ToolResult> {
        let sessions = self.completed_process_sessions.lock().ok()?;
        let state = sessions.get(process_session_id)?;
        if state.session_id != invocation.session_id {
            return Some(ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            ));
        }
        let content = completed_process_status_json(state);
        Some(ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session snapshot".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        })
    }

    pub(crate) fn kill_process_session(
        &self,
        invocation: &ToolInvocation,
        process_session_id: &str,
    ) -> ToolResult {
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
        let Some(mut state) = sessions.remove(process_session_id) else {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session not found",
            );
        };
        if state.session_id != invocation.session_id {
            sessions.insert(process_session_id.to_string(), state);
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "process session belongs to a different session",
            );
        }
        terminate_background_process_wrapper(&state);
        let _ = state.child.kill();
        let _ = state.child.wait();
        state.exit_code = Some(-1);
        state.finished_at_ms = now_ms();
        let output = state
            .output
            .lock()
            .map(|buffer| buffer.snapshot())
            .unwrap_or_default();
        let content = process_status_json(&state, Some(output));
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "process session closed".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "untrusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub: content.to_string(),
        }
    }

    pub(crate) fn kill_processes_for_session(&self, session_id: &str) {
        let mut sessions = match self.process_sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => return,
        };
        let ids = sessions
            .iter()
            .filter_map(|(id, state)| {
                if state.session_id == session_id {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(mut state) = sessions.remove(&id) {
                terminate_background_process_wrapper(&state);
                let _ = state.child.kill();
                let _ = state.child.wait();
            }
        }
    }
}
