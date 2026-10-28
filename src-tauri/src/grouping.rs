//! Lightweight, non-AI grouping of related activity intervals into work blocks.
//!
//! Blocks are computed at read time from stored intervals — nothing is persisted,
//! merged, or rewritten, so grouping can never double-count time or alter a
//! record. Evidence is deliberately conservative:
//!
//! - Only true activity rows (productive / distracting / neutral / unclassified)
//!   form blocks. Idle and tracking gaps are boundaries, never block content.
//! - Intervals chain while the away-span between them (idle + unaccounted time)
//!   stays within `BLOCK_GAP_MAX` (short micro-breaks keep a block alive).
//! - An explicit change of project breaks the block (unrelated projects are
//!   never silently merged); an unassigned interval inherits its neighbors.
//! - User-placed boundary markers (`group_breaks`) always split a block and are
//!   fully reversible — removing the marker restores the computed grouping.
//!
//! Grouped time is *observed continuity*, not verified deep focus: reports show
//! block summaries without equating them with qualified focus.

use rusqlite::Connection;

/// Maximum away-span between two activity intervals that still chains into one
/// work block (coffee/Stretch micro-break tolerance).
pub const BLOCK_GAP_MAX_MS: i64 = 7 * 60_000;

#[derive(Clone, Debug)]
pub struct BlockRow {
    pub id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub app: String,
    pub cat: String,
    pub project: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkBlock {
    #[serde(rename = "startMs")]
    pub start_ms: i64,
    #[serde(rename = "endMs")]
    pub end_ms: i64,
    /// Distinct applications/sites in this block, ordered by first appearance.
    pub apps: Vec<String>,
    /// Application holding the most observed minutes in the block.
    pub dominant: String,
    /// Project of the block (non-empty when its intervals carry one).
    pub project: String,
    /// Sum of interval durations (never the wall-clock span, so internal gaps
    /// never inflate recorded time).
    pub minutes: f64,
    /// Wall-clock span of the block in minutes.
    #[serde(rename = "spanMinutes")]
    pub span_minutes: f64,
    /// Non-activity minutes inside the block span (short idle stretches).
    #[serde(rename = "internalAwayMinutes")]
    pub internal_away_minutes: f64,
    /// Category transitions between consecutive intervals (context switches).
    pub switches: i64,
    /// Interval ids composing the block.
    #[serde(rename = "intervalIds")]
    pub interval_ids: Vec<i64>,
    /// True when this block starts at a user-placed boundary marker.
    #[serde(rename = "userBoundary")]
    pub user_boundary: bool,
}

/// Compute work blocks for one interval set. Rows must be the day's closed
/// activity rows (any category); idle/gap rows are treated as boundaries.
pub fn compute_blocks(rows: &[BlockRow], breaks: &[i64]) -> Vec<WorkBlock> {
    let mut sorted: Vec<&BlockRow> = rows.iter().collect();
    sorted.sort_by_key(|r| r.start_ms);

    let is_activity = |c: &str| matches!(c, "productive" | "distracting" | "neutral" | "unclassified");

    let mut blocks: Vec<WorkBlock> = Vec::new();
    let mut current: Option<WorkBlock> = None;
    let mut last_end: Option<i64> = None;
    let mut last_project: String = String::new();
    let mut last_cat: String = String::new();

    let sorted_breaks = {
        let mut b = breaks.to_vec();
        b.sort_unstable();
        b
    };

    for row in sorted {
        if !is_activity(&row.cat) {
            continue; // idle/gap rows are boundaries, never block members
        }
        let chained = match (&current, last_end) {
            (Some(_), Some(prev_end)) => {
                let away = row.start_ms - prev_end;
                away <= BLOCK_GAP_MAX_MS
                    && !break_at_boundary(prev_end, row.start_ms, &sorted_breaks)
                    && !(is_explicit_project(&last_project) && is_explicit_project(&row.project) && last_project != row.project)
            }
            _ => false,
        };
        if !chained {
            if let Some(b) = current.take() {
                blocks.push(b);
            }
            last_project = row.project.clone();
            let boundary = last_end.map_or(false, |pe| break_at_boundary(pe, row.start_ms, &sorted_breaks));
            current = Some(WorkBlock {
                start_ms: row.start_ms,
                end_ms: row.end_ms,
                apps: vec![row.app.clone()],
                dominant: row.app.clone(),
                project: row.project.clone(),
                minutes: 0.0,
                span_minutes: 0.0,
                internal_away_minutes: 0.0,
                switches: 0,
                interval_ids: vec![row.id],
                user_boundary: boundary,
            });
            last_cat = row.cat.clone();
        } else {
            let b = current.as_mut().expect("chained implies current");
            b.end_ms = b.end_ms.max(row.end_ms);
            if !b.apps.contains(&row.app) {
                b.apps.push(row.app.clone());
            }
            if row.project != "" && b.project == "" {
                b.project = row.project.clone();
            }
            if row.cat != last_cat {
                b.switches += 1;
            }
            b.interval_ids.push(row.id);
            if is_explicit_project(&row.project) {
                last_project = row.project.clone();
            }
        }
        if let Some(b) = current.as_mut() {
            b.minutes += (row.end_ms - row.start_ms) as f64 / 60_000.0;
        }
        last_end = Some(row.end_ms.max(row.start_ms));
        last_cat = row.cat.clone();
    }
    if let Some(b) = current.take() {
        blocks.push(b);
    }

    // Final pass: dominant app by accumulated minutes, span, internal away time.
    let by_block: Vec<(usize, Vec<&BlockRow>)> = {
        let mut m: std::collections::HashMap<usize, Vec<&BlockRow>> = std::collections::HashMap::new();
        for (i, b) in blocks.iter().enumerate() {
            let members: Vec<&BlockRow> = rows.iter().filter(|r| b.interval_ids.contains(&r.id)).collect();
            m.insert(i, members);
        }
        blocks.iter().enumerate().map(|(i, _)| (i, m.remove(&i).unwrap_or_default())).collect()
    };
    for (i, b) in blocks.iter_mut().enumerate() {
        let members = &by_block[i].1;
        let mut best: (&str, i64) = ("", 0);
        let mut totals: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
        for m in members {
            let e = totals.entry(m.app.as_str()).or_insert(0);
            *e += m.end_ms - m.start_ms;
            if *e > best.1 {
                best = (m.app.as_str(), *e);
            }
        }
        if !best.0.is_empty() {
            b.dominant = best.0.to_string();
        }
        b.span_minutes = (b.end_ms - b.start_ms) as f64 / 60_000.0;
        b.internal_away_minutes = (b.span_minutes - b.minutes).max(0.0);
    }
    blocks
}

fn is_explicit_project(p: &str) -> bool {
    !p.is_empty()
}

/// A boundary marker between `prev_end` and `next_start` splits the block when
/// it falls inside that junction.
fn break_at_boundary(prev_end: i64, next_start: i64, sorted_breaks: &[i64]) -> bool {
    sorted_breaks.iter().any(|b| *b > prev_end && *b <= next_start)
}

/// Load boundary markers within a day span.
pub fn load_breaks(conn: &Connection, day_start_ms: i64, day_end_ms: i64) -> Vec<i64> {
    conn.prepare("SELECT at_ts FROM group_breaks WHERE at_ts >= ?1 AND at_ts <= ?2")
        .ok()
        .map(|mut stmt| {
            stmt.query_map(rusqlite::params![day_start_ms, day_end_ms], |r| r.get::<_, i64>(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, s: i64, e: i64, app: &str, cat: &str, project: &str) -> BlockRow {
        BlockRow { id, start_ms: s, end_ms: e, app: app.into(), cat: cat.into(), project: project.into() }
    }

    const MIN: i64 = 60_000;

    #[test]
    fn contiguous_intervals_chain_into_one_block() {
        let rows = vec![
            row(1, 0, 10 * MIN, "VS Code", "productive", "alpha"),
            row(2, 10 * MIN, 14 * MIN, "GitHub", "productive", ""),
            row(3, 14 * MIN, 20 * MIN, "File Explorer", "neutral", ""),
            row(4, 20 * MIN, 30 * MIN, "VS Code", "productive", "alpha"),
        ];
        let blocks = compute_blocks(&rows, &[]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].dominant, "VS Code");
        assert_eq!(blocks[0].minutes, 30.0);
        assert_eq!(blocks[0].interval_ids.len(), 4);
        assert_eq!(blocks[0].switches, 2, "productive→neutral→productive is two category transitions");
    }

    #[test]
    fn short_away_keeps_block_long_away_breaks() {
        let rows = vec![
            row(1, 0, 10 * MIN, "VS Code", "productive", ""),
            row(2, 10 * MIN + 6 * MIN, 20 * MIN + 6 * MIN, "VS Code", "productive", ""), // 6 min away: chains
            row(3, 40 * MIN, 50 * MIN, "VS Code", "productive", ""), // 14 min away: breaks
        ];
        let blocks = compute_blocks(&rows, &[]);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn idle_rows_are_boundaries_not_members() {
        let rows = vec![
            row(1, 0, 10 * MIN, "VS Code", "productive", ""),
            row(2, 11 * MIN, 13 * MIN, "Idle", "idle", ""),
            row(3, 13 * MIN, 20 * MIN, "VS Code", "productive", ""),
        ];
        let blocks = compute_blocks(&rows, &[]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].minutes, 17.0, "idle minutes never count as block time");
        assert_eq!(blocks[0].interval_ids.len(), 2);
    }

    #[test]
    fn distinct_projects_never_merge() {
        let rows = vec![
            row(1, 0, 10 * MIN, "VS Code", "productive", "alpha"),
            row(2, 11 * MIN, 20 * MIN, "VS Code", "productive", "beta"),
        ];
        let blocks = compute_blocks(&rows, &[]);
        assert_eq!(blocks.len(), 2, "explicit project change splits the block");
    }

    #[test]
    fn unassigned_inherits_neighbors() {
        let rows = vec![
            row(1, 0, 10 * MIN, "Terminal", "productive", "alpha"),
            row(2, 11 * MIN, 14 * MIN, "Notes", "neutral", ""),
        ];
        let blocks = compute_blocks(&rows, &[]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].project, "alpha");
    }

    #[test]
    fn user_boundary_marker_splits_and_is_flagged() {
        let rows = vec![
            row(1, 0, 10 * MIN, "VS Code", "productive", ""),
            row(2, 12 * MIN, 20 * MIN, "VS Code", "productive", ""),
        ];
        let blocks = compute_blocks(&rows, &[11 * MIN]);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].user_boundary);
    }

    #[test]
    fn sum_of_block_minutes_equals_sum_of_intervals() {
        let rows = vec![
            row(1, 0, 10 * MIN, "A", "productive", ""),
            row(2, 12 * MIN, 15 * MIN, "B", "neutral", ""),
            row(3, 60 * MIN, 70 * MIN, "C", "productive", ""),
        ];
        let blocks = compute_blocks(&rows, &[]);
        let total: f64 = blocks.iter().map(|b| b.minutes).sum();
        assert!((total - 23.0).abs() < 1e-9, "no double counting: {total}");
    }
}
