//! Real backup, restore, and export over the SQLite database.
//!
//! The core operations (`write_backup`, `inspect_backup`, `perform_restore`) are pure
//! functions over a `Connection` so they are unit-testable against disposable databases;
//! the Tauri commands are thin wrappers.

use crate::commands::AppState;
use crate::db::{self, now_ms};
use crate::models::TrackingInfo;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use tauri::State;

const BACKUP_MAGIC: &str = "CHRONOS-BACKUP-V2";

#[derive(Serialize)]
pub struct BackupManifestOut {
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub intervals: i64,
    pub projects: i64,
    pub rules: i64,
    #[serde(rename = "trackedMinutes")]
    pub tracked_minutes: f64,
    #[serde(rename = "rangeStart")]
    pub range_start: String,
    #[serde(rename = "rangeEnd")]
    pub range_end: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
    pub integrity: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RestorePlanIn {
    pub strategy: String,
    #[serde(rename = "preservePrivacy")]
    pub preserve_privacy: bool,
    #[serde(rename = "allowDeletedReintroduction")]
    pub allow_deleted_reintroduction: bool,
    #[serde(rename = "backupIntervalCount")]
    pub backup_interval_count: i64,
    #[serde(rename = "backupProjectCount")]
    pub backup_project_count: i64,
    pub conflicts: i64,
    #[serde(rename = "previouslyDeletedRecords")]
    pub previously_deleted_records: i64,
}

fn default_backup_dir(state: &AppState) -> PathBuf {
    state.app_data.join("backups")
}

fn tracked_minutes(conn: &Connection) -> f64 {
    conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN category != 'gap' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0 FROM activity_intervals",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0.0)
}

fn verify_backup(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err("Backup file not found.".into());
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Could not open backup: {e}"))?;
    let magic: String = conn
        .query_row("SELECT value FROM meta WHERE key = 'backup_magic'", [], |r| r.get(0))
        .map_err(|_| "This file is not a CHRONOS backup.".to_string())?;
    if magic != BACKUP_MAGIC {
        return Err("Unrecognized backup format.".into());
    }
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_else(|_| "error".into());
    if integrity != "ok" {
        return Err("Backup integrity check failed.".into());
    }
    Ok(conn)
}

/* ---------------- backup core (testable) ---------------- */

/// Write a verified single-file backup of `conn` to `target` and return its manifest data.
pub fn write_backup(conn: &Connection, target: &Path) -> Result<BackupManifestOut, String> {
    let tmp = target.with_extension("tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    conn.execute_batch(&format!("VACUUM INTO '{}'", tmp.to_string_lossy().replace('\'', "''")))
        .map_err(|e| format!("Backup write failed: {e}"))?;
    let integrity = {
        let bconn = Connection::open(&tmp).map_err(|e| e.to_string())?;
        let check: String = bconn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_else(|_| "error".into());
        if check != "ok" {
            let _ = std::fs::remove_file(&tmp);
            return Err("Backup integrity check failed.".into());
        }
        bconn.execute_batch(&format!("INSERT OR REPLACE INTO meta (key, value) VALUES ('backup_magic', '{BACKUP_MAGIC}');"))
            .map_err(|e| format!("Backup stamping failed: {e}"))?;
        "passed".to_string()
    };
    std::fs::rename(&tmp, target).map_err(|e| format!("Backup move failed: {e}"))?;

    let intervals: i64 = conn.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get(0)).unwrap_or(0);
    let projects: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap_or(0);
    let rules: i64 = conn.query_row("SELECT COUNT(*) FROM rules", [], |r| r.get(0)).unwrap_or(0);
    let tracked = tracked_minutes(conn);
    let range_start: String = conn.query_row("SELECT COALESCE(MIN(local_date), '') FROM activity_intervals", [], |r| r.get(0)).unwrap_or_default();
    let range_end: String = conn.query_row("SELECT COALESCE(MAX(local_date), '') FROM activity_intervals", [], |r| r.get(0)).unwrap_or_default();
    let size = std::fs::metadata(target).map(|m| m.len()).unwrap_or(0);
    Ok(BackupManifestOut {
        created_at: chrono::Local::now().to_rfc3339(),
        intervals,
        projects,
        rules,
        tracked_minutes: tracked,
        range_start,
        range_end,
        size_bytes: size,
        integrity,
        path: target.to_string_lossy().to_string(),
    })
}

/// Inspect a backup against the current database: counts, overlap conflicts, and
/// previously-deleted records that reintroduction gating protects.
pub fn inspect_backup(conn: &mut Connection, path: &str) -> Result<RestorePlanIn, String> {
    let (intervals, projects) = {
        let bconn = verify_backup(Path::new(path))?;
        let intervals: i64 = bconn.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get(0)).unwrap_or(0);
        let projects: i64 = bconn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap_or(0);
        (intervals, projects)
    };
    let conflicts = {
        attach(conn, path)?;
        let c = overlap_conflicts(conn).unwrap_or(0);
        detach(conn)?;
        c
    };
    let reintroducible = {
        attach(conn, path)?;
        let c: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bak.activity_intervals b
                 WHERE NOT EXISTS (SELECT 1 FROM main.activity_intervals cur WHERE cur.started_at = b.started_at AND cur.app = b.app)
                   AND EXISTS (SELECT 1 FROM main.deletion_log dl WHERE b.local_date >= dl.start_date AND b.local_date <= dl.end_date)",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        detach(conn)?;
        c
    };
    Ok(RestorePlanIn {
        strategy: "merge".into(),
        preserve_privacy: true,
        allow_deleted_reintroduction: false,
        backup_interval_count: intervals,
        backup_project_count: projects,
        conflicts,
        previously_deleted_records: reintroducible,
    })
}

fn attach(conn: &mut Connection, path: &str) -> Result<(), String> {
    conn.execute(&format!("ATTACH DATABASE '{}' AS bak KEY ''", path.replace('\'', "''")), [])
        .map_err(|e| format!("attach: {e}"))?;
    Ok(())
}

fn detach(conn: &mut Connection) -> Result<(), String> {
    conn.execute("DETACH DATABASE bak", []).map_err(|e| format!("detach: {e}"))?;
    Ok(())
}

fn overlap_conflicts(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM bak.activity_intervals b WHERE EXISTS (SELECT 1 FROM main.activity_intervals cur WHERE cur.started_at < b.ended_at AND cur.ended_at > b.started_at)",
        [],
        |r| r.get(0),
    )
}

/// Transactional restore. Merge skips overlapping rows; replace clears history first.
/// Privacy/pause/exclusion preferences are preserved by default and tracking is never
/// silently re-enabled. Returns (restored_rows, conflicts_skipped).
pub fn perform_restore(
    conn: &mut Connection,
    path: &str,
    merge: bool,
    allow_deleted: bool,
    preserve_privacy: bool,
) -> Result<(i64, i64), String> {
    // Verify in a scope so the read-only connection is fully closed before ATTACH.
    {
        let _bconn = verify_backup(Path::new(path))?;
    }
    let preserved_tracking: Option<TrackingInfo> = if preserve_privacy { Some(db::get_tracking(conn)) } else { None };
    let preserved_exclusions: Vec<String> = if preserve_privacy { crate::classifier::load_exclusions(conn) } else { vec![] };

    attach(conn, path)?;
    let apply = conn.transaction().map_err(|e| e.to_string())?;
    if !merge {
        apply.execute("DELETE FROM activity_intervals", []).map_err(|e| e.to_string())?;
        apply.execute("DELETE FROM projects", []).map_err(|e| e.to_string())?;
        apply.execute("DELETE FROM rules", []).map_err(|e| e.to_string())?;
    }
    let skip_conflict = if merge {
        "AND NOT EXISTS (SELECT 1 FROM main.activity_intervals cur WHERE cur.started_at < b.ended_at AND cur.ended_at > b.started_at)"
    } else {
        ""
    };
    let skip_deleted = if !allow_deleted {
        "AND NOT EXISTS (SELECT 1 FROM main.deletion_log dl WHERE b.local_date >= dl.start_date AND b.local_date <= dl.end_date
                        AND NOT EXISTS (SELECT 1 FROM main.activity_intervals cur WHERE cur.started_at = b.started_at AND cur.app = b.app))"
    } else {
        ""
    };
    // Count would-be-skipped conflicts BEFORE copying (after the insert, restored rows
    // would self-overlap and inflate the count).
    let conflicts_skipped = if merge { overlap_conflicts(&apply).unwrap_or(0) } else { 0 };
    let restored = apply
        .execute(
            &format!(
                "INSERT INTO main.activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, open, project_source)
                 SELECT b.started_at, b.ended_at, b.local_date, b.app, b.detail, b.category, b.project, b.focus_minutes, b.kind, b.source, b.corrected, b.review_status, 0, COALESCE(b.project_source, '')
                 FROM bak.activity_intervals b WHERE 1 = 1 {skip_conflict} {skip_deleted}"
            ),
            [],
        )
        .map_err(|e| format!("copy: {e}"))? as i64;
    // Projects (including client) merge by name; rules merge by app+category (project carried).
    apply
        .execute(
            "INSERT OR IGNORE INTO main.projects (id, name, desc, color, archived, created_at, client)
             SELECT 'r' || b.rowid, b.name, b.desc, b.color, b.archived, b.created_at, COALESCE(b.client, '')
             FROM bak.projects b
             WHERE NOT EXISTS (SELECT 1 FROM main.projects p WHERE LOWER(p.name) = LOWER(b.name))",
            [],
        )
        .map_err(|e| format!("projects merge: {e}"))?;
    // Keep client/grouping fields on projects that already exist by name.
    apply
        .execute(
            "UPDATE main.projects SET client = COALESCE(b.client, '') FROM bak.projects b
             WHERE LOWER(main.projects.name) = LOWER(b.name) AND main.projects.client = ''",
            [],
        )
        .map_err(|e| format!("projects update: {e}"))?;
    apply
        .execute(
            "INSERT INTO main.rules (app, category, project, created_at)
             SELECT b.app, b.category, COALESCE(b.project, ''), b.created_at FROM bak.rules b
             WHERE NOT EXISTS (SELECT 1 FROM main.rules r WHERE LOWER(r.app) = LOWER(b.app) AND r.category = b.category)",
            [],
        )
        .map_err(|e| format!("rules merge: {e}"))?;
    if let Some(t) = preserved_tracking {
        let _ = db::set_tracking(&apply, &t); // never silently re-enable tracking
    }
    for e in preserved_exclusions {
        // Idempotent: only re-insert if the preference is not already present.
        let _ = apply.execute(
            "INSERT INTO exclusions (label, created_at) SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM exclusions WHERE label = ?1)",
            rusqlite::params![e, now_ms()],
        );
    }
    apply.commit().map_err(|e| e.to_string())?;
    detach(conn)?;
    // A restore may reintroduce older, pre-privacy titles: sanitize in place.
    crate::classifier::sanitize_stored_details(conn);
    Ok((restored, conflicts_skipped))
}

/* ---------------- Tauri commands ---------------- */

#[tauri::command]
pub fn backup_create(state: State<AppState>, dir: Option<String>) -> Result<BackupManifestOut, String> {
    create_backup_inner(&state, dir)
}

pub fn create_backup_inner(state: &AppState, dir: Option<String>) -> Result<BackupManifestOut, String> {
    let target = match dir {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => {
            let d = default_backup_dir(state);
            let _ = std::fs::create_dir_all(&d);
            d.join(format!("chronos-backup-{}.chronosbak", chrono::Local::now().format("%Y%m%d-%H%M%S")))
        }
    };
    let manifest = {
        let conn = state.conn.lock();
        write_backup(&conn, &target)?
    };
    let conn = state.conn.lock();
    let _ = conn.execute(
        "INSERT INTO backups_log (path, created_at, intervals, size_bytes, integrity) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![target.to_string_lossy(), now_ms(), manifest.intervals, manifest.size_bytes as i64, manifest.integrity],
    );
    Ok(manifest)
}

#[tauri::command]
pub fn backup_inspect(state: State<AppState>, path: String) -> Result<RestorePlanIn, String> {
    let mut conn = state.conn.lock();
    inspect_backup(&mut conn, &path)
}

#[tauri::command]
pub fn backup_restore(state: State<AppState>, path: String, plan: RestorePlanIn) -> Result<serde_json::Value, String> {
    let merge = plan.strategy == "merge";
    let (restored, conflicts_skipped) = {
        let mut conn = state.conn.lock();
        perform_restore(&mut conn, &path, merge, plan.allow_deleted_reintroduction, plan.preserve_privacy)?
    };
    crate::tracker::emit_event("activity-changed", &json!({ "date": "*" }).to_string());
    Ok(json!({ "restored": restored, "conflictsSkipped": conflicts_skipped }))
}

/* ---------------- export ---------------- */

use crate::commands::ReportRequest;

#[tauri::command]
pub fn export_data(state: State<AppState>, format: String, req: ReportRequest, dir: Option<String>) -> Result<String, String> {
    let fmt = if format.eq_ignore_ascii_case("json") { "json" } else { "csv" };
    let target = match dir {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => default_backup_dir(&state).join("exports"),
    };
    let _ = std::fs::create_dir_all(&target);
    let file = target.join(format!("chronos-export-{}-{}.{}", req.start, req.end, fmt));

    let project_clause = if req.project == "all" { String::new() } else { " AND project = ?3".to_string() };
    let sql = format!(
        "SELECT i.id, i.started_at, i.ended_at, i.local_date, i.app, i.detail, i.category, i.project, i.focus_minutes, i.kind, i.source, i.corrected, COALESCE(p.client, '')
         FROM activity_intervals i LEFT JOIN projects p ON p.id = i.project
         WHERE i.local_date >= ?1 AND i.local_date <= ?2 AND i.open = 0 AND i.category != 'gap'{project_clause}
         ORDER BY i.started_at"
    );
    let params: Vec<&dyn rusqlite::ToSql> = if req.project == "all" {
        vec![&req.start, &req.end]
    } else {
        vec![&req.start, &req.end, &req.project]
    };
    let rows: Vec<(i64, i64, i64, String, String, String, String, String, f64, String, String, i64, String)> = {
        let conn = state.conn.lock();
        let Ok(mut stmt) = conn.prepare(&sql) else { return Err("Query failed.".into()) };
        let collected = stmt
            .query_map(params.as_slice(), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get::<_, Option<String>>(12)?.unwrap_or_default()))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        collected
    };

    let project_names = {
        let conn = state.conn.lock();
        crate::commands::load_projects(&conn).into_iter().map(|p| (p.id, p.name)).collect::<Vec<_>>()
    };
    let pname = |id: &str| project_names.iter().find(|(pid, _)| pid == id).map(|(_, n)| n.clone()).unwrap_or_else(|| id.to_string());

    let content = if fmt == "csv" {
        let mut out = String::from("id,date,start,end,duration_minutes,application,detail,category,project,client,focus_minutes,classification_source,corrected\n");
        for (id, started, ended, date, app, detail, cat, project, _focus, _kind, source, corrected, client) in &rows {
            let start_minutes = crate::db::local_minutes_of(*started);
            let duration = (*ended - *started) as f64 / 60000.0;
            let end_minutes = start_minutes + duration;
            out.push_str(&format!(
                "{},{},{},{},{:.2},{},{},{},{},{},{:.2},{},{}\n",
                id,
                date,
                hhmm(start_minutes),
                hhmm(end_minutes),
                duration,
                csv_escape(app),
                csv_escape(detail),
                cat,
                csv_escape(&pname(project)),
                csv_escape(client),
                _focus,
                csv_escape(source),
                if *corrected == 1 { "yes" } else { "no" },
            ));
        }
        out
    } else {
        let items: Vec<serde_json::Value> = rows
            .iter()
            .map(|(id, started, ended, date, app, detail, cat, project, focus, _kind, source, corrected, client)| {
                json!({
                    "id": id, "date": date,
                    "start": crate::db::local_minutes_of(*started).round(),
                    "end": (crate::db::local_minutes_of(*started).round() as f64 + ((*ended - *started) as f64 / 60000.0).round()) as i64,
                    "application": app, "detail": detail, "category": cat,
                    "project": pname(project), "client": client,
                    "focusMinutes": focus, "source": source, "corrected": *corrected == 1,
                })
            })
            .collect();
        serde_json::to_string_pretty(&json!({ "range": { "start": req.start, "end": req.end }, "intervals": items })).unwrap_or_default()
    };
    std::fs::write(&file, content).map_err(|e| e.to_string())?;
    Ok(file.to_string_lossy().to_string())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn hhmm(minutes: f64) -> String {
    let h = (minutes / 60.0).floor() as i64;
    let m = (minutes % 60.0).round() as i64;
    format!("{:02}:{:02}", h, m)
}

/* ---------------- scheduled backups ---------------- */

pub fn run_scheduled_backup_if_due(state: &AppState) {
    let enabled: bool = {
        let conn = state.conn.lock();
        conn.query_row("SELECT value FROM settings WHERE key = 'scheduled'", [], |r| r.get::<_, String>(0))
            .map(|v| v == "1")
            .unwrap_or(false)
    };
    if !enabled {
        return;
    }
    let last: Option<i64> = {
        let conn = state.conn.lock();
        conn.query_row("SELECT MAX(created_at) FROM backups_log", [], |r| r.get(0)).ok().flatten()
    };
    let due = last.map(|l| now_ms() - l > 20 * 3600 * 1000).unwrap_or(true);
    if due {
        let _ = create_backup_inner(state, None);
    }
}

/* ---------------- controlled tests (disposable databases only) ---------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_db(tag: &str) -> (Connection, PathBuf) {
        let uniq = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("chronos-bk-test-{tag}-{uniq}.db"));
        let conn = Connection::open(&path).unwrap();
        crate::db::migrate(&conn).unwrap();
        (conn, path)
    }

    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, name, desc, color, archived, created_at, client) VALUES ('p1','Aurora','','var(--cyan)',0,1,'Meridian Studios')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, desc, color, archived, created_at, client) VALUES ('p2','Personal','','var(--green)',0,2,'')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO rules (app, category, project, created_at) VALUES ('WezTerm','productive','p1',1)", []).unwrap();
        conn.execute("INSERT INTO exclusions (label, created_at) VALUES ('private.example', 1)", []).unwrap();
        for start in [0i64, 60_000, 120_000] {
            conn.execute(
                "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, project_source)
                 VALUES (?1, ?1 + 60000, '2026-10-24', 'WezTerm', 'opencode', 'productive', 'p1', 1.0, 'app', 'test', 1, 'user')",
                rusqlite::params![start],
            )
            .unwrap();
        }
        db::set_tracking(conn, &crate::models::TrackingInfo {
            state: crate::models::TrackingStateKind::Paused,
            privacy_return: crate::models::TrackingStateKind::Active,
            degraded_reason: None,
        })
        .unwrap();
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn backup_round_trip_and_inspect() {
        let (src, src_path) = tmp_db("src");
        seed(&src);
        let bak_path = src_path.with_extension("chronosbak");
        let manifest = write_backup(&src, &bak_path).unwrap();
        assert_eq!(manifest.integrity, "passed");
        assert_eq!(manifest.intervals, 3);
        assert_eq!(manifest.projects, 2);
        assert_eq!(manifest.rules, 1);
        assert!(bak_path.exists());

        // A non-CHRONOS file is rejected.
        let junk = src_path.with_extension("junk");
        std::fs::write(&junk, b"not a database").unwrap();
        assert!(verify_backup(&junk).is_err());

        // Inspect against a fresh empty database: everything is new (no conflicts).
        let (mut dst, dst_path) = tmp_db("dst");
        let plan = inspect_backup(&mut dst, bak_path.to_str().unwrap()).unwrap();
        assert_eq!(plan.backup_interval_count, 3);
        assert_eq!(plan.conflicts, 0);
        drop(dst);
        let _ = std::fs::remove_file(dst_path);
        let _ = std::fs::remove_file(junk);
        let _ = std::fs::remove_file(bak_path);
        let _ = std::fs::remove_file(src_path);
    }

    #[test]
    fn restore_merge_skips_overlaps_and_preserves_privacy() {
        let (src, src_path) = tmp_db("m-src");
        seed(&src);
        let bak_path = src_path.with_extension("chronosbak");
        write_backup(&src, &bak_path).unwrap();

        // Destination has one overlapping row (same window) and one new row.
        let (dst, dst_path) = tmp_db("m-dst");
        dst.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category)
             VALUES (0, 60000, '2026-10-24', 'Local App', '', 'neutral')",
            [],
        )
        .unwrap();
        // Destination tracking is PAUSED and has its own exclusion: both must survive restore.
        dst.execute("INSERT INTO exclusions (label, created_at) VALUES ('local-only.example', 5)", []).unwrap();
        db::set_tracking(&dst, &crate::models::TrackingInfo {
            state: crate::models::TrackingStateKind::Paused,
            privacy_return: crate::models::TrackingStateKind::Active,
            degraded_reason: None,
        })
        .unwrap();

        let mut dst = dst;
        let (restored, skipped) = perform_restore(&mut dst, bak_path.to_str().unwrap(), true, false, true).unwrap();
        // Backup rows: 3 total; the one overlapping [0,60000) is skipped → 2 restored.
        assert_eq!((restored, skipped), (2, 1));
        // Project with client recovered; rules with project recovered.
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM projects WHERE client = 'Meridian Studios'"), 1);
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM rules WHERE project = 'p1'"), 1);
        // Privacy preserved: still paused, never silently resumed; current exclusion kept.
        assert_eq!(db::get_tracking(&dst).state, crate::models::TrackingStateKind::Paused);
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM exclusions WHERE label = 'local-only.example'"), 1);
        // Overlapping local row untouched (merge keeps current data).
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM activity_intervals WHERE app = 'Local App'"), 1);

        let _ = std::fs::remove_file(&dst_path);
        let _ = std::fs::remove_file(&bak_path);
        let _ = std::fs::remove_file(&src_path);
    }

    #[test]
    fn restore_replace_and_deleted_record_gating() {
        let (src, src_path) = tmp_db("r-src");
        seed(&src);
        let bak_path = src_path.with_extension("chronosbak");
        write_backup(&src, &bak_path).unwrap();

        let (dst, dst_path) = tmp_db("r-dst");
        let mut dst = dst;
        // Destination previously deleted 2026-10-24 history.
        dst.execute("INSERT INTO deletion_log (start_date, end_date, deleted_at) VALUES ('2026-10-24','2026-10-24', 1)", []).unwrap();

        // Merge WITHOUT reintroduction consent: backup rows in the deleted range stay out.
        let (restored, _skipped) = perform_restore(&mut dst, bak_path.to_str().unwrap(), true, false, true).unwrap();
        assert_eq!(restored, 0, "deleted-range rows must not return without explicit consent");
        // Same restore WITH explicit consent: rows return.
        let (restored2, _) = perform_restore(&mut dst, bak_path.to_str().unwrap(), true, true, true).unwrap();
        assert_eq!(restored2, 3);

        // Replace clears current history first.
        dst.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category)
             VALUES (999_999_000, 999_999_999, '2026-10-25', 'Local', '', 'neutral')",
            [],
        )
        .unwrap();
        let (restored3, _) = perform_restore(&mut dst, bak_path.to_str().unwrap(), false, true, true).unwrap();
        assert_eq!(restored3, 3);
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM activity_intervals"), 3, "replace removes local rows");
        // project_source survived the round trip.
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM activity_intervals WHERE project_source = 'user'"), 3);

        let _ = std::fs::remove_file(&dst_path);
        let _ = std::fs::remove_file(&bak_path);
        let _ = std::fs::remove_file(&src_path);
    }

    #[test]
    fn restore_rolls_back_on_failure() {
        let (dst, dst_path) = tmp_db("f-dst");
        let mut dst = dst;
        let before = count(&dst, "SELECT COUNT(*) FROM activity_intervals");
        // Restoring a garbage file fails without touching the database.
        let junk = dst_path.with_extension("junk2");
        std::fs::write(&junk, b"garbage").unwrap();
        let result = perform_restore(&mut dst, junk.to_str().unwrap(), true, false, true);
        assert!(result.is_err());
        assert_eq!(count(&dst, "SELECT COUNT(*) FROM activity_intervals"), before);
        assert_eq!(dst.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'bak'", [], |r| r.get::<_, i64>(0)).unwrap_or(1), 0, "no lingering attach");
        let _ = std::fs::remove_file(&junk);
        let _ = std::fs::remove_file(&dst_path);
    }
}
