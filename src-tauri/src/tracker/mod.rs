use crate::classifier::{self};
use crate::db::{self, local_date_of, local_midnight, now_ms};
use crate::models::*;
use chrono::{Local, Timelike};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Events produced by platform adapters.
#[derive(Clone, Debug)]
pub enum PlatformEvent {
    Foreground { exe: String, title: String, full_path: Option<String> },
    Tick { idle_ms: Option<u64>, foreground: Option<(String, String, Option<String>)> },
    SystemSleep,
    SystemWake,
    Locked,
    Unlocked,
    SourceLost(String),
}

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

/// Webview event emitter: (event name, JSON payload). Set once at app start.
pub type Emitter = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
static EMITTER: OnceLock<Emitter> = OnceLock::new();

pub fn set_emitter(e: Emitter) {
    let _ = EMITTER.set(e);
}

/// Lightweight diagnostic file logger (off unless CHRONOS_DIAG=1).
#[allow(dead_code)]
pub fn diag(msg: &str) {
    if std::env::var("CHRONOS_DIAG").as_deref() != Ok("1") {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("chronos-diag.log"))
    {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%H:%M:%S%.3f"), msg);
    }
}

pub fn emit_event(name: &str, payload: &str) {
    if let Some(e) = EMITTER.get() {
        // Parse the JSON payload so the webview receives a real object (not a string).
        let value: serde_json::Value =
            serde_json::from_str(payload).unwrap_or(serde_json::Value::String(payload.to_string()));
        // Emit on a detached thread so the caller never holds tracker/db locks while the
        // webview (or tray rebuild) processes the event.
        let name = name.to_string();
        std::thread::spawn(move || e(&name, value));
    }
}

/// Shared handle into the running tracker. Thread-safe for Tauri state.
pub struct TrackerHandle {
    pub(crate) inner: Arc<Mutex<TrackerInner>>,
    pub(crate) stop: Arc<AtomicBool>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    started: Instant,
    pub(crate) conn: Arc<Mutex<Connection>>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum RunState {
    Observing,
    NotObserving,
}

pub struct TrackerInner {
    pub tracking: TrackingInfo,
    state: RunState,
    current_app: Option<(String, String)>,
    open: Option<OpenInterval>,
    idle_open: Option<i64>,
    pause_started: Option<(i64, &'static str)>,
    last_event_ts: i64,
    last_activity_input_ts: i64,
    pub session: SessionState,
    current_domain: Option<(String, i64)>,
    /// Timestamp of the last valid browser-extension domain hint (0 = never).
    last_domain_hint_ts: i64,
    /// Contiguous productive run outside a focus session: (run start ts, minutes so far).
    /// Used by the sustained-block qualified-focus model.
    productive_run: Option<(i64, f64)>,
    /// Last distraction-discouragement notice (throttle, ms). 0 = never.
    last_block_notice_ms: i64,
    pub dnd: bool,
    pub settings: Settings,
    undo_depth: usize,
}

pub struct SessionState {
    pub snapshot: SessionSnapshot,
    db_id: Option<i64>,
    started_ms: i64,
    accumulated_ms: i64,
    last_resume_ms: Option<i64>,
    last_persist_ms: i64,
    /// Phase machine (focus/break cycle): wall-clock start of the current phase.
    phase_started_ms: i64,
    /// Wall-clock start of the current break (for the break record's started_ms).
    break_started_ms: i64,
    /// Accumulated explicit extension requests in the current break, ms.
    break_extensions_ms: i64,
    /// Completed breaks in this session (honest wall-clock records).
    break_history: Vec<BreakRecord>,
    /// Totals for persistence.
    break_total_ms: i64,
    break_planned_total_ms: i64,
    break_extensions_total_ms: i64,
    break_overrun_total_ms: i64,
    /// Idle pause: wall-clock moment the focus countdown froze (the tracker's
    /// authoritative last-input time, clamped to the phase start). 0 = running.
    /// While set, the focus countdown holds its remaining time and never
    /// expires or triggers a scheduled break; only an explicit Resume Focus
    /// (or a manual pause/resume) continues it.
    idle_paused_ms: i64,
    /// Wall-clock moment the session was manually paused (0 = not paused).
    /// The phase countdown is re-anchored by the actual pause duration on
    /// resume, so repeated pause/resume cycles inside one phase never refund
    /// focus time.
    manual_paused_ms: i64,
}

impl SessionState {
    /// Fold an active idle pause into a concrete phase re-anchor at `at_ms`.
    /// Used when a stronger pause takes over (manual pause, session end) so
    /// the frozen remaining time is preserved exactly once.
    fn commit_idle_freeze(&mut self, at_ms: i64) {
        if self.idle_paused_ms > 0 {
            self.phase_started_ms += (at_ms - self.idle_paused_ms).max(0);
            self.idle_paused_ms = 0;
        }
    }
}

struct OpenInterval {
    exe: String,
    start_ms: i64,
    cat: Category,
    /// True when this row was attributed to a website via the browser extension.
    is_browser: bool,
    /// The domain (or catalog service label) this row was attributed to, when any.
    domain: Option<String>,
}

/// Qualified-focus model: productive activity counts toward Daily Focus when it forms a
/// sustained contiguous stretch of at least `FOCUS_BLOCK_MINUTES` minutes (idle, gaps,
/// pauses, and non-productive intervals break the stretch), or when it occurs inside an
/// explicit focus session. The allocation stored per record is always bounded by duration.
pub const FOCUS_BLOCK_MINUTES: f64 = 5.0;

/// Pure close-time decision for qualified focus outside sessions.
/// Returns (focus_minutes_for_this_record, new_run, retroactively_qualify_run).
pub fn focus_block_decision(cat: Category, dur_min: f64, start_ts: i64, run: Option<(i64, f64)>) -> (f64, Option<(i64, f64)>, bool) {
    if cat != Category::Productive {
        return (0.0, None, false); // any non-productive interval ends the run
    }
    let (run_start, run_minutes) = match run {
        Some(r) => r,
        None => (start_ts, 0.0),
    };
    let new_total = run_minutes + dur_min;
    let new_run = Some((run_start, new_total));
    if new_total >= FOCUS_BLOCK_MINUTES {
        // The whole sustained block qualifies. Earlier run rows are back-filled exactly
        // once â€” on the close where the threshold is first crossed.
        let crossed_now = run_minutes < FOCUS_BLOCK_MINUTES;
        (dur_min, new_run, crossed_now && run.is_some())
    } else {
        // Below the threshold: productive but not yet qualified focus.
        (0.0, new_run, false)
    }
}

impl TrackerInner {
    fn elapsed_seconds(&self) -> i64 {
        match self.session.snapshot.status.as_str() {
            "running" => {
                let now = now_ms();
                let resume = self.session.last_resume_ms.unwrap_or(now);
                (self.session.accumulated_ms + (now - resume).max(0)) / 1000
            }
            _ => self.session.accumulated_ms / 1000,
        }
    }
    fn session_snapshot(&self) -> SessionSnapshot {
        let mut s = self.session.snapshot.clone();
        s.elapsed = self.elapsed_seconds();
        // Phase countdown: derived from the authoritative wall-clock phase start,
        // never a decrementing counter. Negative = the break has expired.
        if s.status == "running" && !s.phase.is_empty() {
            let now = now_ms();
            match s.phase.as_str() {
                "focus" if s.breaks_enabled && s.focus_minutes > 0 => {
                    // Idle pause: the countdown is frozen at the authoritative
                    // last-input moment and does not advance with wall clock
                    // until the user explicitly resumes.
                    let t = if self.session.idle_paused_ms > 0 {
                        self.session.idle_paused_ms.min(now)
                    } else {
                        now
                    };
                    s.phase_remaining_ms = (self.session.phase_started_ms + s.focus_minutes * 60_000) - t;
                }
                "break" => {
                    let allowance = s.break_minutes * 60_000 + self.session.break_extensions_ms;
                    s.phase_remaining_ms = (self.session.break_started_ms + allowance) - now;
                }
                _ => s.phase_remaining_ms = 0,
            }
            s.break_extensions_min = self.session.break_extensions_ms as f64 / 60_000.0;
        } else if s.status == "paused" && !s.phase.is_empty() {
            // Manually paused: expose the frozen countdown anchored at the
            // pause moment for display. Accounting is unchanged — the freeze
            // is applied as the actual pause duration on resume.
            let t = if self.session.manual_paused_ms > 0 {
                self.session.manual_paused_ms
            } else {
                now_ms()
            };
            match s.phase.as_str() {
                "focus" if s.breaks_enabled && s.focus_minutes > 0 => {
                    s.phase_remaining_ms = (self.session.phase_started_ms + s.focus_minutes * 60_000) - t;
                }
                "break" => {
                    let allowance = s.break_minutes * 60_000 + self.session.break_extensions_ms;
                    s.phase_remaining_ms = (self.session.break_started_ms + allowance) - t;
                }
                _ => s.phase_remaining_ms = 0,
            }
        } else {
            s.phase_remaining_ms = 0;
        }
        // Break totals for the session card: completed breaks plus the running
        // one, so the values update live during a break. Purely derived from
        // the existing break-accounting state — idle pauses are never breaks,
        // and break time never counts as qualified focus (rules unchanged).
        let ongoing_break_ms = if self.session.snapshot.phase == "break" && self.session.break_started_ms > 0 {
            let t = if self.session.snapshot.status == "running" || self.session.manual_paused_ms == 0 {
                now_ms()
            } else {
                // Manually paused mid-break: the break clock is frozen at the
                // pause moment (same anchor as the countdown display).
                self.session.manual_paused_ms
            };
            Some((t - self.session.break_started_ms).max(0))
        } else {
            None
        };
        s.break_count = self.session.break_history.len() as i64 + i64::from(ongoing_break_ms.is_some());
        s.break_total_min = (self.session.break_total_ms + ongoing_break_ms.unwrap_or(0)) as f64 / 60_000.0;
        let ongoing_overrun_ms = ongoing_break_ms
            .map(|ms| (ms - (self.session.snapshot.break_minutes * 60_000 + self.session.break_extensions_ms)).max(0))
            .unwrap_or(0);
        s.break_overrun_total_min = (self.session.break_overrun_total_ms + ongoing_overrun_ms) as f64 / 60_000.0;
        s.focus_idle_paused = s.status == "running" && self.session.idle_paused_ms > 0;
        s
    }
}

impl TrackerHandle {
    pub fn uptime_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
    pub fn snapshot_session(&self) -> SessionSnapshot {
        self.inner.lock().session_snapshot()
    }
    pub fn tracking_info(&self) -> TrackingInfo {
        self.inner.lock().tracking.clone()
    }
    pub fn set_dnd(&self, on: bool) {
        self.inner.lock().dnd = on;
    }
    pub fn set_domain_hint(&self, domain: String) {
        let mut g = self.inner.lock();
        g.current_domain = Some((domain, now_ms()));
        g.last_domain_hint_ts = now_ms();
    }
    /// Timestamp (ms) of the most recent valid domain hint from a browser extension,
    /// for pairing-status feedback. 0 = never received.
    pub fn last_domain_hint_ms(&self) -> i64 {
        self.inner.lock().last_domain_hint_ts
    }

    /// Live activity snapshot for the dashboard and Mini CHRONOS: the currently
    /// open interval when the tracker is observing and the user is not idle.
    /// None otherwise â€” never invented. `idle` distinguishes "away" from
    /// "nothing observed" while tracking is active.
    pub fn current_activity_snapshot(&self) -> serde_json::Value {
        let g = self.inner.lock();
        current_activity_json(&g)
    }
    pub fn refresh_settings(&self, s: Settings) {
        self.inner.lock().settings = s;
    }
    pub fn undo_depth(&self) -> usize {
        self.inner.lock().undo_depth
    }
    pub fn set_undo_depth(&self, n: usize) {
        self.inner.lock().undo_depth = n;
    }
    /// Estimated milliseconds since last user input (derived from the latest platform tick).
    pub fn idle_ms_estimate(&self) -> Option<i64> {
        let g = self.inner.lock();
        if !g.settings.auto_idle {
            return None;
        }
        Some((now_ms() - g.last_activity_input_ts).max(0))
    }
}

/// Recovery decision: unknown (unclean) shutdown never silently enables tracking and never
/// overrides an explicit stop preference.
pub fn recovered_state(persisted: TrackingInfo, clean_shutdown: bool) -> TrackingInfo {
    if clean_shutdown {
        return persisted;
    }
    match persisted.state {
        TrackingStateKind::Paused | TrackingStateKind::Privacy | TrackingStateKind::Disabled => persisted,
        _ => TrackingInfo {
            state: TrackingStateKind::Confirmation,
            privacy_return: persisted.privacy_return,
            degraded_reason: Some("Previous session ended unexpectedly".into()),
        },
    }
}

/// Test-only handle without spawning platform threads.
#[cfg(test)]
    pub fn test_handle(conn: Arc<Mutex<Connection>>) -> Arc<TrackerHandle> {
        let (tracking, settings) = {
            let c = conn.lock();
            (db::get_tracking(&c), db::get_settings(&c))
        };
        let inner = TrackerInner {
            tracking,
        state: RunState::NotObserving,
        current_app: None,
        open: None,
        idle_open: None,
        pause_started: None,
        last_event_ts: now_ms(),
        last_activity_input_ts: now_ms(),
        session: SessionState {
            snapshot: SessionSnapshot::default(),
            db_id: None,
            started_ms: 0,
            accumulated_ms: 0,
            last_resume_ms: None,
            last_persist_ms: 0,
            phase_started_ms: 0,
            break_started_ms: 0,
            break_extensions_ms: 0,
            break_history: Vec::new(),
            break_total_ms: 0,
            break_planned_total_ms: 0,
            break_extensions_total_ms: 0,
            break_overrun_total_ms: 0,
            idle_paused_ms: 0,
            manual_paused_ms: 0,
        },
        current_domain: None,
        last_domain_hint_ts: 0,
        productive_run: None,
        last_block_notice_ms: 0,
        dnd: false,
        settings,
        undo_depth: 0,
    };
    Arc::new(TrackerHandle {
        inner: Arc::new(Mutex::new(inner)),
        stop: Arc::new(AtomicBool::new(false)),
        handles: Mutex::new(Vec::new()),
        started: Instant::now(),
        conn,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem() -> (Arc<Mutex<Connection>>, Arc<TrackerHandle>) {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        crate::db::migrate(&conn.lock()).unwrap();
        let h = test_handle(conn.clone());
        (conn, h)
    }

    #[test]
    fn pause_persists_and_resume_writes_gap() {
        let (conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Active);
        set_tracking_state(&h, TrackingStateKind::Paused);
        // persisted
        {
            let c = conn.lock();
            let t = db::get_tracking(&c);
            assert_eq!(t.state, TrackingStateKind::Paused);
        }
        std::thread::sleep(Duration::from_millis(15)); // ensure the gap interval has positive duration
        set_tracking_state(&h, TrackingStateKind::Active);
        let c = conn.lock();
        let t = db::get_tracking(&c);
        assert_eq!(t.state, TrackingStateKind::Active);
        let gaps: i64 = c
            .query_row("SELECT COUNT(*) FROM activity_intervals WHERE category = 'gap'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(gaps, 1, "pauseâ†’resume writes exactly one gap interval");
    }

    #[test]
    fn resume_cannot_override_privacy_and_leave_restores_prior() {
        let (_conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Active);
        set_tracking_state(&h, TrackingStateKind::Privacy);
        // Resume attempts must not exit privacy.
        set_tracking_state(&h, TrackingStateKind::Active);
        assert_eq!(h.tracking_info().state, TrackingStateKind::Privacy, "privacy cannot be overridden by resume");
        // Explicit leave restores the saved prior preference (active).
        let restored = leave_privacy(&h);
        assert_eq!(restored.state, TrackingStateKind::Active);
    }

    #[test]
    fn privacy_leave_restores_disabled_prior() {
        let (_conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Disabled);
        set_tracking_state(&h, TrackingStateKind::Privacy);
        let restored = leave_privacy(&h);
        assert_eq!(restored.state, TrackingStateKind::Disabled);
    }

    #[test]
    fn recovery_decision() {
        let info = |s: TrackingStateKind| TrackingInfo { state: s, privacy_return: TrackingStateKind::Active, degraded_reason: None };
        // Unclean shutdown while active â†’ confirmation required.
        assert_eq!(recovered_state(info(TrackingStateKind::Active), false).state, TrackingStateKind::Confirmation);
        // Unclean shutdown while paused/privacy/disabled â†’ preserved exactly.
        assert_eq!(recovered_state(info(TrackingStateKind::Paused), false).state, TrackingStateKind::Paused);
        assert_eq!(recovered_state(info(TrackingStateKind::Privacy), false).state, TrackingStateKind::Privacy);
        assert_eq!(recovered_state(info(TrackingStateKind::Disabled), false).state, TrackingStateKind::Disabled);
        // Clean shutdown keeps the persisted state.
        assert_eq!(recovered_state(info(TrackingStateKind::Active), true).state, TrackingStateKind::Active);
    }

    #[test]
    fn focus_blocks_accumulate_and_backfill() {
        use crate::models::Category as Cat;
        // Short productive burst below the threshold: productive but not qualified focus.
        let (f1, run1, retro) = focus_block_decision(Cat::Productive, 2.0, 1_000, None);
        assert_eq!((f1, retro), (0.0, false));
        assert!(run1.is_some());
        // Second contiguous productive interval crosses the 5-minute threshold.
        let (f2, run2, retro2) = focus_block_decision(Cat::Productive, 4.0, 60_000, run1);
        assert_eq!(f2, 4.0);
        assert!(retro2, "earlier run rows are back-filled once");
        assert!(run2.is_some());
        // Continuing the sustained block qualifies directly.
        let (f3, _run3, retro3) = focus_block_decision(Cat::Productive, 3.0, 300_000, run2);
        assert_eq!(f3, 3.0);
        assert!(!retro3);
        // A non-productive interval ends the run.
        let (f4, run4, _) = focus_block_decision(Cat::Neutral, 1.0, 480_000, _run3);
        assert_eq!((f4 as i32, run4.is_none()), (0, true));
        // After the break, a new short burst starts over.
        let (f5, run5, _) = focus_block_decision(Cat::Productive, 1.0, 540_000, run4);
        assert_eq!(f5, 0.0);
        assert!(run5.is_some());
    }

    #[test]
    fn single_long_productive_interval_qualifies_directly() {
        use crate::models::Category as Cat;
        let (f, _run, retro) = focus_block_decision(Cat::Productive, 7.0, 0, None);
        assert_eq!((f, retro), (7.0, false));
    }

    #[test]
    fn session_start_resets_and_toggle_end_flow() {
        let (_conn, h) = mem();
        let s = session_start(&h, "Deep work".into(), String::new(), 25, 5, true);
        assert_eq!(s.status, "running");
        assert_eq!(s.qualified, 0.0);
        assert_eq!(s.elapsed, 0);
        let t = session_toggle(&h).unwrap();
        assert_eq!(t.status, "paused");
        let t2 = session_toggle(&h).unwrap();
        assert_eq!(t2.status, "running");
        let e = session_end(&h);
        assert_eq!(e.status, "ended");
        assert!(session_toggle(&h).is_err());
    }

    #[test]
    fn intervals_split_at_midnight() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        // 23:50 local â†’ 00:10 next day (25 min crossing midnight)
        let start = db::local_midnight(&chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()).unwrap()
            - 10 * 60_000 + 24 * 3600_000; // tomorrow 00:00 minus 10 min
        let end = start + 25 * 60_000;
        insert_interval(&conn, start, end, "App", "x", Category::Productive, "", 10.0, "app", "test", false);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM activity_intervals", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2, "midnight-crossing interval splits into per-date rows");
        let dates: Vec<String> = {
            let mut stmt = conn.prepare("SELECT local_date FROM activity_intervals ORDER BY started_at").unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
        };
        assert_ne!(dates[0], dates[1]);
        let total: f64 = conn
            .query_row("SELECT SUM((ended_at - started_at)) / 60000.0 FROM activity_intervals", [], |r| r.get(0))
            .unwrap();
        assert!((total - 25.0).abs() < 1e-6, "split preserves total duration");
    }

    /// Drives the full browser-attribution pipeline: foreground switch into a browser,
    /// domain hints from the extension, and the tick that must split the open browser
    /// row when the active site changes. Mirrors the UAT four-site scenario.
    /// Note: close_open drops rows shorter than 5s as noise, so the test uses one real
    /// dwell to verify persistence of a completed site row.
    #[test]
    fn browser_domain_hints_attribute_and_split_intervals() {
        let (conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Active);
        let inner = h.inner.clone();
        // The service loop flips the tracker to Observing once tracking is active.
        on_poll(&inner, &conn);

        // User focuses Chrome (no hint yet): a generic browser row opens, no domain.
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "chrome".into(), title: "whatever".into(), full_path: Some("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe".into()),
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 1, "one open Chrome row");
        assert_eq!(rows[0].2, "app", "no domain yet: kind stays app");
        assert_eq!(rows[0].3, "unclassified", "no domain yet: unclassified");

        // Navigates to ChatGPT: fresh hint lands, the next tick re-attributes.
        // (The sub-5s generic row is dropped as noise by design.)
        h.set_domain_hint("chatgpt.com".into());
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(100), foreground: Some(("chrome".into(), "ChatGPT".into(), None)),
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 1, "attributed row replaced the sub-5s generic row");
        assert_eq!((rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2.as_str(), rows[0].3.as_str()),
            ("Google Chrome", "ChatGPT", "website", "productive"),
            "catalog domain â†’ service label, website kind, productive; app identity retained");
        assert_eq!(rows[0].4, 1, "attributed row is open");

        // Dwells on ChatGPT past the noise threshold, then switches to YouTube
        // (catalog Neutral): the completed ChatGPT row is kept closed, a YouTube row opens.
        std::thread::sleep(Duration::from_millis(5_200));
        h.set_domain_hint("youtube.com".into());
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(100), foreground: Some(("chrome".into(), "YouTube".into(), None)),
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 2, "completed site row kept, new site row open");
        assert_eq!((rows[0].1.as_str(), rows[0].2.as_str(), rows[0].3.as_str()), ("YouTube", "website", "neutral"));
        assert_eq!(rows[0].4, 1, "YouTube row is open");
        assert_eq!((rows[1].1.as_str(), rows[1].3.as_str(), rows[1].4), ("ChatGPT", "productive", 0), "ChatGPT row closed and persisted");

        // Rapid Instagram and Twitch switches: each hint splits the open row immediately.
        h.set_domain_hint("instagram.com".into());
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(100), foreground: Some(("chrome".into(), "x".into(), None)),
        });
        h.set_domain_hint("twitch.tv".into());
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(100), foreground: Some(("chrome".into(), "x".into(), None)),
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 2, "Twitch row open; rapid YouTube/Instagram rows dropped as sub-5s noise; the dwelled ChatGPT row kept");
        assert_eq!((rows[0].1.as_str(), rows[0].3.as_str()), ("Twitch", "distracting"), "Twitch attributed");
        assert_eq!(rows[0].0, "Google Chrome", "parent application identity retained on website rows");

        // A hint must never bleed into a non-browser app: focusing the media player
        // with a fresh hint present opens an app row without any domain.
        h.set_domain_hint("youtube.com".into());
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "mpc-hc64".into(), title: "video.mkv".into(), full_path: None,
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].0.as_str(), rows[0].2.as_str()), ("mpc-hc64", "app"), "no domain attribution on non-browser apps");
    }

    /* ---- V1: schedule windows, idle exceptions ---- */

    fn settings_with_schedule(schedule_enabled: bool, start: &str, end: &str, days: &str) -> Settings {
        Settings {
            schedule_enabled,
            schedule_start: start.into(),
            schedule_end: end.into(),
            schedule_days: days.into(),
            ..Settings::default()
        }
    }

    #[test]
    fn schedule_window_logic() {
        let at = |h: u32, m: u32| Local::now().date_naive().and_hms_opt(h, m, 0).unwrap().and_local_timezone(Local).unwrap();
        let s = settings_with_schedule(true, "09:00", "17:30", "Mon,Tue,Wed,Thu,Fri");
        // Inside the window on a selected weekday.
        let t = at(12, 0);
        let mut allowed_mid = schedule_allows(&s, &t);
        let weekday_selected = s.schedule_days.to_lowercase().contains(&t.format("%a").to_string().to_lowercase());
        assert_eq!(allowed_mid, weekday_selected);
        // Outside the window is never allowed on any day.
        allowed_mid = schedule_allows(&s, &at(3, 0));
        assert!(!allowed_mid);
        // Disabled schedule always allows.
        let off = settings_with_schedule(false, "09:00", "17:30", "");
        assert!(schedule_allows(&off, &at(3, 0)));
        // Malformed window never silently blocks tracking.
        let bad = settings_with_schedule(true, "9am", "17:30", "");
        assert!(schedule_allows(&bad, &at(3, 0)));
        // Overnight window covers both sides of midnight.
        let overnight = settings_with_schedule(true, "22:00", "06:00", "");
        assert!(schedule_allows(&overnight, &at(23, 30)));
        assert!(schedule_allows(&overnight, &at(5, 0)));
        assert!(!schedule_allows(&overnight, &at(14, 0)));
        // 24h window on selected days only.
        let all_day = settings_with_schedule(true, "00:00", "00:00", "Sat,Sun");
        let t = at(12, 0);
        let sel = ["sat", "sun"].contains(&t.format("%a").to_string().to_lowercase().as_str());
        assert_eq!(schedule_allows(&all_day, &t), sel);
    }

    #[test]
    fn idle_exception_keeps_meeting_app_active() {
        let (conn, h) = mem();
        {
            let c = conn.lock();
            c.execute("INSERT INTO idle_exceptions (label, created_at) VALUES ('zoom', 0)", []).unwrap();
        }
        set_tracking_state(&h, TrackingStateKind::Active);
        let inner = h.inner.clone();
        on_poll(&inner, &conn);
        // Production adapters send bare process names; the exception matches it.
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "Zoom".into(), title: "Weekly sync".into(), full_path: Some("C:\\Apps\\Zoom\\bin\\Zoom.exe".into()),
        });
        // Idle past the threshold: the exempted meeting app stays open, no idle row.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(20 * 60_000), foreground: Some(("Zoom".into(), "Weekly sync".into(), Some("C:\\Apps\\Zoom\\bin\\Zoom.exe".into()))),
        });
        let rows = query_rows(&conn);
        assert_eq!(rows.len(), 1, "no second row opened");
        assert_eq!(rows[0].0, "Zoom", "meeting app recorded under its name");
        assert_eq!(rows[0].4, 1, "interval stays open despite no input");
        // A non-exempt app under the same conditions enters idle instead.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(20 * 60_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        let idle_open = {
            let g = inner.lock();
            g.idle_open.is_some()
        };
        assert!(idle_open, "non-exempt foreground enters idle despite no input change");
    }

    #[test]
    fn label_matching_is_case_and_path_insensitive() {
        assert!(label_matches_target("Zoom", "C:\\Apps\\Zoom\\bin\\Zoom.exe"));
        assert!(label_matches_target("zoom.exe", "C:\\Apps\\Zoom\\bin\\Zoom.exe"));
        assert!(!label_matches_target("zoom", "msedge.exe"));
        assert!(label_matches_target("Microsoft Edge", "msedge.exe"));
    }

    #[test]
    fn schedule_off_pauses_observation_with_honest_gap() {
        let (conn, h) = mem();
        {
            let mut g = h.inner.lock();
            g.settings.schedule_enabled = true;
            g.settings.schedule_start = "00:00".into();
            g.settings.schedule_end = "23:59".into();
            g.settings.schedule_days = "Mon,Tue,Wed,Thu,Fri,Sat,Sun".into();
        }
        set_tracking_state(&h, TrackingStateKind::Active);
        let inner = h.inner.clone();
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "code".into(), title: "work".into(), full_path: None,
        });
        // Narrow the window so the current time falls outside it: the open interval
        // closes, observation pauses, and a schedule gap opens.
        let now = Local::now();
        let outside_h = (now.hour() + 2) % 24;
        {
            let mut g = h.inner.lock();
            g.settings.schedule_start = format!("{outside_h:02}:00");
            g.settings.schedule_end = format!("{outside_h:02}:59");
        }
        handle_event(&inner, &conn, PlatformEvent::Tick { idle_ms: Some(100), foreground: Some(("code".into(), "work".into(), None)) });
        {
            let g = inner.lock();
            assert_eq!(g.state, RunState::NotObserving, "observation paused outside the window");
            assert!(g.pause_started.is_some(), "schedule gap opened");
        }
        // Re-open the window (24h): the gap is written as an honest interval.
        {
            let mut g = h.inner.lock();
            g.settings.schedule_start = "00:00".into();
            g.settings.schedule_end = "23:59".into();
        }
        std::thread::sleep(Duration::from_millis(20)); // the gap must have positive duration
        handle_event(&inner, &conn, PlatformEvent::Tick { idle_ms: Some(100), foreground: Some(("code".into(), "work".into(), None)) });
        let gap: Option<(String, String)> = {
            let c = conn.lock();
            c.query_row("SELECT app, detail FROM activity_intervals WHERE category = 'gap'", [], |r| Ok((r.get(0)?, r.get(1)?))).ok()
        };
        let (app, detail) = gap.expect("schedule gap row written on re-entry");
        assert_eq!(app, "Tracking gap");
        assert_eq!(detail, "Outside tracking schedule");
    }

    #[test]
    fn idle_resume_reopens_same_app_and_emits_live_state() {
        let (conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Active);
        let inner = h.inner.clone();
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "code".into(), title: "work".into(), full_path: None,
        });
        std::thread::sleep(Duration::from_millis(20));
        // Go idle: the open interval closes and an idle period begins.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(20 * 60_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert!(g.idle_open.is_some());
            assert!(g.open.is_none());
        }
        std::thread::sleep(Duration::from_millis(20));
        // Input returns with the SAME foreground app: the idle interval closes,
        // a new interval opens for that app, and the live snapshot shows it.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(1_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert!(g.idle_open.is_none(), "idle period closed on input");
            assert!(g.open.is_some(), "same-app interval reopened after idle");
            assert_eq!(g.open.as_ref().unwrap().exe, "code");
            let v = current_activity_json(&g);
            assert!(v["activity"].is_object(), "live card has an activity again");
            assert_eq!(v["activity"]["app"].as_str().unwrap(), "Visual Studio Code");
        }
        // The database has: closed app interval, closed idle interval, new open interval.
        let c = conn.lock();
        let (app_closed, idle_rows, open_rows): (i64, i64, i64) = c
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM activity_intervals WHERE category != 'idle' AND open = 0),
                   (SELECT COUNT(*) FROM activity_intervals WHERE category = 'idle' AND open = 0),
                   (SELECT COUNT(*) FROM activity_intervals WHERE open = 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(app_closed, 0, "the 20ms pre-idle interval is dropped as sub-5s noise by design");
        assert_eq!(idle_rows, 1, "the idle period is recorded exactly once");
        assert_eq!(open_rows, 1, "the resumed interval is open");
    }

    #[test]
    fn focus_session_phase_machine_transitions_and_accounting() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        // 25-minute focus / 5-minute break; time is simulated by rewinding the
        // phase start rather than waiting (production presets unchanged).
        session_start(&h, "Test".into(), String::new(), 25, 5, true);
        assert_eq!(inner.lock().session.snapshot.phase, "focus");

        // Simulate the focus interval elapsing: rewind the phase start.
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 26 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        {
            let g = inner.lock();
            assert_eq!(g.session.snapshot.phase, "break", "automatic focus→break transition");
            assert_eq!(g.session.snapshot.focus_intervals, 1);
            assert!(!g.session.snapshot.break_expired);
        }

        // Simulate the break allowance expiring: rewind the break start.
        {
            let mut g = inner.lock();
            g.session.break_started_ms = now_ms() - 6 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert!(inner.lock().session.snapshot.break_expired, "break expiry detected");

        // Extend the break: allowance grows, expiry flag clears.
        session_extend_break(&h);
        assert!(!inner.lock().session.snapshot.break_expired);

        // Resume focus: the break is finalized with actual duration and overrun.
        let snap = session_resume_focus(&h);
        assert_eq!(snap.phase, "focus");
        assert_eq!(snap.focus_intervals, 2);
        {
            let g = inner.lock();
            assert_eq!(g.session.break_history.len(), 1, "one break recorded");
            let br = &g.session.break_history[0];
            assert_eq!(br.planned_ms, 5 * 60_000);
            assert_eq!(br.extensions_ms, 5 * 60_000);
            // The break "started" 6 min ago (rewound), so actual ≈ 6 min.
            // Allowance = 5 + 5 = 10 min. Actual < allowance → no overrun.
            assert!(br.actual_ms >= 5 * 60_000, "actual duration at least the elapsed time");
            assert_eq!(br.overrun_ms, 0, "no overrun when actual < allowance");
        }
        session_end(&h);
    }

    #[test]
    fn focus_session_break_overrun_accounting() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        // 25-min focus / 5-min break; the user takes a 25-minute break
        // (5 planned + 20 of unplanned overrun).
        session_start(&h, "Overrun".into(), String::new(), 25, 5, true);
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 26 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert_eq!(inner.lock().session.snapshot.phase, "break");

        // Simulate: the break started 25 minutes ago (5 min allowance, 20 min overdue).
        {
            let mut g = inner.lock();
            g.session.break_started_ms = now_ms() - 25 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert!(inner.lock().session.snapshot.break_expired);

        session_resume_focus(&h);
        {
            let g = inner.lock();
            let br = &g.session.break_history[0];
            assert_eq!(br.planned_ms, 5 * 60_000, "planned 5 minutes");
            assert_eq!(br.extensions_ms, 0, "no extensions");
            assert!(br.actual_ms >= 24 * 60_000, "actual ≈ 25 minutes");
            assert!(br.overrun_ms >= 19 * 60_000, "overrun ≈ 20 minutes (got {}ms)", br.overrun_ms);
        }
        session_end(&h);
    }

    #[test]
    fn focus_session_no_breaks_mode_stays_in_focus() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        session_start(&h, "Uninterrupted".into(), String::new(), 25, 5, false);
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 30 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert_eq!(inner.lock().session.snapshot.phase, "focus", "no automatic break when breaks are disabled");
        session_end(&h);
    }

    /// While manually paused the snapshot exposes the frozen countdown (the
    /// dial displays it); the value must not drift while paused and the
    /// resumed countdown must continue from it.
    #[test]
    fn focus_session_paused_snapshot_exposes_frozen_countdown() {
        let (_conn, h) = mem();

        session_start(&h, "Frozen".into(), String::new(), 25, 5, true);
        {
            let mut g = h.inner.lock();
            let t = now_ms() - 10 * 60_000;
            g.session.phase_started_ms = t;
            g.session.last_resume_ms = Some(t);
        }
        session_toggle(&h).unwrap(); // pause with ≈15 min remaining
        let snap1 = h.snapshot_session();
        assert_eq!(snap1.status, "paused");
        assert!(
            (snap1.phase_remaining_ms as f64 / 60_000.0 - 15.0).abs() < 0.5,
            "frozen ≈15 minutes exposed while paused, got {}",
            snap1.phase_remaining_ms as f64 / 60_000.0
        );
        std::thread::sleep(Duration::from_millis(30));
        let snap2 = h.snapshot_session();
        assert_eq!(snap1.phase_remaining_ms, snap2.phase_remaining_ms, "frozen value does not drift while paused");
        session_toggle(&h).unwrap(); // resume
        let snap3 = h.snapshot_session();
        assert!(
            (snap3.phase_remaining_ms as f64 / 60_000.0 - 15.0).abs() < 0.2,
            "countdown continues from the frozen value after resume"
        );
        session_end(&h);
    }

    /// Break totals across the session lifecycle: clear zeros before the first
    /// break, live-updating totals (count / actual duration / overrun) while a
    /// break runs, exact hand-off when the break is finalized (no double
    /// counting), and stable values after the session ends.
    #[test]
    fn focus_session_snapshot_break_totals_lifecycle() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        session_start(&h, "Breaks".into(), String::new(), 25, 5, true);
        let s0 = h.snapshot_session();
        assert_eq!((s0.break_count, s0.break_total_min as i64, s0.break_overrun_total_min as i64), (0, 0, 0),
            "zeros displayed before the first break");

        // Force the focus→break transition: the running break counts as taken.
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 26 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        // Give the running break 30 seconds of wall clock (the transition
        // itself starts at ~0).
        {
            let mut g = inner.lock();
            g.session.break_started_ms = now_ms() - 30_000;
        }
        let s1 = h.snapshot_session();
        assert_eq!(s1.break_count, 1, "the running break is counted as taken");
        assert!((s1.break_total_min - 0.5).abs() < 0.05, "actual duration accumulates live");
        assert_eq!(s1.break_overrun_total_min as i64, 0, "no overrun inside the allowance");

        // 30 minutes ago with a 5-minute allowance → 25 minutes of live overrun.
        {
            let mut g = inner.lock();
            g.session.break_started_ms = now_ms() - 30 * 60_000;
        }
        let s2 = h.snapshot_session();
        assert!((s2.break_total_min - 30.0).abs() < 0.5, "total includes the running break's actual time");
        assert!((s2.break_overrun_total_min - 25.0).abs() < 0.5, "live overrun beyond allowance");

        // Resume focus: the break is finalized exactly once — the totals hand
        // over without double counting.
        session_resume_focus(&h);
        let s3 = h.snapshot_session();
        assert_eq!(s3.break_count, 1, "completed break still counted once");
        assert!((s3.break_total_min - s2.break_total_min).abs() < 0.01, "no double count after finalize");
        assert!((s3.break_overrun_total_min - s2.break_overrun_total_min).abs() < 0.01);

        // Session end: values stable.
        session_end(&h);
        let s4 = h.snapshot_session();
        assert_eq!(s4.break_count, 1);
        assert!((s4.break_total_min - s3.break_total_min).abs() < 0.01);
    }

    /// The acceptance example: 20-minute focus, 8 active minutes, then the user
    /// leaves. The countdown freezes at the authoritative last input (≈12 min
    /// remaining), stays frozen after the user returns, never triggers the
    /// scheduled break while paused, and an explicit Resume Focus continues
    /// from the preserved remaining time.
    #[test]
    fn focus_session_idle_pauses_countdown_and_waits_for_explicit_resume() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        set_tracking_state(&h, TrackingStateKind::Active);
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "code".into(), title: "work".into(), full_path: None,
        });
        session_start(&h, "Deep work".into(), String::new(), 20, 5, true);
        // The interval began 25 minutes ago: 8 active minutes, then the user
        // left 17 minutes ago (rewind the phase start to the coherent T0).
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 25 * 60_000;
        }
        // The user left 17 minutes ago; idle detection fires with the
        // authoritative idle duration.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(17 * 60_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert!(g.session.idle_paused_ms > 0, "focus countdown froze when idle began");
            let snap = g.session_snapshot();
            assert!(snap.focus_idle_paused, "snapshot reports the idle pause");
            let remaining_min = snap.phase_remaining_ms as f64 / 60_000.0;
            assert!(
                (remaining_min - 12.0).abs() < 0.5,
                "≈12 minutes preserved (8 of 20 consumed), got {remaining_min}"
            );
        }
        // The user returns (input resumes): the idle period closes but the
        // countdown STAYS paused at the same remaining time.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(1_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert!(g.idle_open.is_none(), "idle period closed on input");
            assert!(g.session.idle_paused_ms > 0, "countdown stays paused after return");
            let snap = g.session_snapshot();
            assert!(snap.focus_idle_paused);
            assert!((snap.phase_remaining_ms as f64 / 60_000.0 - 12.0).abs() < 0.5, "remaining time unchanged");
        }
        // Wall clock long past the phase end while frozen: no scheduled break.
        {
            let mut g = inner.lock();
            // Phase began 60 min ago; frozen at minute 8. Real now is far past
            // the nominal phase end.
            g.session.phase_started_ms = now_ms() - 60 * 60_000;
            g.session.idle_paused_ms = now_ms() - 52 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        {
            let g = inner.lock();
            assert_eq!(g.session.snapshot.phase, "focus", "no automatic break while idle-paused");
            assert_eq!(g.session.snapshot.focus_intervals, 0);
        }
        // Explicit Resume Focus: continues the SAME interval from the frozen
        // remaining time; the interval is not counted complete, not restarted.
        let snap = session_resume_focus(&h);
        assert_eq!(snap.phase, "focus");
        assert_eq!(snap.focus_intervals, 0, "resumed interval was not completed");
        assert!(!snap.focus_idle_paused);
        assert!(
            (snap.phase_remaining_ms as f64 / 60_000.0 - 12.0).abs() < 0.5,
            "countdown continues from ≈12 minutes, got {}",
            snap.phase_remaining_ms as f64 / 60_000.0
        );
        // The idle time itself is recorded by the ordinary engine.
        let idle_rows: i64 = {
            let c = conn.lock();
            c.query_row("SELECT COUNT(*) FROM activity_intervals WHERE category = 'idle'", [], |r| r.get(0)).unwrap()
        };
        assert_eq!(idle_rows, 1, "the away period is recorded as idle exactly once");
        session_end(&h);
    }

    /// Repeated manual pause/resume cycles inside one focus interval must
    /// preserve the exact remaining time (the pre-1.3.1 code shifted the phase
    /// start at pause by the whole run segment, refunding focus time on every
    /// cycle). Verified end-to-end across two real pause/resume pairs: the
    /// countdown after the last resume still shows the frozen ≈15 minutes.
    #[test]
    fn focus_session_manual_pause_multicycle_preserves_countdown() {
        let (_conn, h) = mem();

        session_start(&h, "Cycles".into(), String::new(), 25, 5, true);
        // 10 active minutes consumed, then the user pauses: remaining ≈ 15.
        {
            let mut g = h.inner.lock();
            let t = now_ms() - 10 * 60_000;
            g.session.phase_started_ms = t;
            g.session.last_resume_ms = Some(t);
        }
        session_toggle(&h).unwrap(); // pause
        let anchor_at_pause = h.inner.lock().session.phase_started_ms;
        // The pause branch must not move the phase anchor (the old code
        // shifted it forward by the run segment — the refund defect).
        assert_eq!(
            h.inner.lock().session.phase_started_ms, anchor_at_pause,
            "pause freezes the countdown; it never refunds the run segment"
        );

        // Two full pause/resume pairs with real (short) pause durations. The
        // refund defect would add the run segment back on every pause, pushing
        // the remaining time toward the full 25 minutes.
        session_toggle(&h).unwrap(); // resume (pair 1)
        session_toggle(&h).unwrap(); // pause
        std::thread::sleep(Duration::from_millis(30));
        session_toggle(&h).unwrap(); // resume (pair 2)

        let remaining_min = {
            let g = h.inner.lock();
            g.session_snapshot().phase_remaining_ms as f64 / 60_000.0
        };
        assert!(
            (remaining_min - 15.0).abs() < 0.2,
            "two pause/resume cycles preserve the ≈15 frozen minutes, got {remaining_min}"
        );
        session_end(&h);
    }

    /// Idle during an already-started scheduled break: break accounting and
    /// expiry continue normally (an idle pause never applies to breaks).
    #[test]
    fn focus_session_break_continues_during_idle() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        set_tracking_state(&h, TrackingStateKind::Active);
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "code".into(), title: "work".into(), full_path: None,
        });
        session_start(&h, "Break idle".into(), String::new(), 25, 5, true);
        {
            let mut g = inner.lock();
            g.session.phase_started_ms = now_ms() - 26 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert_eq!(inner.lock().session.snapshot.phase, "break");
        // The user goes idle while on break: no focus freeze, the break keeps
        // its allowance clock.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(30 * 60_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert_eq!(g.session.idle_paused_ms, 0, "idle pause never applies to the break phase");
            assert!(!g.session_snapshot().focus_idle_paused);
        }
        {
            let mut g = inner.lock();
            g.session.break_started_ms = now_ms() - 30 * 60_000;
        }
        check_session_phase(&inner, &conn, &None);
        assert!(inner.lock().session.snapshot.break_expired, "break expiry still detected during idle");
        session_resume_focus(&h);
        {
            let g = inner.lock();
            let br = &g.session.break_history[0];
            assert_eq!(br.planned_ms, 5 * 60_000);
            assert!(br.actual_ms >= 29 * 60_000, "actual ≈ 30 min");
            assert!(br.overrun_ms >= 24 * 60_000, "overrun ≈ 25 min");
        }
        session_end(&h);
    }

    /// A tracker outage (missed ticks > 15 min) is away-time for the focus
    /// countdown too: it freezes at the outage start and cannot expire while
    /// unobserved.
    #[test]
    fn focus_session_discontinuity_freezes_countdown() {
        let (conn, h) = mem();
        let inner = h.inner.clone();

        set_tracking_state(&h, TrackingStateKind::Active);
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "code".into(), title: "work".into(), full_path: None,
        });
        session_start(&h, "Outage".into(), String::new(), 20, 5, true);
        {
            let mut g = inner.lock();
            // The interval began 25 min ago; 5 active minutes, then the tracker
            // saw nothing 20 min ago (the outage began after the phase started).
            g.session.phase_started_ms = now_ms() - 25 * 60_000;
            g.last_event_ts = now_ms() - 20 * 60_000;
        }
        // The next platform event discovers the discontinuity.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(1_000), foreground: Some(("code".into(), "work".into(), None)),
        });
        {
            let g = inner.lock();
            assert!(g.session.idle_paused_ms > 0, "countdown froze at the outage start");
            let snap = g.session_snapshot();
            // Freeze clamped to the phase start (the outage began before the
            // phase did): 5 of 20 minutes consumed → ≈15 remaining.
            assert!(
                (snap.phase_remaining_ms as f64 / 60_000.0 - 15.0).abs() < 0.5,
                "got {}",
                snap.phase_remaining_ms as f64 / 60_000.0
            );
        }
        let gaps: i64 = {
            let c = conn.lock();
            c.query_row("SELECT COUNT(*) FROM activity_intervals WHERE category = 'gap'", [], |r| r.get(0)).unwrap()
        };
        assert_eq!(gaps, 1, "the outage is still recorded as an honest gap");
        session_end(&h);
    }

    #[test]
    fn current_activity_snapshot_is_privacy_safe_and_live() {
        let (conn, h) = mem();
        set_tracking_state(&h, TrackingStateKind::Active);
        let inner = h.inner.clone();
        on_poll(&inner, &conn);
        handle_event(&inner, &conn, PlatformEvent::Foreground {
            exe: "chrome".into(), title: "Secret search - Google Chrome".into(), full_path: None,
        });
        // Live snapshot during the open interval: app identity, category, start.
        {
            let g = inner.lock();
            let v = current_activity_json(&g);
            let act = &v["activity"];
            assert!(act.is_object());
            assert_eq!(act["app"].as_str().unwrap(), "Google Chrome");
            assert!(act["startMs"].as_i64().unwrap() > 0);
            assert_eq!(v["idle"].as_bool().unwrap(), false);
        }
        // Idle begins: activity goes null and the idle flag is set.
        handle_event(&inner, &conn, PlatformEvent::Tick {
            idle_ms: Some(20 * 60_000), foreground: Some(("chrome".into(), "whatever".into(), None)),
        });
        {
            let g = inner.lock();
            let v = current_activity_json(&g);
            assert!(v["activity"].is_null(), "no live activity while idle");
            assert_eq!(v["idle"].as_bool().unwrap(), true);
        }
        // Paused: no live activity, not idle.
        set_tracking_state(&h, TrackingStateKind::Paused);
        {
            let g = inner.lock();
            let v = current_activity_json(&g);
            assert!(v["activity"].is_null());
            assert_eq!(v["idle"].as_bool().unwrap(), false);
        }
    }

    /// id | app | detail | category | open  â€” newest first.
    fn query_rows(conn: &Arc<Mutex<Connection>>) -> Vec<(String, String, String, String, i64)> {
        let c = conn.lock();
        let mut stmt = c.prepare("SELECT app, detail, kind, category, open FROM activity_intervals ORDER BY id DESC").unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
pub fn start(conn: Arc<Mutex<Connection>>, _app_data: PathBuf, app: Option<tauri::AppHandle>) -> Arc<TrackerHandle> {
    let (tx, rx) = channel::<PlatformEvent>();
    let stop = Arc::new(AtomicBool::new(false));

    // ---- recovery-time initialization (single db lock scope) ----
    let inner = {
        let c = conn.lock();
        let persisted = recovered_state(db::get_tracking(&c), {
            let clean: Option<String> = c
                .query_row("SELECT value FROM meta WHERE key='clean_shutdown'", [], |r| r.get(0))
                .ok();
            clean.as_deref() == Some("1")
        });
        let _ = c.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('clean_shutdown', '0')", []);

        // Close intervals left open by an unclean exit; the outage becomes an honest gap.
        let stale: Vec<i64> = {
            let mut stmt = c.prepare("SELECT id FROM activity_intervals WHERE open = 1").unwrap();
            stmt.query_map([], |r| r.get::<_, i64>(0)).unwrap().filter_map(|x| x.ok()).collect()
        };
        for id in stale {
            let started: Option<i64> = c
                .query_row("SELECT started_at FROM activity_intervals WHERE id = ?1", [id], |r| r.get(0))
                .ok();
            if let Some(started) = started {
                let _ = c.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
                insert_interval(
                    &c, started, now_ms(), "Tracking gap", "Tracking interruption Â· unexpected shutdown",
                    Category::Gap, "", 0.0, "gap", "System event", false,
                );
            }
        }
        let _ = c.execute("UPDATE focus_sessions SET open = 0, ended_at = started_at + elapsed_ms WHERE open = 1", []);

        // Consistency pass: recovery gap rows fully contained in an idle interval are
        // subsumed by it (the user was away; the outage adds no information).
        let _ = c.execute(
            "DELETE FROM activity_intervals WHERE category = 'gap' AND EXISTS (
                SELECT 1 FROM activity_intervals i
                WHERE i.category = 'idle' AND i.id != activity_intervals.id
                  AND i.started_at <= activity_intervals.started_at
                  AND i.ended_at >= activity_intervals.ended_at)",
            [],
        );
        // Self-heal duplicate/contained idle rows from older versions or manual restores:
        // keep the larger interval, drop rows fully contained in a same-category sibling.
        let _ = c.execute(
            "DELETE FROM activity_intervals WHERE category = 'idle' AND EXISTS (
                SELECT 1 FROM activity_intervals i
                WHERE i.category = 'idle' AND i.id != activity_intervals.id
                  AND i.started_at <= activity_intervals.started_at
                  AND i.ended_at >= activity_intervals.ended_at
                  AND ((i.ended_at - i.started_at) > (activity_intervals.ended_at - activity_intervals.started_at)
                       OR (i.ended_at - i.started_at) = (activity_intervals.ended_at - activity_intervals.started_at) AND i.id < activity_intervals.id))",
            [],
        );

        let undo_depth = c
            .query_row("SELECT COUNT(*) FROM undo_snapshots", [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .unwrap_or(0);

        let settings = db::get_settings(&c);
        TrackerInner {
            tracking: persisted,
            state: RunState::NotObserving,
            current_app: None,
            open: None,
            idle_open: None,
            pause_started: None,
            last_event_ts: now_ms(),
            last_activity_input_ts: now_ms(),
            session: SessionState {
                snapshot: SessionSnapshot::default(),
                db_id: None,
                started_ms: 0,
                accumulated_ms: 0,
                last_resume_ms: None,
                last_persist_ms: 0,
                phase_started_ms: 0,
                break_started_ms: 0,
                break_extensions_ms: 0,
                break_history: Vec::new(),
                break_total_ms: 0,
                break_planned_total_ms: 0,
                break_extensions_total_ms: 0,
                break_overrun_total_ms: 0,
                idle_paused_ms: 0,
                manual_paused_ms: 0,
            },
            current_domain: None,
            last_domain_hint_ts: 0,
            productive_run: None,
            last_block_notice_ms: 0,
            dnd: false,
            settings,
            undo_depth,
        }
    };

    let inner = Arc::new(Mutex::new(inner));
    let handle = Arc::new(TrackerHandle {
        inner: inner.clone(),
        stop: stop.clone(),
        handles: Mutex::new(Vec::new()),
        started: Instant::now(),
        conn: conn.clone(),
    });

    let mut threads = Vec::new();
    {
        let tx = tx.clone();
        let stop = stop.clone();
        let app = app.clone();
        let adapter: fn(Option<tauri::AppHandle>, Sender<PlatformEvent>, Arc<AtomicBool>) = platform::run;
        threads.push(
            std::thread::Builder::new().name("chronos-platform".into())
                .spawn(move || adapter(app, tx, stop)).expect("platform thread"),
        );
    }
    {
        let stop = stop.clone();
        let inner = inner.clone();
        let conn = conn.clone();
        let app = app.clone();
        threads.push(
            std::thread::Builder::new().name("chronos-service".into())
                .spawn(move || service_loop(rx, inner, conn, stop, app)).expect("service thread"),
        );
    }
    *handle.handles.lock() = threads;
    handle
}

fn service_loop(rx: Receiver<PlatformEvent>, inner: Arc<Mutex<TrackerInner>>, conn: Arc<Mutex<Connection>>, stop: Arc<AtomicBool>, app: Option<tauri::AppHandle>) {
    let mut last_tick = Instant::now();
    let mut last_second = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        for _ in 0..256 {
            match rx.try_recv() {
                Ok(ev) => handle_event(&inner, &conn, ev),
                Err(_) => break,
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(1500) {
            last_tick = Instant::now();
            on_poll(&inner, &conn);
        }
        if last_second.elapsed() >= Duration::from_secs(1) {
            last_second = Instant::now();
            // Focus/break phase machine: automatic focus→break transitions and
            // break-expiry detection (the user's resume/extend choice is never
            // automatic).
            check_session_phase(&inner, &conn, &app);
            let (snapshot, persist) = {
                let g = inner.lock();
                let running = g.session.snapshot.status == "running";
                (g.session_snapshot(), running && now_ms() - g.session.last_persist_ms > 60_000)
            };
            if snapshot.status == "running" || snapshot.status == "paused" {
                emit_event("session-tick", &serde_json::to_string(&snapshot).unwrap_or_default());
            }
            if persist {
                let mut g = inner.lock();
                g.session.last_persist_ms = now_ms();
                persist_session(&g, &conn, false);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Clean shutdown.
    {
        let mut g = inner.lock();
        close_open(&mut g, &conn, now_ms());
        persist_session(&g, &conn, true);
    }
    let _ = conn.lock().execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('clean_shutdown', '1')", []);
}

fn observing(g: &TrackerInner) -> bool {
    matches!(g.tracking.state, TrackingStateKind::Active | TrackingStateKind::Degraded)
        && g.state == RunState::Observing
        && g.idle_open.is_none()
}

fn collectible(g: &TrackerInner) -> bool {
    !matches!(g.tracking.state, TrackingStateKind::Privacy)
}

fn handle_event(inner: &Arc<Mutex<TrackerInner>>, conn: &Arc<Mutex<Connection>>, ev: PlatformEvent) {
    let mut g = inner.lock();
    let now = now_ms();
    let since_last = now - g.last_event_ts;
    g.last_event_ts = now;

    // Wall-clock discontinuity: missed ticks (sleep without power events, tracker stall).
    if since_last > 15 * 60_000 && matches!(g.tracking.state, TrackingStateKind::Active | TrackingStateKind::Degraded) {
        let gap_start = now - since_last;
        close_open(&mut g, conn, gap_start);
        g.productive_run = None; // an outage breaks sustained focus
        if g.idle_open.is_none() {
            insert_interval(&conn.lock(), gap_start, now, "Tracking gap", "Tracking interruption", Category::Gap, "", 0.0, "gap", "System event", false);
        }
        // An unobserved outage is away-time for the focus countdown too: freeze
        // it at the outage start (the last event the tracker actually saw),
        // exactly like an idle period, so it cannot expire while unobserved.
        if g.session.snapshot.status == "running"
            && g.session.snapshot.phase == "focus"
            && g.session.idle_paused_ms == 0
            && gap_start > g.session.phase_started_ms
        {
            g.session.idle_paused_ms = gap_start;
        }
        g.state = RunState::Observing;
    }

    match ev {
        PlatformEvent::Foreground { exe, title, full_path } => {
            let changed = g.current_app.as_ref().map(|(e, _)| !e.eq_ignore_ascii_case(&exe)).unwrap_or(true);
            g.current_app = Some((exe.clone(), title.clone()));
            if changed && observing(&g) && collectible(&g) {
                begin_app_interval(&mut g, conn, now, &exe, &title, full_path.as_deref());
            }
        }
        PlatformEvent::Tick { idle_ms, foreground } => {
            // Optional tracking schedule: outside the window, observation pauses
            // with an honest gap row. Manual Pause/Privacy remain authoritative â€”
            // those tracking states never re-enter observation here.
            let allowed = schedule_allows(&g.settings, &Local::now());
            if !allowed && observing_state(&g.tracking.state) {
                if g.state == RunState::Observing {
                    close_open(&mut g, conn, now);
                    g.productive_run = None; // schedule boundary breaks sustained focus
                    g.state = RunState::NotObserving;
                }
                if g.pause_started.is_none() {
                    g.pause_started = Some((now, SCHEDULE_REASON));
                }
            } else if allowed && matches!(g.pause_started, Some((_, SCHEDULE_REASON))) {
                let (started, _) = g.pause_started.take().expect("checked above");
                insert_interval(
                    &conn.lock(),
                    started,
                    now,
                    "Tracking gap",
                    SCHEDULE_REASON,
                    Category::Gap,
                    "",
                    0.0,
                    "gap",
                    "Tracking schedule",
                    false,
                );
                if observing_state(&g.tracking.state) {
                    g.state = RunState::Observing;
                }
            }
            if !allowed {
                return; // nothing is observed outside the schedule window
            }
            if let Some((exe, title, full_path)) = foreground {
                let changed = g.current_app.as_ref().map(|(e, _)| !e.eq_ignore_ascii_case(&exe)).unwrap_or(true);
                g.current_app = Some((exe.clone(), title.clone()));
                // Optional distraction discouragement (native applications): the
                // blocked window is minimized â€” reversible, narrowly scoped, and
                // honestly reported. Website blocking lives in the browser extension.
                if g.settings.blocking && block_target_active(conn, &exe) {
                    platform::discourage_foreground();
                    if now - g.last_block_notice_ms > 600_000 {
                        g.last_block_notice_ms = now;
                        emit_event(
                            "notice",
                            &serde_json::json!({
                                "type": "block",
                                "title": "Blocked by CHRONOS",
                                "body": format!("{} is on your distraction block list â€” the window was minimized.", display_name(&exe)),
                            })
                            .to_string(),
                        );
                    }
                    close_open(&mut g, conn, now); // a discouraged app accrues no recorded time
                    return;
                }
                if changed && observing(&g) && collectible(&g) {
                    begin_app_interval(&mut g, conn, now, &exe, &title, full_path.as_deref());
                }
                // A fresh domain hint that differs from the open browser row's domain means
                // the user navigated to a different site: split the interval so no site's
                // classification is carried into unrelated browsing.
                if let Some(fresh) = g.current_domain.clone().filter(|(_, ts)| now - *ts < 8_000) {
                    if let Some(open) = &g.open {
                        if open.is_browser && open.domain.as_deref() != Some(fresh.0.as_str()) {
                            close_open(&mut g, conn, now);
                            if observing(&g) && collectible(&g) {
                                begin_app_interval(&mut g, conn, now, &exe, &title, full_path.as_deref());
                            }
                        }
                    }
                }
            }
            if let Some(idle) = idle_ms {
                g.last_activity_input_ts = now.saturating_sub(idle as i64);
                handle_idle(&mut g, conn, now, idle as i64);
            }
        }
        PlatformEvent::SystemSleep => {
            close_open(&mut g, conn, now);
            g.productive_run = None; // sleep breaks sustained focus
            if matches!(g.tracking.state, TrackingStateKind::Active | TrackingStateKind::Degraded) {
                // Keep the earliest pause start: a schedule-off period that slid into
                // sleep must not shorten the honest gap it already opened.
                g.pause_started = g.pause_started.or(Some((now, "System asleep")));
            }
            emit_current_activity(&g);
            if g.session.snapshot.status == "running" {
                pause_session_inner(&mut g);
            }
            g.state = RunState::NotObserving;
        }
        PlatformEvent::SystemWake => {
            if let Some((started, reason)) = g.pause_started.take() {
                insert_interval(&conn.lock(), started, now, "Tracking gap", reason, Category::Gap, "", 0.0, "gap", "System event", false);
            }
            g.state = if observing_state(&g.tracking.state) { RunState::Observing } else { RunState::NotObserving };
        }
        PlatformEvent::Locked => {
            close_open(&mut g, conn, now);
            g.productive_run = None; // locking the session means stepping away
        }
        PlatformEvent::Unlocked => {}
        PlatformEvent::SourceLost(reason) => {
            close_open(&mut g, conn, now);
            if observing_state(&g.tracking.state) {
                g.tracking.state = TrackingStateKind::Degraded;
                g.tracking.degraded_reason = Some(reason);
                persist_tracking(&g, conn);
                emit_event("tracking-changed", &serde_json::to_string(&g.tracking).unwrap_or_default());
            }
        }
    }
}

pub fn observing_state(s: &TrackingStateKind) -> bool {
    matches!(s, TrackingStateKind::Active | TrackingStateKind::Degraded)
}

/* ---------------- schedule, matching, and discouragement ---------------- */

const SCHEDULE_REASON: &str = "Outside tracking schedule";

/// The live-activity view model shared by the `get_current_activity` command
/// and the `current-activity` event. Privacy-safe by construction: application
/// identity or a curated service label â€” never a window title.
fn current_activity_json(g: &TrackerInner) -> serde_json::Value {
    let observing = observing_state(&g.tracking.state) && g.idle_open.is_none();
    let activity = if observing {
        g.open.as_ref().map(|o| {
            let app = display_name(&o.exe);
            let name = if o.is_browser {
                o.domain
                    .as_deref()
                    .and_then(classifier::domain_label)
                    .or(o.domain.as_deref())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| app.clone())
            } else {
                app.clone()
            };
            serde_json::json!({ "name": name, "app": app, "cat": o.cat.as_str(), "startMs": o.start_ms, "isSite": o.is_browser })
        })
    } else {
        None
    };
    serde_json::json!({
        "tracking": g.tracking.state.as_str(),
        "activity": activity,
        "idle": g.idle_open.is_some(),
        "nowMs": crate::db::now_ms(),
    })
}

/// Notify the UI that the live current activity changed (started, switched,
/// ended, idle began, or tracking state changed). The dashboard renders its
/// live card from this; the elapsed duration ticks locally from `startMs`.
fn emit_current_activity(g: &TrackerInner) {
    emit_event("current-activity", &current_activity_json(g).to_string());
}

fn parse_hhmm(v: &str) -> Option<i64> {
    let mut it = v.split(':');
    let h: i64 = it.next()?.trim().parse().ok()?;
    let m: i64 = it.next().unwrap_or("0").trim().parse().ok()?;
    if !(0..=24).contains(&h) || !(0..=59).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

fn prev_weekday(wd: &str) -> &'static str {
    match wd {
        "mon" => "sun",
        "tue" => "mon",
        "wed" => "tue",
        "thu" => "wed",
        "fri" => "thu",
        "sat" => "fri",
        "sun" => "sat",
        _ => "",
    }
}

/// True when the configured schedule permits observation at `now`. A disabled or
/// empty schedule always permits (tracking stays usable without scheduling).
/// Overnight windows (start > end) are supported: the pre-midnight part requires
/// the current weekday to be selected; the post-midnight spill also accepts the
/// previous day, so "Monâ€“Fri 22:00â€“06:00" covers Tue 00:00â€“05:59 after Mon night.
/// A malformed window never silently blocks tracking.
pub fn schedule_allows(s: &Settings, now: &chrono::DateTime<chrono::Local>) -> bool {
    if !s.schedule_enabled {
        return true;
    }
    let (Some(start), Some(end)) = (parse_hhmm(&s.schedule_start), parse_hhmm(&s.schedule_end)) else {
        return true;
    };
    let selected: Vec<String> = s
        .schedule_days
        .split(',')
        .map(|d| d.trim().to_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let weekday = now.format("%a").to_string().to_lowercase();
    let day_selected = selected.is_empty() || selected.iter().any(|d| d == &weekday);
    let prev_selected = selected.is_empty() || selected.iter().any(|d| *d == prev_weekday(&weekday));
    let mins = i64::from(now.hour()) * 60 + i64::from(now.minute());
    if start == end {
        return day_selected; // 24-hour window on selected days
    }
    let (in_pre, in_post) = if start < end {
        (mins >= start && mins < end, false)
    } else {
        (mins >= start, mins < end)
    };
    if in_pre {
        day_selected
    } else if in_post {
        day_selected || prev_selected
    } else {
        false
    }
}

/// Normalized process key for user-configured matching (idle exceptions, block
/// list): lowercased executable base name without extension.
fn proc_key(exe: &str) -> String {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    stem.trim().to_lowercase()
}

/// A user-entered label matches an executable when it equals the process key,
/// the executable file name, or the friendly display name (case-insensitive).
/// Accepts bare process names ("msedge"), file names ("msedge.exe"), and full
/// paths ("C:\...\msedge.exe") as the executable form.
pub fn label_matches_target(label: &str, exe: &str) -> bool {
    let l = label.trim().to_lowercase();
    if l.is_empty() {
        return false;
    }
    let key = proc_key(exe);
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    let display = display_name(&key).to_lowercase();
    l == key || l == file || l == display
}

/// Narrowly scoped idle exceptions (e.g. video meetings): when the foreground
/// application is explicitly exempted, absence of input is treated as continued
/// engagement â€” the app interval stays open and no idle row is written.
fn idle_exception_active(conn: &Arc<Mutex<Connection>>, exe: &str) -> bool {
    let c = conn.lock();
    let labels: Vec<String> = c
        .prepare("SELECT label FROM idle_exceptions")
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    drop(c);
    labels.iter().any(|l| label_matches_target(l, exe))
}

/// Distraction discouragement for native applications: true when the executable
/// matches an enabled app entry on the block list. (Website blocking is enforced
/// inside the browser extension; this never applies to it.)
fn block_target_active(conn: &Arc<Mutex<Connection>>, exe: &str) -> bool {
    let c = conn.lock();
    let targets: Vec<String> = c
        .prepare("SELECT target FROM block_list WHERE enabled = 1 AND kind = 'app'")
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    drop(c);
    targets.iter().any(|t| label_matches_target(t, exe))
}

fn handle_idle(g: &mut TrackerInner, conn: &Arc<Mutex<Connection>>, now: i64, idle_ms: i64) {
    if !g.settings.auto_idle || !collectible(g) || !observing_state(&g.tracking.state) {
        return;
    }
    let threshold = (g.settings.idle_minutes as i64) * 60_000;
    let active_input = idle_ms < threshold;
    if active_input {
        if let Some(idle_start) = g.idle_open.take() {
            insert_interval(&conn.lock(), idle_start, now, "Idle", "Away from keyboard", Category::Idle, "", 0.0, "idle", "Idle detection", false);
            // An idle period that spans a tracker outage subsumes it: remove any recovery
            // gap rows fully contained in this idle interval so the timeline never shows
            // overlapping representations of the same away period.
            {
                let c = conn.lock();
                let _ = c.execute(
                    "DELETE FROM activity_intervals WHERE category = 'gap' AND started_at >= ?1 AND ended_at <= ?2",
                    rusqlite::params![idle_start, now],
                );
            }
            if g.session.snapshot.status == "running" {
                g.session.snapshot.idle += (now - idle_start) as f64 / 60000.0;
            }
            // Input returned after an idle period: observation resumes. The
            // previously open interval was closed when idle began, so re-open
            // one for the (unchanged) foreground application â€” otherwise the
            // resumed activity is neither recorded nor shown live until the
            // user switches applications. Then announce the new live state.
            if g.open.is_none() {
                if let Some((exe, title)) = g.current_app.clone() {
                    if observing(g) && collectible(g) {
                        begin_app_interval(g, conn, now, &exe, &title, None);
                    }
                }
            }
            emit_current_activity(g);
        }
    } else if g.idle_open.is_none() {
        // Narrowly scoped idle exception: an explicitly exempted foreground app
        // (e.g. a video meeting) keeps its interval open â€” no input is not
        // evidence of disengagement for apps the user marked as such.
        if let Some((exe, _title)) = &g.current_app {
            if idle_exception_active(conn, exe) {
                return;
            }
        }
        let last_input = now.saturating_sub(idle_ms);
        close_open(g, conn, last_input);
        g.idle_open = Some(last_input);
        // Re-emit with the idle flag now set: the live card should say "Idle",
        // not merely show nothing. (close_open already announced the end of the
        // previous activity.)
        emit_current_activity(g);
        g.productive_run = None; // idle breaks sustained focus
        if g.session.snapshot.status == "running" {
            g.session.snapshot.interruptions += 1;
            // Focus Session idle pause: freeze the focus countdown at the
            // tracker's authoritative last-input moment (not detection time),
            // clamped to the phase start so a session begun while already idle
            // never reports more than the full interval. Scheduled breaks are
            // unaffected — an already-started break keeps its accounting. The
            // freeze survives the user's return; only an explicit Resume
            // Focus (or manual pause/resume) continues the countdown.
            if g.session.snapshot.phase == "focus" && g.session.idle_paused_ms == 0 {
                g.session.idle_paused_ms = last_input.max(g.session.phase_started_ms);
                let snap = g.session_snapshot();
                emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
            }
        }
    }
}

fn begin_app_interval(g: &mut TrackerInner, conn: &Arc<Mutex<Connection>>, now: i64, exe: &str, title: &str, full_path: Option<&str>) {
    if let Some(open) = &g.open {
        if open.exe.eq_ignore_ascii_case(exe) {
            return;
        }
        close_open(g, conn, now);
    }

    let domain = g
        .current_domain
        .as_ref()
        .filter(|(_, ts)| now - *ts < 8_000)
        .map(|(d, _)| d.clone());
    let dom = if is_browser(exe) { domain.as_deref() } else { None };
    let display = display_name(exe);
    // Steam gameplay evidence (local steamapps\common install path), Windows only.
    let steam_game = full_path.and_then(crate::steam::detect_game);

    let (cat, source, confidence, needs_review, excluded, project, project_source) = {
        let c = conn.lock();
        let rules = classifier::load_rules(&c);
        let exclusions = classifier::load_exclusions(&c);
        if classifier::is_excluded(exe, dom, &exclusions) || classifier::is_excluded(&display, dom, &exclusions) {
            (Category::Unclassified, String::new(), 0.0, false, true, String::new(), "")
        } else {
            let d = classifier::classify_with_steam_hint(&c, exe, &display, title, dom, &rules, steam_game.as_deref());
            let (project, project_source) = classifier::attribute_project(&c, exe, &display, dom, &rules);
            (d.cat, d.source, d.confidence, d.needs_review, false, project, project_source)
        }
    };
    if excluded {
        return; // excluded activity is never collected
    }
    let review = if needs_review { "pending" } else { "none" };
    let app = display;
    // Privacy enforcement happens before persistence: Discord, File Explorer, and browsers
    // never store raw window titles (application identity or curated service label only).
    // Catalog domains store their friendly service label; other domains store the domain.
    let detail = match dom {
        Some(d) => classifier::domain_label(d).unwrap_or(d).to_string(),
        None => classifier::privacy_safe_detail(exe, &app, title, None),
    };
    let kind = if dom.is_some() { "website" } else { "app" };
    {
        let c = conn.lock();
        let _ = c.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, confidence, open, project_source)
             VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, 0, ?9, ?10, 1, ?11)",
            rusqlite::params![now, local_date_of(now), app, detail, cat.as_str(), project, kind, source, review, confidence, project_source],
        );
    }
    // Recognizable icons: one-shot extraction for a newly observed native application.
    if dom.is_none() {
        if let Some(fp) = full_path {
            crate::app_icons::cache_app_icon(conn, &app, Some(std::path::Path::new(fp)));
        }
    }
    g.open = Some(OpenInterval {
        exe: exe.to_string(),
        start_ms: now,
        cat,
        is_browser: dom.is_some() || is_browser(exe),
        domain: dom.map(|d| d.to_string()),
    });
    // A new live activity has begun (app switch, site navigation, resume):
    // the dashboard's live card follows this without polling.
    emit_current_activity(g);

    if g.session.snapshot.status == "running" && cat == Category::Distracting {
        g.session.snapshot.interruptions += 1;
    }
}

fn close_open(g: &mut TrackerInner, conn: &Arc<Mutex<Connection>>, end_ms: i64) {
    let Some(open) = g.open.take() else { return };
    let c = conn.lock();
    let row_id: Option<i64> = c
        .query_row("SELECT id FROM activity_intervals WHERE open = 1 ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
        .ok();
    let Some(id) = row_id else { return };
    if end_ms <= open.start_ms {
        let _ = c.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
        return;
    }
    // Sub-second noise is dropped entirely.
    if end_ms - open.start_ms < 5_000 {
        let _ = c.execute("DELETE FROM activity_intervals WHERE id = ?1", [id]);
        return;
    }
    let _ = c.execute("UPDATE activity_intervals SET ended_at = ?2, open = 0 WHERE id = ?1", rusqlite::params![id, end_ms]);

    // A finalized observed interval is new information for the UI: Recent
    // Activity and the Timeline totals refresh from this. (Idle/gap rows and
    // user mutations already emitted; ordinary closes did not â€” the gap that
    // left the dashboard stale until navigation.)
    emit_event("activity-changed", &serde_json::json!({ "date": local_date_of(open.start_ms) }).to_string());
    // The open interval is gone: the live card must not keep showing it.
    emit_current_activity(g);

    let dur_min = (end_ms - open.start_ms) as f64 / 60000.0;

    // --- qualified focus: explicit-session overlap and sustained productive blocks ---
    let mut focus_to_write: f64 = 0.0;
    if g.session.snapshot.status == "running" && open.cat == Category::Productive {
        let session_start = g.session.started_ms;
        let overlap_start = open.start_ms.max(session_start);
        let overlap_min = (end_ms - overlap_start).max(0) as f64 / 60000.0;
        if overlap_min > 0.0 {
            let focus = overlap_min.min(dur_min);
            g.session.snapshot.qualified += focus;
            focus_to_write = focus_to_write.max(focus);
        }
    }
    let (block_focus, new_run, retroactive) = focus_block_decision(open.cat, dur_min, open.start_ms, g.productive_run);
    g.productive_run = new_run;
    focus_to_write = focus_to_write.max(block_focus);
    if focus_to_write > 0.0 {
        let _ = c.execute("UPDATE activity_intervals SET focus_minutes = ?2 WHERE id = ?1", rusqlite::params![id, focus_to_write]);
    }
    if retroactive {
        // The sustained block crossed the threshold: back-fill earlier run rows once.
        if let Some((run_start, _)) = g.productive_run {
            let _ = c.execute(
                "UPDATE activity_intervals SET focus_minutes = (ended_at - started_at) / 60000.0
                 WHERE category = 'productive' AND focus_minutes = 0 AND open = 0 AND started_at >= ?1 AND ended_at <= ?2",
                rusqlite::params![run_start, end_ms],
            );
        }
    }
}

/// Write an interval, splitting at local midnight so every row belongs to exactly one local date.
#[allow(clippy::too_many_arguments)]
pub fn insert_interval(
    c: &Connection,
    start_ms: i64,
    end_ms: i64,
    app: &str,
    detail: &str,
    cat: Category,
    project: &str,
    focus_minutes: f64,
    kind: &str,
    source: &str,
    corrected: bool,
) {
    if end_ms <= start_ms {
        return;
    }
    let mut segs: Vec<(i64, i64)> = Vec::new();
    let mut s = start_ms;
    for _ in 0..10 {
        let date = local_date_of(s);
        let next_bound = match local_midnight(&add_one_day(&date)) {
            Some(b) if b > s && end_ms > b => b,
            _ => {
                segs.push((s, end_ms));
                break;
            }
        };
        segs.push((s, next_bound));
        s = next_bound;
    }
    let total = (end_ms - start_ms) as f64;
    let mut remaining = focus_minutes;
    let count = segs.len();
    for (idx, (ss, ee)) in segs.iter().enumerate() {
        let seg_dur = (*ee - *ss) as f64;
        let f = if idx + 1 == count { remaining } else { focus_minutes * seg_dur / total };
        remaining -= f;
        let _ = c.execute(
            "INSERT INTO activity_intervals (started_at, ended_at, local_date, app, detail, category, project, focus_minutes, kind, source, corrected, review_status, confidence, open)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'none', NULL, 0)",
            rusqlite::params![ss, ee, local_date_of(*ss), app, detail, cat.as_str(), project, f, kind, source, if corrected { 1 } else { 0 }],
        );
    }
    emit_event("activity-changed", &serde_json::json!({ "date": local_date_of(start_ms) }).to_string());
}

fn add_one_day(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| (d + chrono::Duration::days(1)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.to_string())
}

pub fn display_name(exe: &str) -> String {
    let lower = exe.to_lowercase();
    const NAMES: &[(&str, &str)] = &[
        ("code", "Visual Studio Code"), ("code - insiders", "Visual Studio Code"), ("cursor", "Cursor"),
        ("msedge", "Microsoft Edge"), ("chrome", "Google Chrome"), ("firefox", "Firefox"),
        ("brave", "Brave"), ("zen", "Zen Browser"), ("chromium", "Chromium"),
        ("windowsterminal", "Terminal"), ("alacritty", "Alacritty"), ("kitty", "Kitty"), ("foot", "Foot Terminal"),
        ("slack", "Slack"), ("discord", "Discord"), ("msteams", "Microsoft Teams"), ("outlook", "Outlook"),
        ("thunderbird", "Thunderbird"), ("explorer", "File Explorer"), ("devenv", "Visual Studio"),
        ("idea64", "IntelliJ IDEA"), ("pycharm64", "PyCharm"), ("webstorm64", "WebStorm"), ("rustrover64", "RustRover"),
        ("figma", "Figma"), ("steam", "Steam"), ("steamwebhelper", "Steam"), ("spotify", "Spotify"),
        ("applicationframehost", "App"),
    ];
    for (k, v) in NAMES {
        if *k == lower {
            return (*v).to_string();
        }
    }
    exe.trim_end_matches(".exe").to_string()
}

fn is_browser(exe: &str) -> bool {
    const BROWSERS: &[&str] = &["chrome", "msedge", "firefox", "brave", "zen", "opera", "vivaldi", "chromium", "arc"];
    BROWSERS.contains(&exe.to_lowercase().as_str())
}

fn on_poll(inner: &Arc<Mutex<TrackerInner>>, _conn: &Arc<Mutex<Connection>>) {
    let mut g = inner.lock();
    if observing_state(&g.tracking.state) && g.state == RunState::NotObserving && g.pause_started.is_none() {
        g.state = RunState::Observing;
    }
}

fn persist_tracking(g: &TrackerInner, conn: &Arc<Mutex<Connection>>) {
    let c = conn.lock();
    let _ = db::set_tracking(&c, &g.tracking);
}

pub fn pause_session_inner(g: &mut TrackerInner) {
    if g.session.snapshot.status == "running" {
        let now = now_ms();
        let resume = g.session.last_resume_ms.unwrap_or(now);
        g.session.accumulated_ms += (now - resume).max(0);
        g.session.last_resume_ms = None;
        g.session.snapshot.status = "paused".into();
        // Freeze the phase countdown alongside the session: commit any idle
        // pause and record the manual pause start (applied as the actual
        // pause duration on resume).
        g.session.commit_idle_freeze(now);
        g.session.manual_paused_ms = now;
    }
}

pub fn persist_session(g: &TrackerInner, conn: &Arc<Mutex<Connection>>, final_write: bool) {
    let c = conn.lock();
    if let Some(id) = g.session.db_id {
        let elapsed_ms = if final_write && g.session.snapshot.status != "running" {
            g.session.accumulated_ms
        } else {
            match g.session.snapshot.status.as_str() {
                "running" => {
                    let now = now_ms();
                    let resume = g.session.last_resume_ms.unwrap_or(now);
                    g.session.accumulated_ms + (now - resume).max(0)
                }
                _ => g.session.accumulated_ms,
            }
        };
        let _ = c.execute(
            "UPDATE focus_sessions SET elapsed_ms = ?2, qualified_minutes = ?3, idle_minutes = ?4, interruptions = ?5, open = ?6, ended_at = ?7,
             focus_minutes = ?8, break_minutes = ?9, breaks_enabled = ?10, focus_intervals = ?11,
             break_total_ms = ?12, break_planned_ms = ?13, break_extensions_ms = ?14, break_overrun_ms = ?15, break_history = ?16
             WHERE id = ?1",
            rusqlite::params![
                id, elapsed_ms, g.session.snapshot.qualified, g.session.snapshot.idle, g.session.snapshot.interruptions,
                if final_write { 0 } else { 1 }, now_ms(),
                g.session.snapshot.focus_minutes, g.session.snapshot.break_minutes,
                if g.session.snapshot.breaks_enabled { 1 } else { 0 },
                g.session.snapshot.focus_intervals,
                g.session.break_total_ms, g.session.break_planned_total_ms,
                g.session.break_extensions_total_ms, g.session.break_overrun_total_ms,
                serde_json::to_string(&g.session.break_history).unwrap_or_else(|_| "[]".into()),
            ],
        );
    }
}

/* ---------------- session control ---------------- */

pub fn session_start(handle: &TrackerHandle, intention: String, project: String, focus_minutes: i64, break_minutes: i64, breaks_enabled: bool) -> SessionSnapshot {
    let mut g = handle.inner.lock();
    let now = now_ms();
    {
        let c = handle.conn.lock();
        let _ = c.execute(
            "INSERT INTO focus_sessions (started_at, intention, project, elapsed_ms, qualified_minutes, idle_minutes, interruptions, open, focus_minutes, break_minutes, breaks_enabled)
             VALUES (?1, ?2, ?3, 0, 0, 0, 0, 1, ?4, ?5, ?6)",
            rusqlite::params![now, intention, project, focus_minutes, break_minutes, if breaks_enabled { 1 } else { 0 }],
        );
        g.session.db_id = c.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).ok();
    }
    g.session.started_ms = now;
    g.session.accumulated_ms = 0;
    g.session.last_resume_ms = Some(now);
    g.session.last_persist_ms = now;
    g.session.phase_started_ms = now;
    g.session.break_started_ms = 0;
    g.session.break_extensions_ms = 0;
    g.session.break_history.clear();
    g.session.break_total_ms = 0;
    g.session.break_planned_total_ms = 0;
    g.session.break_extensions_total_ms = 0;
    g.session.break_overrun_total_ms = 0;
    g.session.idle_paused_ms = 0;
    g.session.manual_paused_ms = 0;
    g.session.snapshot = SessionSnapshot {
        status: "running".into(), elapsed: 0, qualified: 0.0, idle: 0.0, interruptions: 0, intention, project,
        phase: "focus".into(),
        phase_remaining_ms: focus_minutes * 60_000,
        break_expired: false,
        focus_idle_paused: false,
        focus_minutes, break_minutes, breaks_enabled,
        focus_intervals: 0, break_extensions_min: 0.0,
        break_count: 0, break_total_min: 0.0, break_overrun_total_min: 0.0,
    };
    g.session_snapshot()
}

pub fn session_toggle(handle: &TrackerHandle) -> Result<SessionSnapshot, String> {
    let mut g = handle.inner.lock();
    match g.session.snapshot.status.as_str() {
        "running" => {
            let now = now_ms();
            let resume = g.session.last_resume_ms.unwrap_or(now);
            g.session.accumulated_ms += (now - resume).max(0);
            g.session.last_resume_ms = None;
            g.session.snapshot.status = "paused".into();
            // Fold any idle pause into the phase anchor now; the manual pause
            // itself is applied on resume as the actual pause duration. (The
            // previous shift-at-pause-by-run-length refunded the whole run
            // segment on every cycle — pause/resume inside one phase now
            // preserves the exact remaining time.)
            g.session.commit_idle_freeze(now);
            g.session.manual_paused_ms = now;
        }
        "paused" => {
            let now = now_ms();
            if g.session.manual_paused_ms > 0 {
                let paused_for = (now - g.session.manual_paused_ms).max(0);
                g.session.phase_started_ms += paused_for;
                if g.session.break_started_ms > 0 {
                    g.session.break_started_ms += paused_for;
                }
            }
            g.session.manual_paused_ms = 0;
            g.session.idle_paused_ms = 0; // an explicit resume means the user is present
            g.session.last_resume_ms = Some(now);
            g.session.snapshot.status = "running".into();
        }
        _ => return Err("session ended".into()),
    }
    Ok(g.session_snapshot())
}

/// User chose "Resume Focus": after a break this finalizes the break record
/// with its actual wall-clock duration and overrun, then begins the next focus
/// interval. After an idle pause (focus phase still active) it continues the
/// SAME interval from the preserved remaining time — never restarts it.
pub fn session_resume_focus(handle: &TrackerHandle) -> SessionSnapshot {
    let mut g = handle.inner.lock();
    if g.session.snapshot.status == "ended" {
        return g.session_snapshot();
    }
    if g.session.snapshot.phase == "focus" {
        if g.session.idle_paused_ms > 0 {
            let now = now_ms();
            g.session.phase_started_ms += (now - g.session.idle_paused_ms).max(0);
            g.session.idle_paused_ms = 0;
            let snap = g.session_snapshot();
            persist_session(&g, &handle.conn, false);
            drop(g);
            emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
            return snap;
        }
        return g.session_snapshot();
    }
    if g.session.snapshot.phase != "break" {
        return g.session_snapshot();
    }
    let now = now_ms();
    finalize_break(&mut g, now);
    g.session.snapshot.focus_intervals += 1;
    g.session.phase_started_ms = now;
    g.session.snapshot.phase = "focus".into();
    g.session.snapshot.break_expired = false;
    g.session.snapshot.phase_remaining_ms = g.session.snapshot.focus_minutes * 60_000;
    g.session.snapshot.break_extensions_min = 0.0;
    let snap = g.session_snapshot();
    persist_session(&g, &handle.conn, false);
    drop(g);
    emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
    snap
}

/// User chose "Another 5 Minutes": extend the current break's allowance.
pub fn session_extend_break(handle: &TrackerHandle) -> SessionSnapshot {
    let mut g = handle.inner.lock();
    if g.session.snapshot.status == "ended" || g.session.snapshot.phase != "break" {
        return g.session_snapshot();
    }
    const EXTENSION_MS: i64 = 5 * 60_000;
    g.session.break_extensions_ms += EXTENSION_MS;
    g.session.break_extensions_total_ms += EXTENSION_MS;
    g.session.snapshot.break_expired = false;
    g.session.snapshot.break_extensions_min = g.session.break_extensions_ms as f64 / 60_000.0;
    let snap = g.session_snapshot();
    persist_session(&g, &handle.conn, false);
    drop(g);
    emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
    snap
}

/// Finalize the current break period into an honest wall-clock record.
fn finalize_break(g: &mut TrackerInner, now: i64) {
    if g.session.break_started_ms <= 0 {
        return;
    }
    let planned = g.session.snapshot.break_minutes * 60_000;
    let extensions = g.session.break_extensions_ms;
    let actual = (now - g.session.break_started_ms).max(0);
    let allowance = planned + extensions;
    let overrun = (actual - allowance).max(0);
    g.session.break_history.push(BreakRecord {
        started_ms: g.session.break_started_ms,
        ended_ms: now,
        planned_ms: planned,
        extensions_ms: extensions,
        actual_ms: actual,
        overrun_ms: overrun,
    });
    g.session.break_total_ms += actual;
    g.session.break_planned_total_ms += planned;
    g.session.break_extensions_total_ms += extensions;
    g.session.break_overrun_total_ms += overrun;
    g.session.break_started_ms = 0;
    g.session.break_extensions_ms = 0;
}

pub fn session_end(handle: &TrackerHandle) -> SessionSnapshot {
    let mut g = handle.inner.lock();
    if g.session.snapshot.status != "ended" {
        if g.session.snapshot.status == "running" {
            let now = now_ms();
            let resume = g.session.last_resume_ms.unwrap_or(now);
            g.session.accumulated_ms += (now - resume).max(0);
        }
        if g.session.snapshot.phase == "break" {
            finalize_break(&mut g, now_ms());
        }
        if g.session.accumulated_ms > 0 && g.session.snapshot.phase == "focus" {
            g.session.snapshot.focus_intervals += 1;
        }
        g.session.snapshot.status = "ended".into();
        g.session.snapshot.phase = String::new();
        g.session.snapshot.break_expired = false;
        g.session.snapshot.focus_idle_paused = false;
        g.session.idle_paused_ms = 0;
        g.session.manual_paused_ms = 0;
        persist_session(&g, &handle.conn, true);
        g.session.db_id = None;
    }
    g.session_snapshot()
}

/// Phase machine check, called from the 1-second service tick. Transitions
/// focus→break automatically and marks expired breaks; the user's explicit
/// choice (resume / extend) is a separate command — never automatic.
/// Notifications are event-only (the lib.rs emitter handles OS notifications).
fn check_session_phase(inner: &Arc<Mutex<TrackerInner>>, conn: &Arc<Mutex<Connection>>, _app: &Option<tauri::AppHandle>) {
    let mut g = inner.lock();
    if g.session.snapshot.status != "running" {
        return;
    }
    let now = now_ms();
    match g.session.snapshot.phase.as_str() {
        "focus" if g.session.snapshot.breaks_enabled && g.session.snapshot.focus_minutes > 0 => {
            // While idle-paused the countdown is frozen at the last-input
            // moment: the interval only completes when the user's active time
            // actually consumed it — never while away.
            let t = if g.session.idle_paused_ms > 0 { g.session.idle_paused_ms.min(now) } else { now };
            let focus_end = g.session.phase_started_ms + g.session.snapshot.focus_minutes * 60_000;
            if t >= focus_end {
                g.session.snapshot.focus_intervals += 1;
                g.session.snapshot.phase = "break".into();
                g.session.phase_started_ms = now;
                g.session.break_started_ms = now;
                g.session.break_extensions_ms = 0;
                g.session.snapshot.break_expired = false;
                let snap = g.session_snapshot();
                persist_session(&g, conn, false);
                drop(g);
                let body = format!(
                    "Focus interval complete. Time for your {}-minute break.",
                    snap.break_minutes,
                );
                emit_event("notice", &serde_json::json!({ "type": "break-schedule", "title": "Focus interval complete.", "body": body }).to_string());
                emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
            }
        }
        "break" => {
            let allowance = g.session.snapshot.break_minutes * 60_000 + g.session.break_extensions_ms;
            let break_end = g.session.break_started_ms + allowance;
            if now >= break_end && !g.session.snapshot.break_expired {
                g.session.snapshot.break_expired = true;
                let snap = g.session_snapshot();
                drop(g);
                let total_min = snap.break_minutes + (snap.break_extensions_min as i64);
                let body = format!("Your {}-minute break has ended. Resume focus or take another five minutes.", total_min);
                emit_event("notice", &serde_json::json!({ "type": "break-end", "title": "Break ended.", "body": body }).to_string());
                emit_event("session-tick", &serde_json::to_string(&snap).unwrap_or_default());
            }
        }
        _ => {}
    }
}

/* ---------------- tracking-state transitions ---------------- */

pub fn set_tracking_state(handle: &TrackerHandle, next: TrackingStateKind) -> TrackingInfo {
    set_tracking_state_inner(handle, next, false)
}

pub fn leave_privacy(handle: &TrackerHandle) -> TrackingInfo {
    {
        let g = handle.inner.lock();
        if g.tracking.state != TrackingStateKind::Privacy {
            return g.tracking.clone();
        }
    }
    let restore = handle.inner.lock().tracking.privacy_return;
    set_tracking_state_inner(handle, restore, true)
}

fn set_tracking_state_inner(handle: &TrackerHandle, next: TrackingStateKind, explicit_leave: bool) -> TrackingInfo {
    let mut g = handle.inner.lock();
    let now = now_ms();
    let prev = g.tracking.state;
    // A state change that stops observation also ends any open idle period:
    // record it as idle (never silently dropped or credited to an app).
    let close_open_idle = |g: &mut TrackerInner| {
        if let Some(idle_start) = g.idle_open.take() {
            insert_interval(&handle.conn.lock(), idle_start, now, "Idle", "Away from keyboard", Category::Idle, "", 0.0, "idle", "Idle detection", false);
        }
    };
    match next {
        TrackingStateKind::Paused => {
            if observing_state(&prev) {
                close_open(&mut g, &handle.conn, now);
                close_open_idle(&mut g);
                g.productive_run = None; // pausing breaks sustained focus
                g.pause_started = Some((now, "Tracking was manually paused"));
                g.tracking.state = TrackingStateKind::Paused;
                g.state = RunState::NotObserving;
            }
        }
        TrackingStateKind::Active => {
            // A generic resume/enable never overrides privacy mode; an explicit leave does.
            if prev == TrackingStateKind::Privacy && !explicit_leave {
                let info = g.tracking.clone();
                drop(g);
                return info;
            }
            if let Some((started, reason)) = g.pause_started.take() {
                insert_interval(&handle.conn.lock(), started, now, "Tracking gap", reason, Category::Gap, "", 0.0, "gap", "System event", false);
            }
            g.tracking.state = TrackingStateKind::Active;
            g.tracking.degraded_reason = None;
            g.state = RunState::Observing;
        }
        TrackingStateKind::Disabled => {
            if observing_state(&prev) {
                close_open(&mut g, &handle.conn, now);
                close_open_idle(&mut g);
                g.productive_run = None; // disabling breaks sustained focus
                g.pause_started = Some((now, "Tracking was disabled"));
            }
            g.tracking.state = TrackingStateKind::Disabled;
            g.state = RunState::NotObserving;
        }
        TrackingStateKind::Privacy => {
            if prev != TrackingStateKind::Privacy {
                if observing_state(&prev) {
                    close_open(&mut g, &handle.conn, now);
                }
                // Privacy periods are never recorded, not even as idle or gaps.
                g.idle_open = None;
                g.productive_run = None; // privacy breaks sustained focus
                g.tracking.privacy_return = prev;
                g.tracking.state = TrackingStateKind::Privacy;
                g.state = RunState::NotObserving;
                g.pause_started = None; // privacy periods are never recorded, not even as gaps
            }
        }
        TrackingStateKind::Confirmation => {
            g.tracking.state = TrackingStateKind::Confirmation;
            g.state = RunState::NotObserving;
        }
        TrackingStateKind::Degraded => {
            if observing_state(&prev) {
                g.tracking.state = TrackingStateKind::Degraded;
                g.state = RunState::Observing;
            }
        }
    }
    persist_tracking(&g, &handle.conn);
    let info = g.tracking.clone();
    // State transitions (pause, privacy, resume, degradation) change what the
    // live card should show â€” announce the new snapshot alongside the state.
    emit_current_activity(&g);
    drop(g);
    emit_event("tracking-changed", &serde_json::to_string(&info).unwrap_or_default());
    info
}

/* ---------------- capabilities ---------------- */

pub fn capabilities() -> Vec<Capability> {
    let mut caps = Vec::new();
    if cfg!(target_os = "windows") {
        caps.push(Capability { name: "Foreground application".into(), status: "available".into(), detail: "WinEvent foreground hook".into() });
        caps.push(Capability { name: "Window title context".into(), status: "available".into(), detail: "Foreground window title".into() });
        caps.push(Capability { name: "Idle detection".into(), status: "available".into(), detail: "Last input information".into() });
        caps.push(Capability { name: "Lock, sleep and wake handling".into(), status: "available".into(), detail: "Session and power notifications".into() });
    } else if cfg!(target_os = "macos") {
        caps.push(Capability { name: "Frontmost application".into(), status: "available".into(), detail: "Workspace frontmost application".into() });
        caps.push(Capability { name: "Window title context".into(), status: "permission-required".into(), detail: "Requires Accessibility permission".into() });
        caps.push(Capability { name: "Idle detection".into(), status: "available".into(), detail: "Combined event-source idle time".into() });
        caps.push(Capability { name: "Sleep and wake handling".into(), status: "available".into(), detail: "System sleep notifications".into() });
    } else {
        let hypr = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();
        let wayland = std::env::var("WAYLAND_DISPLAY").is_ok() || hypr;
        let x11 = std::env::var("DISPLAY").is_ok() && !wayland;
        if hypr {
            caps.push(Capability { name: "Foreground application".into(), status: "available".into(), detail: "Hyprland event IPC".into() });
            caps.push(Capability { name: "Window title context".into(), status: "available".into(), detail: "Hyprland activewindow title".into() });
        } else if x11 {
            caps.push(Capability { name: "Foreground application".into(), status: "available".into(), detail: "X11 active window property".into() });
            caps.push(Capability { name: "Window title context".into(), status: "limited".into(), detail: "Depends on toolkit and window manager".into() });
        } else if wayland {
            caps.push(Capability { name: "Foreground application".into(), status: "unsupported".into(), detail: "This compositor exposes no foreground-window protocol; nothing is fabricated".into() });
            caps.push(Capability { name: "Window title context".into(), status: "unsupported".into(), detail: "Compositor security boundary".into() });
        } else {
            caps.push(Capability { name: "Foreground application".into(), status: "temporarily-unavailable".into(), detail: "No desktop session detected".into() });
            caps.push(Capability { name: "Window title context".into(), status: "temporarily-unavailable".into(), detail: "No desktop session detected".into() });
        }
        let idle = if x11 {
            ("available".to_string(), "X screen saver extension".to_string())
        } else {
            ("limited".to_string(), "Idle time is not exposed by this compositor".to_string())
        };
        caps.push(Capability { name: "Idle detection".into(), status: idle.0, detail: idle.1 });
        caps.push(Capability { name: "Sleep and wake handling".into(), status: "limited".into(), detail: "Outages detected from wall-clock discontinuities".into() });
    }
    caps.push(Capability { name: "Website attribution".into(), status: "limited".into(), detail: "Requires the optional browser extension".into() });
    caps.push(Capability { name: "Local focus timer".into(), status: "available".into(), detail: "Owned by the CHRONOS core".into() });
    caps
}

pub fn tracker_backend_name() -> String {
    if cfg!(target_os = "windows") {
        "Win32 foreground hook + input idle".to_string()
    } else if cfg!(target_os = "macos") {
        "NSWorkspace frontmost + event-source idle".to_string()
    } else if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        "Hyprland IPC".to_string()
    } else if std::env::var("DISPLAY").is_ok() {
        "X11".to_string()
    } else if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "Wayland (no foreground protocol available)".to_string()
    } else {
        "none detected".to_string()
    }
}

/* ---------------- platform adapters ---------------- */

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub fn run(_app: Option<tauri::AppHandle>, tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>) {
        let _ = tx.send(PlatformEvent::SourceLost("Unsupported platform".into()));
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
