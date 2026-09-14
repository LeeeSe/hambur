use crate::*;

#[allow(dead_code)]
pub(crate) fn run_terminal_command(
    invocation: &ToolInvocation,
    command: &str,
    cwd: &std::path::Path,
    timeout_ms: u64,
) -> RawToolOutput {
    let shell = platform_shell();
    let mut child = match Command::new(shell)
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: true,
                content: error.to_string(),
                summary: "terminal execution failed".to_string(),
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: "spawn_failed".to_string(),
            };
        }
    };
    let deadline = Instant::now() + StdDuration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(StdDuration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: json!({
                        "command": command,
                        "cwd": cwd.to_string_lossy(),
                        "timeoutMs": timeout_ms,
                        "timedOut": true
                    })
                    .to_string(),
                    summary: "terminal timed out".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "timeout".to_string(),
                };
            }
            Err(error) => {
                let _ = child.kill();
                return RawToolOutput {
                    tool_call_id: invocation.tool_call_id.clone(),
                    tool_name: invocation.name.clone(),
                    is_error: true,
                    content: error.to_string(),
                    summary: "terminal wait failed".to_string(),
                    trust_level: "untrusted".to_string(),
                    command_or_url: command.to_string(),
                    status: "wait_failed".to_string(),
                };
            }
        }
    }

    match child.wait_with_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            let content = json!({
                "command": command,
                "cwd": cwd.to_string_lossy(),
                "timeoutMs": timeout_ms,
                "exitCode": exit_code,
                "stdout": stdout,
                "stderr": stderr
            })
            .to_string();
            RawToolOutput {
                tool_call_id: invocation.tool_call_id.clone(),
                tool_name: invocation.name.clone(),
                is_error: !output.status.success(),
                summary: if output.status.success() {
                    format!(
                        "terminal exited 0 (stdout {} bytes, stderr {} bytes)",
                        output.stdout.len(),
                        output.stderr.len()
                    )
                } else {
                    format!("terminal exited {exit_code}")
                },
                content,
                trust_level: "untrusted".to_string(),
                command_or_url: command.to_string(),
                status: exit_code.to_string(),
            }
        }
        Err(error) => RawToolOutput {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: true,
            content: error.to_string(),
            summary: "terminal execution failed".to_string(),
            trust_level: "untrusted".to_string(),
            command_or_url: command.to_string(),
            status: "spawn_failed".to_string(),
        },
    }
}

#[allow(dead_code)]
pub(crate) fn platform_shell() -> &'static str {
    if cfg!(target_os = "android") {
        "/system/bin/sh"
    } else {
        "/bin/sh"
    }
}

pub(crate) fn spawn_process_pipe_reader(
    mut reader: impl Read + Send + 'static,
    output: Arc<Mutex<ProcessOutputBuffer>>,
    stdout: bool,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if let Ok(mut output) = output.lock() {
                        if stdout {
                            output.push_stdout(&buffer[..size]);
                        } else {
                            output.push_stderr(&buffer[..size]);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub(crate) fn push_ring(buffer: &mut VecDeque<u8>, bytes: &[u8]) {
    const PROCESS_OUTPUT_RING_BYTES: usize = 64 * 1024;
    for byte in bytes {
        if buffer.len() >= PROCESS_OUTPUT_RING_BYTES {
            buffer.pop_front();
        }
        buffer.push_back(*byte);
    }
}

pub(crate) fn refresh_process_exit(state: &mut BackgroundProcessSession) {
    if state.exit_code.is_some() {
        return;
    }
    if let Ok(Some(status)) = state.child.try_wait() {
        state.exit_code = Some(status.code().unwrap_or(-1));
        state.finished_at_ms = now_ms();
    }
}

pub(crate) fn terminate_background_process_wrapper(state: &BackgroundProcessSession) {
    let Some(pid_file) = &state.pid_file else {
        return;
    };
    let Ok(pid) = fs::read_to_string(pid_file) else {
        return;
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return;
    };
    let _ = Command::new("su")
        .arg("-c")
        .arg(format!("kill -TERM {pid} 2>/dev/null || true"))
        .output();
}

pub(crate) fn process_status_json(
    state: &BackgroundProcessSession,
    output: Option<ProcessOutputSnapshot>,
) -> Value {
    let mut value = json!({
        "processSessionId": state.process_session_id,
        "backend": state.backend,
        "command": state.command,
        "cwd": state.cwd,
        "startedAt": state.started_at_ms,
        "pid": state.pid,
        "running": state.exit_code.is_none(),
        "exitCode": state.exit_code,
        "finishedAt": state.finished_at_ms
    });
    if let Some(output) = output
        && let Some(object) = value.as_object_mut()
    {
        object.insert("stdout".to_string(), json!(output.stdout));
        object.insert("stderr".to_string(), json!(output.stderr));
        object.insert(
            "stdoutTotalBytes".to_string(),
            json!(output.stdout_total_bytes),
        );
        object.insert(
            "stderrTotalBytes".to_string(),
            json!(output.stderr_total_bytes),
        );
    }
    value
}

pub(crate) fn completed_process_session_from_state(
    state: &BackgroundProcessSession,
    process_session_id: &str,
) -> Option<CompletedProcessSession> {
    let output = state.output.lock().ok().map(|buffer| buffer.snapshot())?;
    Some(CompletedProcessSession {
        session_id: state.session_id.clone(),
        process_session_id: process_session_id.to_string(),
        backend: state.backend.clone(),
        command: state.command.clone(),
        cwd: state.cwd.clone(),
        started_at_ms: state.started_at_ms,
        pid: state.pid,
        output,
        exit_code: state.exit_code,
        finished_at_ms: state.finished_at_ms,
    })
}

pub(crate) fn completed_process_status_json(state: &CompletedProcessSession) -> Value {
    json!({
        "processSessionId": state.process_session_id,
        "backend": state.backend,
        "command": state.command,
        "cwd": state.cwd,
        "startedAt": state.started_at_ms,
        "pid": state.pid,
        "running": false,
        "exitCode": state.exit_code,
        "finishedAt": state.finished_at_ms,
        "stdout": state.output.stdout,
        "stderr": state.output.stderr,
        "stdoutTotalBytes": state.output.stdout_total_bytes,
        "stderrTotalBytes": state.output.stderr_total_bytes
    })
}

pub(crate) fn get_rootfs_backend(settings: &[hambur_db::AppSettingRecord]) -> &str {
    for s in settings {
        if s.key == "rootfsBackend" || s.key == "rootfs_setting:rootfsBackend" {
            let val = s.value.trim_matches('"');
            if val == "chroot" || val == "proot" {
                return val;
            }
        }
    }
    "proot"
}
