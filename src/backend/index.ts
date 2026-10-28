import type {
  ActivityRecord, BackupManifest, Capability, Diagnostics, Goals, Project, ReportDay,
  RestorePlanSummary, Rule, SessionHistoryEntry, SessionSnapshot, Settings, Totals, TrackingInfo,
} from '../types'
import { tauriBackend } from './tauri'
import { mockBackend } from './mock'

/** View-layer aggregate for the Overview screen. */
export interface OverviewData {
  date: string
  totals: Totals
  recent: ActivityRecord[]
  week: { date: string; totals: Totals }[]
  reviewCount: number
  /** Observed intervals overlapping the current/most-recent focus session (for the quality graph). */
  sessionIntervals: ActivityRecord[]
  /** Intervals assigned to the goal project today (non-gap), for the project goal row. */
  projectMinutesToday: number
}

export interface ReportRequest {
  period: 'day' | 'week' | 'month' | 'custom'
  start: string
  end: string
  project: string // 'all' or project id
}

export interface ReportData {
  range: { start: string; end: string }
  totals: Totals
  prior: Totals | null
  breakdowns: {
    applications: { label: string; minutes: number }[]
    websites: { label: string; minutes: number }[]
    categories: { label: string; minutes: number }[]
    projects: { label: string; minutes: number; client?: string; projectId?: string }[]
  }
  daily: ReportDay[]
  /** Goal-project progress across the report period (null when no project exists). */
  goalProject: { id: string; name: string; minutes: number; targetMinutes: number } | null
}

export interface ProjectWithTime extends Project {
  minutes: number
  intervalCount: number
}
export interface AppState {
  tracking: TrackingInfo
  session: SessionSnapshot
  goals: Goals
  projects: Project[]
  rules: Rule[]
  exclusions: string[]
  settings: Settings
  dnd: boolean
  onboarded: boolean
  undoDepth: number
  profile: { name: string; initials: string; workspace: string }
}

/** Read-time work block computed by the core (grouping never alters records). */
export interface WorkBlock {
  startMs: number
  endMs: number
  apps: string[]
  dominant: string
  project: string
  minutes: number
  spanMinutes: number
  internalAwayMinutes: number
  switches: number
  intervalIds: number[]
  userBoundary: boolean
}

export interface BlockListEntry {
  id: number
  label: string
  kind: 'app' | 'domain'
  target: string
  enabled: boolean
}

export interface RetentionStatus {
  active: boolean
  preference: string
  cutoffDays: number | null
  lastRunMs: number | null
}

export type NoticeType = 'break' | 'complete' | 'degraded' | 'block' | 'break-end' | 'break-schedule'

/** Live snapshot of the currently observed activity (never a finalized record). */
export interface CurrentActivity {
  tracking: string
  activity: { name: string; app: string; cat: string; startMs: number; isSite: boolean } | null
  idle: boolean
  nowMs: number
}

export interface AppEventMap {
  'tracking-changed': TrackingInfo
  'session-tick': SessionSnapshot
  'activity-changed': { date: string }
  'state-changed': Record<string, never>
  'current-activity': CurrentActivity
  'notice': { type: NoticeType; title: string; body: string }
}

export type AppEvent = keyof AppEventMap

export interface Backend {
  readonly kind: 'tauri' | 'dev-mock'

  getState(): Promise<AppState>
  getCapabilities(): Promise<{ platform: string; capabilities: Capability[] }>
  /** Installed application version from packaged metadata (authoritative). */
  getAppVersion(): Promise<string>
  getDiagnostics(): Promise<Diagnostics>
  /** Live current-activity snapshot (the same source the mini bar uses). */
  getCurrentActivity(): Promise<CurrentActivity>

  getOverview(date: string): Promise<OverviewData>
  getTimeline(date: string): Promise<ActivityRecord[]>
  getReport(req: ReportRequest): Promise<ReportData>
  getFocusHistory(date: string): Promise<SessionHistoryEntry[]>
  getProjectsWithTime(): Promise<ProjectWithTime[]>

  correctRecord(id: number, cat: string, project: string, createRule: boolean): Promise<void>
  splitRecord(id: number, minutesInFirst: number): Promise<void>
  undoLast(): Promise<boolean>
  addRule(app: string, cat: string, project: string): Promise<void>
  applyRuleRetroactive(index: number): Promise<number>
  addExclusion(label: string): Promise<void>
  /** Remove an exclusion by its stored label; history is never touched. */
  removeExclusion(label: string): Promise<void>
  deleteHistory(start: string, end: string): Promise<number>

  startSession(intention: string, project: string, focusMinutes: number, breakMinutes: number, breaksEnabled: boolean): Promise<void>
  resumeFocus(): Promise<void>
  extendBreak(): Promise<void>
  toggleSession(): Promise<'paused' | 'resumed'>
  endSession(): Promise<void>

  setGoals(goals: Goals): Promise<void>
  createProject(name: string, desc: string, client: string): Promise<void>
  renameProject(id: string, name: string, desc: string, client: string): Promise<void>
  setProjectArchived(id: string, archived: boolean): Promise<void>
  /** Permanent deletion: clears project references on intervals and rules. Returns affected interval count. */
  deleteProject(id: string): Promise<number>
  /** Bulk assignment (project '' unassigns). Returns the number of processed ids. */
  bulkAssign(ids: number[], project: string): Promise<number>

  manualEntryAdd(startMs: number, endMs: number, label: string, cat: string, project: string): Promise<void>
  manualEntryUpdate(id: number, startMs: number, endMs: number, label: string, cat: string, project: string): Promise<void>
  manualEntryDelete(id: number): Promise<void>

  getWorkBlocks(date: string): Promise<{ date: string; blocks: WorkBlock[]; breakCount: number }>
  groupBreakAdd(atTs: number): Promise<void>
  groupBreakRemove(atTs: number): Promise<void>

  getIdleExceptions(): Promise<{ id: number; label: string }[]>
  idleExceptionAdd(label: string): Promise<void>
  idleExceptionRemove(id: number): Promise<void>

  getBlockList(): Promise<BlockListEntry[]>
  blockListAdd(kind: 'app' | 'domain', target: string, label: string): Promise<void>
  blockListRemove(id: number): Promise<void>
  blockListSetEnabled(id: number, enabled: boolean): Promise<void>

  retentionStatus(): Promise<RetentionStatus>
  retentionSetActive(active: boolean): Promise<number>
  retentionRunNow(): Promise<number>

  pauseTracking(): Promise<void>
  resumeTracking(): Promise<void>
  enableTracking(): Promise<void>
  disableTracking(): Promise<void>
  setPrivacy(on: boolean): Promise<void>
  /** Puts tracking into the confirmation state as if the previous state were unknown (diagnostics demonstration). */
  simulateUnknownRecovery(): Promise<void>

  setDnd(on: boolean): Promise<void>
  updateSettings(patch: Partial<Settings>): Promise<void>
  completeOnboarding(consent: boolean): Promise<void>

  createBackup(dir: string | null): Promise<BackupManifest>
  inspectBackup(path: string): Promise<RestorePlanSummary>
  restoreBackup(path: string, plan: RestorePlanSummary): Promise<{ restored: number; conflictsSkipped: number }>
  exportData(format: 'CSV' | 'JSON', req: ReportRequest, dir: string | null): Promise<string>
  quitApp(): Promise<void>

  on<E extends AppEvent>(event: E, handler: (payload: AppEventMap[E]) => void): () => void
}

function selectBackend(): Backend {
  const hasTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  return hasTauri ? tauriBackend : mockBackend
}

export const backend: Backend = selectBackend()
