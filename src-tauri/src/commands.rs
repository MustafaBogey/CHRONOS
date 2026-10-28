use crate::accounting::{client_project_breakdown, sql_totals};
use crate::classifier;
use crate::db::{self, now_ms};
use crate::models::*;
use crate::tracker::{self, TrackerHandle};
use chrono::{Datelike, Local, TimeZone};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

pub struct AppState {
    pub tracker: Arc<TrackerHandle>,
    pub conn: Arc<parking_lot::Mutex<Connection>>,
    pub app_data: PathBuf,
    pub last_break_ms: std::sync::atomic::AtomicI64,
}

/* ---------------- view-model queries ---------------- */

fn row_to_view(r: &rusqlite::Row) -> rusqlite::Result<ActivityRecord> {
    let started: i64 = r.get(1)?;
    let ended: i64 = r.get(2)?;
    let start_min = db::local_minutes_of(started);
    Ok(ActivityRecord {
        id: r.get(0)?,
        date: r.get(3)?,
        start: start_min,
        end: start_min + (ended - started) as f64 / 60000.0,
        app: r.get(4)?,
        detail: r.get(5)?,
        cat: Category::parse(&r.get::<_, String>(6)?).unwrap_or(Category::Unclassified),
        project: r.get(7)?,
        focus: r.get(8)?,
        kind: r.get(9)?,
        source: r.get(10)?,
        corrected: r.get::<_, i64>(11)? == 1,
        confidence: r.get::<_, Option<f64>>(12)?,
        project_source: r.get::<_, Option<String>>(13)?.unwrap_or_default(),
        manual: r.get::<_, Option<i64>>(14).map(|v| v == Some(1)).unwrap_or(false),
    })
}

const RECORD_COLS: &str = "id, started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, confidence, project_source, manual";

pub fn load_projects(conn: &Connection) -> Vec<Project> {
    let mut stmt = match conn.prepare("SELECT id, name, desc, color, archived, client FROM projects ORDER BY created_at") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        color: r.get(3)?,
        archived: r.get::<_, i64>(4)? == 1,
        client: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
    }))
    .map(|it| it.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

fn ensure_default_project(conn: &Connection) {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap_or(0);
    if n == 0 {
        let _ = conn.execute(
            "INSERT INTO projects (id, name, desc, color, archived, created_at) VALUES ('p1', 'Personal', 'Your default project.', 'var(--violet)', 0, ?1)",
            [now_ms()],
        );
    }
}

fn profile() -> Profile {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "CHRONOS".into());
    let fallback = if user.is_empty() { "CHRONOS".to_string() } else { user };
    let display = fallback;
    let initials: String = display
        .split_whitespace()
        .filter(|w| !w.is_empty() && w.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false))
        .take(2)
        .map(|w| w.chars().next().unwrap_or('?').to_uppercase().to_string())
        .collect::<Vec<_>>()
        .join("");
    Profile { name: display, initials: if initials.is_empty() { "··".into() } else { initials }, workspace: "Personal workspace".into() }
}

/// Profile with the user's preferred display name applied (empty → system fallback).
fn profile_with_name(display_name: &str) -> Profile {
    let mut p = profile();
    let trimmed = display_name.trim();
    if !trimmed.is_empty() {
        p.name = trimmed.to_string();
        let initials: String = trimmed
            .split_whitespace()
            .filter(|w| !w.is_empty() && w.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false))
            .take(2)
            .map(|w| w.chars().next().unwrap_or('?').to_uppercase().to_string())
            .collect::<Vec<_>>()
            .join("");
        if !initials.is_empty() {
            p.initials = initials;
        }
    }
    p
}

#[tauri::command]
pub fn get_state(state: State<AppState>) -> AppStateOut {
    // Lock order discipline: tracker.inner is NEVER taken while holding conn
    // (tracker threads take inner → conn; commands must take conn only, or inner only).
    let (tracking, session, undo_depth) = {
        let t = state.tracker.tracking_info();
        let s = state.tracker.snapshot_session();
        let u = state.tracker.undo_depth();
        (t, s, u)
    };
    let conn = state.conn.lock();
    ensure_default_project(&conn);
    let settings = db::get_settings(&conn);
    let display_name: String = conn
        .query_row("SELECT value FROM settings WHERE key='display_name'", [], |r| r.get::<_, String>(0))
        .unwrap_or_default();
    let onboarded = conn
        .query_row("SELECT value FROM settings WHERE key='onboarded'", [], |r| r.get::<_, String>(0))
        .map(|v| v == "1")
        .unwrap_or(false);
    let dnd = conn
        .query_row("SELECT value FROM settings WHERE key='dnd'", [], |r| r.get::<_, String>(0))
        .map(|v| v == "1")
        .unwrap_or(false);
    AppStateOut {
        tracking,
        session,
        goals: db::get_goals(&conn),
        projects: load_projects(&conn),
        rules: classifier::load_rules(&conn),
        exclusions: classifier::load_exclusions(&conn),
        settings,
        dnd,
        onboarded,
        undo_depth,
        profile: profile_with_name(&display_name),
    }
}

#[tauri::command]
pub fn get_capabilities() -> serde_json::Value {
    json!({ "platform": tracker::platform_name(), "capabilities": tracker::capabilities() })
}

/// Authoritative application version from the packaged metadata (never hardcoded).
#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Pairing information for the optional browser-extension website attribution.
/// Local-only: the token is a shared secret between this app and the extension on
/// the same machine, and the listener binds to 127.0.0.1 only. Includes the
/// extension's last blocking-rule acknowledgment, when any (the only honest
/// evidence that website enforcement is real).
#[tauri::command]
pub fn browser_integration_info(state: State<AppState>) -> serde_json::Value {
    let conn = state.conn.lock();
    let token: String = conn
        .query_row("SELECT value FROM settings WHERE key='ext_token'", [], |r| r.get(0))
        .unwrap_or_default();
    let ack_ms: Option<i64> = conn
        .query_row("SELECT value FROM meta WHERE key='blocklist_ack_ms'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok());
    let ack_applied: i64 = conn
        .query_row("SELECT value FROM meta WHERE key='blocklist_ack_applied'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let ack_enabled: bool = conn
        .query_row("SELECT value FROM meta WHERE key='blocklist_ack_enabled'", [], |r| r.get::<_, String>(0))
        .map(|v| v == "1")
        .unwrap_or(false);
    drop(conn);
    let last_hint = state.tracker.last_domain_hint_ms();
    json!({
        "port": crate::integrations::INTEGRATION_PORT,
        "token": token,
        "lastHintMs": last_hint,
        "nowMs": crate::db::now_ms(),
        "blocklistAck": { "ms": ack_ms, "applied": ack_applied, "enabled": ack_enabled },
    })
}

#[tauri::command]
pub fn get_overview(state: State<AppState>, date: String) -> serde_json::Value {
    // Session snapshot first (inner lock), then the connection (never both nested conn→inner).
    let snapshot = state.tracker.snapshot_session();
    let conn = state.conn.lock();
    let totals = sql_totals(&conn, "local_date = ?1", &[&date]);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {RECORD_COLS} FROM activity_intervals WHERE local_date = ?1 AND category NOT IN ('gap','idle') AND open = 0 AND ended_at > started_at ORDER BY started_at DESC LIMIT 4"
        ))
        .unwrap();
    let recent: Vec<ActivityRecord> = stmt.query_map([&date], |r| row_to_view(r).map_err(|e| e))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default();

    // Week: Monday-start containing `date`.
    let date_parsed = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").unwrap_or_else(|_| Local::now().date_naive());
    let dow = date_parsed.weekday().num_days_from_monday();
    let monday = date_parsed - chrono::Duration::days(dow as i64);
    let mut week = Vec::new();
    for i in 0..7 {
        let d = (monday + chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
        week.push(serde_json::json!({ "date": d, "totals": sql_totals(&conn, "local_date = ?1", &[&d]) }));
    }
    let review_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_intervals WHERE local_date = ?1 AND category = 'unclassified' AND open = 0", [&date], |r| r.get(0))
        .unwrap_or(0);

    // Session intervals: rows overlapping the live/last session window.
    let (session_intervals, project_minutes_today) = session_and_project_data(&conn, &snapshot, &date);

    json!({ "date": date, "totals": totals, "recent": recent, "week": week, "reviewCount": review_count,
            "sessionIntervals": session_intervals, "projectMinutesToday": project_minutes_today })
}

fn session_and_project_data(conn: &Connection, _snapshot: &SessionSnapshot, date: &str) -> (Vec<ActivityRecord>, f64) {
    let started = conn
        .query_row("SELECT MAX(started_at) FROM focus_sessions", [], |r| r.get::<_, Option<i64>>(0))
        .ok()
        .flatten();
    let session_intervals = if let Some(s) = started {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {RECORD_COLS} FROM activity_intervals WHERE started_at >= ?1 AND open = 0 AND category NOT IN ('gap') ORDER BY started_at"
            ))
            .unwrap();
        stmt.query_map([s], |r| row_to_view(r))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };
    // Project-time goal target: the user's selected project when set and still
    // existing, otherwise the first active project (original behavior).
    let settings = db::get_settings(conn);
    let goal_project: Option<String> = conn
        .query_row("SELECT id FROM projects WHERE id = ?1", [&settings.goal_project], |r| r.get::<_, String>(0))
        .ok()
        .or_else(|| {
            conn.query_row("SELECT id FROM projects WHERE archived = 0 ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0))
                .ok()
        });
    let project_minutes = match goal_project {
        Some(pid) => conn
            .query_row(
                "SELECT COALESCE(SUM(ended_at - started_at), 0) / 60000.0 FROM activity_intervals WHERE local_date = ?1 AND project = ?2 AND category != 'gap' AND open = 0",
                rusqlite::params![date, pid],
                |r| r.get::<_, f64>(0),
            )
            .unwrap_or(0.0),
        None => 0.0,
    };
    (session_intervals, project_minutes)
}

#[tauri::command]
pub fn get_timeline(state: State<AppState>, date: String) -> Vec<ActivityRecord> {
    let conn = state.conn.lock();
    let mut stmt = match conn.prepare(&format!(
        "SELECT {RECORD_COLS} FROM activity_intervals WHERE local_date = ?1 AND open = 0 AND ended_at > started_at ORDER BY started_at, id"
    )) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([&date], |r| row_to_view(r))
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
}

#[derive(Deserialize)]
pub struct ReportRequest {
    pub period: String,
    pub start: String,
    pub end: String,
    pub project: String,
}

#[tauri::command]
pub fn get_report(state: State<AppState>, req: ReportRequest) -> serde_json::Value {
    let conn = state.conn.lock();
    let project_clause = if req.project == "all" { String::new() } else { " AND project = ?3".to_string() };

    let totals = if req.project == "all" {
        sql_totals(&conn, "local_date >= ?1 AND local_date <= ?2", &[&req.start, &req.end])
    } else {
        sql_totals(&conn, "local_date >= ?1 AND local_date <= ?2 AND project = ?3", &[&req.start, &req.end, &req.project])
    };

    // Previous equal-length period.
    let s = chrono::NaiveDate::parse_from_str(&req.start, "%Y-%m-%d");
    let e = chrono::NaiveDate::parse_from_str(&req.end, "%Y-%m-%d");
    let (prior_start, prior_end) = match (s, e) {
        (Ok(s), Ok(e)) => {
            let len = (e - s).num_days() + 1;
            (s - chrono::Duration::days(len), s - chrono::Duration::days(1))
        }
        _ => (chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(), chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
    };
    let ps = prior_start.format("%Y-%m-%d").to_string();
    let pe = prior_end.format("%Y-%m-%d").to_string();
    let prior_count: i64 = if req.project == "all" {
        conn.query_row("SELECT COUNT(*) FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?2", [&ps, &pe], |r| r.get(0)).unwrap_or(0)
    } else {
        conn.query_row("SELECT COUNT(*) FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?2 AND project = ?3", rusqlite::params![ps, pe, req.project], |r| r.get(0)).unwrap_or(0)
    };
    let prior = if prior_count > 0 {
        let t = if req.project == "all" {
            sql_totals(&conn, "local_date >= ?1 AND local_date <= ?2", &[&ps, &pe])
        } else {
            sql_totals(&conn, "local_date >= ?1 AND local_date <= ?2 AND project = ?3", &[&ps, &pe, &req.project])
        };
        json!(t)
    } else {
        serde_json::Value::Null
    };

    let mut breakdowns = serde_json::Map::new();
    for (key, sql_filter, key_expr) in [
        ("applications", "category != 'gap'", "app"),
        // Website reporting shows real destinations only. The extension sends
        // the registrable domain (or a curated service label like "ChatGPT"),
        // so real entries always contain a letter and are either dotted
        // domains or known catalog labels. Excluded conservatively: known
        // browser-internal pages (newtab, extensions), dotless 32-character
        // extension-origin identifiers, and letterless fragments (IP bits
        // like "0.1"). They remain visible in Applications under the browser.
        ("websites", "kind = 'website' AND category != 'gap' AND detail GLOB '*[a-zA-Z]*' AND NOT (LENGTH(detail) = 32 AND detail NOT LIKE '%.%') AND detail NOT IN ('newtab', 'extensions')", "detail"),
        ("categories", "category != 'gap'", "category"),
    ] {
        let sql = format!(
            "SELECT {key_expr} AS label, SUM((ended_at - started_at)) / 60000.0 AS minutes
             FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?2 AND open = 0 AND {sql_filter}{project_clause}
             GROUP BY label ORDER BY minutes DESC LIMIT 50"
        );
        let rows: Vec<serde_json::Value> = if req.project == "all" {
            query_breakdown(&conn, &sql, rusqlite::params![req.start, req.end])
        } else {
            query_breakdown(&conn, &sql, rusqlite::params![req.start, req.end, req.project])
        };
        breakdowns.insert(key.to_string(), json!(rows));
    }
    // Projects breakdown carries client grouping for the client/project report.
    {
        let filter = if req.project == "all" { None } else { Some(req.project.as_str()) };
        let rows: Vec<serde_json::Value> = client_project_breakdown(&conn, &req.start, &req.end, filter)
            .into_iter()
            .map(|r| json!({ "label": r.project, "minutes": r.minutes, "client": r.client, "projectId": r.project_id }))
            .collect();
        breakdowns.insert("projects".to_string(), json!(rows));
    }

    // Daily rows.
    let daily_sql = format!(
        "SELECT local_date, COALESCE(SUM(CASE WHEN category != 'gap' THEN (ended_at - started_at) ELSE 0 END),0)/60000.0
         FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?2 AND open = 0 GROUP BY local_date ORDER BY local_date"
    );
    let dates: Vec<String> = {
        let mut stmt = conn.prepare(&daily_sql).unwrap();
        stmt.query_map(rusqlite::params![req.start, req.end], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
    };
    let daily: Vec<serde_json::Value> = dates
        .iter()
        .map(|d| {
            let t = if req.project == "all" {
                sql_totals(&conn, "local_date = ?1", &[d])
            } else {
                sql_totals(&conn, "local_date = ?1 AND project = ?2", &[d, &req.project])
            };
            json!({ "date": d, "totals": t })
        })
        .collect();

    json!({ "range": { "start": req.start, "end": req.end }, "totals": totals, "prior": prior, "breakdowns": breakdowns, "daily": daily,
            "goalProject": goal_project_json(&conn, &req.start, &req.end) })
}

/// Goal-project progress for the report period: the same selection logic as the
/// overview (user-selected project, validated, first-project fallback).
fn goal_project_json(conn: &Connection, start: &str, end: &str) -> serde_json::Value {
    let settings = db::get_settings(conn);
    let pid: Option<String> = conn
        .query_row("SELECT id FROM projects WHERE id = ?1", [&settings.goal_project], |r| r.get::<_, String>(0))
        .ok()
        .or_else(|| {
            conn.query_row("SELECT id FROM projects WHERE archived = 0 ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0))
                .ok()
        });
    match pid {
        Some(pid) => {
            let (name, minutes): (String, f64) = conn
                .query_row(
                    "SELECT (SELECT name FROM projects WHERE id = ?2), COALESCE(SUM(ended_at - started_at), 0) / 60000.0
                     FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?3 AND project = ?2 AND category != 'gap' AND open = 0",
                    rusqlite::params![start, pid, end],
                    |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?)),
                )
                .unwrap_or(("".into(), 0.0));
            json!({ "id": pid, "name": name, "minutes": minutes, "targetMinutes": db::get_goals(conn).project })
        }
        None => serde_json::Value::Null,
    }
}

fn query_breakdown(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Vec<serde_json::Value> {
    let Ok(mut stmt) = conn.prepare(sql) else { return vec![] };
    stmt.query_map(params, |r| {
        let label: String = r.get(0)?;
        let minutes: f64 = r.get(1)?;
        Ok(json!({ "label": label, "minutes": minutes }))
    })
    .map(|it| it.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

#[tauri::command]
pub fn get_focus_history(state: State<AppState>, date: String) -> Vec<SessionHistoryEntry> {
    let conn = state.conn.lock();
    let start_bound = db::local_midnight(&date).unwrap_or(0);
    let end_bound = db::local_midnight(&chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").map(|d| (d + chrono::Duration::days(1)).format("%Y-%m-%d").to_string()).unwrap_or(date)).unwrap_or(i64::MAX);
    let mut stmt = match conn.prepare(
        "SELECT id, started_at, ended_at, intention, project, elapsed_ms, qualified_minutes,
                focus_minutes, break_minutes, breaks_enabled, focus_intervals,
                break_total_ms, break_planned_ms, break_extensions_ms, break_overrun_ms, break_history
         FROM focus_sessions
         WHERE started_at >= ?1 AND started_at < ?2 AND open = 0 ORDER BY started_at DESC LIMIT 50",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(rusqlite::params![start_bound, end_bound], |r| {
        let started: i64 = r.get(1)?;
        let ended: Option<i64> = r.get(2)?;
        let break_history_json: String = r.get(15)?;
        let break_history: Vec<BreakRecord> = serde_json::from_str(&break_history_json).unwrap_or_default();
        Ok(SessionHistoryEntry {
            id: r.get(0)?,
            started_at: fmt_dt(started),
            ended_at: fmt_dt(ended.unwrap_or(started)),
            intention: r.get(3)?,
            project: r.get(4)?,
            elapsed: r.get::<_, i64>(5)? as f64 / 60000.0,
            qualified: r.get(6)?,
            focus_minutes: r.get(7)?,
            break_minutes: r.get(8)?,
            breaks_enabled: r.get::<_, i64>(9)? == 1,
            focus_intervals: r.get(10)?,
            break_planned_min: r.get::<_, i64>(11)? as f64 / 60000.0,
            break_extensions_min: r.get::<_, i64>(12)? as f64 / 60000.0,
            break_actual_min: r.get::<_, i64>(13)? as f64 / 60000.0,
            break_overrun_min: r.get::<_, i64>(14)? as f64 / 60000.0,
            break_history,
        })
    })
    .map(|it| it.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

fn fmt_dt(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%Y-%m-%dT%H:%M").to_string()).unwrap_or_default()
}

#[tauri::command]
pub fn get_projects_with_time(state: State<AppState>) -> serde_json::Value {
    let conn = state.conn.lock();
    let projects = load_projects(&conn);
    let out: Vec<serde_json::Value> = projects
        .iter()
        .map(|p| {
            let (minutes, count): (f64, i64) = conn
                .query_row(
                    "SELECT COALESCE(SUM(ended_at - started_at),0)/60000.0, COUNT(*) FROM activity_intervals WHERE project = ?1 AND category != 'gap' AND open = 0",
                    [&p.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((0.0, 0));
            json!({ "id": p.id, "name": p.name, "desc": p.description, "color": p.color, "archived": p.archived, "client": p.client, "minutes": minutes, "intervalCount": count })
        })
        .collect();
    json!(out)
}

/* ---------------- undo (bounded, 12 snapshots) ---------------- */

fn push_snapshot(conn: &Connection, filter_sql: &str, params: &[&dyn rusqlite::ToSql], rules_added: Vec<Rule>, rules_removed: Vec<Rule>) -> usize {
    push_snapshot_with_added(conn, filter_sql, params, rules_added, rules_removed, vec![])
}

fn push_snapshot_with_added(conn: &Connection, filter_sql: &str, params: &[&dyn rusqlite::ToSql], rules_added: Vec<Rule>, rules_removed: Vec<Rule>, rows_added: Vec<i64>) -> usize {
    let sql = format!(
        "SELECT id, started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual
         FROM activity_intervals WHERE {filter_sql}"
    );
    let records: Vec<serde_json::Value> = conn
        .prepare(&sql)
        .map(|mut stmt| {
            stmt.query_map(params, |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?, "started_at": r.get::<_, i64>(1)?, "ended_at": r.get::<_, i64>(2)?,
                        "local_date": r.get::<_, String>(3)?, "app": r.get::<_, String>(4)?, "detail": r.get::<_, String>(5)?,
                        "category": r.get::<_, String>(6)?, "project": r.get::<_, String>(7)?, "focus_minutes": r.get::<_, f64>(8)?,
                        "kind": r.get::<_, String>(9)?, "source": r.get::<_, String>(10)?, "corrected": r.get::<_, i64>(11)? == 1,
                        "review_status": r.get::<_, String>(12)?, "open": r.get::<_, i64>(13)? == 1, "manual": r.get::<_, i64>(14)? == 1,
                    }))
                })
                .map(|it| it.filter_map(|x| x.ok()).collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let payload = json!({ "records": records, "rules_added": rules_added, "rules_removed": rules_removed, "rows_added": rows_added });
    let _ = conn.execute("INSERT INTO undo_snapshots (created_at, payload) VALUES (?1, ?2)", rusqlite::params![now_ms(), payload.to_string()]);
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM undo_snapshots", [], |r| r.get(0)).unwrap_or(0);
    if count > 12 {
        let _ = conn.execute("DELETE FROM undo_snapshots WHERE id = (SELECT MIN(id) FROM undo_snapshots)", []);
    }
    count.clamp(0, 12) as usize
}

#[tauri::command]
pub fn undo_last(state: State<AppState>) -> Result<bool, String> {
    let conn = state.conn.lock();
    let payload: Option<String> = conn
        .query_row("SELECT payload FROM undo_snapshots ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
        .ok();
    let Some(payload) = payload else { return Ok(false) };
    let _ = conn.execute("DELETE FROM undo_snapshots WHERE id = (SELECT MAX(id) FROM undo_snapshots)", []);
    let v: serde_json::Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
    // Rows created by the undone action (e.g. manual entries) are removed first.
    if let Some(added) = v["rows_added"].as_array() {
        for id in added.iter().filter_map(|x| x.as_i64()) {
            let _ = conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
        }
    }
    if let Some(records) = v["records"].as_array() {
        for rec in records {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO activity_intervals (id, started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, confidence, open, manual)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,NULL,?14,?15)",
                rusqlite::params![
                    rec["id"].as_i64().unwrap_or(0), rec["started_at"].as_i64().unwrap_or(0), rec["ended_at"].as_i64().unwrap_or(0),
                    rec["local_date"].as_str().unwrap_or(""), rec["app"].as_str().unwrap_or(""), rec["detail"].as_str().unwrap_or(""),
                    rec["category"].as_str().unwrap_or("unclassified"), rec["project"].as_str().unwrap_or(""), rec["focus_minutes"].as_f64().unwrap_or(0.0),
                    rec["kind"].as_str().unwrap_or("app"), rec["source"].as_str().unwrap_or(""), if rec["corrected"].as_bool().unwrap_or(false) { 1 } else { 0 },
                    rec["review_status"].as_str().unwrap_or("none"), if rec["open"].as_bool().unwrap_or(false) { 1 } else { 0 },
                    if rec["manual"].as_bool().unwrap_or(false) { 1 } else { 0 }
                ],
            );
        }
    }
    // Rules: added by the undone action → remove; removed → restore.
    if let Some(added) = v["rules_added"].as_array() {
        for rule in added {
            let _ = conn.execute(
                "DELETE FROM rules WHERE rowid = (SELECT rowid FROM rules WHERE app = ?1 AND category = ?2 LIMIT 1)",
                rusqlite::params![rule["app"].as_str().unwrap_or(""), rule["cat"].as_str().unwrap_or("")],
            );
        }
    }
    if let Some(removed) = v["rules_removed"].as_array() {
        for rule in removed {
            let _ = conn.execute(
                "INSERT INTO rules (app, category, project, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![rule["app"].as_str().unwrap_or(""), rule["cat"].as_str().unwrap_or(""), rule["project"].as_str().unwrap_or(""), now_ms()],
            );
        }
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM undo_snapshots", [], |r| r.get(0)).unwrap_or(0);
    drop(conn);
    state.tracker.set_undo_depth(count as usize);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(true)
}

/* ---------------- corrections, splitting ---------------- */

#[tauri::command]
pub fn correct_record(state: State<AppState>, id: i64, cat: String, project: String, create_rule: bool) -> Result<(), String> {
    let depth = {
        let conn = state.conn.lock();
        let existing: Option<String> = conn.query_row("SELECT category FROM activity_intervals WHERE id = ?1", [id], |r| r.get(0)).ok();
        let Some(_existing) = existing else { return Err("Interval not found.".into()) };
        let depth = push_snapshot(&conn, "id = ?1", &[&id], vec![], vec![]);
        let category = Category::parse(&cat).ok_or("Invalid category")?;
        if category == Category::Gap || category == Category::Idle {
            return Err("Idle and gap intervals cannot be reclassified.".into());
        }
        // Explicit user assignments carry their own attribution reason; a project
        // assignment also feeds the learned-attribution evidence.
        conn.execute(
            "UPDATE activity_intervals SET category = ?2, project = ?3, corrected = 1, source = 'Explicit user correction', review_status = 'none',
             project_source = CASE WHEN ?3 != '' THEN 'user' ELSE '' END,
             focus_minutes = CASE WHEN ?2 = 'productive' THEN focus_minutes ELSE 0 END
             WHERE id = ?1",
            rusqlite::params![id, category.as_str(), project],
        )
        .map_err(|e| e.to_string())?;
        if create_rule {
            let app: String = conn.query_row("SELECT app FROM activity_intervals WHERE id = ?1", [id], |r| r.get(0)).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO rules (app, category, project, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![app, category.as_str(), if project.is_empty() { String::new() } else { project }, now_ms()],
            )
            .map_err(|e| e.to_string())?;
        }
        depth
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

#[tauri::command]
pub fn split_record(state: State<AppState>, id: i64, minutes_in_first: i64) -> Result<(), String> {
    let depth = {
        let conn = state.conn.lock();
        let row = conn.query_row(
            "SELECT started_at, ended_at, focus_minutes FROM activity_intervals WHERE id = ?1 AND open = 0",
            [id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?)),
        );
        let Some((started, ended, focus)) = row.ok() else { return Err("Interval not found or still open.".into()) };
        let duration_min = (ended - started) as f64 / 60000.0;
        if minutes_in_first < 1 || (minutes_in_first as f64) >= duration_min {
            return Err("Choose a valid split point within the interval.".into());
        }
        let depth = push_snapshot(&conn, "id = ?1", &[&id], vec![], vec![]);
        let cut = started + minutes_in_first * 60_000;
        let focus_first = focus * minutes_in_first as f64 / duration_min;
        let focus_second = focus - focus_first;
        conn.execute("UPDATE activity_intervals SET ended_at = ?2, focus_minutes = ?3 WHERE id = ?1", rusqlite::params![id, cut, focus_first]).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open)
             SELECT started_at, ?2, local_date, app, detail, category, project, ?3, kind, source, corrected, review_status, 0 FROM activity_intervals WHERE id = ?1",
            rusqlite::params![id, cut, focus_second],
        )
        .map_err(|e| e.to_string())?;
        depth
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

#[tauri::command]
pub fn add_rule(state: State<AppState>, app: String, cat: String, project: String) -> Result<(), String> {
    if app.trim().is_empty() {
        return Err("Enter an application or website label.".into());
    }
    let conn = state.conn.lock();
    let category = Category::parse(&cat).ok_or("Invalid category")?;
    conn.execute(
        "INSERT INTO rules (app, category, project, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![app.trim(), category.as_str(), if project.is_empty() { String::new() } else { project }, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn apply_rule_retroactive(state: State<AppState>, index: i64) -> Result<i64, String> {
    let (changed, depth) = {
        let conn = state.conn.lock();
        let rule: (String, String, String) = conn
            .query_row("SELECT app, category, project FROM rules ORDER BY id LIMIT 1 OFFSET ?1", [index], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default())))
            .map_err(|_| "Rule not found.".to_string())?;
        let app_lower = rule.0.to_lowercase();
        let depth = push_snapshot(
            &conn,
            "LOWER(app) = ?1 AND corrected = 0 AND category NOT IN ('idle','gap')",
            &[&app_lower],
            vec![],
            vec![],
        );
        // The rule carries category and (optionally) a project assignment. Corrections
        // made directly by the user are always preserved (corrected = 0 filter).
        let changed = conn
            .execute(
                "UPDATE activity_intervals SET category = ?2, source = 'User-defined rule · explicitly reapplied',
                 focus_minutes = CASE WHEN ?2 = 'productive' THEN focus_minutes ELSE 0 END,
                 project = CASE WHEN ?3 != '' THEN ?3 ELSE project END,
                 project_source = CASE WHEN ?3 != '' THEN 'rule' ELSE project_source END
                 WHERE LOWER(app) = ?1 AND corrected = 0 AND category NOT IN ('idle','gap')",
                rusqlite::params![app_lower, rule.1, rule.2],
            )
            .map_err(|e| e.to_string())?;
        (changed, depth)
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(changed as i64)
}

#[tauri::command]
pub fn add_exclusion(state: State<AppState>, label: String) -> Result<(), String> {
    let normalized = classifier::normalize_exclusion_label(&label);
    if normalized.is_empty() {
        return Err("Enter an application or domain.".into());
    }
    let conn = state.conn.lock();
    let dup: i64 = conn.query_row("SELECT COUNT(*) FROM exclusions WHERE LOWER(label) = LOWER(?1)", [&normalized], |r| r.get(0)).unwrap_or(0);
    if dup > 0 {
        return Err("That exclusion already exists.".into());
    }
    conn.execute("INSERT INTO exclusions (label, created_at) VALUES (?1, ?2)", rusqlite::params![normalized, now_ms()]).map_err(|e| e.to_string())?;
    drop(conn);
    // Exclusions take effect on observations after this point; existing records
    // are never rewritten.
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

/// Remove an exclusion by its stored label. Future matching activity is tracked
/// again; historical records are untouched.
#[tauri::command]
pub fn remove_exclusion(state: State<AppState>, label: String) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("DELETE FROM exclusions WHERE label = ?1", [label]).map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

#[tauri::command]
pub fn delete_history(state: State<AppState>, start: String, end: String) -> Result<i64, String> {
    if start > end {
        return Err("Choose a valid, ordered date range.".into());
    }
    let (deleted, depth) = {
        let conn = state.conn.lock();
        let depth = push_snapshot(&conn, "local_date >= ?1 AND local_date <= ?2", &[&start, &end], vec![], vec![]);
        let deleted = conn
            .execute("DELETE FROM activity_intervals WHERE local_date >= ?1 AND local_date <= ?2", rusqlite::params![start, end])
            .map_err(|e| e.to_string())? as i64;
        conn.execute("INSERT INTO deletion_log (start_date, end_date, deleted_at) VALUES (?1, ?2, ?3)", rusqlite::params![start, end, now_ms()]).map_err(|e| e.to_string())?;
        (deleted, depth)
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(deleted)
}

/* ---------------- sessions ---------------- */

#[tauri::command]
pub fn session_start(state: State<AppState>, intention: String, project: String, focus_minutes: i64, break_minutes: i64, breaks_enabled: bool) -> Result<(), String> {
    let snapshot = tracker::session_start(&state.tracker, intention, project, focus_minutes, break_minutes, breaks_enabled);
    tracker::emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
    Ok(())
}

/// User chose "Resume Focus" after a break-expiry prompt.
#[tauri::command]
pub fn session_resume_focus(state: State<AppState>) -> Result<(), String> {
    let snapshot = tracker::session_resume_focus(&state.tracker);
    tracker::emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
    Ok(())
}

/// User chose "Another 5 Minutes" during a break.
#[tauri::command]
pub fn session_extend_break(state: State<AppState>) -> Result<(), String> {
    let snapshot = tracker::session_extend_break(&state.tracker);
    tracker::emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
    Ok(())
}

#[tauri::command]
pub fn session_toggle(state: State<AppState>) -> Result<String, String> {
    let snapshot = tracker::session_toggle(&state.tracker)?;
    tracker::emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
    Ok(if snapshot.status == "paused" { "paused".into() } else { "resumed".into() })
}

#[tauri::command]
pub fn session_end(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let snapshot = tracker::session_end(&state.tracker);
    tracker::emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
    if snapshot.elapsed > 0 {
        let elapsed_min = snapshot.elapsed / 60;
        let qualified_min = snapshot.qualified.round() as i64;
        let body = format!(
            "Elapsed {elapsed_min} min · qualified focus {qualified_min} min · idle {} min · {} interruption{}.",
            snapshot.idle.round() as i64,
            snapshot.interruptions,
            if snapshot.interruptions == 1 { "" } else { "s" },
        );
        crate::show_notice(&app, "complete", "Focus session complete.", &body, false);
    }
    Ok(())
}

/* ---------------- goals & projects ---------------- */

#[tauri::command]
pub fn set_goals(state: State<AppState>, goals: Goals) -> Result<(), String> {
    let conn = state.conn.lock();
    db::set_goals(&conn, &goals).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn project_create(state: State<AppState>, name: String, desc: String, client: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give your project a name.".into());
    }
    let conn = state.conn.lock();
    let dup: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE LOWER(name) = LOWER(?1)", [name], |r| r.get(0)).unwrap_or(0);
    if dup > 0 {
        return Err("A project with this name already exists.".into());
    }
    let id = format!("p{}", now_ms());
    conn.execute(
        "INSERT INTO projects (id, name, desc, color, archived, created_at, client) VALUES (?1, ?2, ?3, 'var(--green)', 0, ?4, ?5)",
        rusqlite::params![id, name, if desc.trim().is_empty() { "A new intention, a little time.".to_string() } else { desc.trim().to_string() }, now_ms(), client.trim()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn project_rename(state: State<AppState>, id: String, name: String, desc: String, client: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give your project a name.".into());
    }
    let conn = state.conn.lock();
    let dup: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE id != ?1 AND LOWER(name) = LOWER(?2)", rusqlite::params![id, name], |r| r.get(0)).unwrap_or(0);
    if dup > 0 {
        return Err("A project with this name already exists.".into());
    }
    let desc_val = if desc.trim().is_empty() {
        conn.query_row("SELECT desc FROM projects WHERE id = ?1", [&id], |r| r.get::<_, String>(0)).unwrap_or_default()
    } else {
        desc.trim().to_string()
    };
    conn.execute(
        "UPDATE projects SET name = ?2, desc = ?3, client = ?4 WHERE id = ?1",
        rusqlite::params![id, name, desc_val, client.trim()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn project_set_archived(state: State<AppState>, id: String, archived: bool) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("UPDATE projects SET archived = ?2 WHERE id = ?1", rusqlite::params![id, if archived { 1 } else { 0 }]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Permanent project deletion — the irreversible counterpart to Archive.
/// Tracked intervals are preserved with their project references cleared (never
/// reassigned); rules carrying this project lose only the assignment; a goal
/// targeting this project falls back to the default selection.
#[tauri::command]
pub fn project_delete(state: State<AppState>, id: String) -> Result<i64, String> {
    let affected = {
        let conn = state.conn.lock();
        project_delete_core(&conn, &id)?
    };
    tracker::emit_event("state-changed", "{}");
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(affected)
}

pub fn project_delete_core(conn: &Connection, id: &str) -> Result<i64, String> {
    let exists: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE id = ?1", [id], |r| r.get(0)).unwrap_or(0);
    if exists == 0 {
        return Err("Project not found.".into());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let affected: i64 = tx
        .execute("UPDATE activity_intervals SET project = '', project_source = '' WHERE project = ?1", [id])
        .map_err(|e| e.to_string())? as i64;
    tx.execute("UPDATE rules SET project = '' WHERE project = ?1", [id]).map_err(|e| e.to_string())?;
    tx.execute("UPDATE settings SET value = '' WHERE key = 'goal_project' AND value = ?1", [id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM projects WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(affected)
}

/* ---------------- bulk activity assignment ---------------- */

/// Assign (or unassign, when `project` is empty) many intervals at once.
/// Attribution is recorded as an explicit user action; classification and the
/// corrected flag are untouched so this never feeds category learning.
#[tauri::command]
pub fn bulk_assign(state: State<AppState>, ids: Vec<i64>, project: String) -> Result<i64, String> {
    if ids.is_empty() {
        return Err("Select at least one activity.".into());
    }
    if !project.is_empty() {
        let conn = state.conn.lock();
        let exists: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE id = ?1", [&project], |r| r.get(0)).unwrap_or(0);
        drop(conn);
        if exists == 0 {
            return Err("Project not found.".into());
        }
    }
    let depth = {
        let conn = state.conn.lock();
        let depth = bulk_assign_core(&conn, &ids, &project)?;
        depth
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(ids.len() as i64)
}

pub fn bulk_assign_core(conn: &Connection, ids: &[i64], project: &str) -> Result<usize, String> {
    let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let depth = push_snapshot(conn, &format!("id IN ({list})"), &[], vec![], vec![]);
    conn.execute(
        &format!(
            "UPDATE activity_intervals SET project = ?1, project_source = CASE WHEN ?1 != '' THEN 'user' ELSE '' END
             WHERE id IN ({list}) AND category NOT IN ('idle','gap')"
        ),
        rusqlite::params![project],
    )
    .map_err(|e| e.to_string())?;
    Ok(depth)
}

/* ---------------- manual time entries ---------------- */

/// Validation result for a proposed manual entry.
struct ManualCheck {
    conflict: Option<String>,
    gap_ids: Vec<i64>,
    idle_adjust: Vec<(i64, i64, i64)>, // (row id, new start, new end) — trimmed/split idle rows
    idle_delete: Vec<i64>,
}

/// A manual entry may fill tracking gaps and replace idle time, but never
/// overlaps observed activity or another manual entry — that would silently
/// double-count time. Conflicts are reported for explicit resolution.
fn validate_manual_span(conn: &Connection, start_ms: i64, end_ms: i64, exclude_id: Option<i64>) -> Result<ManualCheck, String> {
    let mut check = ManualCheck { conflict: None, gap_ids: vec![], idle_adjust: vec![], idle_delete: vec![] };
    let mut stmt = conn
        .prepare(
            "SELECT id, started_at, ended_at, app, category, manual FROM activity_intervals
             WHERE open = 0 AND ended_at > ?1 AND started_at < ?2 AND id != ?3 ORDER BY started_at",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, i64, i64, String, String, i64)> = stmt
        .query_map(rusqlite::params![start_ms, end_ms, exclude_id.unwrap_or(-1)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    drop(stmt);
    for (id, s, e, app, cat, manual) in rows {
        match cat.as_str() {
            "gap" => check.gap_ids.push(id),
            "idle" => {
                // Trim/split the idle row around the manual span; drop when covered.
                let _ = app;
                if s >= start_ms && e <= end_ms {
                    check.idle_delete.push(id);
                } else {
                    check.idle_adjust.push((id, s.min(start_ms), e.max(end_ms))); // handled precisely below
                }
            }
            _ => {
                let kind = if manual == 1 { "manual entry" } else { "recorded activity" };
                check.conflict = Some(format!(
                    "Overlaps {kind} “{app}” ({:.0} min). Adjust the times or edit that record first.",
                    (e - s) as f64 / 60000.0
                ));
                return Ok(check);
            }
        }
    }
    Ok(check)
}

/// Apply gap/idle dispositions for a manual span: gap rows fully covered are
/// removed (the time is now known); partially covered gap and idle rows are
/// split so the manual entry occupies exactly its own span.
fn carve_manual_span(conn: &Connection, start_ms: i64, end_ms: i64, check: &ManualCheck) {
    for id in &check.gap_ids {
        let row = conn.query_row(
            "SELECT started_at, ended_at, detail, source FROM activity_intervals WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        );
        if let Ok((s, e, detail, source)) = row {
            if s >= start_ms && e <= end_ms {
                let _ = conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
            } else {
                // keep uncovered sides as their own gap rows
                let _ = conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
                if s < start_ms {
                    let _ = conn.execute(
                        "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
                         VALUES (?1, ?2, ?3, 'Tracking gap', ?4, 'gap', '', 0, 'gap', ?5, 0, 'none', 0, 0)",
                        rusqlite::params![s, start_ms, db::local_date_of(s), detail, source],
                    );
                }
                if e > end_ms {
                    let _ = conn.execute(
                        "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
                         VALUES (?1, ?2, ?3, 'Tracking gap', ?4, 'gap', '', 0, 'gap', ?5, 0, 'none', 0, 0)",
                        rusqlite::params![end_ms, e, db::local_date_of(end_ms), detail, source],
                    );
                }
            }
        }
    }
    for id in &check.idle_delete {
        let _ = conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
    }
    for (id, s, e) in &check.idle_adjust {
        let row = conn.query_row(
            "SELECT started_at, ended_at FROM activity_intervals WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        );
        if let Ok((rs, re)) = row {
            let _ = conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
            for (seg_s, seg_e) in [(rs, start_ms.min(re)), (end_ms.max(rs), re)] {
                if seg_e - seg_s >= 60_000 {
                    let _ = conn.execute(
                        "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
                         VALUES (?1, ?2, ?3, 'Idle', 'Away from keyboard', 'idle', '', 0, 'idle', 'Idle detection', 0, 'none', 0, 0)",
                        rusqlite::params![seg_s, seg_e, db::local_date_of(seg_s)],
                    );
                }
            }
        }
    }
}

fn manual_entry_write(conn: &mut Connection, existing_id: Option<i64>, start_ms: i64, end_ms: i64, label: &str, category: Category, project: &str) -> Result<usize, String> {
    let check = validate_manual_span(conn, start_ms, end_ms, existing_id)?;
    if let Some(conflict) = check.conflict {
        return Err(conflict);
    }
    let (lo, hi) = match existing_id {
        Some(id) => {
            let (old_start, old_end): (i64, i64) = conn
                .query_row("SELECT started_at, ended_at FROM activity_intervals WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|_| "Entry not found.".to_string())?;
            (old_start.min(start_ms), old_end.max(end_ms))
        }
        None => (start_ms, end_ms),
    };
    // Snapshot everything the write will touch (bounded window), then mutate in
    // one transaction: a manual entry occupies exactly its own span.
    let depth = push_snapshot(conn, "ended_at > ?1 AND started_at < ?2", &[&lo, &hi], vec![], vec![]);
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    if let Some(id) = existing_id {
        tx.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    }
    carve_manual_span(&tx, start_ms, end_ms, &check);
    tx.execute(
        "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, 0, 'manual', 'Manual entry', 1, 'none', 0, 1)",
        rusqlite::params![start_ms, end_ms, db::local_date_of(start_ms), label, category.as_str(), project],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(depth)
}

#[tauri::command]
pub fn manual_entry_add(state: State<AppState>, start_ms: i64, end_ms: i64, label: String, cat: String, project: String) -> Result<(), String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("Describe what you were doing.".into());
    }
    if end_ms <= start_ms {
        return Err("End time must be after start time.".into());
    }
    if end_ms - start_ms > 24 * 60 * 60_000 {
        return Err("Manual entries are limited to 24 hours.".into());
    }
    let category = Category::parse(&cat).filter(|c| !matches!(c, Category::Gap | Category::Idle)).ok_or("Invalid category")?;
    if !project.is_empty() {
        let conn = state.conn.lock();
        let exists: i64 = conn.query_row("SELECT COUNT(*) FROM projects WHERE id = ?1", [&project], |r| r.get(0)).unwrap_or(0);
        if exists == 0 {
            return Err("Project not found.".into());
        }
    }
    let depth = {
        let mut conn = state.conn.lock();
        manual_entry_write(&mut conn, None, start_ms, end_ms, &label, category, &project)?
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

#[tauri::command]
pub fn manual_entry_update(state: State<AppState>, id: i64, start_ms: i64, end_ms: i64, label: String, cat: String, project: String) -> Result<(), String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("Describe what you were doing.".into());
    }
    if end_ms <= start_ms {
        return Err("End time must be after start time.".into());
    }
    if end_ms - start_ms > 24 * 60 * 60_000 {
        return Err("Manual entries are limited to 24 hours.".into());
    }
    let category = Category::parse(&cat).filter(|c| !matches!(c, Category::Gap | Category::Idle)).ok_or("Invalid category")?;
    let depth = {
        let mut conn = state.conn.lock();
        let manual: i64 = conn.query_row("SELECT manual FROM activity_intervals WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "Entry not found.".to_string())?;
        if manual != 1 {
            return Err("Only manual entries can be edited here.".into());
        }
        manual_entry_write(&mut conn, Some(id), start_ms, end_ms, &label, category, &project)?
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

#[tauri::command]
pub fn manual_entry_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    let depth = {
        let conn = state.conn.lock();
        let manual: i64 = conn.query_row("SELECT manual FROM activity_intervals WHERE id = ?1", [id], |r| r.get(0)).map_err(|_| "Entry not found.".to_string())?;
        if manual != 1 {
            return Err("Only manual entries can be removed here.".into());
        }
        let depth = push_snapshot(&conn, "id = ?1", &[&id], vec![], vec![]);
        conn.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
        depth
    };
    state.tracker.set_undo_depth(depth);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

/* ---------------- work blocks (grouping) ---------------- */

#[tauri::command]
pub fn get_work_blocks(state: State<AppState>, date: String) -> serde_json::Value {
    let conn = state.conn.lock();
    let day_start = db::local_midnight(&date).unwrap_or(0);
    let next_day = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map(|d| (d + chrono::Duration::days(1)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.clone());
    let day_end = db::local_midnight(&next_day).unwrap_or(i64::MAX);
    let rows: Vec<crate::grouping::BlockRow> = conn
        .prepare(
            "SELECT id, started_at, ended_at, app, category, project FROM activity_intervals
             WHERE local_date = ?1 AND open = 0 AND ended_at > started_at",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([&date], |r| {
                Ok(crate::grouping::BlockRow {
                    id: r.get(0)?,
                    start_ms: r.get(1)?,
                    end_ms: r.get(2)?,
                    app: r.get(3)?,
                    cat: r.get(4)?,
                    project: r.get(5)?,
                })
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .ok()
        })
        .unwrap_or_default();
    let breaks = crate::grouping::load_breaks(&conn, day_start, day_end);
    let blocks = crate::grouping::compute_blocks(&rows, &breaks);
    json!({ "date": date, "blocks": blocks, "breakCount": breaks.len() })
}

#[tauri::command]
pub fn group_break_add(state: State<AppState>, at_ts: i64) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("INSERT OR IGNORE INTO group_breaks (at_ts) VALUES (?1)", [at_ts]).map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

#[tauri::command]
pub fn group_break_remove(state: State<AppState>, at_ts: i64) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("DELETE FROM group_breaks WHERE at_ts = ?1", [at_ts]).map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(())
}

/* ---------------- idle exceptions ---------------- */

#[tauri::command]
pub fn get_idle_exceptions(state: State<AppState>) -> Vec<serde_json::Value> {
    let conn = state.conn.lock();
    conn.prepare("SELECT id, label FROM idle_exceptions ORDER BY id")
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "label": r.get::<_, String>(1)? })))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn idle_exception_add(state: State<AppState>, label: String) -> Result<(), String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("Enter an application name (e.g. zoom, ms-teams, vlc).".into());
    }
    let conn = state.conn.lock();
    conn.execute("INSERT INTO idle_exceptions (label, created_at) VALUES (?1, ?2)", rusqlite::params![label, now_ms()]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn idle_exception_remove(state: State<AppState>, id: i64) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("DELETE FROM idle_exceptions WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    Ok(())
}

/* ---------------- distraction block list ---------------- */

#[tauri::command]
pub fn get_block_list(state: State<AppState>) -> Vec<serde_json::Value> {
    let conn = state.conn.lock();
    conn.prepare("SELECT id, label, kind, target, enabled FROM block_list ORDER BY id")
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |r| {
                Ok(json!({ "id": r.get::<_, i64>(0)?, "label": r.get::<_, String>(1)?, "kind": r.get::<_, String>(2)?, "target": r.get::<_, String>(3)?, "enabled": r.get::<_, i64>(4)? == 1 }))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn block_list_add(state: State<AppState>, kind: String, target: String, label: String) -> Result<(), String> {
    let kind = if kind == "app" { "app" } else { "domain" };
    // Domain targets are canonicalized exactly like exclusions, so
    // "https://www.youtube.com/" and "youtube.com" are one identity.
    let target = if kind == "domain" {
        classifier::normalize_exclusion_label(&target)
    } else {
        target.trim().to_string()
    };
    if target.is_empty() {
        return Err(if kind == "app" {
            "Enter an application name (e.g. steam, discord).".into()
        } else {
            "Enter a website domain (e.g. youtube.com).".into()
        });
    }
    if kind == "domain" && !target.contains('.') {
        // A bare word like "twitch" is not a domain: it would silently match
        // nothing. Require the full domain so the rule is real.
        return Err("Enter the full domain — for example twitch.tv, not just twitch. (Application names belong under Application.)".into());
    }
    let conn = state.conn.lock();
    let dup: i64 = conn.query_row(
        "SELECT COUNT(*) FROM block_list WHERE kind = ?1 AND LOWER(target) = LOWER(?2)",
        rusqlite::params![kind, target],
        |r| r.get(0),
    )
    .unwrap_or(0);
    if dup > 0 {
        return Err("That target is already on the block list.".into());
    }
    conn.execute(
        "INSERT INTO block_list (label, kind, target, enabled, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
        rusqlite::params![if label.trim().is_empty() { target.clone() } else { label.trim().to_string() }, kind, target, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

#[tauri::command]
pub fn block_list_remove(state: State<AppState>, id: i64) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("DELETE FROM block_list WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

#[tauri::command]
pub fn block_list_set_enabled(state: State<AppState>, id: i64, enabled: bool) -> Result<(), String> {
    let conn = state.conn.lock();
    conn.execute("UPDATE block_list SET enabled = ?2 WHERE id = ?1", rusqlite::params![id, if enabled { 1 } else { 0 }]).map_err(|e| e.to_string())?;
    drop(conn);
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

/* ---------------- functional retention ---------------- */

/// Retention cutoff in local days before today, by stored preference.
/// Unknown values never delete (fail-safe).
fn retention_days(preference: &str) -> Option<i64> {
    match preference {
        "30 days" => Some(30),
        "3 months" => Some(90),
        "6 months" => Some(180),
        "1 year" => Some(365),
        _ => None,
    }
}

/// One retention sweep: deletes intervals, focus sessions, and undo snapshots
/// older than the configured cutoff. Runs only when explicitly activated.
/// Returns the number of deleted intervals.
pub fn retention_sweep(state: &AppState) -> i64 {
    let conn = state.conn.lock();
    let settings = db::get_settings(&conn);
    if !settings.retention_active {
        return 0;
    }
    let Some(days) = retention_days(&settings.retention) else {
        return 0;
    };
    let cutoff_date = (Local::now().date_naive() - chrono::Duration::days(days)).format("%Y-%m-%d").to_string();
    let cutoff_ms = db::local_midnight(&cutoff_date).unwrap_or(0);
    let deleted = conn
        .execute("DELETE FROM activity_intervals WHERE local_date < ?1", [&cutoff_date])
        .unwrap_or(0) as i64;
    let _ = conn.execute("DELETE FROM focus_sessions WHERE started_at < ?1", [cutoff_ms]);
    // Snapshots that predate the cutoff could resurrect deleted rows on undo.
    let _ = conn.execute("DELETE FROM undo_snapshots WHERE created_at < ?1", [cutoff_ms]);
    if deleted > 0 {
        let _ = conn.execute(
            "INSERT INTO deletion_log (start_date, end_date, deleted_at) VALUES ('retention', ?1, ?2)",
            rusqlite::params![cutoff_date, now_ms()],
        );
    }
    let _ = conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('retention_last_run', ?1)",
        [now_ms().to_string()],
    );
    if deleted > 0 {
        tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    }
    deleted
}

#[tauri::command]
pub fn retention_status(state: State<AppState>) -> serde_json::Value {
    let conn = state.conn.lock();
    let settings = db::get_settings(&conn);
    let last_run: Option<i64> = conn
        .query_row("SELECT value FROM meta WHERE key='retention_last_run'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok());
    let cutoff_days = retention_days(&settings.retention);
    json!({
        "active": settings.retention_active && cutoff_days.is_some(),
        "preference": settings.retention,
        "cutoffDays": cutoff_days,
        "lastRunMs": last_run,
    })
}

/// Explicit activation switch for retention enforcement. Activating requires the
/// user-facing confirmation; the first sweep runs immediately so the state is
/// honest from the moment it is on.
#[tauri::command]
pub fn retention_set_active(state: State<AppState>, active: bool) -> Result<i64, String> {
    {
        let conn = state.conn.lock();
        if active && retention_days(&db::get_settings(&conn).retention).is_none() {
            return Err("Choose a retention period before activating automatic deletion.".into());
        }
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('retention_active', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            [if active { "1" } else { "0" }],
        )
        .map_err(|e| e.to_string())?;
    }
    let deleted = if active { retention_sweep(&state) } else { 0 };
    tracker::emit_event("state-changed", "{}");
    Ok(deleted)
}

#[tauri::command]
pub fn retention_run_now(state: State<AppState>) -> Result<i64, String> {
    Ok(retention_sweep(&state))
}

/* ---------------- tracking state ---------------- */

#[tauri::command]
pub fn tracking_pause(state: State<AppState>) -> Result<(), String> {
    tracker::set_tracking_state(&state.tracker, TrackingStateKind::Paused);
    Ok(())
}

#[tauri::command]
pub fn tracking_resume(state: State<AppState>) -> Result<(), String> {
    tracker::set_tracking_state(&state.tracker, TrackingStateKind::Active);
    Ok(())
}

#[tauri::command]
pub fn tracking_enable(state: State<AppState>) -> Result<(), String> {
    tracker::set_tracking_state(&state.tracker, TrackingStateKind::Active);
    Ok(())
}

#[tauri::command]
pub fn tracking_disable(state: State<AppState>) -> Result<(), String> {
    tracker::set_tracking_state(&state.tracker, TrackingStateKind::Disabled);
    Ok(())
}

#[tauri::command]
pub fn tracking_simulate_unknown(state: State<AppState>) -> Result<(), String> {
    tracker::set_tracking_state(&state.tracker, TrackingStateKind::Confirmation);
    Ok(())
}

#[tauri::command]
pub fn set_privacy(state: State<AppState>, on: bool) -> Result<(), String> {
    if on {
        tracker::set_tracking_state(&state.tracker, TrackingStateKind::Privacy);
    } else {
        tracker::leave_privacy(&state.tracker);
    }
    Ok(())
}

#[tauri::command]
pub fn set_dnd(state: State<AppState>, on: bool) -> Result<(), String> {
    state.tracker.set_dnd(on);
    {
        let conn = state.conn.lock();
        conn.execute("INSERT INTO settings (key, value) VALUES ('dnd', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1", [if on { "1" } else { "0" }]).map_err(|e| e.to_string())?;
    }
    // Frontend state sync: without this the toggle UI never learns the stored value.
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

/* ---------------- settings & onboarding ---------------- */

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<AppState>, patch: serde_json::Value) -> Result<(), String> {
    let settings = {
        let conn = state.conn.lock();
        db::set_settings(&conn, &patch).map_err(|e| e.to_string())?;
        db::get_settings(&conn)
    };
    // Real OS autostart registration.
    if let Some(want) = patch.get("startup").and_then(|v| v.as_bool()) {
        use tauri_plugin_autostart::ManagerExt;
        let autostart = app.autolaunch();
        let result = if want { autostart.enable() } else { autostart.disable() };
        result.map_err(|e| format!("Startup registration failed: {e}"))?;
    }
    // Mini CHRONOS companion window follows its setting.
    if let Some(want) = patch.get("miniBar").and_then(|v| v.as_bool()) {
        if want {
            crate::mini::show(&app)?;
        } else {
            crate::mini::hide(&app);
        }
    }
    state.tracker.refresh_settings(settings);
    // Frontend state sync: without this the toggle UI never learns the stored value,
    // so toggles appear dead and repeat clicks send the stale value.
    tracker::emit_event("state-changed", "{}");
    Ok(())
}

/* ---------------- activity icons (offline) ---------------- */

/// data-URL icons for the given activity display names, from the one-shot local cache.
#[tauri::command]
pub fn get_app_icons(state: State<AppState>, names: Vec<String>) -> serde_json::Value {
    let conn = state.conn.lock();
    serde_json::to_value(crate::app_icons::icon_data_urls(&conn, &names)).unwrap_or_default()
}

/* ---------------- mini CHRONOS ---------------- */

#[tauri::command]
pub fn mini_show(app: AppHandle) -> Result<(), String> {
    crate::mini::show(&app)
}

#[tauri::command]
pub fn mini_hide(app: AppHandle) {
    crate::mini::hide(&app);
}

#[tauri::command]
pub fn mini_open_main(app: AppHandle) {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
pub fn mini_save_position(state: State<AppState>, x: f64, y: f64) {
    let conn = state.conn.lock();
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES ('mini_x', ?1) ON CONFLICT(key) DO UPDATE SET value=?1",
        [format!("{}", x.round() as i64)],
    );
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES ('mini_y', ?1) ON CONFLICT(key) DO UPDATE SET value=?1",
        [format!("{}", y.round() as i64)],
    );
}

/// Live activity snapshot for the mini bar: the currently open interval (if the tracker
/// is observing and the user is not idle), from the same authoritative state the
/// dashboard uses. Never invents an activity.
#[tauri::command]
pub fn get_current_activity(state: State<AppState>) -> serde_json::Value {
    state.tracker.current_activity_snapshot()
}

#[tauri::command]
pub fn complete_onboarding(state: State<AppState>, consent: bool) -> Result<(), String> {
    {
        let conn = state.conn.lock();
        conn.execute("INSERT INTO settings (key, value) VALUES ('onboarded', '1') ON CONFLICT(key) DO UPDATE SET value = '1'", []).map_err(|e| e.to_string())?;
    }
    let current = state.tracker.tracking_info();
    if !consent {
        if current.state == TrackingStateKind::Privacy {
            // keep privacy; its return preference becomes disabled
            let conn = state.conn.lock();
            let mut info = current.clone();
            info.privacy_return = TrackingStateKind::Disabled;
            let _ = db::set_tracking(&conn, &info);
        } else {
            tracker::set_tracking_state(&state.tracker, TrackingStateKind::Disabled);
        }
    } else if current.state == TrackingStateKind::Confirmation {
        tracker::set_tracking_state(&state.tracker, TrackingStateKind::Active);
    }
    Ok(())
}

/* ---------------- diagnostics ---------------- */

/// Absolute path of the bundled browser-extension folder for a family ("chromium" | "firefox").
/// In packaged builds these live under the app resource directory; in dev, in the repo.
#[tauri::command]
pub fn browser_extension_dir(app: AppHandle, family: String) -> Result<String, String> {
    resolve_extension_dir(&app, &family)
}

/// Open the bundled browser-extension folder for a family in the system file explorer.
/// The path is resolved entirely by this command (never accepted from the webview), so
/// this stays scoped to the two bundled folders; the opener plugin's own Rust API
/// performs no IPC scope check by design for trusted app-initiated opens.
#[tauri::command]
pub fn open_extension_folder(app: AppHandle, family: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = resolve_extension_dir(&app, &family)?;
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| e.to_string())
}

fn resolve_extension_dir(app: &AppHandle, family: &str) -> Result<String, String> {
    use tauri::Manager;
    let sub = match family {
        "firefox" => "browser-extension/firefox",
        _ => "browser-extension/chromium",
    };
    if let Ok(res) = app.path().resolve(sub, tauri::path::BaseDirectory::Resource) {
        if res.exists() {
            return Ok(res.canonicalize().map_err(|e| e.to_string())?.to_string_lossy().to_string());
        }
    }
    // Dev fallback: the repo folders next to src-tauri.
    let dev = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("..")
        .join(if family == "firefox" { "browser-extension-firefox" } else { "browser-extension" });
    if dev.exists() {
        return Ok(dev.canonicalize().map_err(|e| e.to_string())?.to_string_lossy().to_string());
    }
    Err("Extension folder not found in this build.".into())
}

#[tauri::command]
pub fn get_diagnostics(state: State<AppState>) -> Diagnostics {
    let conn = state.conn.lock();
    let path = conn.query_row("SELECT file FROM pragma_database_list WHERE name = 'main'", [], |r| r.get::<_, String>(0)).unwrap_or_default();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let intervals = conn.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_else(|_| "error".into());
    let schema_version = conn.query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    drop(conn);

    Diagnostics {
        platform: format!("{} · {}", tracker::platform_name(), std::env::consts::ARCH),
        tracker_backend: tracker::tracker_backend_name(),
        tracking: state.tracker.tracking_info(),
        capabilities: tracker::capabilities(),
        db: DbHealth { path, size_bytes: size, intervals, schema_version, healthy: integrity == "ok" },
        measured: Some(Measured {
            tracker_thread_cpu_percent: crate::platform_metrics::tracker_cpu_percent(),
            process_memory_mb: crate::platform_metrics::process_memory_mb(),
            uptime_seconds: state.tracker.uptime_seconds(),
        }),
    }
}

/* ---------------- lifecycle ---------------- */

#[tauri::command]
pub fn app_quit(app: AppHandle) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<AppState>() {
        state.tracker.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        // Give the service loop a moment to write the clean-shutdown marker and close intervals.
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
    app.exit(0);
}


pub fn broadcast_state(state: &AppState) {
    tracker::emit_event("tracking-changed", &serde_json::to_string(&state.tracker.tracking_info()).unwrap_or_default());
}


#[cfg(test)]
mod v1_tests {
    use super::*;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    fn insert(conn: &Connection, id: i64, start_ms: i64, end_ms: i64, cat: &str, manual: bool) {
        conn.execute(
            "INSERT INTO activity_intervals (id, started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
             VALUES (?1, ?2, ?3, ?4, 'App', '', ?5, '', 0, 'app', 'test', 0, 'none', 0, ?6)",
            rusqlite::params![id, start_ms, end_ms, db::local_date_of(start_ms), cat, if manual { 1 } else { 0 }],
        )
        .unwrap();
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    const MIN: i64 = 60_000;

    /* ---- manual entries: overlap policy ---- */

    #[test]
    fn manual_entry_fills_gap_and_rejects_activity_overlap() {
        let conn = &mut mem();
        let h = 60 * MIN;
        // A gap row 10:00–11:00 and an activity row 12:00–13:00.
        insert(conn, 1, 10 * h, 11 * h, "gap", false);
        insert(conn, 2, 12 * h, 13 * h, "productive", false);

        // Filling the middle of the gap is fine: the uncovered sides stay honest gaps.
        manual_entry_write(conn, None, 10 * h + 15 * MIN, 10 * h + 45 * MIN, "Deep work", Category::Productive, "").unwrap();
        assert_eq!(count(conn, "SELECT COUNT(*) FROM activity_intervals WHERE category = 'gap'"), 2, "partially covered gap splits into both uncovered sides");
        assert_eq!(count(conn, "SELECT COUNT(*) FROM activity_intervals WHERE manual = 1"), 1);

        // Overlapping observed activity is rejected with a clear message.
        let err = manual_entry_write(conn, None, 12 * h + 30 * MIN, 13 * h + 30 * MIN, "Conflict", Category::Productive, "").unwrap_err();
        assert!(err.contains("Overlaps"), "{err}");
    }

    #[test]
    fn manual_entry_splits_idle_and_gap_at_boundaries() {
        let conn = &mut mem();
        insert(conn, 1, 0, 60 * MIN, "idle", false);
        insert(conn, 2, 60 * MIN, 90 * MIN, "gap", false);
        // Manual 30�60 min: idle trimmed to 0�30, gap untouched (starts exactly at 60).
        manual_entry_write(conn, None, 30 * MIN, 60 * MIN, "Planning", Category::Productive, "").unwrap();
        let idle: (i64, i64) = conn.query_row("SELECT started_at, ended_at FROM activity_intervals WHERE category = 'idle'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(idle, (0, 30 * MIN));
        assert_eq!(count(conn, "SELECT COUNT(*) FROM activity_intervals WHERE category = 'gap'"), 1);
        // No double counting: idle + manual + gap = 90 min of rows.
        let total: f64 = conn.query_row("SELECT COALESCE(SUM(ended_at - started_at),0)/60000.0 FROM activity_intervals", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 90.0);
    }

    #[test]
    fn manual_entry_rejects_other_manual_overlap() {
        let conn = &mut mem();
        manual_entry_write(conn, None, 0, 30 * MIN, "First", Category::Neutral, "").unwrap();
        let err = manual_entry_write(conn, None, 20 * MIN, 40 * MIN, "Second", Category::Neutral, "").unwrap_err();
        assert!(err.contains("manual entry"), "{err}");
    }

    #[test]
    fn manual_entry_edit_moves_without_conflict() {
        let conn = &mut mem();
        manual_entry_write(conn, None, 0, 30 * MIN, "First", Category::Neutral, "").unwrap();
        let id: i64 = conn.query_row("SELECT id FROM activity_intervals WHERE manual = 1", [], |r| r.get(0)).unwrap();
        // Moving within the freed span succeeds; the old row is replaced, not duplicated.
        manual_entry_write(conn, Some(id), 10 * MIN, 50 * MIN, "Moved", Category::Productive, "").unwrap();
        assert_eq!(count(conn, "SELECT COUNT(*) FROM activity_intervals WHERE manual = 1"), 1);
        let span: (i64, i64) = conn.query_row("SELECT started_at, ended_at FROM activity_intervals WHERE manual = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(span, (10 * MIN, 50 * MIN));
    }

    /* ---- bulk assignment ---- */

    #[test]
    fn bulk_assign_sets_project_and_user_source() {
        let conn = &mut mem();
        insert(conn, 1, 0, 10 * MIN, "productive", false);
        insert(conn, 2, 10 * MIN, 20 * MIN, "neutral", false);
        insert(conn, 3, 20 * MIN, 30 * MIN, "idle", false); // idle rows are never assignable
        conn.execute("INSERT INTO projects (id, name, desc, color, archived, created_at) VALUES ('pX', 'X', '', 'var(--green)', 0, 0)", []).unwrap();
        bulk_assign_core(conn, &[1, 2, 3], "pX").unwrap();
        let (p1, s1): (String, String) = conn.query_row("SELECT project, project_source FROM activity_intervals WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((p1.as_str(), s1.as_str()), ("pX", "user"));
        let p3: String = conn.query_row("SELECT project FROM activity_intervals WHERE id = 3", [], |r| r.get(0)).unwrap();
        assert_eq!(p3, "", "idle rows stay unassigned");
        // And back to unassigned:
        bulk_assign_core(conn, &[1, 2], "").unwrap();
        let p1: String = conn.query_row("SELECT project FROM activity_intervals WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(p1, "");
    }

    /* ---- project deletion ---- */

    #[test]
    fn project_delete_clears_references_and_preserves_intervals() {
        let conn = &mut mem();
        conn.execute("INSERT INTO projects (id, name, desc, color, archived, created_at) VALUES ('pD', 'Doomed', '', 'var(--green)', 0, 0)", []).unwrap();
        insert(conn, 1, 0, 10 * MIN, "productive", false);
        conn.execute("UPDATE activity_intervals SET project = 'pD' WHERE id = 1", []).unwrap();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('App', 'productive', 'pD', 0)", []).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('goal_project', 'pD')", []).unwrap();

        let affected = project_delete_core(conn, "pD").unwrap();
        assert_eq!(affected, 1);
        assert_eq!(count(conn, "SELECT COUNT(*) FROM activity_intervals"), 1, "intervals preserved");
        let p: String = conn.query_row("SELECT project FROM activity_intervals WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(p, "");
        let rule_project: String = conn.query_row("SELECT project FROM rules", [], |r| r.get(0)).unwrap();
        assert_eq!(rule_project, "");
        let goal: String = conn.query_row("SELECT value FROM settings WHERE key = 'goal_project'", [], |r| r.get(0)).unwrap();
        assert_eq!(goal, "");
        assert!(project_delete_core(conn, "pD").is_err());
    }

    /* ---- retention ---- */

    #[test]
    fn retention_days_fails_safe_on_unknown() {
        assert_eq!(retention_days("30 days"), Some(30));
        assert_eq!(retention_days("1 year"), Some(365));
        assert_eq!(retention_days("Forever"), None);
        assert_eq!(retention_days("something odd"), None);
    }

    #[test]
    fn retention_sweep_deletes_only_old_rows_when_active() {
        use parking_lot::Mutex;
        use std::sync::Arc;
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        let conn = Arc::new(Mutex::new(conn));
        let handle = crate::tracker::test_handle(conn.clone());
        let state = AppState {
            tracker: handle,
            conn: conn.clone(),
            app_data: std::path::PathBuf::new(),
            last_break_ms: std::sync::atomic::AtomicI64::new(0),
        };
        let today = Local::now().date_naive();
        let old_date = (today - chrono::Duration::days(400)).format("%Y-%m-%d").to_string();
        let recent = today.format("%Y-%m-%d").to_string();
        {
            let c = conn.lock();
            for (id, date) in [(1i64, old_date.clone()), (2, recent.clone())] {
                let ts = db::local_midnight(&date).unwrap() + 60 * MIN;
                c.execute(
                    "INSERT INTO activity_intervals (id, started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, manual)
                     VALUES (?1, ?2, ?3, ?4, 'App', '', 'productive', '', 0, 'app', 'test', 0, 'none', 0, 0)",
                    rusqlite::params![id, ts, ts + 10 * MIN, date],
                )
                .unwrap();
            }
        }
        // Inactive: nothing deleted even with an old preference stored.
        {
            let c = conn.lock();
            c.execute("INSERT INTO settings (key, value) VALUES ('retention', '1 year')", []).unwrap();
        }
        assert_eq!(retention_sweep(&state), 0, "preference alone never deletes");
        // Activated: old rows go, recent rows stay, run is logged.
        {
            let c = conn.lock();
            c.execute("INSERT INTO settings (key, value) VALUES ('retention_active', '1')", []).unwrap();
        }
        assert_eq!(retention_sweep(&state), 1);
        let c = conn.lock();
        let remaining: i64 = c.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get(0)).unwrap();
        assert_eq!(remaining, 1);
        let logged: i64 = c.query_row("SELECT COUNT(*) FROM deletion_log WHERE start_date = 'retention'", [], |r| r.get(0)).unwrap();
        assert_eq!(logged, 1);
    }
}
