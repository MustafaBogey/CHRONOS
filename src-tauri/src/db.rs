use crate::models::*;
use chrono::{Local, TimeZone, Timelike};
use rusqlite::Connection;
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 6;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS activity_intervals (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           started_at INTEGER NOT NULL,
           ended_at INTEGER NOT NULL,
           local_date TEXT NOT NULL,
           app TEXT NOT NULL,
           detail TEXT NOT NULL DEFAULT '',
           category TEXT NOT NULL,
           project TEXT NOT NULL DEFAULT '',
           focus_minutes REAL NOT NULL DEFAULT 0,
           kind TEXT NOT NULL DEFAULT 'app',
           source TEXT NOT NULL DEFAULT 'Built-in default',
           corrected INTEGER NOT NULL DEFAULT 0,
           review_status TEXT NOT NULL DEFAULT 'none',
           confidence REAL,
           open INTEGER NOT NULL DEFAULT 0,
           CHECK (ended_at >= started_at)
         );
         CREATE INDEX IF NOT EXISTS idx_intervals_date ON activity_intervals(local_date);
         CREATE INDEX IF NOT EXISTS idx_intervals_project ON activity_intervals(project);
         CREATE INDEX IF NOT EXISTS idx_intervals_app ON activity_intervals(app);
         CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL,
           desc TEXT NOT NULL DEFAULT '',
           color TEXT NOT NULL DEFAULT 'var(--green)',
           archived INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS rules (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           app TEXT NOT NULL,
           category TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS exclusions (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           label TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS goals (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS focus_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            intention TEXT NOT NULL DEFAULT 'Deep work',
            project TEXT NOT NULL DEFAULT '',
            elapsed_ms INTEGER NOT NULL DEFAULT 0,
            qualified_minutes REAL NOT NULL DEFAULT 0,
            idle_minutes REAL NOT NULL DEFAULT 0,
            interruptions INTEGER NOT NULL DEFAULT 0,
            open INTEGER NOT NULL DEFAULT 0,
            focus_minutes INTEGER NOT NULL DEFAULT 25,
            break_minutes INTEGER NOT NULL DEFAULT 5,
            breaks_enabled INTEGER NOT NULL DEFAULT 1,
            focus_intervals INTEGER NOT NULL DEFAULT 0,
            break_total_ms INTEGER NOT NULL DEFAULT 0,
            break_planned_ms INTEGER NOT NULL DEFAULT 0,
            break_extensions_ms INTEGER NOT NULL DEFAULT 0,
            break_overrun_ms INTEGER NOT NULL DEFAULT 0,
            break_history TEXT NOT NULL DEFAULT '[]'
          );
         CREATE TABLE IF NOT EXISTS tracking_state (
           id INTEGER PRIMARY KEY CHECK (id = 1),
           state TEXT NOT NULL,
           privacy_return TEXT NOT NULL,
           degraded_reason TEXT,
           updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS undo_snapshots (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           created_at INTEGER NOT NULL,
           payload TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS deletion_log (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           start_date TEXT NOT NULL,
           end_date TEXT NOT NULL,
           deleted_at INTEGER NOT NULL
         );
          CREATE TABLE IF NOT EXISTS backups_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            intervals INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            integrity TEXT NOT NULL
          );
           CREATE TABLE IF NOT EXISTS app_icon_cache (
             app_name TEXT PRIMARY KEY,
             exe TEXT NOT NULL DEFAULT '',
             icon_png BLOB,
             updated_at INTEGER NOT NULL
           );
           CREATE TABLE IF NOT EXISTS idle_exceptions (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             label TEXT NOT NULL,
             created_at INTEGER NOT NULL
           );
           CREATE TABLE IF NOT EXISTS group_breaks (
             at_ts INTEGER PRIMARY KEY
           );
           CREATE TABLE IF NOT EXISTS block_list (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             label TEXT NOT NULL,
             kind TEXT NOT NULL DEFAULT 'domain',
             target TEXT NOT NULL,
             enabled INTEGER NOT NULL DEFAULT 1,
             created_at INTEGER NOT NULL
           );"
    )?;
    let version: i64 = conn
        .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get::<_, String>(0))
        .and_then(|v| v.parse::<i64>().map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))))
        .unwrap_or(0);
    if version == 0 {
        conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION.to_string()])?;
    }
    // ---- additive migrations (safe on existing user history) ----
    if version < 2 {
        // Clients: optional client grouping for projects.
        let _ = conn.execute("ALTER TABLE projects ADD COLUMN client TEXT NOT NULL DEFAULT ''", []);
        // Rules may carry an explicit project assignment.
        let _ = conn.execute("ALTER TABLE rules ADD COLUMN project TEXT NOT NULL DEFAULT ''", []);
        // Why an interval was attributed to its project (rule/learned/user).
        let _ = conn.execute("ALTER TABLE activity_intervals ADD COLUMN project_source TEXT NOT NULL DEFAULT ''", []);
        // Faster learned-pattern queries over corrections.
        conn.execute("CREATE INDEX IF NOT EXISTS idx_intervals_corrected ON activity_intervals(corrected, app)", [])?;
        conn.execute("UPDATE meta SET value = '2' WHERE key = 'schema_version'", [])?;
    }
    if version < 3 {
        // Activity-detail privacy: sanitize previously stored Discord / File Explorer /
        // browser titles in place (identity or curated service label only). Timestamps,
        // durations, classifications, and project assignments are preserved.
        crate::classifier::sanitize_stored_details(conn);
        conn.execute("UPDATE meta SET value = '3' WHERE key = 'schema_version'", [])?;
    }
    if version < 4 {
        // Recognizable offline activity icons: one-shot native icon cache
        // (also present in the initial batch for fresh databases).
        conn.execute(
            "CREATE TABLE IF NOT EXISTS app_icon_cache (
               app_name TEXT PRIMARY KEY, exe TEXT NOT NULL DEFAULT '', icon_png BLOB, updated_at INTEGER NOT NULL
             )",
            [],
        )?;
        crate::app_icons::warm_cache(conn);
        conn.execute("UPDATE meta SET value = '4' WHERE key = 'schema_version'", [])?;
    }
    if version < 5 {
        // V1 intelligence & completeness: manual entries, idle exceptions,
        // work-block break markers, and the distraction block list.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS idle_exceptions (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               label TEXT NOT NULL,
               created_at INTEGER NOT NULL
             )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS group_breaks (
               at_ts INTEGER PRIMARY KEY
             )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS block_list (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               label TEXT NOT NULL,
               kind TEXT NOT NULL DEFAULT 'domain',
               target TEXT NOT NULL,
               enabled INTEGER NOT NULL DEFAULT 1,
               created_at INTEGER NOT NULL
             )",
            [],
        )?;
        // Manual time entries are stored as intervals; the flag keeps their
        // origin explicit so they can never be presented as observed activity.
        conn.execute("ALTER TABLE activity_intervals ADD COLUMN manual INTEGER NOT NULL DEFAULT 0", [])?;
        conn.execute("UPDATE meta SET value = '5' WHERE key = 'schema_version'", [])?;
    }
    if version < 6 {
        // Focus Session rework: configurable focus/break cycles with honest
        // break accounting (planned allowance, requested extensions, actual
        // duration, unplanned overrun) and per-break history.
        let cols: &[(&str, &str)] = &[
            ("focus_minutes", "INTEGER NOT NULL DEFAULT 25"),
            ("break_minutes", "INTEGER NOT NULL DEFAULT 5"),
            ("breaks_enabled", "INTEGER NOT NULL DEFAULT 1"),
            ("focus_intervals", "INTEGER NOT NULL DEFAULT 0"),
            ("break_total_ms", "INTEGER NOT NULL DEFAULT 0"),
            ("break_planned_ms", "INTEGER NOT NULL DEFAULT 0"),
            ("break_extensions_ms", "INTEGER NOT NULL DEFAULT 0"),
            ("break_overrun_ms", "INTEGER NOT NULL DEFAULT 0"),
            ("break_history", "TEXT NOT NULL DEFAULT '[]'"),
        ];
        for (name, ty) in cols {
            let _ = conn.execute(&format!("ALTER TABLE focus_sessions ADD COLUMN {name} {ty}"), []);
        }
        conn.execute("UPDATE meta SET value = '6' WHERE key = 'schema_version'", [])?;
    }
    Ok(())
}

/* ---------- settings ---------- */

pub fn get_settings(conn: &Connection) -> Settings {
    let mut s = Settings::default();
    let Ok(mut stmt) = conn.prepare("SELECT key, value FROM settings") else { return s };
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();
    for (k, v) in rows {
        match k.as_str() {
            "appearance" => s.appearance = v,
            "startup" => s.startup = v == "1",
            "tray" => s.tray = v == "1",
            "auto_idle" => s.auto_idle = v == "1",
            "idle_minutes" => s.idle_minutes = v.parse().unwrap_or(5),
            "breaks" => s.breaks = v == "1",
            "break_every_minutes" => s.break_every_minutes = v.parse().unwrap_or(50),
            "blocking" => s.blocking = v == "1",
            "scheduled" => s.scheduled = v == "1",
            "schedule_enabled" => s.schedule_enabled = v == "1",
            "quiet" => s.quiet = v == "1",
            "quiet_start" => s.quiet_start = v.parse().unwrap_or(20),
            "quiet_end" => s.quiet_end = v.parse().unwrap_or(8),
            "retention" => s.retention = v,
            "time" => s.time = v,
            "display_name" => s.display_name = v,
            "mini_bar" => s.mini_bar = v == "1",
            "date_format" => s.date_format = v,
            "goal_project" => s.goal_project = v,
            "schedule_start" => s.schedule_start = v,
            "schedule_end" => s.schedule_end = v,
            "schedule_days" => s.schedule_days = v,
            "retention_active" => s.retention_active = v == "1",
            _ => {}
        }
    }
    if s.platform.is_empty() {
        s.platform = crate::tracker::platform_name().to_string();
    }
    s
}

pub fn set_settings(conn: &Connection, patch: &serde_json::Value) -> rusqlite::Result<()> {
    let map: Option<serde_json::Map<String, serde_json::Value>> = patch.as_object().cloned();
    let Some(map) = map else { return Ok(()) };
    let tx = conn.unchecked_transaction()?;
    for (k, v) in map {
        let key = match k.as_str() {
            "appearance" => "appearance",
            "startup" => "startup",
            "tray" => "tray",
            "autoIdle" => "auto_idle",
            "idleMinutes" => "idle_minutes",
            "breaks" => "breaks",
            "breakEveryMinutes" => "break_every_minutes",
            "blocking" => "blocking",
            "scheduled" => "scheduled",
            "scheduleEnabled" => "schedule_enabled",
            "quiet" => "quiet",
            "quietStart" => "quiet_start",
            "quietEnd" => "quiet_end",
            "retention" => "retention",
            "time" => "time",
            "platform" => "platform",
            "displayName" => "display_name",
            "miniBar" => "mini_bar",
            "dateFormat" => "date_format",
            "goalProject" => "goal_project",
            "scheduleStart" => "schedule_start",
            "scheduleEnd" => "schedule_end",
            "scheduleDays" => "schedule_days",
            "retentionActive" => "retention_active",
            _ => continue,
        };
        let val = match v {
            serde_json::Value::Bool(b) => if b { "1".to_string() } else { "0".to_string() },
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s,
            _ => continue,
        };
        tx.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2", rusqlite::params![key, val])?;
    }
    tx.commit()
}

/* ---------- goals ---------- */

pub fn get_goals(conn: &Connection) -> Goals {
    let mut g = Goals::default();
    let Ok(mut stmt) = conn.prepare("SELECT key, value FROM goals") else { return g };
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();
    for (k, v) in rows {
        match k.as_str() {
            "daily" => g.daily = v,
            "weekly" => g.weekly = v,
            "project" => g.project = v,
            "limit" => g.limit = v,
            _ => {}
        }
    }
    g
}

pub fn set_goals(conn: &Connection, g: &Goals) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO goals (key, value) VALUES ('daily', ?1) ON CONFLICT(key) DO UPDATE SET value=?1", [g.daily])?;
    conn.execute("INSERT INTO goals (key, value) VALUES ('weekly', ?1) ON CONFLICT(key) DO UPDATE SET value=?1", [g.weekly])?;
    conn.execute("INSERT INTO goals (key, value) VALUES ('project', ?1) ON CONFLICT(key) DO UPDATE SET value=?1", [g.project])?;
    conn.execute("INSERT INTO goals (key, value) VALUES ('limit', ?1) ON CONFLICT(key) DO UPDATE SET value=?1", [g.limit])?;
    Ok(())
}

/* ---------- tracking state ---------- */

pub fn get_tracking(conn: &Connection) -> TrackingInfo {
    conn.query_row("SELECT state, privacy_return, degraded_reason FROM tracking_state WHERE id=1", [], |r| {
        Ok(TrackingInfo {
            state: TrackingStateKind::parse(&r.get::<_, String>(0)?).unwrap_or(TrackingStateKind::Confirmation),
            privacy_return: TrackingStateKind::parse(&r.get::<_, String>(1)?).unwrap_or(TrackingStateKind::Active),
            degraded_reason: r.get(2)?,
        })
    })
    .unwrap_or(TrackingInfo { state: TrackingStateKind::Confirmation, privacy_return: TrackingStateKind::Active, degraded_reason: None })
}

pub fn set_tracking(conn: &Connection, info: &TrackingInfo) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tracking_state (id, state, privacy_return, degraded_reason, updated_at) VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET state=?1, privacy_return=?2, degraded_reason=?3, updated_at=?4",
        rusqlite::params![info.state.as_str(), info.privacy_return.as_str(), info.degraded_reason, now_ms()],
    )?;
    Ok(())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Local calendar date (YYYY-MM-DD) for a UTC-ms timestamp.
pub fn local_date_of(ts_ms: i64) -> String {
    match Local.timestamp_millis_opt(ts_ms).single() {
        Some(t) => t.format("%Y-%m-%d").to_string(),
        None => "1970-01-01".to_string(),
    }
}

/// Minutes since local midnight (fractional) for a UTC-ms timestamp.
pub fn local_minutes_of(ts_ms: i64) -> f64 {
    match Local.timestamp_millis_opt(ts_ms).single() {
        Some(t) => {
            let secs = t.hour() * 3600 + t.minute() * 60 + t.second();
            let micro = t.nanosecond() / 1_000_000;
            (secs * 1000 + micro as u32) as f64 / 60000.0
        }
        None => 0.0,
    }
}

/// UTC-ms for the start (00:00) of the given local date.
pub fn local_midnight(date: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let dt = d.and_hms_opt(0, 0, 0)?;
    Some(Local.from_local_datetime(&dt).single()?.timestamp_millis())
}
