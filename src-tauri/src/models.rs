use serde::{Deserialize, Serialize};

/// Approved classification categories. Idle and gap are time states, not work categories.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Productive,
    Distracting,
    Neutral,
    Unclassified,
    Idle,
    Gap,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Productive => "productive",
            Category::Distracting => "distracting",
            Category::Neutral => "neutral",
            Category::Unclassified => "unclassified",
            Category::Idle => "idle",
            Category::Gap => "gap",
        }
    }
    pub fn parse(s: &str) -> Option<Category> {
        match s {
            "productive" => Some(Category::Productive),
            "distracting" => Some(Category::Distracting),
            "neutral" => Some(Category::Neutral),
            "unclassified" => Some(Category::Unclassified),
            "idle" => Some(Category::Idle),
            "gap" => Some(Category::Gap),
            _ => None,
        }
    }
}

/// View-model record for the interface: minutes since local midnight on `date`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ActivityRecord {
    pub id: i64,
    pub date: String,
    pub start: f64,
    pub end: f64,
    pub app: String,
    pub detail: String,
    pub cat: Category,
    pub project: String,
    pub focus: f64,
    pub kind: String,
    pub source: String,
    pub corrected: bool,
    /// Classification confidence at write time, when recorded.
    #[serde(default)]
    pub confidence: Option<f64>,
    /// Why this record is attributed to its project: 'rule' | 'learned' | 'user' | ''.
    #[serde(default)]
    pub project_source: String,
    /// True for manually entered time — never presented as observed activity.
    #[serde(default)]
    pub manual: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Totals {
    pub tracked: f64,
    pub active: f64,
    pub productive: f64,
    pub distracting: f64,
    pub neutral: f64,
    pub unclassified: f64,
    pub idle: f64,
    pub gap: f64,
    pub focused: f64,
    pub classified: f64,
    pub coverage: i64,
    pub productivity: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(rename = "desc")]
    pub description: String,
    pub color: String,
    pub archived: bool,
    /// Optional client this project belongs to; empty for standalone personal projects.
    #[serde(default)]
    pub client: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rule {
    pub app: String,
    pub cat: Category,
    /// Optional explicit project assignment carried by this rule.
    #[serde(default)]
    pub project: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Goals {
    pub daily: i64,
    pub weekly: i64,
    pub project: i64,
    pub limit: i64,
}

impl Default for Goals {
    fn default() -> Self {
        Goals { daily: 240, weekly: 1200, project: 240, limit: 60 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TrackingStateKind {
    Active,
    Paused,
    Privacy,
    Disabled,
    Degraded,
    Confirmation,
}

impl TrackingStateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackingStateKind::Active => "active",
            TrackingStateKind::Paused => "paused",
            TrackingStateKind::Privacy => "privacy",
            TrackingStateKind::Disabled => "disabled",
            TrackingStateKind::Degraded => "degraded",
            TrackingStateKind::Confirmation => "confirmation",
        }
    }
    pub fn parse(s: &str) -> Option<TrackingStateKind> {
        match s {
            "active" => Some(TrackingStateKind::Active),
            "paused" => Some(TrackingStateKind::Paused),
            "privacy" => Some(TrackingStateKind::Privacy),
            "disabled" => Some(TrackingStateKind::Disabled),
            "degraded" => Some(TrackingStateKind::Degraded),
            "confirmation" => Some(TrackingStateKind::Confirmation),
            _ => None,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct TrackingInfo {
    pub state: TrackingStateKind,
    pub privacy_return: TrackingStateKind,
    #[serde(rename = "degradedReason")]
    pub degraded_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub status: String, // "running" | "paused" | "ended"
    pub elapsed: i64,   // seconds
    pub qualified: f64, // minutes
    pub idle: f64,      // minutes
    pub interruptions: i64,
    pub intention: String,
    pub project: String,
    /// Current phase: "focus" | "break" (empty when no session).
    #[serde(default)]
    pub phase: String,
    /// Milliseconds until the next automatic transition (focus→break, or break
    /// expiry). Negative when a break has expired and awaits the user's choice.
    /// While idle-paused this is the frozen remaining focus time.
    #[serde(default)]
    pub phase_remaining_ms: i64,
    /// True when a break has expired and the user must choose (resume / extend).
    #[serde(default)]
    pub break_expired: bool,
    /// True when the focus countdown is paused because the user went idle.
    /// The session itself stays running; only an explicit Resume Focus
    /// continues the countdown from the preserved remaining time.
    #[serde(default)]
    pub focus_idle_paused: bool,
    /// Configured focus duration in minutes.
    #[serde(default)]
    pub focus_minutes: i64,
    /// Configured break duration in minutes.
    #[serde(default)]
    pub break_minutes: i64,
    /// Whether scheduled breaks are enabled.
    #[serde(default)]
    pub breaks_enabled: bool,
    /// Completed focus intervals so far.
    #[serde(default)]
    pub focus_intervals: i64,
    /// Current break's requested extensions in minutes (0 outside a break).
    #[serde(default)]
    pub break_extensions_min: f64,
    /// Scheduled breaks started in this session (completed + the running one).
    #[serde(default)]
    pub break_count: i64,
    /// Actual accumulated break duration in minutes, including the running
    /// break (extensions and overruns included; idle pauses are never breaks).
    #[serde(default)]
    pub break_total_min: f64,
    /// Total break overrun in minutes — actual time beyond the scheduled
    /// allowances plus explicitly requested extensions, including the running
    /// break's live overrun.
    #[serde(default)]
    pub break_overrun_total_min: f64,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        SessionSnapshot {
            status: "ended".into(), elapsed: 0, qualified: 0.0, idle: 0.0, interruptions: 0,
            intention: String::new(), project: String::new(),
            phase: String::new(), phase_remaining_ms: 0, break_expired: false, focus_idle_paused: false,
            focus_minutes: 25, break_minutes: 5, breaks_enabled: true,
            focus_intervals: 0, break_extensions_min: 0.0,
            break_count: 0, break_total_min: 0.0, break_overrun_total_min: 0.0,
        }
    }
}

/// One completed break within a focus session — honest wall-clock accounting.
/// Serialized camelCase for the frontend; the snake_case aliases keep rows
/// written by 1.3.0 (stored as JSON in focus_sessions.break_history) readable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BreakRecord {
    #[serde(rename = "startedMs", alias = "started_ms")]
    pub started_ms: i64,
    #[serde(rename = "endedMs", alias = "ended_ms")]
    pub ended_ms: i64,
    /// Planned allowance (configured break duration), ms.
    #[serde(rename = "plannedMs", alias = "planned_ms")]
    pub planned_ms: i64,
    /// Explicitly requested extensions, ms.
    #[serde(rename = "extensionsMs", alias = "extensions_ms")]
    pub extensions_ms: i64,
    /// Actual wall-clock duration (ended - started), ms.
    #[serde(rename = "actualMs", alias = "actual_ms")]
    pub actual_ms: i64,
    /// Time beyond the allowance (actual - planned - extensions), ms; >= 0.
    #[serde(rename = "overrunMs", alias = "overrun_ms")]
    pub overrun_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct SessionHistoryEntry {
    pub id: i64,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "endedAt")]
    pub ended_at: String,
    pub intention: String,
    pub project: String,
    pub elapsed: f64,    // minutes
    pub qualified: f64,  // minutes
    /// Focus/break cycle configuration and honest break accounting.
    #[serde(rename = "focusMinutes")]
    pub focus_minutes: i64,
    #[serde(rename = "breakMinutes")]
    pub break_minutes: i64,
    #[serde(rename = "breaksEnabled")]
    pub breaks_enabled: bool,
    #[serde(rename = "focusIntervals")]
    pub focus_intervals: i64,
    #[serde(rename = "breakPlannedMin")]
    pub break_planned_min: f64,
    #[serde(rename = "breakExtensionsMin")]
    pub break_extensions_min: f64,
    #[serde(rename = "breakActualMin")]
    pub break_actual_min: f64,
    #[serde(rename = "breakOverrunMin")]
    pub break_overrun_min: f64,
    #[serde(rename = "breakHistory")]
    pub break_history: Vec<BreakRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub appearance: String, // "Midnight" | "Soft midnight"
    pub startup: bool,
    pub tray: bool,
    #[serde(rename = "autoIdle")]
    pub auto_idle: bool,
    #[serde(rename = "idleMinutes")]
    pub idle_minutes: i64,
    pub breaks: bool,
    #[serde(rename = "breakEveryMinutes")]
    pub break_every_minutes: i64,
    pub blocking: bool,
    pub scheduled: bool,
    /// Tracking-schedule observation window enabled (independent of the
    /// scheduled-backup preference, which owns `scheduled`).
    #[serde(default, rename = "scheduleEnabled")]
    pub schedule_enabled: bool,
    pub quiet: bool,
    #[serde(rename = "quietStart")]
    pub quiet_start: i64,
    #[serde(rename = "quietEnd")]
    pub quiet_end: i64,
    pub retention: String,
    pub time: String, // "24-hour" | "12-hour"
    /// Selected date presentation (e.g. "medium" = Oct 27, 2026). Storage stays ISO.
    #[serde(default = "default_date_format", rename = "dateFormat")]
    pub date_format: String,
    pub platform: String,
    /// Preferred display name; empty  system username fallback.
    #[serde(default)]
    pub display_name: String,
    /// Mini CHRONOS floating companion enabled.
    #[serde(default, rename = "miniBar")]
    pub mini_bar: bool,
    /// Project measured by the project-time goal (project id; '' = first project).
    #[serde(default, rename = "goalProject")]
    pub goal_project: String,
    /// Optional tracking schedule: local window "HH:MM" boundaries. Overnight
    /// windows (start > end) are supported. Empty strings when unused.
    #[serde(default, rename = "scheduleStart")]
    pub schedule_start: String,
    #[serde(default, rename = "scheduleEnd")]
    pub schedule_end: String,
    /// Comma-separated weekday short names the schedule applies to (e.g. "Mon,Tue").
    #[serde(default, rename = "scheduleDays")]
    pub schedule_days: String,
    /// Retention enforcement is explicitly activated (never implied by an old
    /// stored retention preference — destructive automation requires opt-in).
    #[serde(default, rename = "retentionActive")]
    pub retention_active: bool,
}

fn default_date_format() -> String {
    "medium".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: "Midnight".into(),
            startup: false,
            tray: true,
            auto_idle: true,
            idle_minutes: 5,
            breaks: true,
            break_every_minutes: 50,
            blocking: false,
            scheduled: false,
            schedule_enabled: false,
            quiet: false,
            quiet_start: 20,
            quiet_end: 8,
            retention: "Forever".into(),
            time: "24-hour".into(),
            date_format: default_date_format(),
            platform: String::new(),
            display_name: String::new(),
            mini_bar: false,
            goal_project: String::new(),
            schedule_start: String::new(),
            schedule_end: String::new(),
            schedule_days: "Mon,Tue,Wed,Thu,Fri".into(),
            retention_active: false,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Capability {
    pub name: String,
    pub status: String, // available | limited | permission-required | unsupported | temporarily-unavailable
    pub detail: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Diagnostics {
    pub platform: String,
    #[serde(rename = "trackerBackend")]
    pub tracker_backend: String,
    pub tracking: TrackingInfo,
    pub capabilities: Vec<Capability>,
    pub db: DbHealth,
    pub measured: Option<Measured>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DbHealth {
    pub path: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
    pub intervals: i64,
    #[serde(rename = "schemaVersion")]
    pub schema_version: i64,
    pub healthy: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Measured {
    #[serde(rename = "trackerThreadCpuPercent")]
    pub tracker_thread_cpu_percent: Option<f64>,
    #[serde(rename = "processMemoryMb")]
    pub process_memory_mb: Option<f64>,
    #[serde(rename = "uptimeSeconds")]
    pub uptime_seconds: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub initials: String,
    pub workspace: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AppStateOut {
    pub tracking: TrackingInfo,
    pub session: SessionSnapshot,
    pub goals: Goals,
    pub projects: Vec<Project>,
    pub rules: Vec<Rule>,
    pub exclusions: Vec<String>,
    pub settings: Settings,
    pub dnd: bool,
    pub onboarded: bool,
    #[serde(rename = "undoDepth")]
    pub undo_depth: usize,
    pub profile: Profile,
}
