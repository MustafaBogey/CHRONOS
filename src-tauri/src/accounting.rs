use crate::models::{ActivityRecord, Category, Totals};
use rusqlite::Connection;

/// Aggregates authoritative intervals into the approved time-accounting contract.
///
/// Invariants (covered by tests):
/// - tracked = observed intervals excluding gaps
/// - active = tracked - idle
/// - classified active = productive + distracting + neutral
/// - coverage = classified / active (0 when active == 0)
/// - productivity = productive / classified active (0 when classified == 0); unclassified and idle excluded from denominator
/// - focused = Σ min(focus, duration) over productive records only
pub fn totals(rows: &[ActivityRecord]) -> Totals {
    let mut t = Totals::default();
    for r in rows {
        let d = (r.end - r.start) as f64;
        match r.cat {
            Category::Productive => t.productive += d,
            Category::Distracting => t.distracting += d,
            Category::Neutral => t.neutral += d,
            Category::Unclassified => t.unclassified += d,
            Category::Idle => t.idle += d,
            Category::Gap => t.gap += d,
        }
        if r.cat != Category::Gap {
            t.tracked += d;
            if r.cat != Category::Idle {
                t.active += d;
            }
        }
        if matches!(r.cat, Category::Productive | Category::Distracting | Category::Neutral) {
            t.classified += d;
        }
        if r.cat == Category::Productive {
            t.focused += qualified_focus_of(r);
        }
    }
    t.coverage = if t.active > 0.0 { (t.classified / t.active * 100.0).round() as i64 } else { 0 };
    t.productivity = if t.classified > 0.0 { (t.productive / t.classified * 100.0).round() as i64 } else { 0 };
    t
}

/// Bounded focus allocation: min(focus, duration), productive records only (enforced by caller).
pub fn qualified_focus_of(r: &ActivityRecord) -> f64 {
    if r.cat != Category::Productive {
        return 0.0;
    }
    let d = r.end - r.start;
    r.focus.min(d).max(0.0)
}

/* ---------------- SQL aggregation (single source for every screen) ---------------- */

/// Aggregates intervals straight from SQLite using the approved contract. Every view
/// (Overview, Timeline, Focus & Goals, Reports, exports) reads through this function so
/// their numbers cannot drift apart.
pub fn sql_totals(conn: &Connection, where_clause: &str, params: &[&dyn rusqlite::ToSql]) -> Totals {
    let sql = format!(
        "SELECT
           COALESCE(SUM(CASE WHEN category != 'gap' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category NOT IN ('gap','idle') THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'productive' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'distracting' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'neutral' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'unclassified' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'idle' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'gap' THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0,
           COALESCE(SUM(CASE WHEN category = 'productive' THEN MIN(focus_minutes, (ended_at - started_at) / 60000.0) ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN category IN ('productive','distracting','neutral') THEN (ended_at - started_at) ELSE 0 END), 0) / 60000.0
         FROM activity_intervals WHERE {where_clause} AND open = 0"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Totals::default(),
    };
    stmt.query_row(params, |r| {
        let mut t = Totals {
            tracked: r.get(0)?, active: r.get(1)?, productive: r.get(2)?, distracting: r.get(3)?,
            neutral: r.get(4)?, unclassified: r.get(5)?, idle: r.get(6)?, gap: r.get(7)?,
            focused: r.get(8)?, classified: r.get(9)?, coverage: 0, productivity: 0,
        };
        t.coverage = if t.active > 0.0 { (t.classified / t.active * 100.0).round() as i64 } else { 0 };
        t.productivity = if t.classified > 0.0 { (t.productive / t.classified * 100.0).round() as i64 } else { 0 };
        Ok(t)
    })
    .unwrap_or_default()
}

/// One row per project within a date range, carrying its client for grouping.
/// Client totals are the sum over that client's projects — attribution and productivity
/// stay independent dimensions.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientProjectRow {
    pub client: String,
    pub project_id: String,
    pub project: String,
    pub minutes: f64,
}

pub fn client_project_breakdown(conn: &Connection, start: &str, end: &str, project_filter: Option<&str>) -> Vec<ClientProjectRow> {
    let filter = match project_filter {
        Some(p) => format!(" AND i.project = '{}'", p.replace('\'', "''")),
        None => String::new(),
    };
    let sql = format!(
        "SELECT COALESCE(p.client, ''), COALESCE(i.project, ''), COALESCE(p.name, CASE WHEN i.project = '' THEN 'No project' ELSE i.project END), SUM((i.ended_at - i.started_at)) / 60000.0
         FROM activity_intervals i LEFT JOIN projects p ON p.id = i.project
         WHERE i.local_date >= ?1 AND i.local_date <= ?2 AND i.open = 0 AND i.category NOT IN ('gap','idle'){filter}
         GROUP BY 1, 2, 3 ORDER BY 4 DESC"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else { return vec![] };
    stmt.query_map(rusqlite::params![start, end], |r| {
        Ok(ClientProjectRow {
            client: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            project_id: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            project: r.get(2)?,
            minutes: r.get(3)?,
        })
    })
    .map(|it| it.filter_map(|x| x.ok()).collect())
    .unwrap_or_default()
}

/// Split result: two records with exact original total duration and proportionally allocated focus.
pub fn split_record_view(r: &ActivityRecord, minutes_in_first: i64, next_id: i64) -> Option<(ActivityRecord, ActivityRecord)> {
    let duration = r.end - r.start;
    let first_minutes = minutes_in_first as f64;
    if first_minutes < 1.0 || first_minutes >= duration {
        return None;
    }
    let mut first = r.clone();
    let mut second = r.clone();
    first.end = r.start + first_minutes;
    second.id = next_id;
    second.start = first.end;
    second.end = r.end;
    let total_focus = r.focus;
    first.focus = total_focus * first_minutes / duration;
    second.focus = total_focus - first.focus;
    Some((first, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: i64, start: f64, end: f64, cat: Category, focus: f64) -> ActivityRecord {        ActivityRecord {
            id, date: "2026-10-24".into(), start, end,
            app: "App".into(), detail: String::new(), cat, project: String::new(),
            focus, kind: "app".into(), source: "test".into(), corrected: false,
            confidence: None, project_source: String::new(), manual: false,
        }
    }

    #[test]
    fn gaps_excluded_from_tracked_and_active() {
        let rows = [rec(1, 0.0, 30.0, Category::Productive, 0.0), rec(2, 30.0, 50.0, Category::Gap, 0.0), rec(3, 50.0, 80.0, Category::Neutral, 0.0)];
        let t = totals(&rows);
        assert_eq!(t.tracked, 60.0);
        assert_eq!(t.active, 60.0);
        assert_eq!(t.gap, 20.0);
    }

    #[test]
    fn idle_excluded_from_active() {
        let rows = [rec(1, 0.0, 50.0, Category::Productive, 0.0), rec(2, 50.0, 80.0, Category::Idle, 0.0)];
        let t = totals(&rows);
        assert_eq!(t.tracked, 80.0);
        assert_eq!(t.active, 50.0);
        assert_eq!(t.idle, 30.0);
    }

    #[test]
    fn unclassified_excluded_from_productivity_denominator() {
        let rows = [
            rec(1, 0.0, 60.0, Category::Productive, 0.0),
            rec(2, 60.0, 90.0, Category::Unclassified, 0.0),
            rec(3, 90.0, 120.0, Category::Distracting, 0.0),
        ];
        let t = totals(&rows);
        assert_eq!(t.classified, 90.0);
        assert_eq!(t.productivity, 67); // 60/90
        assert_eq!(t.active, 120.0);
        assert_eq!(t.coverage, 75); // 90/120
    }

    #[test]
    fn zero_denominators_are_honest() {
        let rows = [rec(1, 0.0, 30.0, Category::Idle, 0.0)];
        let t = totals(&rows);
        assert_eq!(t.coverage, 0);
        assert_eq!(t.productivity, 0);
        let empty = totals(&[]);
        assert_eq!(empty.tracked, 0.0);
        assert_eq!(empty.productivity, 0);
    }

    #[test]
    fn qualified_focus_bounded_and_productive_only() {
        let rows = [rec(1, 0.0, 30.0, Category::Productive, 50.0), rec(2, 30.0, 60.0, Category::Distracting, 30.0)];
        let t = totals(&rows);
        assert_eq!(t.focused, 30.0); // min(50,30); distracting contributes nothing
    }

    #[test]
    fn correction_to_nonproductive_removes_focus_but_not_duration() {
        let mut rows = [rec(1, 0.0, 40.0, Category::Productive, 40.0)];
        rows[0].cat = Category::Neutral;
        rows[0].focus = 0.0; // corrections cannot keep qualified focus on nonproductive records
        let t = totals(&rows);
        assert_eq!(t.neutral, 40.0);
        assert_eq!(t.focused, 0.0);
        assert_eq!(t.tracked, 40.0);
    }

    #[test]
    fn split_preserves_duration_and_allocates_focus_proportionally() {
        let r = rec(7, 100.0, 160.0, Category::Productive, 30.0);
        let (a, b) = split_record_view(&r, 20, 99).unwrap();
        assert_eq!(a.end - a.start + (b.end - b.start), 60.0);
        assert_eq!(a.start, 100.0);
        assert_eq!(a.end, 120.0);
        assert_eq!(b.start, 120.0);
        assert_eq!(b.end, 160.0);
        let fa = a.focus;
        let fb = b.focus;
        assert!((fa + fb - 30.0).abs() < 1e-9, "focus total preserved");
        assert!((fa - 10.0).abs() < 1e-9, "proportional allocation");
        let before = totals(&[r]);
        let after = totals(&[a.clone(), b.clone()]);
        assert_eq!(before.focused, after.focused);
        assert_eq!(before.tracked, after.tracked);
    }

    #[test]
    fn split_rejects_invalid_points() {
        let r = rec(7, 100.0, 160.0, Category::Productive, 30.0);
        assert!(split_record_view(&r, 0, 99).is_none());
        assert!(split_record_view(&r, 60, 99).is_none());
        assert!(split_record_view(&r, -3, 99).is_none());
    }

    #[test]
    fn grouping_preserves_totals() {
        let rows = [
            rec(1, 0.0, 30.0, Category::Productive, 20.0),
            rec(2, 30.0, 60.0, Category::Idle, 0.0),
            rec(3, 60.0, 90.0, Category::Distracting, 0.0),
            rec(4, 90.0, 95.0, Category::Unclassified, 0.0),
        ];
        let whole = totals(&rows);
        let parts = totals(&rows[..2]);
        let parts2 = totals(&rows[2..]);
        // Summing category buckets across groupings must equal the whole.
        assert_eq!(whole.tracked, parts.tracked + parts2.tracked);
        assert_eq!(whole.focused, parts.focused + parts2.focused);
        assert_eq!(whole.productive, parts.productive + parts2.productive);
    }

    #[test]
    fn prototype_baseline_matches_handover() {
        // Handover baseline: tracked 6h20m, active 5h30m, productive 4h25m, distracting 25m,
        // neutral 15m, unclassified 25m, idle 50m, focus 2h45m, coverage 92%, productivity 87%.
        let mut rows = vec![
            rec(1, 0.0, 265.0, Category::Productive, 165.0),
            rec(2, 265.0, 280.0, Category::Idle, 0.0),
            rec(3, 280.0, 295.0, Category::Unclassified, 0.0),
        ];
        // 265 productive + 25 distracting + 15 neutral = 305 classified; tracked 6h20=380 - gaps 20 => 360? adjust:
        rows[0].end = 265.0; // 265m productive
        rows[1].start = 265.0; rows[1].end = 290.0; // 25m idle
        rows[2].start = 290.0; rows[2].end = 305.0; // 15m... keep simple: construct explicitly
        let rows2 = vec![
            rec(1, 540.0, 540.0 + 265.0, Category::Productive, 165.0),
            rec(2, 0.0, 25.0, Category::Distracting, 0.0),
            rec(3, 0.0, 15.0, Category::Neutral, 0.0),
            rec(4, 0.0, 25.0, Category::Unclassified, 0.0),
            rec(5, 0.0, 50.0, Category::Idle, 0.0),
        ];
        let t = totals(&rows2);
        assert_eq!(t.tracked, 380.0);
        assert_eq!(t.active, 330.0);
        assert_eq!(t.classified, 305.0);
        assert_eq!(t.coverage, 92);
        assert_eq!(t.productivity, 87);
        assert_eq!(t.focused, 165.0);
        let _ = rows; // construction helper above intentionally unused beyond demonstration
    }

    /* ---- client/project attribution and SQL aggregation ---- */

    fn mem() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    fn interval(conn: &rusqlite::Connection, app: &str, cat: &str, project: &str, minutes: i64) {
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes)
             VALUES (0, ?4, '2026-10-24', ?1, '', ?2, ?3, 0)",
            rusqlite::params![app, cat, project, minutes * 60_000],
        )
        .unwrap();
    }

    #[test]
    fn client_breakdown_aggregates_multiple_apps_into_projects_and_clients() {
        let conn = mem();
        conn.execute("INSERT INTO projects (id, name, created_at, client) VALUES ('p1','Aurora',1,'Meridian Studios')", []).unwrap();
        conn.execute("INSERT INTO projects (id, name, created_at, client) VALUES ('p2','Aurora Site',2,'Meridian Studios')", []).unwrap();
        conn.execute("INSERT INTO projects (id, name, created_at, client) VALUES ('p3','Learning',3,'')", []).unwrap();
        // Multiple applications feeding one project…
        interval(&conn, "WezTerm", "productive", "p1", 30);
        interval(&conn, "Google Chrome", "neutral", "p1", 20);
        interval(&conn, "Visual Studio Code", "productive", "p1", 10);
        // …a second project under the same client…
        interval(&conn, "Figma", "productive", "p2", 15);
        // …a standalone personal project…
        interval(&conn, "Google Chrome", "neutral", "p3", 5);
        // …unassigned and idle time…
        interval(&conn, "Terminal", "neutral", "", 7);
        interval(&conn, "Idle", "idle", "", 9);
        // …and a gap that must not contribute.
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category) VALUES (0, 600000, '2026-10-24','Tracking gap','','gap')",
            [],
        )
        .unwrap();

        let rows = client_project_breakdown(&conn, "2026-10-24", "2026-10-24", None);
        let total: f64 = rows.iter().map(|r| r.minutes).sum();
        assert!((total - (30.0 + 20.0 + 10.0 + 15.0 + 5.0 + 7.0)).abs() < 1e-6, "gap and idle excluded: {total}");
        let meridian: f64 = rows.iter().filter(|r| r.client == "Meridian Studios").map(|r| r.minutes).sum();
        assert!((meridian - 75.0).abs() < 1e-6, "client total = sum of its projects");
        let aurora = rows.iter().find(|r| r.project == "Aurora").unwrap();
        assert!((aurora.minutes - 60.0).abs() < 1e-6, "three apps aggregated into one project");
        let none = rows.iter().find(|r| r.project == "No project").unwrap();
        assert!(none.client.is_empty());
        assert!((none.minutes - 7.0).abs() < 1e-6);

        // Project filter narrows to that project only.
        let filtered = client_project_breakdown(&conn, "2026-10-24", "2026-10-24", Some("p2"));
        assert_eq!(filtered.len(), 1);
        assert!((filtered[0].minutes - 15.0).abs() < 1e-6);
    }

    #[test]
    fn reassignment_moves_minutes_between_projects_without_double_counting() {
        let conn = mem();
        conn.execute("INSERT INTO projects (id, name, created_at, client) VALUES ('a','A',1,'ACME')", []).unwrap();
        conn.execute("INSERT INTO projects (id, name, created_at, client) VALUES ('b','B',2,'ACME')", []).unwrap();
        interval(&conn, "WezTerm", "productive", "a", 40);

        let before: f64 = client_project_breakdown(&conn, "2026-10-24", "2026-10-24", None).iter().map(|r| r.minutes).sum();
        // Manual reassignment (as correct_record does).
        conn.execute("UPDATE activity_intervals SET project = 'b', project_source = 'user', corrected = 1 WHERE project = 'a'", []).unwrap();
        let rows = client_project_breakdown(&conn, "2026-10-24", "2026-10-24", None);
        let after: f64 = rows.iter().map(|r| r.minutes).sum();
        assert!((before - after).abs() < 1e-6, "reassignment preserves tracked duration");
        assert_eq!(rows.iter().filter(|r| r.project == "A").count(), 0, "origin project no longer holds the minutes");
        let b = rows.iter().find(|r| r.project == "B").expect("destination holds the minutes");
        assert!((b.minutes - 40.0).abs() < 1e-6);
        // Client total follows automatically.
        let acme: f64 = rows.iter().filter(|r| r.client == "ACME").map(|r| r.minutes).sum();
        assert!((acme - 40.0).abs() < 1e-6);
    }

    #[test]
    fn sql_totals_matches_record_totals_on_identical_data() {
        let conn = mem();
        // Equivalent of: productive 60 (focus 40), idle 30, unclassified 25, gap 20.
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, focus_minutes)
             VALUES (0, 3600000, '2026-10-24', 'VSCode', '', 'productive', 40.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category)
             VALUES (3600000, 5400000, '2026-10-24', 'Idle', '', 'idle')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category)
             VALUES (5400000, 6900000, '2026-10-24', 'Chrome', '', 'unclassified')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category)
             VALUES (6900000, 8100000, '2026-10-24', 'Tracking gap', '', 'gap')",
            [],
        )
        .unwrap();
        let sql = sql_totals(&conn, "local_date = ?1", &[&"2026-10-24"]);
        let view = vec![
            rec(1, 0.0, 60.0, Category::Productive, 40.0),
            rec(2, 60.0, 90.0, Category::Idle, 0.0),
            rec(3, 90.0, 115.0, Category::Unclassified, 0.0),
            rec(4, 115.0, 135.0, Category::Gap, 0.0),
        ];
        let mem = totals(&view);
        for (name, a, b) in [
            ("tracked", sql.tracked, mem.tracked),
            ("active", sql.active, mem.active),
            ("productive", sql.productive, mem.productive),
            ("idle", sql.idle, mem.idle),
            ("unclassified", sql.unclassified, mem.unclassified),
            ("gap", sql.gap, mem.gap),
            ("focused", sql.focused, mem.focused),
        ] {
            assert!((a - b).abs() < 1e-6, "{name}: sql {a} vs memory {b}");
        }
    }
}
