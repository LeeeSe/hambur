use std::path::Path;

use hambur_core::{HamburError, HamburResult, new_id, now_ms};
use turso::{Builder, Connection, Row, params};

const DEFAULT_SESSION_TITLE: &str = "Untitled session";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub message_count: u32,
    pub latest_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineItemSnapshot {
    pub id: String,
    pub stable_key: String,
    pub content_type: String,
    pub display_sequence: u64,
    pub version_sequence: u64,
    pub payload_ref: String,
    pub small_summary: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content_text: String,
    pub created_at_ms: u64,
    pub version_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRecord {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub finished_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTimelineItem {
    pub stable_key: String,
    pub content_type: String,
    pub display_sequence: u64,
    pub payload_ref: String,
    pub small_summary: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSnapshot {
    pub sessions: Vec<SessionSummary>,
    pub selected_session_id: String,
    pub timeline_items: Vec<TimelineItemSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelinePageData {
    pub items: Vec<TimelineItemSnapshot>,
    pub next_before_cursor: u64,
    pub has_more: bool,
}

pub struct HamburDatabase {
    connection: Connection,
}

impl HamburDatabase {
    pub async fn open(path: impl AsRef<Path>) -> HamburResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HamburError::Internal(format!("create database directory: {error}"))
            })?;
        }

        let database = Builder::new_local(path.to_string_lossy().as_ref())
            .build()
            .await
            .map_err(database_error)?;
        let connection = database.connect().map_err(database_error)?;
        let database = Self { connection };
        database.migrate().await?;
        Ok(database)
    }

    pub async fn bootstrap_snapshot(&self) -> HamburResult<AppSnapshot> {
        self.snapshot_for_selected(None).await
    }

    pub async fn create_session(&self, title: &str) -> HamburResult<AppSnapshot> {
        let id = new_id("ses");
        let now = now_ms();
        let title = normalize_title(title);

        self.connection
            .execute(
                "INSERT INTO sessions (id, title, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id.clone(), title, now as i64, now as i64],
            )
            .await
            .map_err(database_error)?;

        self.set_active_session(Some(&id)).await?;
        self.snapshot_for_selected(Some(&id)).await
    }

    pub async fn open_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        if !self.session_exists(session_id).await? {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        let now = now_ms();
        self.connection
            .execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        self.set_active_session(Some(session_id)).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn delete_session(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE sessions
                 SET deleted_at_ms = ?1, updated_at_ms = ?1
                 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        }

        if self.active_session_id().await?.as_deref() == Some(session_id) {
            self.set_active_session(None).await?;
        }
        self.snapshot_for_selected(None).await
    }

    pub async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content_text: &str,
    ) -> HamburResult<MessageRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("msg");
        let now = now_ms();
        let role = normalize_role(role)?;
        self.connection
            .execute(
                "INSERT INTO messages
                    (id, session_id, role, content_text, created_at_ms, version_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![
                    id.clone(),
                    session_id,
                    role.clone(),
                    content_text,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(MessageRecord {
            id,
            session_id: session_id.to_string(),
            role,
            content_text: content_text.to_string(),
            created_at_ms: now,
            version_sequence: 1,
        })
    }

    pub async fn upsert_timeline_item(
        &self,
        session_id: &str,
        item: NewTimelineItem,
    ) -> HamburResult<TimelineItemSnapshot> {
        self.ensure_session_exists(session_id).await?;

        let stable_key = item.stable_key;
        if stable_key.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline stable_key must not be empty".to_string(),
            ));
        }
        if item.content_type.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "timeline content_type must not be empty".to_string(),
            ));
        }

        let id = new_id("tl");
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO timeline_items
                    (
                        id,
                        session_id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind,
                        created_at_ms,
                        updated_at_ms
                    )
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, ?9, ?9)
                 ON CONFLICT(session_id, stable_key) DO UPDATE SET
                    content_type = excluded.content_type,
                    display_sequence = excluded.display_sequence,
                    version_sequence = timeline_items.version_sequence + 1,
                    payload_ref = excluded.payload_ref,
                    small_summary = excluded.small_summary,
                    kind = excluded.kind,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    id,
                    session_id,
                    stable_key.clone(),
                    item.content_type,
                    item.display_sequence as i64,
                    item.payload_ref,
                    item.small_summary,
                    item.kind,
                    now as i64
                ],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        self.timeline_item_by_stable_key(session_id, &stable_key).await
    }

    pub async fn create_turn(&self, session_id: &str, status: &str) -> HamburResult<TurnRecord> {
        self.ensure_session_exists(session_id).await?;

        let id = new_id("turn");
        let now = now_ms();
        let status = normalize_status(status)?;
        self.connection
            .execute(
                "INSERT INTO turns
                    (id, session_id, status, created_at_ms, updated_at_ms, finished_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?4, NULL)",
                params![id.clone(), session_id, status.clone(), now as i64],
            )
            .await
            .map_err(database_error)?;
        self.touch_session(session_id, now).await?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            status,
            created_at_ms: now,
            updated_at_ms: now,
            finished_at_ms: 0,
        })
    }

    pub async fn update_turn_status(
        &self,
        turn_id: &str,
        status: &str,
        finished: bool,
    ) -> HamburResult<TurnRecord> {
        let status = normalize_status(status)?;
        let now = now_ms();
        let changed = self
            .connection
            .execute(
                "UPDATE turns
                 SET status = ?1,
                     updated_at_ms = ?2,
                     finished_at_ms = CASE WHEN ?3 THEN ?2 ELSE finished_at_ms END
                 WHERE id = ?4",
                params![status, now as i64, finished, turn_id],
            )
            .await
            .map_err(database_error)?;

        if changed == 0 {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        }

        self.turn_by_id(turn_id).await
    }

    pub async fn session_snapshot(&self, session_id: &str) -> HamburResult<AppSnapshot> {
        self.ensure_session_exists(session_id).await?;
        self.snapshot_for_selected(Some(session_id)).await
    }

    pub async fn session_list(
        &self,
        limit: u32,
        offset: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(limit, offset).await
    }

    pub async fn session_summary(&self, session_id: &str) -> HamburResult<SessionSummary> {
        self.ensure_session_exists(session_id).await?;
        self.session_summary_by_id(session_id).await
    }

    pub async fn timeline_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        self.ensure_session_exists(session_id).await?;
        self.timeline_items_page(session_id, before_cursor, limit)
            .await
    }

    pub async fn message_snapshot(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        if message_id.trim().is_empty() {
            return Err(HamburError::InvalidCommand(
                "message_id must not be empty".to_string(),
            ));
        }
        self.message_by_id(message_id).await
    }

    pub async fn search_sessions(
        &self,
        query: &str,
        limit: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let query = query.trim();
        if query.is_empty() {
            return self.list_sessions_page(limit, 0).await;
        }

        let limit = clamp_limit(limit, 1, 100);
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                  AND s.title LIKE ?1 ESCAPE '\\'
                ORDER BY s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?2
                ",
                params![pattern, limit as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }

    async fn migrate(&self) -> HamburResult<()> {
        self.connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    deleted_at_ms INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_sessions_active_updated
                    ON sessions(deleted_at_ms, updated_at_ms DESC, created_at_ms DESC);

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    role TEXT NOT NULL,
                    content_text TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL DEFAULT 1
                );

                CREATE INDEX IF NOT EXISTS idx_messages_session_order
                    ON messages(session_id, created_at_ms, id);

                CREATE TABLE IF NOT EXISTS timeline_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    stable_key TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    display_sequence INTEGER NOT NULL,
                    version_sequence INTEGER NOT NULL,
                    payload_ref TEXT NOT NULL,
                    small_summary TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    UNIQUE(session_id, stable_key)
                );

                CREATE INDEX IF NOT EXISTS idx_timeline_items_session_order
                    ON timeline_items(session_id, display_sequence, id);

                CREATE TABLE IF NOT EXISTS turns (
                    id TEXT PRIMARY KEY NOT NULL,
                    session_id TEXT NOT NULL REFERENCES sessions(id),
                    status TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    finished_at_ms INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_turns_session_updated
                    ON turns(session_id, updated_at_ms DESC, id);
                ",
            )
            .await
            .map_err(database_error)
    }

    async fn snapshot_for_selected(
        &self,
        requested_session_id: Option<&str>,
    ) -> HamburResult<AppSnapshot> {
        let sessions = self.list_sessions().await?;
        let active_session_id = self.active_session_id().await?;
        let selected_session_id = match requested_session_id {
            Some(session_id) if sessions.iter().any(|session| session.id == session_id) => {
                session_id.to_string()
            }
            _ if active_session_id
                .as_ref()
                .is_some_and(|session_id| sessions.iter().any(|session| session.id == *session_id)) =>
            {
                active_session_id.unwrap_or_default()
            }
            _ => sessions
                .first()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
        };
        let timeline_items = if selected_session_id.is_empty() {
            Vec::new()
        } else {
            self.timeline_items_for_session(&selected_session_id)
                .await?
        };

        Ok(AppSnapshot {
            sessions,
            selected_session_id,
            timeline_items,
        })
    }

    async fn list_sessions(&self) -> HamburResult<Vec<SessionSummary>> {
        self.list_sessions_page(100, 0).await
    }

    async fn list_sessions_page(
        &self,
        limit: u32,
        offset: u32,
    ) -> HamburResult<Vec<SessionSummary>> {
        let limit = clamp_limit(limit, 1, 200);
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.deleted_at_ms IS NULL
                ORDER BY s.updated_at_ms DESC, s.created_at_ms DESC, s.id DESC
                LIMIT ?1 OFFSET ?2
                ",
                params![limit as i64, offset as i64],
            )
            .await
            .map_err(database_error)?;

        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            sessions.push(session_summary_from_row(&row)?);
        }

        Ok(sessions)
    }

    async fn timeline_items_for_session(
        &self,
        session_id: &str,
    ) -> HamburResult<Vec<TimelineItemSnapshot>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    stable_key,
                    content_type,
                    display_sequence,
                    version_sequence,
                    payload_ref,
                    small_summary,
                    kind
                FROM timeline_items
                WHERE session_id = ?1
                ORDER BY display_sequence ASC, id ASC
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(TimelineItemSnapshot {
                id: row.get::<String>(0).map_err(database_error)?,
                stable_key: row.get::<String>(1).map_err(database_error)?,
                content_type: row.get::<String>(2).map_err(database_error)?,
                display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
                version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
                payload_ref: row.get::<String>(5).map_err(database_error)?,
                small_summary: row.get::<String>(6).map_err(database_error)?,
                kind: row.get::<String>(7).map_err(database_error)?,
            });
        }

        Ok(items)
    }

    async fn timeline_items_page(
        &self,
        session_id: &str,
        before_cursor: u64,
        limit: u32,
    ) -> HamburResult<TimelinePageData> {
        let limit = clamp_limit(limit, 1, 100);
        let fetch_limit = limit.saturating_add(1);
        let mut rows = if before_cursor == 0 {
            self.connection
                .query(
                    "
                    SELECT
                        id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind
                    FROM timeline_items
                    WHERE session_id = ?1
                    ORDER BY display_sequence DESC, id DESC
                    LIMIT ?2
                    ",
                    params![session_id, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        } else {
            self.connection
                .query(
                    "
                    SELECT
                        id,
                        stable_key,
                        content_type,
                        display_sequence,
                        version_sequence,
                        payload_ref,
                        small_summary,
                        kind
                    FROM timeline_items
                    WHERE session_id = ?1
                      AND display_sequence < ?2
                    ORDER BY display_sequence DESC, id DESC
                    LIMIT ?3
                    ",
                    params![session_id, before_cursor as i64, fetch_limit as i64],
                )
                .await
                .map_err(database_error)?
        };

        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(database_error)? {
            items.push(timeline_item_from_row(&row)?);
        }

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        items.reverse();
        let next_before_cursor = if has_more {
            items
                .first()
                .map(|item| item.display_sequence)
                .unwrap_or_default()
        } else {
            0
        };

        Ok(TimelinePageData {
            items,
            next_before_cursor,
            has_more,
        })
    }

    async fn session_exists(&self, session_id: &str) -> HamburResult<bool> {
        let mut rows = self
            .connection
            .query(
                "SELECT id FROM sessions WHERE id = ?1 AND deleted_at_ms IS NULL LIMIT 1",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        Ok(rows.next().await.map_err(database_error)?.is_some())
    }

    async fn session_summary_by_id(&self, session_id: &str) -> HamburResult<SessionSummary> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    s.id,
                    s.title,
                    s.created_at_ms,
                    s.updated_at_ms,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS message_count,
                    COALESCE(
                        (
                            SELECT ti.small_summary
                            FROM timeline_items ti
                            WHERE ti.session_id = s.id
                            ORDER BY ti.display_sequence DESC, ti.id DESC
                            LIMIT 1
                        ),
                        ''
                    ) AS latest_preview
                FROM sessions s
                WHERE s.id = ?1 AND s.deleted_at_ms IS NULL
                LIMIT 1
                ",
                params![session_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )));
        };

        session_summary_from_row(&row)
    }

    async fn ensure_session_exists(&self, session_id: &str) -> HamburResult<()> {
        if self.session_exists(session_id).await? {
            Ok(())
        } else {
            Err(HamburError::InvalidCommand(format!(
                "session not found: {session_id}"
            )))
        }
    }

    async fn touch_session(&self, session_id: &str, now: u64) -> HamburResult<()> {
        self.connection
            .execute(
                "UPDATE sessions SET updated_at_ms = ?1 WHERE id = ?2 AND deleted_at_ms IS NULL",
                params![now as i64, session_id],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn set_active_session(&self, session_id: Option<&str>) -> HamburResult<()> {
        let now = now_ms();
        match session_id {
            Some(session_id) => self
                .connection
                .execute(
                    "INSERT INTO app_state (key, value, updated_at_ms)
                     VALUES ('active_session_id', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET
                        value = excluded.value,
                        updated_at_ms = excluded.updated_at_ms",
                    params![session_id, now as i64],
                )
                .await
                .map(|_| ())
                .map_err(database_error),
            None => self
                .connection
                .execute("DELETE FROM app_state WHERE key = 'active_session_id'", ())
                .await
                .map(|_| ())
                .map_err(database_error),
        }
    }

    async fn active_session_id(&self) -> HamburResult<Option<String>> {
        let mut rows = self
            .connection
            .query(
                "SELECT value FROM app_state WHERE key = 'active_session_id' LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };
        let value = row.get::<String>(0).map_err(database_error)?;
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    async fn timeline_item_by_stable_key(
        &self,
        session_id: &str,
        stable_key: &str,
    ) -> HamburResult<TimelineItemSnapshot> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT
                    id,
                    stable_key,
                    content_type,
                    display_sequence,
                    version_sequence,
                    payload_ref,
                    small_summary,
                    kind
                FROM timeline_items
                WHERE session_id = ?1 AND stable_key = ?2
                LIMIT 1
                ",
                params![session_id, stable_key],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::Internal(format!(
                "timeline item not found after upsert: {stable_key}"
            )));
        };

        timeline_item_from_row(&row)
    }

    async fn message_by_id(&self, message_id: &str) -> HamburResult<Option<MessageRecord>> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, session_id, role, content_text, created_at_ms, version_sequence
                FROM messages
                WHERE id = ?1
                LIMIT 1
                ",
                params![message_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };

        Ok(Some(MessageRecord {
            id: row.get::<String>(0).map_err(database_error)?,
            session_id: row.get::<String>(1).map_err(database_error)?,
            role: row.get::<String>(2).map_err(database_error)?,
            content_text: row.get::<String>(3).map_err(database_error)?,
            created_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
            version_sequence: unsigned_ms(row.get::<i64>(5).map_err(database_error)?),
        }))
    }

    async fn turn_by_id(&self, turn_id: &str) -> HamburResult<TurnRecord> {
        let mut rows = self
            .connection
            .query(
                "
                SELECT id, session_id, status, created_at_ms, updated_at_ms, finished_at_ms
                FROM turns
                WHERE id = ?1
                LIMIT 1
                ",
                params![turn_id],
            )
            .await
            .map_err(database_error)?;

        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Err(HamburError::InvalidCommand(format!(
                "turn not found: {turn_id}"
            )));
        };

        Ok(TurnRecord {
            id: row.get::<String>(0).map_err(database_error)?,
            session_id: row.get::<String>(1).map_err(database_error)?,
            status: row.get::<String>(2).map_err(database_error)?,
            created_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
            updated_at_ms: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
            finished_at_ms: row
                .get::<Option<i64>>(5)
                .map_err(database_error)?
                .map(unsigned_ms)
                .unwrap_or_default(),
        })
    }
}

fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        DEFAULT_SESSION_TITLE.to_string()
    } else {
        title.chars().take(120).collect()
    }
}

fn normalize_role(role: &str) -> HamburResult<String> {
    let role = role.trim();
    match role {
        "system" | "user" | "assistant" | "tool" => Ok(role.to_string()),
        _ => Err(HamburError::InvalidCommand(format!(
            "invalid message role: {role}"
        ))),
    }
}

fn normalize_status(status: &str) -> HamburResult<String> {
    let status = status.trim();
    if status.is_empty() {
        return Err(HamburError::InvalidCommand(
            "status must not be empty".to_string(),
        ));
    }

    Ok(status.chars().take(80).collect())
}

fn unsigned_ms(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn unsigned_count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn clamp_limit(value: u32, min: u32, max: u32) -> u32 {
    value.clamp(min, max)
}

fn session_summary_from_row(row: &Row) -> HamburResult<SessionSummary> {
    Ok(SessionSummary {
        id: row.get::<String>(0).map_err(database_error)?,
        title: row.get::<String>(1).map_err(database_error)?,
        created_at_ms: unsigned_ms(row.get::<i64>(2).map_err(database_error)?),
        updated_at_ms: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        message_count: unsigned_count(row.get::<i64>(4).map_err(database_error)?),
        latest_preview: row.get::<String>(5).map_err(database_error)?,
    })
}

fn timeline_item_from_row(row: &Row) -> HamburResult<TimelineItemSnapshot> {
    Ok(TimelineItemSnapshot {
        id: row.get::<String>(0).map_err(database_error)?,
        stable_key: row.get::<String>(1).map_err(database_error)?,
        content_type: row.get::<String>(2).map_err(database_error)?,
        display_sequence: unsigned_ms(row.get::<i64>(3).map_err(database_error)?),
        version_sequence: unsigned_ms(row.get::<i64>(4).map_err(database_error)?),
        payload_ref: row.get::<String>(5).map_err(database_error)?,
        small_summary: row.get::<String>(6).map_err(database_error)?,
        kind: row.get::<String>(7).map_err(database_error)?,
    })
}

fn database_error(error: turso::Error) -> HamburError {
    HamburError::Internal(format!("turso: {error}"))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use hambur_core::new_id;
    use tokio::runtime::Runtime;

    use super::{HamburDatabase, NewTimelineItem};

    #[test]
    fn sessions_survive_restart_and_delete_from_snapshot() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("First")
                .await
                .expect("create session");
            assert_eq!(created.sessions.len(), 1);
            assert_eq!(created.sessions[0].title, "First");
            let session_id = created.selected_session_id.clone();
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let bootstrap = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(bootstrap.sessions.len(), 1);
            assert_eq!(bootstrap.selected_session_id, session_id);

            let deleted = restarted
                .delete_session(&session_id)
                .await
                .expect("delete session");
            assert!(deleted.sessions.is_empty());
            assert!(deleted.selected_session_id.is_empty());
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn active_session_survives_restart_after_opening_older_session() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let first = database
                .create_session("First")
                .await
                .expect("create first");
            let first_id = first.selected_session_id;
            let second = database
                .create_session("Second")
                .await
                .expect("create second");
            assert_eq!(second.selected_session_id, second.sessions[0].id);

            let opened_first = database
                .open_session(&first_id)
                .await
                .expect("open first");
            assert_eq!(opened_first.selected_session_id, first_id);
            drop(database);

            let restarted = HamburDatabase::open(&path).await.expect("reopen database");
            let snapshot = restarted
                .bootstrap_snapshot()
                .await
                .expect("bootstrap snapshot");
            assert_eq!(snapshot.selected_session_id, first_id);
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn message_timeline_and_turn_repositories_write_basic_records() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello")
                .await
                .expect("insert message");
            assert_eq!(message.role, "user");
            assert_eq!(message.content_text, "hello");

            let item = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: message.id.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: message.id.clone(),
                        small_summary: "hello".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("upsert timeline item");
            assert_eq!(item.stable_key, message.id);
            assert_eq!(item.version_sequence, 1);

            let updated = database
                .upsert_timeline_item(
                    &session_id,
                    NewTimelineItem {
                        stable_key: item.stable_key.clone(),
                        content_type: "message".to_string(),
                        display_sequence: message.created_at_ms,
                        payload_ref: item.payload_ref.clone(),
                        small_summary: "hello again".to_string(),
                        kind: "UserMessage".to_string(),
                    },
                )
                .await
                .expect("update timeline item");
            assert_eq!(updated.version_sequence, 2);
            assert_eq!(updated.small_summary, "hello again");

            let turn = database
                .create_turn(&session_id, "Preparing")
                .await
                .expect("create turn");
            let finished = database
                .update_turn_status(&turn.id, "Finished", true)
                .await
                .expect("finish turn");
            assert_eq!(finished.status, "Finished");
            assert!(finished.finished_at_ms > 0);

            let snapshot = database
                .session_snapshot(&session_id)
                .await
                .expect("session snapshot");
            assert_eq!(snapshot.timeline_items.len(), 1);
            assert_eq!(snapshot.timeline_items[0].small_summary, "hello again");
        });

        let _ = fs::remove_file(path);
    }

    #[test]
    fn snapshot_query_methods_are_paginated_and_side_effect_free() {
        let path = temp_database_path();
        let runtime = Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let database = HamburDatabase::open(&path).await.expect("open database");
            let created = database
                .create_session("Searchable Chat")
                .await
                .expect("create session");
            let session_id = created.selected_session_id;

            let message = database
                .insert_message(&session_id, "user", "hello snapshot")
                .await
                .expect("insert message");

            for index in 0..3u64 {
                database
                    .upsert_timeline_item(
                        &session_id,
                        NewTimelineItem {
                            stable_key: format!("item-{index}"),
                            content_type: "message".to_string(),
                            display_sequence: message.created_at_ms + index,
                            payload_ref: message.id.clone(),
                            small_summary: format!("summary {index}"),
                            kind: "UserMessage".to_string(),
                        },
                    )
                    .await
                    .expect("upsert timeline item");
            }

            let sessions = database
                .session_list(10, 0)
                .await
                .expect("session list");
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].title, "Searchable Chat");

            let search = database
                .search_sessions("Searchable", 10)
                .await
                .expect("search sessions");
            assert_eq!(search.len(), 1);

            let page = database
                .timeline_page(&session_id, 0, 2)
                .await
                .expect("timeline page");
            assert_eq!(page.items.len(), 2);
            assert!(page.has_more);
            assert!(page.next_before_cursor > 0);

            let snapshot = database
                .message_snapshot(&message.id)
                .await
                .expect("message snapshot");
            assert_eq!(snapshot.expect("message").content_text, "hello snapshot");
        });

        let _ = fs::remove_file(path);
    }

    fn temp_database_path() -> PathBuf {
        std::env::temp_dir().join(format!("{}.db", new_id("hambur_db_test")))
    }
}
