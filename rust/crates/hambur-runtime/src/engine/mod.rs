use crate::*;

mod chat;
mod config_tool;
mod delegate;
mod events;
mod markdown;
mod memory;
mod model_catalog;
mod platform_tools;
mod process_tools;
mod sandbox_tools;
mod skills;
mod vision_tool;

impl RuntimeEngine {
    pub fn create(bootstrap: AppBootstrap) -> HamburResult<Arc<Self>> {
        if bootstrap.app_files_dir.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "app_files_dir must not be empty".to_string(),
            ));
        }

        let tokio = Runtime::new()
            .map_err(|error| HamburError::Internal(format!("tokio runtime: {error}")))?;
        let database_path = database_path(&bootstrap);
        let database = HamburDatabase::open(database_path)?;
        let filestore = FileStore::new(&bootstrap.app_files_dir)?;
        let sandbox = SandboxService::new_with_native_library_dir(
            &bootstrap.app_files_dir,
            &bootstrap.native_library_dir,
        )?;
        seed_bundled_skills(
            &PathBuf::from(&bootstrap.app_files_dir)
                .join("sandbox")
                .join("global")
                .join("skills"),
        )?;
        let jobs = database.cleanup_pending_attachments(0)?;
        for job in jobs {
            let _ = filestore.delete_relative_if_exists(&job.relative_path);
            let _ = database.mark_file_cleanup_done(&job.id);
        }
        let snapshot = database.bootstrap_snapshot()?;
        for session in &snapshot.sessions {
            let _ = sandbox.prepare_session(&session.id);
        }
        let tools = ToolScheduler::new(PathBuf::from(&bootstrap.app_files_dir).join("offloads"))?
            .with_app_files_dir(&bootstrap.app_files_dir);
        let (sender, receiver) = mpsc::channel(1024);
        let engine = Arc::new(Self {
            tokio,
            self_ref: Mutex::new(Weak::new()),
            bootstrap,
            database,
            filestore,
            sandbox,
            markdown_streams: Mutex::new(HashMap::new()),
            active_turns: Mutex::new(HashMap::new()),
            platform_requests: Mutex::new(HashMap::new()),
            delegate_tasks: Mutex::new(HashMap::new()),
            delegate_sessions: Mutex::new(HashSet::new()),
            process_sessions: Mutex::new(HashMap::new()),
            completed_process_sessions: Mutex::new(HashMap::new()),
            memory_review_sessions: Mutex::new(HashSet::new()),
            router: Mutex::new(ModelRouter::default()),
            tools,
            idempotency: Mutex::new(HashMap::new()),
            sender,
            receiver: Mutex::new(receiver),
            sequence: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });

        if let Ok(mut self_ref) = engine.self_ref.lock() {
            *self_ref = Arc::downgrade(&engine);
        }
        engine.emit_snapshot(RuntimeEventKind::RuntimeReady, snapshot)?;
        engine.schedule_startup_memory_review_check();
        engine.schedule_model_catalog_sync();
        Ok(engine)
    }

    pub fn next_event(&self) -> Option<RuntimeEvent> {
        let mut receiver = self.receiver.lock().ok()?;
        receiver.blocking_recv()
    }

    pub(crate) fn finish_settings_command(
        &self,
        command: RuntimeCommand,
        result: HamburResult<()>,
        message: &'static str,
    ) -> RuntimeCommandAck {
        match result {
            Ok(()) => {
                let _ = self.emit_plain(
                    RuntimeEventKind::SettingsChanged,
                    String::new(),
                    String::new(),
                    message,
                );
                accepted_ack(command.command_id, command.idempotency_key)
            }
            Err(error) => {
                let _ = self.emit_error(error.clone());
                rejected_ack(command.command_id, command.idempotency_key, error)
            }
        }
    }

    pub fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Ok(active_turns) = self.active_turns.lock() {
            for active in active_turns.values() {
                active.cancel.store(true, Ordering::SeqCst);
            }
        }

                let _ = self.emit_plain(RuntimeEventKind::RuntimeClosed, String::new(), String::new(), "");
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn app_files_dir(&self) -> &str {
        &self.bootstrap.app_files_dir
    }

    pub(crate) fn snapshot_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    pub(crate) fn active_turn_for_session(&self, session_id: &str) -> Option<ActiveTurn> {
        self.active_turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(session_id).cloned())
    }

    pub(crate) fn clear_active_turn(&self, session_id: &str, turn_id: &str) {
        if let Ok(mut turns) = self.active_turns.lock()
            && turns
                .get(session_id)
                .is_some_and(|active| active.turn_id == turn_id)
        {
            turns.remove(session_id);
        }
    }
}
