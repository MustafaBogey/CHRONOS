export type Page = 'overview' | 'timeline' | 'focus' | 'reports' | 'settings'
export type FocusTab = 'sessions' | 'goals' | 'projects' | 'distractions'
export type ReportTab = 'summary' | 'applications' | 'websites' | 'categories' | 'projects' | 'trends'
export type SettingsTab = 'General' | 'Tracking & Classification' | 'Privacy & Data' | 'Focus & Notifications' | 'Integrations' | 'Advanced & Diagnostics' | 'Help & About'

export type Category = 'productive' | 'distracting' | 'neutral' | 'unclassified' | 'idle' | 'gap'

export interface CategoryMeta { label: string; color: string }

export const cats: Record<Category, CategoryMeta> = {
  productive: { label: 'Productive', color: 'var(--green)' },
  distracting: { label: 'Distracting', color: 'var(--pink)' },
  neutral: { label: 'Neutral', color: 'var(--cyan)' },
  unclassified: { label: 'Unclassified', color: 'var(--violet)' },
  idle: { label: 'Idle', color: 'var(--idle)' },
  gap: { label: 'Tracking gap', color: 'var(--amber)' },
}

/** An authoritative observed interval. Times are minutes from local midnight on `date` (prototype-compatible view model). */
export interface ActivityRecord {
  id: number
  date: string
  start: number
  end: number
  app: string
  detail: string
  cat: Category
  project: string
  focus: number
  kind: string
  source: string
  corrected: boolean
  /** Classification confidence at write time (0.4–0.99), when recorded. */
  confidence?: number | null
  /** Why this record is attributed to its project: 'rule' | 'learned' | 'user' | ''. */
  projectSource?: string
  /** True for manually entered time — never presented as observed activity. */
  manual?: boolean
}

export interface Project {
  id: string
  name: string
  desc: string
  color: string
  archived: boolean
  /** Optional client this project belongs to; '' for standalone personal projects. */
  client: string
}

export interface Totals {
  tracked: number; active: number; productive: number; distracting: number; neutral: number
  unclassified: number; idle: number; gap: number; focused: number; classified: number
  coverage: number; productivity: number
}

export interface Rule { app: string; cat: Category; project?: string }
export interface Exclusion { label: string }

export interface Goals { daily: number; weekly: number; project: number; limit: number }

export type TrackingState = 'active' | 'paused' | 'privacy' | 'disabled' | 'degraded' | 'confirmation'

export interface TrackingInfo {
  state: TrackingState
  privacyReturn: TrackingState
  degradedReason: string | null
}

export interface SessionSnapshot {
  status: 'running' | 'paused' | 'ended'
  /** seconds */
  elapsed: number
  /** minutes */
  qualified: number
  idle: number
  interruptions: number
  intention: string
  project: string
  /** Current phase: "focus" | "break" | "" (no session). */
  phase?: string
  /** Ms until the next automatic transition; negative = break expired. */
  phaseRemainingMs?: number
  /** True when a break has expired and the user must choose. */
  breakExpired?: boolean
  /** True when the focus countdown is paused because the user went idle.
   *  The session stays running; only an explicit Resume Focus continues it. */
  focusIdlePaused?: boolean
  focusMinutes?: number
  breakMinutes?: number
  breaksEnabled?: boolean
  focusIntervals?: number
  breakExtensionsMin?: number
  /** Scheduled breaks started (completed + the running one). */
  breakCount?: number
  /** Actual accumulated break minutes, including the running break. */
  breakTotalMin?: number
  /** Total break overrun minutes (beyond allowances + extensions), live. */
  breakOverrunTotalMin?: number
}

export interface BreakRecord {
  startedMs: number
  endedMs: number
  plannedMs: number
  extensionsMs: number
  actualMs: number
  overrunMs: number
}

export interface SessionHistoryEntry {
  id: number
  startedAt: string
  endedAt: string
  intention: string
  project: string
  /** minutes */
  elapsed: number
  qualified: number
}

export type CapabilityStatus = 'available' | 'limited' | 'permission-required' | 'unsupported' | 'temporarily-unavailable'

export interface Capability {
  name: string
  status: CapabilityStatus
  detail: string
}

export interface Diagnostics {
  platform: string
  trackerBackend: string
  tracking: TrackingInfo
  capabilities: Capability[]
  db: { path: string; sizeBytes: number; intervals: number; schemaVersion: number; healthy: boolean }
  measured: {
    trackerThreadCpuPercent: number | null
    processMemoryMb: number | null
    uptimeSeconds: number
  } | null
}

export interface Settings {
  appearance: 'Midnight' | 'Soft midnight'
  startup: boolean
  tray: boolean
  autoIdle: boolean
  idleMinutes: number
  breaks: boolean
  breakEveryMinutes: number
  blocking: boolean
  scheduled: boolean
  /** Tracking-schedule observation window (independent from scheduled backup). */
  scheduleEnabled: boolean
  quiet: boolean
  quietStart: number
  quietEnd: number
  retention: string
  time: '24-hour' | '12-hour'
  /** Selected date presentation; storage stays ISO (YYYY-MM-DD). */
  dateFormat: string
  platform: string
  /** Preferred display name; empty → system username fallback. */
  displayName: string
  /** Mini CHRONOS floating companion enabled. */
  miniBar: boolean
  /** Project measured by the project-time goal ('' = first project). */
  goalProject: string
  /** Optional tracking schedule: local "HH:MM" window boundaries ('' when unused). */
  scheduleStart: string
  scheduleEnd: string
  /** Weekday short names the schedule applies to, comma-separated. */
  scheduleDays: string
  /** Retention enforcement explicitly activated (never implied by the preference). */
  retentionActive: boolean
}

export interface WeeklyDay { date: string; totals: Totals }

export interface ReportDay { date: string; totals: Totals }

export interface RangeInfo { start: string; end: string; hasComparison: boolean }

export interface RestorePlanSummary {
  strategy: 'merge' | 'replace'
  preservePrivacy: boolean
  allowDeletedReintroduction: boolean
  backupIntervalCount: number
  backupProjectCount: number
  conflicts: number
  previouslyDeletedRecords: number
}

export interface BackupManifest {
  createdAt: string
  intervals: number
  projects: number
  rules: number
  trackedMinutes: number
  rangeStart: string
  rangeEnd: string
  sizeBytes: number
  integrity: 'passed' | 'failed'
  path: string
}
