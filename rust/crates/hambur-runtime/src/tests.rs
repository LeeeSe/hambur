use super::*;

use std::{
        collections::HashSet,
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::PathBuf,
        process::{Command, Stdio},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use base64::Engine;
    use hambur_core::{new_id, now_ms};
    use hambur_llm::ModelMessage;
    use hambur_sandbox::SandboxAccess;
    use hambur_tools::ToolInvocation;
    use serde_json::Value;
    use tokio::sync::oneshot;

    use super::{
        AppBootstrap, BackgroundProcessSession, DelegateTaskState, ModelRouteSnapshot,
        NewTraceSpan, ProcessOutputBuffer, RouteStreamSource, RuntimeCommand, RuntimeEngine,
        RuntimeEvent, platform_shell, provider_stream_source, spawn_process_pipe_reader,
        validate_web_fetch_url,
    };

    #[test]
    fn bundled_skills_are_seeded_and_exposed_by_tools() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let skills = runtime.list_skills();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "skill-creator")
            .expect("seeded skill");
        assert_eq!(skill.path, "system/skill-creator/SKILL.md");
        assert!(skill.built_in);
        assert!(skill.enabled);

        let list_invocation = ToolInvocation::from_model_call(
            0,
            "call_skills".to_string(),
            "turn".to_string(),
            "session".to_string(),
            "skills_list".to_string(),
            "{}".to_string(),
        )
        .expect("invocation");
        let list = runtime
            .tokio
            .block_on(
                runtime.resolve_knowledge_tool_result(&list_invocation, &serde_json::json!({})),
            )
            .content_json;
        let list_json: Value = serde_json::from_str(&list).expect("skills list json");
        assert_eq!(list_json["success"], true);
        assert_eq!(list_json["count"], 1);
        assert_eq!(list_json["categories"][0], "system");
        assert_eq!(
            list_json["hint"],
            "Use skill_view(name) to see full content, tags, and linked files."
        );

        let alias_invocation = ToolInvocation::from_model_call(
            0,
            "call_skill_list_alias".to_string(),
            "turn".to_string(),
            "session".to_string(),
            "skill_list".to_string(),
            "{}".to_string(),
        )
        .expect("alias invocation");
        let alias = runtime.tokio.block_on(
            runtime.resolve_knowledge_tool_result(&alias_invocation, &serde_json::json!({})),
        );
        assert!(!alias.is_error, "alias failed: {}", alias.summary);

        let detail = runtime.get_skill_detail("skill-creator".to_string(), String::new());
        assert!(detail.content.contains("# Skill Creator"));
        assert_eq!(detail.skill_dir_path, "system/skill-creator");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn skills_index_prompt_is_injected_into_initial_request() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let prompt = runtime.build_skills_index_prompt();
        assert!(prompt.contains("Hambur has a local Skills system at /var/hambur/skills."));
        assert!(prompt.contains("- skill-creator [system]: Create or update Hambur skills"));

        let route = ModelRouteSnapshot {
            supports_tool_call: true,
            supports_reasoning: true,
            output_limit: 1024,
            ..Default::default()
        };
        let source = provider_stream_source(
            "session",
            "turn",
            vec![ModelMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                ..Default::default()
            }],
            &route,
            "[]",
            &prompt,
            "",
            false,
            false,
        );
        let RouteStreamSource::Provider(request) = source else {
            panic!("expected provider source");
        };
        assert!(request.system_blocks.iter().any(|block| {
            block.contains("Skills are reusable task instructions")
                && block.contains("skill_view")
                && block.contains("skill-creator")
        }));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn memory_prompt_is_injected_into_initial_request() {
        let app_files_dir = temp_app_dir();
        let memory_dir = app_files_dir.join("sandbox").join("global").join("memory");
        fs::create_dir_all(&memory_dir).expect("create memory dir");
        fs::write(
            memory_dir.join("MEMORY.md"),
            "Project uses Rust backend and Compose frontend.",
        )
        .expect("write memory");
        fs::write(memory_dir.join("USER.md"), "User prefers concise Chinese.")
            .expect("write user memory");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let memory_prompt = runtime.build_memory_system_prompt();
        assert!(memory_prompt.contains("You have persistent memory across chats."));
        assert!(memory_prompt.contains("Project uses Rust backend and Compose frontend."));
        assert!(memory_prompt.contains("User prefers concise Chinese."));

        let route = ModelRouteSnapshot {
            supports_tool_call: true,
            output_limit: 1024,
            ..Default::default()
        };
        let source = provider_stream_source(
            "session",
            "turn",
            vec![ModelMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                ..Default::default()
            }],
            &route,
            "[]",
            "",
            &memory_prompt,
            false,
            false,
        );
        let RouteStreamSource::Provider(request) = source else {
            panic!("expected provider source");
        };
        assert!(
            request
                .system_blocks
                .iter()
                .any(|block| block.contains("MEMORY (your personal notes)")
                    && block.contains("USER PROFILE"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn bootstrap_snapshot_survives_restart_without_replayed_session_event() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let first = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = first.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");
        assert!(ready.snapshot.sessions.is_empty());

        let ack = first.create_session("Persisted".to_string());
        assert!(ack.accepted, "create session rejected: {}", ack.message);
        let created = first.next_event().expect("created event");
        assert_eq!(created.kind.as_str(), "SessionCreated");
        assert_eq!(created.snapshot.sessions.len(), 1);
        let created_session_id = created.snapshot.selected_session_id.clone();
        let workspace = first
            .sandbox
            .resolve(
                &created_session_id,
                "/var/hambur/workspace",
                SandboxAccess::Read,
            )
            .expect("created session workspace");
        assert!(workspace.host_path.is_dir());
        first.shutdown();
        drop(first);

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let restarted_ready = restarted.next_event().expect("restart ready event");
        assert_eq!(restarted_ready.kind.as_str(), "RuntimeReady");
        assert_eq!(restarted_ready.snapshot.sessions.len(), 1);
        assert_eq!(restarted_ready.snapshot.sessions[0].title, "Persisted");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn markdown_delta_emits_render_update_event() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = runtime.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");

        let ack = runtime.append_markdown_delta(
            "session".to_string(),
            "message".to_string(),
            "# Heading\n\n".to_string(),
            false,
        );
        assert!(ack.accepted, "markdown command rejected: {}", ack.message);

        let event = runtime.next_event().expect("markdown event");
        assert_eq!(event.kind.as_str(), "MarkdownRenderUpdate");
        assert_eq!(event.session_id, "session");
        assert_eq!(event.markdown_render_update.message_id, "message");
        assert_eq!(event.markdown_render_update.committed_nodes.len(), 1);
        assert_eq!(
            event.markdown_render_update.committed_nodes[0].node_kind,
            "Heading"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn duplicate_idempotency_key_does_not_repeat_session_creation() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = runtime.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");

        let command = RuntimeCommand {
            command_id: "cmd_create_once".to_string(),
            idempotency_key: "session:create:once".to_string(),
            kind: "CreateSession".to_string(),
            title: "Once".to_string(),
            ..RuntimeCommand::default()
        };
        let first_ack = runtime.dispatch(command.clone());
        assert!(first_ack.accepted, "first command rejected");
        let created = runtime.next_event().expect("created event");
        assert_eq!(created.kind.as_str(), "SessionCreated");

        let duplicate_ack = runtime.dispatch(command);
        assert!(duplicate_ack.accepted);
        assert!(duplicate_ack.duplicate);

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].title, "Once");
        assert_eq!(snapshot.snapshot_sequence, created.sequence);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn open_session_can_switch_back_to_previously_opened_session() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let ready = runtime.next_event().expect("ready event");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");

        let first_session_id = create_test_session(&runtime);
        let second_session_id = create_test_session(&runtime);
        assert_ne!(first_session_id, second_session_id);

        let first_open = runtime.open_session(first_session_id.clone());
        assert!(
            first_open.accepted,
            "first open rejected: {}",
            first_open.message
        );
        assert!(!first_open.duplicate);
        let first_opened = next_event_with_timeout(&runtime, "first session opened");
        assert_eq!(first_opened.kind.as_str(), "SessionOpened");
        assert_eq!(first_opened.snapshot.selected_session_id, first_session_id);

        let second_open = runtime.open_session(second_session_id.clone());
        assert!(
            second_open.accepted,
            "second open rejected: {}",
            second_open.message
        );
        assert!(!second_open.duplicate);
        let second_opened = next_event_with_timeout(&runtime, "second session opened");
        assert_eq!(second_opened.kind.as_str(), "SessionOpened");
        assert_eq!(
            second_opened.snapshot.selected_session_id,
            second_session_id
        );

        let reopen_first = runtime.open_session(first_session_id.clone());
        assert!(
            reopen_first.accepted,
            "reopen first rejected: {}",
            reopen_first.message
        );
        assert!(!reopen_first.duplicate);
        let first_reopened = next_event_with_timeout(&runtime, "first session reopened");
        assert_eq!(first_reopened.kind.as_str(), "SessionOpened");
        assert_eq!(
            first_reopened.snapshot.selected_session_id,
            first_session_id
        );

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(snapshot.selected_session_id, first_session_id);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn provider_config_rejects_plain_api_key_secret() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_provider".to_string(),
            idempotency_key: "provider:update:bad-secret".to_string(),
            kind: "UpdateProvider".to_string(),
            provider_id: "provider_bad".to_string(),
            title: "Bad".to_string(),
            chunk: "https://api.test/v1".to_string(),
            payload_json: "sk-raw-secret".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!ack.accepted);
        assert_eq!(ack.rejection_code, "InvalidCommand");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_persists_reasoning_content_and_model_snapshot() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send".to_string(),
            idempotency_key: "message:client-1".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "hello".to_string(),
            payload_json: r#"{"content":"hello world","reasoning":"thinking separately"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_content = false;
        let mut saw_reasoning = false;
        let mut saw_markdown = false;
        let mut finished = None;
        for _ in 0..32 {
            let event = runtime.next_event().expect("stream event");
            match event.kind.as_str() {
                "AssistantContentDelta" => {
                    saw_content = true;
                    assert!(event.message.contains("hello") || event.message.contains("world"));
                }
                "AssistantReasoningDelta" => {
                    saw_reasoning = true;
                    assert_eq!(event.message, "thinking separately");
                }
                "MarkdownRenderUpdate" => {
                    saw_markdown = true;
                    assert!(!event.markdown_render_update.message_id.is_empty());
                }
                "TurnFinished" => {
                    finished = Some(event);
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_content, "missing content delta");
        assert!(saw_reasoning, "missing reasoning delta");
        assert!(saw_markdown, "missing markdown update");
        let finished = finished.expect("turn finished");

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("hello world"));
        assert_eq!(finished.session_id, session_id);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn local_http_provider_streams_openai_compatible_sse() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let text = read_http_request(&mut stream);
                *captured.lock().expect("capture request") = text;
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"real reasoning\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"real provider answer\"}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write provider response");
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_http_test_provider(
            &runtime,
            "provider_http",
            "gpt-real",
            &format!("http://{addr}/v1"),
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_real_provider".to_string(),
            idempotency_key: "message:real-provider:http".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "use real HTTP".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("authorization: Bearer test-api-key"));
        assert!(request.contains("content-type: application/json"));

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("real provider answer"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn local_http_provider_request_includes_visible_conversation_history() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let text = read_http_request(&mut stream);
                *captured.lock().expect("capture request") = text;
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"second answer\"}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write provider response");
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-scripted");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_history_first".to_string(),
            idempotency_key: "message:history:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "first user fact".to_string(),
            payload_json: r#"{"content":"first assistant memory"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "first rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        configure_http_test_provider(
            &runtime,
            "provider_test",
            "gpt-real-history",
            &format!("http://{addr}/v1"),
        );
        let second = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_history_second".to_string(),
            idempotency_key: "message:history:second".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "second user asks".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(second.accepted, "second rejected: {}", second.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        let body = request.split("\r\n\r\n").nth(1).expect("request body");
        let value: Value = serde_json::from_str(body).expect("request JSON");
        let messages = value
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages array");
        let projected = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
            .map(|message| {
                (
                    message.get("role").and_then(Value::as_str).unwrap_or(""),
                    message.get("content").and_then(Value::as_str).unwrap_or(""),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            vec![
                ("user", "first user fact"),
                ("assistant", "first assistant memory"),
                ("user", "second user asks"),
            ]
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_streams_text() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_real",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_provider".to_string(),
            idempotency_key: "message:e2e-real-provider".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with a short sentence containing hambur-e2e-ok.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_content_delta = false;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real provider event");
            match event.kind.as_str() {
                "AssistantContentDelta" => saw_content_delta = true,
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real provider turn did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_content_delta, "real provider emitted no content delta");

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_real");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert!(
            !message.content_text.trim().is_empty(),
            "assistant content should not be empty"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_streams_markdown_updates() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_markdown",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_markdown".to_string(),
            idempotency_key: "message:e2e-real-markdown".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply in Markdown with one heading, one bullet list, and one fenced code block. Keep it short.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut markdown_updates = 0;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real markdown provider event");
            match event.kind.as_str() {
                "MarkdownRenderUpdate" => {
                    let update = event.markdown_render_update;
                    if !update.committed_nodes.is_empty() || update.pending_node.is_some() {
                        markdown_updates += 1;
                    }
                }
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real markdown provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(
            markdown_updates > 0,
            "real provider emitted no markdown updates"
        );

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("assistant message snapshot");
        assert!(!message.content_text.trim().is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_can_be_cancelled_after_stream_start() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_cancel",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_cancel".to_string(),
            idempotency_key: "message:e2e-real-cancel".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Count from 1 to 5000, one number per line. Start immediately.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut turn_id = String::new();
        let mut saw_content_delta = false;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real cancel provider event");
            match event.kind.as_str() {
                "TurnStarted" => turn_id = event.turn_id,
                "AssistantContentDelta" => {
                    saw_content_delta = true;
                    break;
                }
                "TurnFailed" | "TurnCancelled" | "TurnFinished" => {
                    panic!(
                        "turn ended before cancellation could be issued: {} {}",
                        event.kind.as_str(),
                        event.message
                    );
                }
                _ => {}
            }
        }
        assert!(
            saw_content_delta,
            "real provider emitted no content before cancellation"
        );
        assert!(!turn_id.is_empty(), "missing turn id before cancellation");

        let cancel = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_cancel_turn".to_string(),
            idempotency_key: format!("{turn_id}:cancel"),
            kind: "CancelTurn".to_string(),
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(cancel.accepted, "cancel rejected: {}", cancel.message);

        let mut cancelled = None;
        for _ in 0..128 {
            let event = runtime.next_event().expect("real cancel terminal event");
            match event.kind.as_str() {
                "TurnCancelled" => {
                    cancelled = Some(event);
                    break;
                }
                "TurnFinished" | "TurnFailed" => {
                    panic!(
                        "expected cancellation but got {}: {}",
                        event.kind.as_str(),
                        event.message
                    );
                }
                _ => {}
            }
        }
        let cancelled = cancelled.expect("real provider turn did not cancel");
        assert_eq!(cancelled.turn_id, turn_id);
        assert_eq!(cancelled.error_code, "Cancelled");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_is_used_after_failed_primary_fallback() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_a_e2e_bad",
            "bad-e2e-model",
            "http://127.0.0.1:9/v1",
            &config.secret_env,
        );
        configure_env_secret_provider(
            &runtime,
            "provider_z_e2e_real",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_fallback".to_string(),
            idempotency_key: "message:e2e-real-fallback".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with one short sentence containing hambur-fallback-ok.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        let mut terminal = None;
        for _ in 0..512 {
            let event = runtime.next_event().expect("real fallback provider event");
            if event.kind.as_str() == "TurnStateChanged" && event.message.contains("Fallback to") {
                saw_fallback = true;
            }
            if matches!(
                event.kind.as_str(),
                "TurnFinished" | "TurnFailed" | "TurnCancelled"
            ) {
                terminal = Some(event);
                break;
            }
        }
        let terminal = terminal.expect("real fallback provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "fallback did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_fallback, "missing fallback state event");

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("assistant message snapshot");
        assert_eq!(message.provider_id_snapshot, "provider_z_e2e_real");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert_eq!(message.status, "completed");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider"]
    fn e2e_real_openai_compatible_provider_regenerate_uses_real_provider() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider(
            &runtime,
            "provider_e2e_regen",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
        );

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_regen_seed".to_string(),
            idempotency_key: "message:e2e-regenerate:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "Reply with a short sentence containing hambur-regenerate-seed.".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "seed rejected: {}", first.message);
        wait_for_session_finished(&runtime, &session_id, "seed turn");

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let first_assistant = timeline
            .items
            .iter()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant timeline item")
            .clone();
        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_regenerate".to_string(),
            idempotency_key: format!("{}:regenerate:e2e", first_assistant.payload_ref),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: first_assistant.payload_ref,
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_finished(&runtime, &session_id, "regenerate turn");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let assistant_messages = timeline
            .items
            .iter()
            .filter(|item| item.kind == "AssistantMessage")
            .collect::<Vec<_>>();
        assert_eq!(assistant_messages.len(), 2);
        let latest = assistant_messages.last().expect("latest assistant");
        let message = runtime
            .get_message_snapshot(latest.payload_ref.clone())
            .message
            .expect("latest assistant message");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_regen");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert_eq!(message.status, "completed");
        assert!(!message.content_text.trim().is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    #[ignore = "requires HAMBUR_E2E_OPENAI_API_KEY and a real OpenAI-compatible provider with tool calling"]
    fn e2e_real_openai_compatible_provider_executes_tool_call() {
        let config = RealProviderTestConfig::from_env();
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_env_secret_provider_with_capabilities(
            &runtime,
            "provider_e2e_tools",
            &config.model_id,
            &config.base_url,
            &config.secret_env,
            true,
            false,
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_e2e_real_tools".to_string(),
            idempotency_key: "message:e2e-real-tools".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: concat!(
                "Call the echo tool exactly once with JSON arguments ",
                "{\"text\":\"hambur-real-tool-ok\"}. ",
                "Do not answer directly before calling the tool."
            )
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_tool_delta = false;
        let mut finished_tools = 0;
        let mut terminal = None;
        for _ in 0..256 {
            let event = runtime.next_event().expect("real tool provider event");
            if event.session_id != session_id {
                continue;
            }
            match event.kind.as_str() {
                "ToolCallDelta" => saw_tool_delta = true,
                "ToolCallFinished" => {
                    finished_tools += 1;
                    assert!(
                        event.message.contains("hambur-real-tool-ok"),
                        "unexpected tool summary: {}",
                        event.message
                    );
                }
                "TurnFinished" | "TurnFailed" | "TurnCancelled" => {
                    terminal = Some(event);
                    break;
                }
                _ => {}
            }
        }
        let terminal = terminal.expect("real tool provider did not reach terminal event");
        assert_eq!(
            terminal.kind.as_str(),
            "TurnFinished",
            "real provider tool turn did not finish: {} {}",
            terminal.error_code,
            terminal.message
        );
        assert!(saw_tool_delta, "real provider emitted no tool call delta");
        assert!(finished_tools > 0, "real provider executed no tools");

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        assert!(
            timeline.items.iter().any(|item| item.kind == "ToolTrace"),
            "missing tool trace"
        );
        let continuation = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.kind == "AssistantMessage")
            .expect("assistant continuation");
        let message = runtime
            .get_message_snapshot(continuation.payload_ref.clone())
            .message
            .expect("continuation message");
        assert_eq!(message.provider_id_snapshot, "provider_e2e_tools");
        assert_eq!(message.model_id_snapshot, config.model_id);
        assert!(message.content_text.contains("hambur-real-tool-ok"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn duplicate_send_message_is_rejected_while_turn_is_active() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_first".to_string(),
            idempotency_key: "message:busy:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "first".to_string(),
            payload_json: r#"{"content":"one two three four five six","reasoning":"busy"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted);

        let second = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_second".to_string(),
            idempotency_key: "message:busy:second".to_string(),
            kind: "SendMessage".to_string(),
            session_id,
            content: "second".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!second.accepted);
        assert_eq!(second.rejection_code, "SessionBusy");

        let turn_started = loop {
            let event = runtime.next_event().expect("turn started");
            if event.kind.as_str() == "TurnStarted" {
                break event;
            }
        };
        let _ = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_busy_cancel".to_string(),
            idempotency_key: format!("{}:cancel", turn_started.turn_id),
            kind: "CancelTurn".to_string(),
            turn_id: turn_started.turn_id,
            ..RuntimeCommand::default()
        });
        for _ in 0..32 {
            let event = runtime.next_event().expect("terminal busy event");
            if matches!(
                event.kind.as_str(),
                "TurnCancelled" | "TurnFinished" | "TurnFailed"
            ) {
                break;
            }
        }

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_cancel_turn_stops_streaming() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_cancel_send".to_string(),
            idempotency_key: "message:cancel:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "cancel me".to_string(),
            payload_json:
                r#"{"content":"one two three four five six seven eight","reasoning":"cancel path"}"#
                    .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted);

        let turn_started = loop {
            let event = runtime.next_event().expect("turn started");
            if event.kind.as_str() == "TurnStarted" {
                break event;
            }
        };
        let cancel = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_cancel".to_string(),
            idempotency_key: format!("{}:cancel", turn_started.turn_id),
            kind: "CancelTurn".to_string(),
            session_id,
            turn_id: turn_started.turn_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(cancel.accepted);

        let mut cancelled = None;
        for _ in 0..32 {
            let event = runtime.next_event().expect("cancel event");
            if event.kind.as_str() == "TurnCancelled" {
                cancelled = Some(event);
                break;
            }
        }
        let cancelled = cancelled.expect("turn cancelled");
        assert_eq!(cancelled.turn_id, turn_started.turn_id);
        assert_eq!(cancelled.error_code, "Cancelled");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_regenerate_message_uses_source_user_content() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_seed".to_string(),
            idempotency_key: "message:regenerate:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "seed prompt".to_string(),
            payload_json: r#"{"content":"first answer","reasoning":"seed reasoning"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "seed rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let assistant_message_id = latest_assistant_message_id(&runtime, &session_id);

        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate".to_string(),
            idempotency_key: format!("{assistant_message_id}:regenerate:test"),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: assistant_message_id,
            payload_json: r#"{"content":"regenerated answer","reasoning":"regen reasoning"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let assistant_count = timeline
            .items
            .iter()
            .filter(|item| item.content_type == "assistant_markdown_block")
            .count();
        let user_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "UserMessage")
            .count();
        assert_eq!(assistant_count, 1);
        assert_eq!(user_count, 1);

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("latest assistant message");
        assert_eq!(message.content_text, "regenerated answer");
        assert_eq!(message.reasoning_content, "regen reasoning");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_stream_regenerate_from_user_message_keeps_user_message_visible() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-test");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_seed".to_string(),
            idempotency_key: "message:regenerate:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "seed prompt".to_string(),
            payload_json: r#"{"content":"first answer","reasoning":"seed reasoning"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "seed rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let user_message_id = timeline
            .items
            .iter()
            .find(|item| item.kind == "UserMessage")
            .expect("user message timeline item")
            .payload_ref
            .clone();

        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate".to_string(),
            idempotency_key: format!("{user_message_id}:regenerate:test"),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: user_message_id,
            payload_json: r#"{"content":"regenerated answer","reasoning":"regen reasoning"}"#
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let assistant_count = timeline
            .items
            .iter()
            .filter(|item| item.content_type == "assistant_markdown_block")
            .count();
        let user_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "UserMessage")
            .count();
        assert_eq!(assistant_count, 1);
        assert_eq!(user_count, 1);

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("latest assistant message");
        assert_eq!(message.content_text, "regenerated answer");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn regenerate_from_user_message_includes_that_user_message_in_provider_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let text = read_http_request(&mut stream);
                *captured.lock().expect("capture request") = text;
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"regenerated second answer\"}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write provider response");
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-scripted");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_context_first".to_string(),
            idempotency_key: "message:regenerate-context:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "hello".to_string(),
            payload_json: r#"{"content":"hello answer"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "first rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let second = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_context_second".to_string(),
            idempotency_key: "message:regenerate-context:second".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "how many cats do I have?".to_string(),
            payload_json: r#"{"content":"old second answer"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(second.accepted, "second rejected: {}", second.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 20);
        let user_message_id = timeline
            .items
            .iter()
            .filter(|item| item.kind == "UserMessage")
            .last()
            .expect("second user message timeline item")
            .payload_ref
            .clone();

        configure_http_test_provider(
            &runtime,
            "provider_test",
            "gpt-regenerate-context",
            &format!("http://{addr}/v1"),
        );
        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_regenerate_context".to_string(),
            idempotency_key: format!("{user_message_id}:regenerate:context"),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: user_message_id,
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_event(&runtime, "TurnFinished", &session_id);
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        let body = request.split("\r\n\r\n").nth(1).expect("request body");
        let value: Value = serde_json::from_str(body).expect("request JSON");
        let messages = value
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages array");
        let projected = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
            .map(|message| {
                (
                    message.get("role").and_then(Value::as_str).unwrap_or(""),
                    message.get("content").and_then(Value::as_str).unwrap_or(""),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            vec![
                ("user", "hello"),
                ("assistant", "hello answer"),
                ("user", "how many cats do I have?"),
            ]
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn regenerated_branch_history_excludes_hidden_old_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let text = read_http_request(&mut stream);
                *captured.lock().expect("capture request") = text;
                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"after branch\"}}]}\n\n",
                    "data: [DONE]\n\n"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write provider response");
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-scripted");

        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_branch_seed".to_string(),
            idempotency_key: "message:branch:seed".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "branch prompt".to_string(),
            payload_json: r#"{"content":"old hidden answer"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "first rejected: {}", first.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let old_assistant_message_id = latest_assistant_message_id(&runtime, &session_id);
        let regenerate = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_branch_regenerate".to_string(),
            idempotency_key: "message:branch:regenerate".to_string(),
            kind: "RegenerateMessage".to_string(),
            session_id: session_id.clone(),
            source_message_id: old_assistant_message_id,
            payload_json: r#"{"content":"new visible answer"}"#.to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            regenerate.accepted,
            "regenerate rejected: {}",
            regenerate.message
        );
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        configure_http_test_provider(
            &runtime,
            "provider_test",
            "gpt-branch-real",
            &format!("http://{addr}/v1"),
        );
        let followup = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_branch_followup".to_string(),
            idempotency_key: "message:branch:followup".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "follow current branch".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(followup.accepted, "followup rejected: {}", followup.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        let body = request.split("\r\n\r\n").nth(1).expect("request body");
        let value: Value = serde_json::from_str(body).expect("request JSON");
        let message_text = value.get("messages").expect("messages").to_string();
        assert!(message_text.contains("new visible answer"));
        assert!(message_text.contains("follow current branch"));
        assert!(!message_text.contains("old hidden answer"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_routes_fallback_switches_target_before_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_a", "model-a");
        configure_named_test_provider(&runtime, "provider_b", "model-b");
        configure_fallback_group(
            &runtime,
            "grp_fallback_before_output",
            &[
                ("provider_a", "model-a"),
                ("provider_b", "model-b"),
            ],
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_fallback_before_output".to_string(),
            idempotency_key: "message:fallback:before-output".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "fallback please".to_string(),
            payload_json: serde_json::json!({
                "routes": {
                    "provider_a": {
                        "sse": "data: {\"error\":{\"code\":\"Http5xx\",\"message\":\"first target down\"}}\n\n"
                    },
                    "provider_b": {
                        "content": "fallback answer",
                        "reasoning": "second target selected"
                    }
                }
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        for _ in 0..128 {
            let event = next_event_with_timeout(&runtime, "fallback event");
            if event.kind.as_str() == "TurnStateChanged" {
                saw_fallback = true;
            }
            if event.kind.as_str() == "TurnFinished" {
                break;
            }
            if event.kind.as_str() == "TurnFailed" {
                panic!("turn failed: {}", event.message);
            }
        }
        assert!(saw_fallback, "missing fallback state event");

        let message = latest_assistant_message_snapshot(&runtime, &session_id);
        assert_eq!(message.provider_id_snapshot, "provider_b");
        assert_eq!(message.model_id_snapshot, "model-b");
        assert_eq!(message.status, "completed");
        assert_eq!(message.content_text, "fallback answer");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_routes_fallback_does_not_switch_after_semantic_output() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_a", "model-a");
        configure_named_test_provider(&runtime, "provider_b", "model-b");
        configure_fallback_group(
            &runtime,
            "grp_fallback_after_output",
            &[
                ("provider_a", "model-a"),
                ("provider_b", "model-b"),
            ],
        );

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_fallback_after_output".to_string(),
            idempotency_key: "message:fallback:after-output".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "partial then fail".to_string(),
            payload_json: serde_json::json!({
                "routes": {
                    "provider_a": {
                        "sse": "data: {\"choices\":[{\"delta\":{\"content\":\"partial \"}}]}\n\ndata: {\"error\":{\"code\":\"Http5xx\",\"message\":\"late failure\"}}\n\n"
                    },
                    "provider_b": {
                        "content": "should not be used"
                    }
                }
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut saw_fallback = false;
        for _ in 0..128 {
            let event = next_event_with_timeout(&runtime, "partial failure event");
            if event.kind.as_str() == "TurnStateChanged" {
                saw_fallback = true;
            }
            if event.kind.as_str() == "TurnFailed" {
                break;
            }
        }
        assert!(!saw_fallback, "fallback happened after semantic output");

        let message = latest_assistant_message_snapshot(&runtime, &session_id);
        assert_eq!(message.provider_id_snapshot, "provider_a");
        assert_eq!(message.model_id_snapshot, "model-a");
        assert_eq!(message.status, "failed_partial");
        assert_eq!(message.content_text, "partial ");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn scripted_tool_calls_execute_as_one_batch_and_render_traces() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-tools");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_time\",\"function\":{\"name\":\"get_current_time\",\"arguments\":\"{}\"}},",
            "{\"index\":1,\"id\":\"call_echo\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"hello tools\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Tool results received: hello tools\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_tools".to_string(),
            idempotency_key: "message:tools:batch".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "use tools".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let mut finished_tools = 0;
        let mut turn_finished = false;
        let mut seen_events = Vec::new();
        for _ in 0..96 {
            let event = next_event_with_timeout(&runtime, "tool event");
            seen_events.push(event.kind.clone());
            match event.kind.as_str() {
                "ToolCallFinished" => finished_tools += 1,
                "TurnFinished" => {
                    turn_finished = true;
                    break;
                }
                "TurnFailed" => panic!("turn failed: {}", event.message),
                _ => {}
            }
        }
        assert!(turn_finished, "missing turn finish; seen={seen_events:?}");
        assert_eq!(finished_tools, 2);

        let timeline = runtime.get_timeline_page(session_id, 0, 50);
        let trace_count = timeline
            .items
            .iter()
            .filter(|item| item.kind == "ToolTrace")
            .count();
        assert!(trace_count >= 2, "missing tool traces: {trace_count}");
        let assistant_block = timeline
            .items
            .iter()
            .rev()
            .find(|item| item.content_type == "assistant_markdown_block")
            .expect("assistant markdown block");
        let payload = timeline
            .markdown_block_payloads
            .iter()
            .find(|payload| payload.id == assistant_block.payload_ref)
            .expect("assistant markdown payload");
        assert!(payload.raw.contains("hello tools"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn next_provider_request_preserves_prior_tool_protocol_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider server");
        let addr = listener.local_addr().expect("local addr");
        let captured_request = Arc::new(Mutex::new(String::new()));
        let captured = captured_request.clone();
        let server = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("set provider listener nonblocking");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let text = read_http_request(&mut stream);
                        *captured.lock().expect("capture request") = text;
                        let body = concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"tool history accepted\"}}]}\n\n",
                            "data: [DONE]\n\n"
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                            body.len(),
                            body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("write provider response");
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            panic!("provider server timed out waiting for request");
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("provider accept failed: {error}"),
                }
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");
        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-tools");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_echo_history\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\\\"text\\\":\\\"history tool result\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"tool turn done\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let first = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_tool_history_first".to_string(),
            idempotency_key: "message:tool-history:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "call echo".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(first.accepted, "first rejected: {}", first.message);
        wait_for_session_finished(&runtime, &session_id, "tool history first");

        configure_http_test_provider(
            &runtime,
            "provider_test",
            "gpt-tool-history-real",
            &format!("http://{addr}/v1"),
        );
        let second = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_tool_history_second".to_string(),
            idempotency_key: "message:tool-history:second".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "continue after tool".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(second.accepted, "second rejected: {}", second.message);
        wait_for_session_finished(&runtime, &session_id, "tool history second");
        server.join().expect("provider server");

        let request = captured_request.lock().expect("captured request").clone();
        let body = request.split("\r\n\r\n").nth(1).expect("request body");
        let value: Value = serde_json::from_str(body).expect("request JSON");
        let messages = value
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages array");
        let assistant_with_tool_calls = messages.iter().find(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some()
        });
        assert!(
            assistant_with_tool_calls.is_some(),
            "missing assistant tool_calls"
        );
        assert!(
            messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("tool")
                    && message.get("tool_call_id").and_then(Value::as_str)
                        == Some("call_echo_history")
                    && message
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .contains("history tool result")
            }),
            "missing tool result message"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn attachment_import_remove_and_startup_cleanup() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_attachment".to_string(),
            idempotency_key: "content://image:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png",
                "byteSize": 12,
                "originalUri": "content://images/photo"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted, "import rejected: {}", import.message);
        let imported = runtime.next_event().expect("attachment imported");
        assert_eq!(imported.kind.as_str(), "AttachmentImported");
        assert_eq!(imported.snapshot.pending_attachments.len(), 1);
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let remove = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_remove_attachment".to_string(),
            idempotency_key: format!("{attachment_id}:remove"),
            kind: "RemovePendingAttachment".to_string(),
            session_id: session_id.clone(),
            message_id: attachment_id,
            ..RuntimeCommand::default()
        });
        assert!(remove.accepted, "remove rejected: {}", remove.message);
        let removed = runtime.next_event().expect("attachment removed");
        assert_eq!(removed.kind.as_str(), "PendingAttachmentRemoved");
        assert!(removed.snapshot.pending_attachments.is_empty());

        let import_stale = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_stale".to_string(),
            idempotency_key: "content://stale:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "stale.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import_stale.accepted);
        let stale_event = runtime.next_event().expect("stale imported");
        assert_eq!(stale_event.snapshot.pending_attachments.len(), 1);
        runtime.shutdown();
        drop(runtime);

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let ready = restarted.next_event().expect("ready after cleanup");
        assert_eq!(ready.kind.as_str(), "RuntimeReady");
        assert!(ready.snapshot.pending_attachments.is_empty());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn send_message_consumes_image_attachment_and_requires_vision_route() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_models(
            &runtime,
            "provider_vision",
            &[("model-text", false), ("model-vision", true)],
        );

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_for_send".to_string(),
            idempotency_key: "content://image:send:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_with_image".to_string(),
            idempotency_key: "message:image:send".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "describe this".to_string(),
            payload_json: serde_json::json!({
                "attachmentIds": [attachment_id],
                "content": "image accepted",
                "reasoning": "vision"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_session_finished(&runtime, &session_id, "image attachment");
        let timeline = runtime.get_timeline_page(session_id, 0, 20);
        let user = timeline
            .items
            .iter()
            .find(|item| item.kind == "UserMessage")
            .expect("user message");
        let user_message = runtime
            .get_message_snapshot(user.payload_ref.clone())
            .message
            .expect("user message snapshot");
        assert!(user_message.content_text.contains("ImagePart"));
        assert!(
            runtime
                .get_session_snapshot(user_message.session_id)
                .timeline_items
                .len()
                >= 2
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn text_only_route_rejects_image_attachment_without_vision_model() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "model-text");

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_image_text_only".to_string(),
            idempotency_key: "content://image:text-only:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "photo.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let attachment_id = imported.snapshot.pending_attachments[0].id.clone();

        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_send_image_text_only".to_string(),
            idempotency_key: "message:image:text-only".to_string(),
            kind: "SendMessage".to_string(),
            session_id,
            content: "describe this".to_string(),
            payload_json: serde_json::json!({"attachmentIds": [attachment_id]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!send.accepted);
        assert_eq!(send.rejection_code, "CapabilityMismatch");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn view_image_text_route_hands_off_to_vision_continuation() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_models(
            &runtime,
            "provider_combo",
            &[("model-text", false), ("model-vision", true)],
        );

        let import = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_import_view_image".to_string(),
            idempotency_key: "content://view-image:import:test".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "view.png",
                "mimeType": "image/png"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted);
        let imported = runtime.next_event().expect("attachment imported");
        let path = imported.snapshot.pending_attachments[0]
            .sandbox_path
            .clone();

        let sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_view\",\"function\":{{\"name\":\"view_image\",\"arguments\":\"{{\\\"path\\\":\\\"{}\\\"}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
            path
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ImagePart(fileId=continuation)\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_view_image_turn".to_string(),
            idempotency_key: "message:view-image:handoff".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "inspect image by tool".to_string(),
            payload_json: serde_json::json!({"sse": sse, "sse_sequence": [continuation_sse]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);

        let mut saw_handoff = false;
        for _ in 0..96 {
            let event = next_event_with_timeout(&runtime, "view image event");
            if event.kind.as_str() == "TurnStateChanged"
                && event.message.contains("ImageInspectionRequired")
            {
                saw_handoff = true;
            }
            if event.kind.as_str() == "TurnFinished" {
                break;
            }
            if event.kind.as_str() == "TurnFailed" {
                panic!("turn failed: {}", event.message);
            }
        }
        assert!(saw_handoff, "missing vision handoff event");
        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("continuation message");
        assert_eq!(message.model_id_snapshot, "model-vision");
        assert!(message.content_text.contains("ImagePart"));
        assert!(!message.content_text.contains("base64"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn settings_snapshot_redacts_secrets_and_audits_config_mutations() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");

        let provider = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_settings_provider".to_string(),
            idempotency_key: "settings:provider:update".to_string(),
            kind: "UpdateProvider".to_string(),
            provider_id: "provider_settings".to_string(),
            title: "Settings Provider".to_string(),
            chunk: "https://api.settings.test/v1".to_string(),
            payload_json: serde_json::json!({
                "secretRef": "android-secret://providers/settings",
                "enabled": true
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let event = runtime.next_event().expect("settings event");
        assert_eq!(event.kind.as_str(), "SettingsChanged");

        let snapshot = runtime.get_settings_snapshot();
        assert_eq!(snapshot.settings.providers.len(), 1);
        let provider = &snapshot.settings.providers[0];
        assert_eq!(provider.id, "provider_settings");
        assert_eq!(provider.secret_label, "Android Secret Store");
        assert!(!provider.secret_label.contains("settings"));
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "UpdateProvider"
                    && audit.redacted_summary.contains("redacted secret"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn destructive_config_mutations_require_approval() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_named_test_provider(&runtime, "provider_delete", "model-delete");

        let rejected_delete = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_delete_without_approval".to_string(),
            idempotency_key: "settings:delete:without-approval".to_string(),
            kind: "DeleteProvider".to_string(),
            provider_id: "provider_delete".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected_delete.accepted);
        assert_eq!(rejected_delete.rejection_code, "InvalidCommand");

        let rejected_rootfs = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_rootfs_without_approval".to_string(),
            idempotency_key: "settings:rootfs:without-approval".to_string(),
            kind: "UpdateRootfsSettings".to_string(),
            payload_json: serde_json::json!({"enabled": true}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected_rootfs.accepted);
        assert_eq!(rejected_rootfs.rejection_code, "InvalidCommand");

        let approved_rootfs = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_rootfs_with_approval".to_string(),
            idempotency_key: "settings:rootfs:with-approval".to_string(),
            kind: "UpdateRootfsSettings".to_string(),
            payload_json: serde_json::json!({
                "enabled": true,
                "approvalToken": "approve:rootfs_settings"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            approved_rootfs.accepted,
            "rootfs rejected: {}",
            approved_rootfs.message
        );
        let _ = runtime.next_event().expect("rootfs settings event");

        let approved_delete = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_delete_with_approval".to_string(),
            idempotency_key: "settings:delete:with-approval".to_string(),
            kind: "DeleteProvider".to_string(),
            provider_id: "provider_delete".to_string(),
            payload_json: serde_json::json!({
                "approvalToken": "approve:delete-provider"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            approved_delete.accepted,
            "delete rejected: {}",
            approved_delete.message
        );
        let _ = runtime.next_event().expect("provider deleted event");

        let snapshot = runtime.get_settings_snapshot();
        assert!(snapshot.settings.providers.is_empty());
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "DeleteProvider" && audit.approval_required)
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn reset_rootfs_requires_approval_and_preserves_session_dirs() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        let workspace = runtime
            .sandbox
            .resolve(
                &session_id,
                "/var/hambur/workspace/rootfs-marker.txt",
                SandboxAccess::Write,
            )
            .expect("workspace path");
        if let Some(parent) = workspace.host_path.parent() {
            fs::create_dir_all(parent).expect("workspace parent");
        }
        fs::write(&workspace.host_path, "stale").expect("write marker");
        assert!(workspace.host_path.exists());

        let rejected = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_reset_rootfs_rejected".to_string(),
            idempotency_key: "rootfs:reset:rejected".to_string(),
            kind: "ResetRootfs".to_string(),
            session_id: session_id.clone(),
            ..RuntimeCommand::default()
        });
        assert!(!rejected.accepted);
        assert_eq!(rejected.rejection_code, "InvalidCommand");

        let accepted = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_reset_rootfs_approved".to_string(),
            idempotency_key: "rootfs:reset:approved".to_string(),
            kind: "ResetRootfs".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "approvalToken": "approve:rootfs_reset"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(accepted.accepted, "reset rejected: {}", accepted.message);
        let event = wait_for_session_event_result(&runtime, "TurnStateChanged", &session_id);
        assert!(event.message.contains("\"action\":\"ResetRootfs\""));
        let prepared = runtime
            .sandbox
            .resolve(&session_id, "/var/hambur/workspace", SandboxAccess::Read)
            .expect("prepared workspace");
        assert!(prepared.host_path.exists());
        assert!(workspace.host_path.exists());

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn model_groups_and_app_settings_persist_through_restart() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        {
            let runtime = RuntimeEngine::create(AppBootstrap {
                app_files_dir: app_files_dir.to_string_lossy().to_string(),
                native_library_dir: String::new(),
            })
            .expect("create runtime");
            let _ = runtime.next_event().expect("ready event");
            configure_named_test_provider(&runtime, "provider_persist", "model-persist");

            let group = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_group_persist".to_string(),
                idempotency_key: "settings:group:persist".to_string(),
                kind: "UpdateModelGroup".to_string(),
                message_id: "grp_persist".to_string(),
                title: "Persistent Group".to_string(),
                payload_json: serde_json::json!({
                    "groupId": "grp_persist",
                    "name": "Persistent Group",
                    "routingStrategy": "fallback",
                    "fallbackPolicy": "default"
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(group.accepted, "group rejected: {}", group.message);
            let _ = runtime.next_event().expect("group event");

            let member = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_group_member_persist".to_string(),
                idempotency_key: "settings:group-member:persist".to_string(),
                kind: "UpdateModelGroupMember".to_string(),
                message_id: "grp_persist".to_string(),
                provider_id: "provider_persist".to_string(),
                model_id: "model-persist".to_string(),
                payload_json: serde_json::json!({
                    "groupId": "grp_persist",
                    "providerId": "provider_persist",
                    "modelId": "model-persist",
                    "position": 0,
                    "enabled": true
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(member.accepted, "member rejected: {}", member.message);
            let _ = runtime.next_event().expect("member event");

            let default = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_default_group_persist".to_string(),
                idempotency_key: "settings:default-group:persist".to_string(),
                kind: "SetDefaultModelGroup".to_string(),
                chunk: "primary".to_string(),
                message_id: "grp_persist".to_string(),
                payload_json: serde_json::json!({
                    "key": "primary",
                    "groupId": "grp_persist"
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(default.accepted, "default rejected: {}", default.message);
            let _ = runtime.next_event().expect("default event");

            let tool_settings = runtime.dispatch(RuntimeCommand {
                command_id: "cmd_tool_settings_persist".to_string(),
                idempotency_key: "settings:tool:persist".to_string(),
                kind: "UpdateToolSettings".to_string(),
                payload_json: serde_json::json!({
                    "terminal": false,
                    "browser": true
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(
                tool_settings.accepted,
                "tool setting rejected: {}",
                tool_settings.message
            );
            let _ = runtime.next_event().expect("tool settings event");
            runtime.shutdown();
        }

        let restarted = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("restart runtime");
        let _ = restarted.next_event().expect("restart ready");
        let snapshot = restarted.get_settings_snapshot();
        assert!(
            snapshot
                .settings
                .model_groups
                .iter()
                .any(|group| group.id == "grp_persist")
        );
        assert!(
            snapshot
                .settings
                .model_group_members
                .iter()
                .any(|member| member.group_id == "grp_persist"
                    && member.provider_id == "provider_persist"
                    && member.model_id == "model-persist")
        );
        assert!(
            snapshot
                .settings
                .default_model_groups
                .iter()
                .any(|default| default.key == "primary" && default.group_id == "grp_persist")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "tool_settings" && setting.value.contains("browser"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone6_global_settings_commands_match_architecture_contract() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_named_test_provider(&runtime, "provider_m6", "model-m6");

        let group = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_group".to_string(),
            idempotency_key: "settings:m6:group".to_string(),
            kind: "UpdateModelGroup".to_string(),
            message_id: "grp_m6".to_string(),
            title: "Milestone 6".to_string(),
            payload_json: serde_json::json!({
                "groupId": "grp_m6",
                "name": "Milestone 6",
                "routingStrategy": "load_balance",
                "fallbackPolicy": "always"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(group.accepted, "group rejected: {}", group.message);
        let _ = runtime.next_event().expect("group event");

        let default_groups = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_defaults".to_string(),
            idempotency_key: "settings:m6:defaults".to_string(),
            kind: "UpdateDefaultModelGroups".to_string(),
            payload_json: serde_json::json!({
                "primaryGroupId": "grp_m6",
                "secondaryGroupId": "grp_m6"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(
            default_groups.accepted,
            "defaults rejected: {}",
            default_groups.message
        );
        let _ = runtime.next_event().expect("defaults event");

        let theme = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_theme".to_string(),
            idempotency_key: "settings:m6:theme".to_string(),
            kind: "UpdateAppSetting".to_string(),
            chunk: "themeMode".to_string(),
            payload_json: serde_json::json!({"settingKey": "themeMode", "value": "dark"})
                .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(theme.accepted, "theme rejected: {}", theme.message);
        let _ = runtime.next_event().expect("theme event");

        let bad_theme = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_bad_theme".to_string(),
            idempotency_key: "settings:m6:bad-theme".to_string(),
            kind: "UpdateAppSetting".to_string(),
            chunk: "themeMode".to_string(),
            payload_json: serde_json::json!({"value": "purple"}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!bad_theme.accepted);
        assert_eq!(bad_theme.rejection_code, "InvalidCommand");

        let browser = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_browser".to_string(),
            idempotency_key: "settings:m6:browser".to_string(),
            kind: "UpdateBrowserToolSettings".to_string(),
            payload_json: serde_json::json!({
                "acceptCookies": false,
                "acceptThirdPartyCookies": false,
                "maxFetchBytes": 250000,
                "autoCloseMinutes": 30
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(browser.accepted, "browser rejected: {}", browser.message);
        let _ = runtime.next_event().expect("browser event");

        let bad_browser = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_bad_browser".to_string(),
            idempotency_key: "settings:m6:bad-browser".to_string(),
            kind: "UpdateBrowserToolSettings".to_string(),
            payload_json: serde_json::json!({
                "acceptCookies": false,
                "acceptThirdPartyCookies": true,
                "maxFetchBytes": 250000
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!bad_browser.accepted);
        assert_eq!(bad_browser.rejection_code, "InvalidCommand");

        let skill = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_skill".to_string(),
            idempotency_key: "settings:m6:skill".to_string(),
            kind: "UpdateSkillEnabled".to_string(),
            message_id: "skills/test".to_string(),
            payload_json: serde_json::json!({"enabled": false}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(skill.accepted, "skill rejected: {}", skill.message);
        let _ = runtime.next_event().expect("skill event");

        let rootfs_without_approval = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_rootfs_no_approval".to_string(),
            idempotency_key: "settings:m6:rootfs:no-approval".to_string(),
            kind: "UpdateRootfsSetting".to_string(),
            chunk: "backend".to_string(),
            payload_json: serde_json::json!({"value": "proot"}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(!rootfs_without_approval.accepted);
        assert_eq!(rootfs_without_approval.rejection_code, "InvalidCommand");

        let startup = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m6_startup".to_string(),
            idempotency_key: "settings:m6:startup".to_string(),
            kind: "UpdateStartupTask".to_string(),
            message_id: "task_m6".to_string(),
            payload_json: serde_json::json!({
                "id": "task_m6",
                "name": "Warmup",
                "script": "echo ready",
                "enabled": true,
                "approvalToken": "approve:startup_tasks"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(startup.accepted, "startup rejected: {}", startup.message);
        let _ = runtime.next_event().expect("startup event");

        let snapshot = runtime.get_settings_snapshot();
        assert!(
            snapshot
                .settings
                .default_model_groups
                .iter()
                .any(|default| default.key == "secondary" && default.group_id == "grp_m6")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "themeMode" && setting.value == "dark")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "skill_enabled:skills-test"
                    && setting.value == "false")
        );
        assert!(
            snapshot
                .settings
                .settings
                .iter()
                .any(|setting| setting.key == "startup_task:task_m6")
        );
        assert!(
            snapshot
                .settings
                .config_audits
                .iter()
                .any(|audit| audit.action == "UpdateStartupTask" && audit.approval_required)
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_session_search_tool_uses_database_matches() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let alpha = runtime.create_session("Alpha Project".to_string());
        assert!(alpha.accepted);
        let _ = runtime.next_event().expect("alpha event");
        let beta = runtime.create_session("Beta Research".to_string());
        assert!(beta.accepted);
        let _ = runtime.next_event().expect("beta event");
        configure_test_provider(&runtime, "gpt-tools");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_search\",\"function\":{\"name\":\"session_search\",\"arguments\":\"{\\\"query\\\":\\\"Alpha\\\",\\\"limit\\\":5}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Found Alpha Project in prior sessions.\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_session_search".to_string(),
            idempotency_key: "message:m7:session-search".to_string(),
            kind: "SendMessage".to_string(),
            session_id: runtime.get_session_list_snapshot(10, 0).selected_session_id,
            content: "search old sessions".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        let selected = snapshot.selected_session_id;
        let assistant_text = assistant_markdown_text(&runtime, &selected);
        assert!(assistant_text.contains("Alpha Project"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_browser_use_waits_for_submit_platform_result() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-browser");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_browser\",\"function\":{\"name\":\"browser_use\",\"arguments\":\"{\\\"action\\\":\\\"get_text\\\",\\\"url\\\":\\\"https://example.test\\\",\\\"timeout_ms\\\":5000}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Example Domain <untrusted_tool_result\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_browser".to_string(),
            idempotency_key: "message:m7:browser".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "read page".to_string(),
            payload_json: serde_json::json!({
                "sse": sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);

        let platform_request = loop {
            let event = runtime.next_event().expect("event");
            if event.kind.as_str() == "PlatformRequest" {
                break event.platform_request;
            }
        };
        assert_eq!(platform_request.kind, "BrowserAction");
        assert_eq!(platform_request.session_id, session_id);
        assert!(platform_request.payload_json.contains("call_browser"));

        let submit = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_submit_browser".to_string(),
            idempotency_key: "platform:m7:browser:result".to_string(),
            kind: "SubmitPlatformResult".to_string(),
            message_id: platform_request.request_id,
            payload_json: serde_json::json!({
                "payloadJson": {"text": "Example Domain", "url": "https://example.test"},
                "isError": false
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(submit.accepted);
        wait_for_session_finished(&runtime, &session_id, "browser use");

        let assistant_text = assistant_markdown_text(&runtime, &session_id);
        assert!(assistant_text.contains("Example Domain"));
        assert!(assistant_text.contains("<untrusted_tool_result"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_browser_screenshot_payload_materializes_to_filestore() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        let content = serde_json::json!({
            "url": "https://example.test",
            "mimeType": "image/png",
            "width": 1,
            "height": 1,
            "byteSize": 3,
            "base64": "AQID"
        })
        .to_string();
        let materialized = runtime
            .tokio
            .block_on(runtime.materialize_browser_artifacts(
                &session_id,
                "call_browser_screenshot",
                &content,
            ))
            .expect("materialize screenshot");
        assert!(!materialized.contains("AQID"));
        assert!(materialized.contains("sandboxPath"));
        let value = serde_json::from_str::<Value>(&materialized).expect("json");
        let file_id = value
            .get("fileId")
            .and_then(Value::as_str)
            .expect("file id");
        let file = runtime
            .tokio
            .block_on(runtime.database.file_by_id(file_id))
            .expect("file record");
        assert_eq!(file.session_id, session_id);
        assert_eq!(file.mime_type, "image/png");
        assert_eq!(file.byte_size, 3);
        let host_path = runtime
            .filestore
            .host_path_for_relative(&file.relative_path)
            .expect("host path");
        assert_eq!(fs::read(host_path).expect("read artifact"), vec![1, 2, 3]);

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_delegate_submit_rejects_artifact_path_escape() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-delegate");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_delegate_submit\",\"function\":{\"name\":\"submit_delegate_result\",\"arguments\":\"{\\\"summary\\\":\\\"done\\\",\\\"artifact_paths\\\":[\\\"/var/hambur/workspace/../memory/MEMORY.md\\\"]}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"sandbox path must not escape\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_delegate_escape".to_string(),
            idempotency_key: "message:m7:delegate-escape".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "delegate submit".to_string(),
            payload_json: serde_json::json!({"sse": sse, "sse_sequence": [continuation_sse]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("message");
        assert!(
            message
                .content_text
                .contains("sandbox path must not escape")
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_delegate_task_waits_for_child_submit_result() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-delegate");

        let child_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_child_submit\",\"function\":{\"name\":\"submit_delegate_result\",\"arguments\":\"{\\\"summary\\\":\\\"child done\\\",\\\"findings\\\":[\\\"verified\\\"],\\\"changed_files\\\":[\\\"src/lib.rs\\\"],\\\"risks\\\":[],\\\"next_steps\\\":[]}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let child_payload_json = serde_json::json!({"sse": child_sse}).to_string();
        let delegate_args = serde_json::json!({
            "task": "inspect a narrow implementation detail",
            "timeout_ms": 30_000,
            "payload_json": child_payload_json
        })
        .to_string();
        let escaped_delegate_args = delegate_args.replace('\\', "\\\\").replace('"', "\\\"");
        let parent_sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_delegate_task\",\"function\":{{\"name\":\"delegate_task\",\"arguments\":\"{escaped_delegate_args}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
             data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Delegate result: child done verified changedFiles\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_delegate_task".to_string(),
            idempotency_key: "message:m7:delegate-task".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "delegate work".to_string(),
            payload_json: serde_json::json!({
                "sse": parent_sse,
                "sse_sequence": [continuation_sse]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_session_event(&runtime, "TurnFinished", &session_id);

        let timeline = runtime.get_timeline_page(session_id.clone(), 0, 50);
        assert!(
            timeline
                .items
                .iter()
                .any(|item| item.kind == "ToolTrace" && item.trace_title == "Delegate session")
        );
        let message_text = assistant_markdown_text(&runtime, &session_id);
        assert!(message_text.contains("child done"));
        assert!(message_text.contains("verified"));
        assert!(message_text.contains("changedFiles"));

        let sessions = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(sessions.selected_session_id, session_id);
        assert!(
            sessions
                .sessions
                .iter()
                .any(|session| session.title.starts_with("Delegate: inspect"))
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_submit_delegate_result_copies_child_artifacts_to_parent_workspace() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let child_snapshot = runtime
            .tokio
            .block_on(runtime.database.create_session("Delegate: artifact"))
            .expect("create child");
        let delegate_session_id = child_snapshot.selected_session_id;
        runtime
            .tokio
            .block_on(runtime.database.open_session(&session_id))
            .expect("restore parent");
        runtime
            .sandbox
            .prepare_session(&delegate_session_id)
            .expect("prepare child sandbox");
        let child_report = runtime
            .sandbox
            .resolve(
                &delegate_session_id,
                "/var/hambur/workspace/report.txt",
                SandboxAccess::Write,
            )
            .expect("child report path");
        fs::write(&child_report.host_path, "delegate report").expect("write child report");
        let (sender, receiver) = oneshot::channel();
        let trace = runtime
            .tokio
            .block_on(runtime.database.insert_trace_span(NewTraceSpan {
                session_id: session_id.clone(),
                kind: "tool".to_string(),
                title: "Delegate session".to_string(),
                content: format!("delegateSessionId={delegate_session_id}"),
                status: "running".to_string(),
                tool_call_id: "call_delegate_artifact".to_string(),
                visible: true,
                ..Default::default()
            }))
            .expect("delegate trace");
        runtime
            .delegate_tasks
            .lock()
            .expect("delegate registry")
            .insert(
                delegate_session_id.clone(),
                DelegateTaskState {
                    parent_session_id: session_id.clone(),
                    parent_turn_id: "turn_parent".to_string(),
                    child_session_id: delegate_session_id.clone(),
                    trace_id: trace.id,
                    sender,
                },
            );

        let invocation = ToolInvocation::from_model_call(
            0,
            "call_child_submit_artifact".to_string(),
            "turn_child".to_string(),
            delegate_session_id.clone(),
            "submit_delegate_result".to_string(),
            serde_json::json!({
                "summary": "artifact ready",
                "artifact_paths": ["/var/hambur/workspace/report.txt"]
            })
            .to_string(),
        )
        .expect("invocation");
        let result = runtime
            .tokio
            .block_on(runtime.resolve_submit_delegate_result(
                &delegate_session_id,
                &invocation,
                &invocation.arguments_value().expect("arguments"),
            ));
        assert!(!result.is_error, "submit failed: {}", result.summary);
        let completion = receiver
            .blocking_recv()
            .expect("delegate completion payload");
        assert_eq!(completion.summary, "artifact ready");

        let parent_path =
            format!("/var/hambur/workspace/delegates/{delegate_session_id}/report.txt");
        assert!(result.content_json.contains(&parent_path));
        let parent_report = runtime
            .sandbox
            .resolve(&session_id, &parent_path, SandboxAccess::Read)
            .expect("parent report path");
        assert_eq!(
            fs::read_to_string(parent_report.host_path).expect("read copied report"),
            "delegate report"
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_terminal_uses_sandbox_path_policy_before_execution() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-terminal");

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[",
            "{\"index\":0,\"id\":\"call_terminal\",\"function\":{\"name\":\"terminal\",\"arguments\":\"{\\\"command\\\":\\\"echo no\\\",\\\"cwd\\\":\\\"/var/hambur/workspace/../memory\\\"}\"}}",
            "]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"sandbox path must not escape\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_terminal_escape".to_string(),
            idempotency_key: "message:m7:terminal-escape".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "run terminal".to_string(),
            payload_json: serde_json::json!({"sse": sse, "sse_sequence": [continuation_sse]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("message");
        assert!(
            message
                .content_text
                .contains("sandbox path must not escape")
        );

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_process_wait_keeps_finished_log_snapshot() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);

        let mut child = Command::new(platform_shell())
            .arg("-lc")
            .arg("printf process-ready")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn background process");
        let output = Arc::new(Mutex::new(ProcessOutputBuffer::default()));
        if let Some(stdout) = child.stdout.take() {
            spawn_process_pipe_reader(stdout, output.clone(), true);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_process_pipe_reader(stderr, output.clone(), false);
        }
        let process_session_id = new_id("proc");
        runtime
            .process_sessions
            .lock()
            .expect("process registry")
            .insert(
                process_session_id.clone(),
                BackgroundProcessSession {
                    session_id: session_id.clone(),
                    process_session_id: process_session_id.clone(),
                    backend: "host-test".to_string(),
                    command: "printf process-ready".to_string(),
                    cwd: "/var/hambur/workspace".to_string(),
                    started_at_ms: now_ms(),
                    pid: child.id(),
                    pid_file: None,
                    child,
                    output,
                    exit_code: None,
                    finished_at_ms: 0,
                },
            );

        let invocation = ToolInvocation::from_model_call(
            0,
            "call_process_wait".to_string(),
            "turn_process".to_string(),
            session_id.clone(),
            "process".to_string(),
            serde_json::json!({
                "action": "wait",
                "process_session_id": process_session_id.clone(),
                "timeout_ms": 30_000
            })
            .to_string(),
        )
        .expect("process invocation");
        let result = runtime
            .resolve_process_tool_result(&invocation, &invocation.arguments_value().unwrap());
        assert!(!result.is_error, "process wait failed: {}", result.summary);
        assert!(result.context_stub.contains("process-ready"));
        assert!(result.context_stub.contains("processSessionId"));
        assert!(
            runtime
                .process_sessions
                .lock()
                .expect("process registry")
                .is_empty()
        );
        let log_invocation = ToolInvocation::from_model_call(
            0,
            "call_process_log".to_string(),
            "turn_process".to_string(),
            session_id.clone(),
            "process".to_string(),
            serde_json::json!({
                "action": "log",
                "process_session_id": process_session_id
            })
            .to_string(),
        )
        .expect("process log invocation");
        let log_result = runtime.resolve_process_tool_result(
            &log_invocation,
            &log_invocation.arguments_value().unwrap(),
        );
        assert!(
            !log_result.is_error,
            "process log failed: {}",
            log_result.summary
        );
        assert!(log_result.context_stub.contains("process-ready"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_web_fetch_returns_untrusted_http_content() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let body = "hello from local web";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_test_provider(&runtime, "gpt-web");

        let url = format!("http://{addr}/page");
        let sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_web_fetch\",\"function\":{{\"name\":\"web_fetch\",\"arguments\":\"{{\\\"urls\\\":[\\\"{}\\\"]}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n",
            url
        );
        let continuation_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<untrusted_tool_result hello from local web\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let send = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_m7_web_fetch".to_string(),
            idempotency_key: "message:m7:web-fetch".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "fetch local web".to_string(),
            payload_json: serde_json::json!({"sse": sse, "sse_sequence": [continuation_sse]}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_event(&runtime, "TurnFinished");
        server.join().expect("server thread");

        let message = runtime
            .get_message_snapshot(latest_assistant_message_id(&runtime, &session_id))
            .message
            .expect("message");
        assert!(message.content_text.contains("hello from local web"));
        assert!(message.content_text.contains("<untrusted_tool_result"));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn milestone7_web_fetch_accepts_https_urls_for_tls_provider_client() {
        assert!(validate_web_fetch_url("https://example.test/path").is_ok());
        assert!(validate_web_fetch_url("http://example.test/path").is_ok());
        assert!(validate_web_fetch_url("file:///tmp/nope").is_err());
    }

    #[test]
    fn session_rename_and_pin_are_persisted_and_sorted_first() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let first = create_test_session(&runtime);
        let second = create_test_session(&runtime);

        let rename = runtime.dispatch(RuntimeCommand {
            idempotency_key: "rename:first".to_string(),
            kind: "RenameSession".to_string(),
            session_id: first.clone(),
            title: "renamed first".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(rename.accepted, "rename rejected: {}", rename.message);
        let _ = runtime.next_event().expect("rename event");

        let pin = runtime.dispatch(RuntimeCommand {
            idempotency_key: "pin:first".to_string(),
            kind: "SetSessionPinned".to_string(),
            session_id: first.clone(),
            payload_json: serde_json::json!({"pinned": true}).to_string(),
            ..RuntimeCommand::default()
        });
        assert!(pin.accepted, "pin rejected: {}", pin.message);
        let _ = runtime.next_event().expect("pin event");

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        assert_eq!(
            snapshot.sessions.first().map(|session| session.id.as_str()),
            Some(first.as_str())
        );
        let first_summary = snapshot
            .sessions
            .iter()
            .find(|session| session.id == first)
            .expect("first session");
        assert_eq!(first_summary.title, "renamed first");
        assert!(first_summary.pinned_at_ms > 0);
        assert!(snapshot.sessions.iter().any(|session| session.id == second));

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn first_user_message_titles_default_session() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_test_provider(&runtime, "gpt-test");

        let create = runtime.create_session("New chat".to_string());
        assert!(create.accepted, "create rejected: {}", create.message);
        let created = runtime.next_event().expect("created event");
        let session_id = created.snapshot.selected_session_id;

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_title_first_message".to_string(),
            idempotency_key: "message:title:first".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "请帮我解释 Kotlin Flow 的背压问题，以及怎么处理".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .expect("session summary");
        assert_eq!(session.title, "请帮我解释 Kotlin Flow 的背压问题，以及怎么处理");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn first_user_message_keeps_custom_session_title() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        configure_test_provider(&runtime, "gpt-test");

        let create = runtime.create_session("Research notes".to_string());
        assert!(create.accepted, "create rejected: {}", create.message);
        let created = runtime.next_event().expect("created event");
        let session_id = created.snapshot.selected_session_id;

        let ack = runtime.dispatch(RuntimeCommand {
            command_id: "cmd_keep_custom_title".to_string(),
            idempotency_key: "message:title:custom".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "This should not replace the title".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(ack.accepted, "send rejected: {}", ack.message);

        let snapshot = runtime.get_session_list_snapshot(10, 0);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .expect("session summary");
        assert_eq!(session.title, "Research notes");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    #[test]
    fn message_snapshot_contains_attached_attachment_records() {
        let app_files_dir = temp_app_dir();
        fs::create_dir_all(&app_files_dir).expect("create temp app dir");

        let runtime = RuntimeEngine::create(AppBootstrap {
            app_files_dir: app_files_dir.to_string_lossy().to_string(),
            native_library_dir: String::new(),
        })
        .expect("create runtime");
        let _ = runtime.next_event().expect("ready event");
        let session_id = create_test_session(&runtime);
        configure_named_test_provider(&runtime, "provider_attach_dto", "model-attach-dto");

        let import = runtime.dispatch(RuntimeCommand {
            idempotency_key: "attachment:dto:import".to_string(),
            kind: "ImportAttachmentFromUri".to_string(),
            session_id: session_id.clone(),
            payload_json: serde_json::json!({
                "displayName": "note.txt",
                "mimeType": "text/plain",
                "base64": base64::engine::general_purpose::STANDARD.encode("hello attachment")
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(import.accepted, "import rejected: {}", import.message);
        let imported = runtime.next_event().expect("import event");
        let attachment_id = imported
            .snapshot
            .pending_attachments
            .first()
            .expect("pending attachment")
            .id
            .clone();

        let send = runtime.dispatch(RuntimeCommand {
            idempotency_key: "attachment:dto:send".to_string(),
            kind: "SendMessage".to_string(),
            session_id: session_id.clone(),
            content: "see attached".to_string(),
            payload_json: serde_json::json!({
                "attachmentIds": [attachment_id],
                "content": "ok"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(send.accepted, "send rejected: {}", send.message);
        wait_for_session_finished(&runtime, &session_id, "attachment dto");

        let user_item = runtime
            .get_session_snapshot(session_id)
            .timeline_items
            .into_iter()
            .find(|item| item.kind == "UserMessage")
            .expect("user message item");
        let message = runtime
            .get_message_snapshot(user_item.payload_ref)
            .message
            .expect("message snapshot");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].display_name, "note.txt");
        assert_eq!(message.attachments[0].status, "attached");

        let _ = fs::remove_dir_all(app_files_dir);
    }

    fn temp_app_dir() -> PathBuf {
        unsafe {
            std::env::set_var("HAMBUR_TEST_MOCK_ROOTFS", "1");
        }
        std::env::temp_dir().join(new_id("hambur_runtime_test"))
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = stream.read(&mut buffer).expect("read provider request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(header_end) = find_header_end(&request) {
                let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        if name.eq_ignore_ascii_case("content-length") {
                            value.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                let body_start = header_end + 4;
                if request.len().saturating_sub(body_start) >= content_length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&request).to_string()
    }

    fn find_header_end(bytes: &[u8]) -> Option<usize> {
        bytes.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn create_test_session(runtime: &RuntimeEngine) -> String {
        let ack = runtime.create_session("Chat".to_string());
        assert!(ack.accepted, "create rejected: {}", ack.message);
        let event = runtime.next_event().expect("session created");
        assert_eq!(event.kind.as_str(), "SessionCreated");
        event.snapshot.selected_session_id
    }

    fn latest_assistant_message_id(runtime: &RuntimeEngine, session_id: &str) -> String {
        let page = runtime.get_timeline_page(session_id.to_string(), 0, 100);
        let block = page
            .items
            .iter()
            .rev()
            .find(|item| item.content_type == "assistant_markdown_block")
            .expect("assistant markdown block");
        page.markdown_block_payloads
            .iter()
            .find(|payload| payload.id == block.payload_ref)
            .expect("assistant markdown payload")
            .message_id
            .clone()
    }

    fn latest_assistant_message_snapshot(
        runtime: &RuntimeEngine,
        session_id: &str,
    ) -> MessageRecord {
        runtime
            .tokio
            .block_on(runtime.database.messages_for_session(session_id))
            .expect("session messages")
            .into_iter()
            .rev()
            .find(|message| message.role == "assistant")
            .expect("assistant message snapshot")
    }

    fn configure_test_provider(runtime: &RuntimeEngine, model_id: &str) {
        configure_named_test_provider(runtime, "provider_test", model_id);
    }

    fn configure_named_test_provider(runtime: &RuntimeEngine, provider_id: &str, model_id: &str) {
        configure_named_models(runtime, provider_id, &[(model_id, false)]);
    }

    fn configure_fallback_group(
        runtime: &RuntimeEngine,
        group_id: &str,
        targets: &[(&str, &str)],
    ) {
        let group = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_group_{group_id}"),
            idempotency_key: format!("{group_id}:group:test"),
            kind: "UpdateModelGroup".to_string(),
            message_id: group_id.to_string(),
            title: format!("Test Group {group_id}"),
            payload_json: serde_json::json!({
                "groupId": group_id,
                "name": format!("Test Group {group_id}"),
                "routingStrategy": "fallback",
                "fallbackPolicy": "default"
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(group.accepted, "group rejected: {}", group.message);
        let _ = runtime.next_event().expect("group event");

        for (position, (provider_id, model_id)) in targets.iter().enumerate() {
            let member = runtime.dispatch(RuntimeCommand {
                command_id: format!("cmd_group_member_{group_id}_{position}"),
                idempotency_key: format!("{group_id}:{provider_id}:{model_id}:member:test"),
                kind: "UpdateModelGroupMember".to_string(),
                message_id: group_id.to_string(),
                provider_id: (*provider_id).to_string(),
                model_id: (*model_id).to_string(),
                payload_json: serde_json::json!({
                    "groupId": group_id,
                    "providerId": provider_id,
                    "modelId": model_id,
                    "position": position,
                    "enabled": true
                })
                .to_string(),
                ..RuntimeCommand::default()
            });
            assert!(member.accepted, "member rejected: {}", member.message);
            let _ = runtime.next_event().expect("member event");
        }

        let defaults = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_default_group_{group_id}"),
            idempotency_key: format!("{group_id}:default:test"),
            kind: "UpdateDefaultModelGroups".to_string(),
            payload_json: serde_json::json!({
                "primaryGroupId": group_id
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(defaults.accepted, "defaults rejected: {}", defaults.message);
        let _ = runtime.next_event().expect("default group event");
    }

    fn configure_named_models(runtime: &RuntimeEngine, provider_id: &str, models: &[(&str, bool)]) {
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_good_{provider_id}"),
            idempotency_key: format!("{provider_id}:update:test"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("Test Provider {provider_id}"),
            chunk: "https://api.test/v1".to_string(),
            payload_json: format!("android-secret://provider/{provider_id}"),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let data = models
            .iter()
            .map(|(model_id, supports_image_input)| {
                serde_json::json!({
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": true,
                    "supports_tool_call": true,
                    "supports_image_input": supports_image_input,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 4096
                })
            })
            .collect::<Vec<_>>();
        let models_json = serde_json::json!({"data": data}).to_string();
        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_{provider_id}"),
            idempotency_key: format!("{provider_id}:models:test"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: models_json,
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    fn configure_http_test_provider(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
    ) {
        // Tests use env:// so the database still stores only a secret reference.
        unsafe {
            std::env::set_var("HAMBUR_TEST_OPENAI_KEY", "test-api-key");
        }
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_http_{provider_id}"),
            idempotency_key: format!("{provider_id}:http-provider:test"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("HTTP Provider {provider_id}"),
            chunk: base_url.to_string(),
            payload_json: "env://HAMBUR_TEST_OPENAI_KEY".to_string(),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_http_{provider_id}"),
            idempotency_key: format!("{provider_id}:http-models:test"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: serde_json::json!({
                "data": [{
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": true,
                    "supports_tool_call": true,
                    "supports_image_input": false,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 4096
                }]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    struct RealProviderTestConfig {
        secret_env: String,
        base_url: String,
        model_id: String,
    }

    impl RealProviderTestConfig {
        fn from_env() -> Self {
            let secret_env = "HAMBUR_E2E_OPENAI_API_KEY".to_string();
            let api_key = std::env::var(&secret_env)
                .expect("set HAMBUR_E2E_OPENAI_API_KEY to run this ignored integration test");
            assert!(!api_key.trim().is_empty(), "API key must not be empty");
            Self {
                secret_env,
                base_url: std::env::var("HAMBUR_E2E_OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://opencode.ai/zen/v1".to_string()),
                model_id: std::env::var("HAMBUR_E2E_OPENAI_MODEL")
                    .unwrap_or_else(|_| "deepseek-v4-flash-free".to_string()),
            }
        }
    }

    fn configure_env_secret_provider(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
        secret_env: &str,
    ) {
        configure_env_secret_provider_with_capabilities(
            runtime,
            provider_id,
            model_id,
            base_url,
            secret_env,
            false,
            false,
        );
    }

    fn configure_env_secret_provider_with_capabilities(
        runtime: &RuntimeEngine,
        provider_id: &str,
        model_id: &str,
        base_url: &str,
        secret_env: &str,
        supports_tool_call: bool,
        supports_image_input: bool,
    ) {
        let provider = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_provider_env_{provider_id}"),
            idempotency_key: format!("{provider_id}:env-provider:e2e"),
            kind: "UpdateProvider".to_string(),
            provider_id: provider_id.to_string(),
            title: format!("E2E Provider {provider_id}"),
            chunk: base_url.to_string(),
            payload_json: format!("env://{secret_env}"),
            ..RuntimeCommand::default()
        });
        assert!(provider.accepted, "provider rejected: {}", provider.message);
        let _ = runtime.next_event().expect("provider event");

        let models = runtime.dispatch(RuntimeCommand {
            command_id: format!("cmd_models_env_{provider_id}"),
            idempotency_key: format!("{provider_id}:env-models:e2e"),
            kind: "RefreshProviderModels".to_string(),
            provider_id: provider_id.to_string(),
            payload_json: serde_json::json!({
                "data": [{
                    "id": model_id,
                    "display_name": model_id,
                    "supports_reasoning": false,
                    "supports_tool_call": supports_tool_call,
                    "supports_image_input": supports_image_input,
                    "supports_structured_output": false,
                    "supports_temperature": true,
                    "context_limit": 32000,
                    "output_limit": 1024
                }]
            })
            .to_string(),
            ..RuntimeCommand::default()
        });
        assert!(models.accepted, "models rejected: {}", models.message);
        let _ = runtime.next_event().expect("models event");
    }

    fn wait_for_event(runtime: &RuntimeEngine, kind: &str) {
        for _ in 0..64 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind {
                return;
            }
        }
        panic!("missing event: {kind}");
    }

    fn next_event_with_timeout(runtime: &RuntimeEngine, label: &str) -> RuntimeEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(mut receiver) = runtime.receiver.lock()
                && let Ok(event) = receiver.try_recv()
            {
                return event;
            }
            if Instant::now() >= deadline {
                panic!("timed out waiting for event: {label}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn assistant_markdown_text(runtime: &RuntimeEngine, session_id: &str) -> String {
        let timeline = runtime.get_timeline_page(session_id.to_string(), 0, 100);
        let payload_ids = timeline
            .items
            .iter()
            .filter(|item| item.content_type == "assistant_markdown_block")
            .map(|item| item.payload_ref.as_str())
            .collect::<HashSet<_>>();
        timeline
            .markdown_block_payloads
            .iter()
            .filter(|payload| payload_ids.contains(payload.id.as_str()))
            .map(|payload| payload.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_for_session_event(runtime: &RuntimeEngine, kind: &str, session_id: &str) {
        for _ in 0..128 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind && event.session_id == session_id {
                return;
            }
        }
        panic!("missing event: {kind} for session {session_id}");
    }

    fn wait_for_session_finished(runtime: &RuntimeEngine, session_id: &str, label: &str) {
        for _ in 0..256 {
            let event = runtime.next_event().expect("runtime event");
            if event.session_id != session_id {
                continue;
            }
            match event.kind.as_str() {
                "TurnFinished" => return,
                "TurnFailed" | "TurnCancelled" => {
                    panic!(
                        "{label} ended as {}: {} {}",
                        event.kind.as_str(),
                        event.error_code,
                        event.message
                    );
                }
                _ => {}
            }
        }
        panic!("missing TurnFinished for {label} in session {session_id}");
    }

    fn wait_for_session_event_result(
        runtime: &RuntimeEngine,
        kind: &str,
        session_id: &str,
    ) -> super::RuntimeEvent {
        for _ in 0..128 {
            let event = runtime.next_event().expect("runtime event");
            if event.kind.as_str() == kind && event.session_id == session_id {
                return event;
            }
        }
        panic!("missing event: {kind} for session {session_id}");
    }
