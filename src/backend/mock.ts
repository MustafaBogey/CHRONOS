import type { Backend, AppState, OverviewData, ReportRequest, ReportData, ProjectWithTime, AppEvent, AppEventMap, WorkBlock, BlockListEntry, RetentionStatus, CurrentActivity } from './index'
import type {
  ActivityRecord, BackupManifest, Category, Diagnostics, Goals, Project, RestorePlanSummary,
  Rule, SessionHistoryEntry, SessionSnapshot, Settings, Totals, TrackingInfo,
} from '../types'
import { addDays, fmt, localDateStr } from '../format'

/** Domain-ish labels reduce to the bare host (mirrors the Rust normalizer). */
function normalizeExclusionLabel(label: string): string {
  let s = label.trim()
  if (s.startsWith('http://') || s.startsWith('https://')) s = s.replace(/^https?:\/\//, '')
  const host = s.split(/[/?#]/)[0] ?? ''
  const bare = host.split(':')[0]?.trim().toLowerCase() ?? ''
  const looksLikeHost = /^[a-z0-9.-]+\.[a-z0-9-]+$/.test(bare)
  if (!looksLikeHost) return label.trim()
  return bare.startsWith('www.') ? bare.slice(4) : bare
}

/**
 * DEV-ONLY browser mock used when the UI runs outside Tauri (vite dev / visual review).
 * Reproduces the approved prototype's illustrative dataset and in-memory behavior.
 * The packaged application always uses the real Rust core; this file never runs there.
 */

const TODAY = '2026-10-24'

/** The prototype's illustrative dataset is pinned to October 2026; the dev mock maps the
 *  real "today" onto the sample day so the UI shows the same content the approved HTML shows. */
function realToday(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}
function mapDate(date: string): string {
  return date === realToday() ? TODAY : date
}

const template: [string, string, number, Category, string, number, string][] = [
  ['Visual Studio Code', 'Workspace · Chronos core', 50, 'productive', 'p1', 50, 'development'],
  ['Documentation', 'docs.rust-lang.org', 20, 'productive', 'p1', 0, 'website'],
  ['Messages', 'Personal communication', 15, 'neutral', '', 0, 'communication'],
  ['Idle', 'Away from keyboard', 10, 'idle', '', 0, 'idle'],
  ['Design Studio', 'Aurora · Interface explorations', 45, 'productive', 'p2', 25, 'design'],
  ['Browser research', 'research.example · Untitled page', 25, 'unclassified', 'p2', 0, 'website'],
  ['Visual Studio Code', 'Workspace · Chronos core', 15, 'productive', 'p1', 15, 'development'],
  ['Idle', 'Away from keyboard', 30, 'idle', '', 0, 'idle'],
  ['Tracking gap', 'Tracking was manually paused', 20, 'gap', '', 0, 'gap'],
  ['Visual Studio Code', 'Workspace · Chronos core', 45, 'productive', 'p1', 0, 'development'],
  ['Video browser', 'video.example · Recommended videos', 25, 'distracting', '', 0, 'website'],
  ['Documentation', 'developer.mozilla.org', 20, 'productive', 'p1', 20, 'website'],
  ['Idle', 'Away from keyboard', 10, 'idle', '', 0, 'idle'],
  ['Visual Studio Code', 'Workspace · Chronos core', 40, 'productive', 'p1', 40, 'development'],
  ['Browser research', 'web.dev · Rendering performance', 30, 'productive', 'p1', 15, 'website'],
]

interface UndoSnapshot { records: ActivityRecord[]; rules: Rule[] }

export class MockBackend implements Backend {
  readonly kind = 'dev-mock' as const
  private nextId = 1
  private records: ActivityRecord[] = []
  private projects: Project[] = [
    { id: 'p1', name: 'CHRONOS', desc: 'A calmer relationship with time.', color: 'var(--violet)', archived: false, client: 'Meridian Studios' },
    { id: 'p2', name: 'Aurora', desc: 'Thoughtful interfaces, bright ideas.', color: 'var(--cyan)', archived: false, client: 'Meridian Studios' },
    { id: 'p3', name: 'Learning', desc: 'Make room for curiosity.', color: 'var(--green)', archived: false, client: '' },
  ]
  private goals: Goals = { daily: 240, weekly: 1200, project: 240, limit: 60 }
  private rules: Rule[] = []
  private exclusions: string[] = []
  private undoStack: UndoSnapshot[] = []
  private tracking: TrackingInfo = { state: 'active', privacyReturn: 'active', degradedReason: null }
  private dnd = false
  private onboarded = false
  private profileName = 'Alex Morgan'
  private settings: Settings = {
    appearance: 'Midnight', startup: true, tray: true, autoIdle: true, idleMinutes: 5,
    breaks: true, breakEveryMinutes: 50, blocking: false, scheduled: false, scheduleEnabled: false, quiet: false,
    quietStart: 20, quietEnd: 8, retention: 'Forever', time: '24-hour', platform: 'Windows', displayName: '', miniBar: false, dateFormat: 'medium',
    goalProject: '', scheduleStart: '', scheduleEnd: '', scheduleDays: 'Mon,Tue,Wed,Thu,Fri', retentionActive: false,
  }
  private groupBreaks: number[] = []
  private idleExceptions: { id: number; label: string }[] = [{ id: 1, label: 'zoom' }]
  private blockList: BlockListEntry[] = []
  private retentionLastRunMs: number | null = null
  private session: SessionSnapshot = { status: 'running', elapsed: 6000, qualified: 75, idle: 10, interruptions: 2, intention: 'Deep work', project: 'p1' }
  private history: SessionHistoryEntry[] = []
  private lastTick = Date.now()
  private listeners = new Map<string, Set<(p: unknown) => void>>()
  private tickTimer: number | null = null

  constructor() {
    for (let day = 12; day <= 24; day++) {
      const date = `2026-10-${day}`
      let t = 540
      const rows = day === 24 ? template : template.slice(0, day % 3 === 0 ? 12 : day % 3 === 1 ? 14 : 15)
      rows.forEach((a, i) => {
        const duration = day === 24 ? a[2] : Math.max(5, a[2] + ((day + i) % 3 - 1) * 5)
        const focus = Math.min(duration, a[5])
        this.records.push({
          id: this.nextId++, date, start: t, end: t + duration, app: a[0], detail: a[1], cat: a[3],
          project: a[4], focus, kind: a[6],
          source: a[3] === 'unclassified' ? 'Low confidence · 42%' : 'Built-in default', corrected: false,
        })
        t += duration
      })
    }
    this.startTicker()
  }

  /* ---------- helpers ---------- */

  private min(r: ActivityRecord): number { return r.end - r.start }

  private totals(rows: ActivityRecord[]): Totals {
    const t: Totals = { tracked: 0, active: 0, productive: 0, distracting: 0, neutral: 0, unclassified: 0, idle: 0, gap: 0, focused: 0, classified: 0, coverage: 0, productivity: 0 }
    rows.forEach((r) => {
      const d = this.min(r)
      t[r.cat] += d
      if (r.cat !== 'gap') { t.tracked += d; if (r.cat !== 'idle') t.active += d }
      if (r.cat === 'productive' || r.cat === 'distracting' || r.cat === 'neutral') t.classified += d
      if (r.cat === 'productive') t.focused += Math.min(r.focus, d)
    })
    t.coverage = t.active ? Math.round((t.classified / t.active) * 100) : 0
    t.productivity = t.classified ? Math.round((t.productive / t.classified) * 100) : 0
    return t
  }

  private snapshot() {
    this.undoStack.push({ records: this.records.map((r) => ({ ...r })), rules: this.rules.map((r) => ({ ...r })) })
    if (this.undoStack.length > 12) this.undoStack.shift()
  }

  private emit<E extends AppEvent>(event: E, payload: AppEventMap[E]) {
    this.listeners.get(event)?.forEach((fn) => fn(payload))
  }

  private startTicker() {
    if (this.tickTimer !== null) return
    this.tickTimer = window.setInterval(() => {
      if (this.session.status === 'running') {
        const now = Date.now()
        this.session.elapsed += Math.floor((now - this.lastTick) / 1000)
        this.lastTick = now
        this.emit('session-tick', { ...this.session })
      } else {
        this.lastTick = Date.now()
      }
    }, 1000)
  }

  private sessionSeconds(): number { return this.session.elapsed }

  /* ---------- Backend interface ---------- */

  async getState(): Promise<AppState> {
    return {
      tracking: { ...this.tracking },
      session: { ...this.session },
      goals: { ...this.goals },
      projects: this.projects.map((p) => ({ ...p })),
      rules: this.rules.map((r) => ({ ...r })),
      exclusions: [...this.exclusions],
      settings: { ...this.settings },
      dnd: this.dnd,
      onboarded: this.onboarded,
      undoDepth: this.undoStack.length,
      profile: { name: this.profileName, initials: this.profileName.split(' ').filter(Boolean).slice(0, 2).map((w) => w[0]?.toUpperCase() ?? '?').join('') || 'AL', workspace: 'Personal workspace' },
    }
  }

  async getCapabilities() {
    const platform = this.settings.platform
    const vals: Record<string, string[]> = {
      Windows: ['available', 'available', 'limited', 'available'],
      macOS: ['permission-required', 'permission-required', 'limited', 'available'],
      'Linux · Wayland': ['limited', 'unsupported', 'temporarily-unavailable', 'available'],
      'Linux · X11': ['available', 'limited', 'limited', 'available'],
    }
    const statuses = vals[platform] ?? vals.Windows
    const names = ['Foreground application', 'Window title context', 'Website attribution', 'Local focus timer']
    return {
      platform,
      capabilities: names.map((name, i) => ({
        name,
        status: statuses[i] as Diagnostics['capabilities'][number]['status'],
        detail: 'Illustrative dev-mock capability row.',
      })),
    }
  }

  async getAppVersion(): Promise<string> {
    return 'dev preview (browser mock)'
  }

  /* Live current activity: a synthetic but stable "ongoing" observation that
     exercises the live UI paths (card, ticking duration, ongoing rows). */
  private liveStartMs = Date.now() - 7 * 60_000
  async getCurrentActivity(): Promise<CurrentActivity> {
    return {
      tracking: this.tracking.state,
      activity: this.tracking.state === 'active'
        ? { name: 'Documentation', app: 'Google Chrome', cat: 'productive', startMs: this.liveStartMs, isSite: true }
        : null,
      idle: false,
      nowMs: Date.now(),
    }
  }

  async getDiagnostics(): Promise<Diagnostics> {
    const caps = await this.getCapabilities()
    return {
      platform: 'Browser dev mock',
      trackerBackend: 'mock (no native observation in the browser)',
      tracking: { ...this.tracking },
      capabilities: caps.capabilities,
      db: { path: '(in-memory mock)', sizeBytes: 0, intervals: this.records.length, schemaVersion: 0, healthy: true },
      measured: null,
    }
  }

  async getOverview(date: string): Promise<OverviewData> {
    date = mapDate(date)
    const rows = this.records.filter((r) => r.date === date)
    const recent = rows.filter((r) => r.cat !== 'gap' && r.cat !== 'idle').slice(-4).reverse()
    const ws = addDays(date, -((new Date(date + 'T12:00:00').getDay() + 6) % 7))
    const week = Array.from({ length: 7 }, (_, i) => {
      const d = addDays(ws, i)
      return { date: d, totals: this.totals(this.records.filter((r) => r.date === d)) }
    })
    const isNew = this.session.elapsed < 60 * 8 && this.session.qualified === 0
    const sessionIntervals = isNew ? [] : rows.filter((r) => r.start >= 840 && r.end <= 940)
    const projectMinutesToday = rows.filter((r) => r.project === 'p1' && r.cat !== 'gap').reduce((a, r) => a + this.min(r), 0)
    return { date, totals: this.totals(rows), recent, week, reviewCount: rows.filter((r) => r.cat === 'unclassified').length, sessionIntervals, projectMinutesToday }
  }

  async getTimeline(date: string): Promise<ActivityRecord[]> {
    return this.records.filter((r) => r.date === mapDate(date))
  }

  async getReport(req: ReportRequest): Promise<ReportData> {
    const inRange = (r: ActivityRecord) => r.date >= req.start && r.date <= req.end && (req.project === 'all' || r.project === req.project)
    const rows = this.records.filter(inRange)
    const length = Math.round((new Date(req.end).getTime() - new Date(req.start).getTime()) / 86400000) + 1
    const priorRows = this.records.filter((r) => {
      const s = addDays(req.start, -length)
      return r.date >= s && r.date < req.start && (req.project === 'all' || r.project === req.project)
    })
    const breakdown = (type: 'applications' | 'websites' | 'categories' | 'projects') => {
      const grouped: Record<string, number> = {}
      rows.filter((r) => r.cat !== 'gap' && (type !== 'websites' || r.kind === 'website')).forEach((r) => {
        const key = type === 'categories' ? r.cat : type === 'projects' ? (this.projects.find((p) => p.id === r.project)?.name ?? 'No project') : type === 'websites' ? r.detail.split(' · ')[0] : r.app
        grouped[key] = (grouped[key] ?? 0) + this.min(r)
      })
      return Object.entries(grouped).sort((a, b) => b[1] - a[1]).map(([label, minutes]) => ({ label, minutes }))
    }
    const days = [...new Set(rows.map((r) => r.date))].sort()
    const goalPid = this.settings.goalProject && this.projects.some((p) => p.id === this.settings.goalProject)
      ? this.settings.goalProject
      : this.projects.find((p) => !p.archived)?.id ?? ''
    const goalProject = goalPid
      ? {
          id: goalPid,
          name: this.projects.find((p) => p.id === goalPid)?.name ?? '',
          minutes: rows.filter((r) => r.project === goalPid && r.cat !== 'gap').reduce((a, r) => a + this.min(r), 0),
          targetMinutes: this.goals.project,
        }
      : null
    return {
      range: { start: req.start, end: req.end },
      totals: this.totals(rows),
      prior: priorRows.length ? this.totals(priorRows) : null,
      breakdowns: {
        applications: breakdown('applications'),
        websites: breakdown('websites'),
        categories: breakdown('categories'),
        projects: breakdown('projects'),
      },
      daily: days.map((d) => ({ date: d, totals: this.totals(rows.filter((r) => r.date === d)) })),
      goalProject,
    }
  }

  async getFocusHistory(date: string): Promise<SessionHistoryEntry[]> {
    date = mapDate(date)
    const seeded: SessionHistoryEntry[] = [
      { id: 1, startedAt: `${date}T09:00`, endedAt: `${date}T10:50`, intention: 'Morning momentum', project: 'p1', elapsed: 110, qualified: 90 },
      { id: 2, startedAt: `${date}T14:00`, endedAt: `${date}T15:40`, intention: 'Deep work', project: 'p1', elapsed: 100, qualified: 75 },
    ]
    return [...this.history, ...seeded]
  }

  async getProjectsWithTime(): Promise<ProjectWithTime[]> {
    return this.projects.map((p) => {
      const rows = this.records.filter((r) => r.project === p.id && r.cat !== 'gap')
      return { ...p, minutes: rows.reduce((s, r) => s + this.min(r), 0), intervalCount: rows.length }
    })
  }

  async correctRecord(id: number, cat: string, project: string, createRule: boolean): Promise<void> {
    const r = this.records.find((x) => x.id === id)
    if (!r) return
    this.snapshot()
    r.cat = cat as Category
    r.project = project
    r.projectSource = project ? 'user' : ''
    r.corrected = true
    r.source = 'Explicit user correction'
    if (cat !== 'productive') r.focus = 0 // corrections cannot invent focus; nonproductive records carry none
    if (createRule) this.rules.push({ app: r.app, cat: cat as Category, project })
  }

  async splitRecord(id: number, minutesInFirst: number): Promise<void> {
    const r = this.records.find((x) => x.id === id)
    if (!r) return
    const duration = this.min(r)
    if (!Number.isInteger(minutesInFirst) || minutesInFirst < 1 || minutesInFirst >= duration) throw new Error('invalid split point')
    this.snapshot()
    const oldEnd = r.end
    const f = r.focus
    const cut = r.start + minutesInFirst
    r.end = cut
    r.focus = (f * minutesInFirst) / duration
    const second: ActivityRecord = { ...r, id: this.nextId++, start: cut, end: oldEnd, focus: f - r.focus }
    this.records.splice(this.records.indexOf(r) + 1, 0, second)
  }

  async undoLast(): Promise<boolean> {
    const prev = this.undoStack.pop()
    if (!prev) return false
    this.records = prev.records
    this.rules = prev.rules
    return true
  }

  async addRule(app: string, cat: string, project: string): Promise<void> {
    this.rules.push({ app, cat: cat as Category, project })
  }

  async applyRuleRetroactive(index: number): Promise<number> {
    const rule = this.rules[index]
    if (!rule) return 0
    this.snapshot()
    let count = 0
    this.records.forEach((r) => {
      if (r.app === rule.app && !r.corrected && r.cat !== 'idle' && r.cat !== 'gap') {
        r.cat = rule.cat
        r.source = 'User-defined rule · explicitly reapplied'
        if (rule.cat !== 'productive') r.focus = 0
        count++
      }
    })
    return count
  }

  async addExclusion(label: string): Promise<void> {
    // Mirrors the Rust core: domain-ish input is canonicalized to the bare domain.
    const normalized = normalizeExclusionLabel(label)
    if (!normalized) throw new Error('Enter an application or domain.')
    if (this.exclusions.some((e) => e.toLowerCase() === normalized.toLowerCase())) throw new Error('That exclusion already exists.')
    this.exclusions.push(normalized)
    this.emit('state-changed', {})
  }

  async removeExclusion(label: string): Promise<void> {
    this.exclusions = this.exclusions.filter((e) => e !== label)
    this.emit('state-changed', {})
  }

  async deleteHistory(start: string, end: string): Promise<number> {
    const count = this.records.filter((r) => r.date >= start && r.date <= end).length
    this.snapshot()
    this.records = this.records.filter((r) => r.date < start || r.date > end)
    return count
  }

  async startSession(intention: string, project: string, focusMinutes = 25, breakMinutes = 5, breaksEnabled = true): Promise<void> {
    this.session = { status: 'running', elapsed: 0, qualified: 0, idle: 0, interruptions: 0, intention: intention || 'Deep work', project,
      phase: 'focus', phaseRemainingMs: focusMinutes * 60000, breakExpired: false, focusIdlePaused: false, focusMinutes, breakMinutes, breaksEnabled, focusIntervals: 0, breakExtensionsMin: 0,
      breakCount: 0, breakTotalMin: 0, breakOverrunTotalMin: 0 }
    this.lastTick = Date.now()
    this.emit('session-tick', { ...this.session })
  }

  /** Dev/test hook: force the live session snapshot (simulates engine events
   *  the mock does not model, e.g. the idle pause freezing the countdown). */
  debugSetSession(patch: Partial<SessionSnapshot>) {
    this.session = { ...this.session, ...patch }
    this.emit('session-tick', { ...this.session })
  }

  async resumeFocus(): Promise<void> {
    if (this.session.status === 'ended') return
    if (this.session.phase === 'focus') {
      // Idle-paused focus: continue the same interval (mock keeps its remaining time).
      this.session.focusIdlePaused = false
      this.emit('session-tick', { ...this.session })
      return
    }
    if (this.session.phase !== 'break') return
    this.session.phase = 'focus'
    this.session.breakExpired = false
    this.session.focusIntervals = (this.session.focusIntervals ?? 0) + 1
    this.session.phaseRemainingMs = (this.session.focusMinutes ?? 25) * 60000
    this.session.breakExtensionsMin = 0
    this.emit('session-tick', { ...this.session })
  }

  async extendBreak(): Promise<void> {
    if (this.session.status === 'ended' || this.session.phase !== 'break') return
    this.session.breakExtensionsMin = (this.session.breakExtensionsMin ?? 0) + 5
    this.session.breakExpired = false
    this.session.phaseRemainingMs = 5 * 60000
    this.emit('session-tick', { ...this.session })
  }

  async toggleSession(): Promise<'paused' | 'resumed'> {
    if (this.session.status === 'ended') throw new Error('session ended')
    this.session.status = this.session.status === 'running' ? 'paused' : 'running'
    this.lastTick = Date.now()
    this.emit('session-tick', { ...this.session })
    return this.session.status === 'paused' ? 'paused' : 'resumed'
  }

  async endSession(): Promise<void> {
    if (this.session.status === 'ended') return
    this.history.unshift({
      id: Date.now(), startedAt: localDateStr(), endedAt: localDateStr(),
      intention: this.session.intention, project: this.session.project,
      elapsed: Math.round(this.sessionSeconds() / 60), qualified: Math.round(this.session.qualified),
    })
    this.session = { ...this.session, status: 'ended' }
    this.emit('session-tick', { ...this.session })
  }

  async setGoals(goals: Goals): Promise<void> { this.goals = { ...goals } }

  async createProject(name: string, desc: string, client: string): Promise<void> {
    if (this.projects.some((p) => p.name.toLowerCase() === name.toLowerCase())) throw new Error('duplicate project name')
    this.projects.push({ id: 'p' + Date.now(), name, desc: desc || 'A new intention, a little time.', color: 'var(--green)', archived: false, client })
  }

  async renameProject(id: string, name: string, desc: string, client: string): Promise<void> {
    if (this.projects.some((p) => p.id !== id && p.name.toLowerCase() === name.toLowerCase())) throw new Error('duplicate project name')
    const p = this.projects.find((x) => x.id === id)
    if (p) {
      p.name = name
      p.client = client
      if (desc.trim()) p.desc = desc.trim()
    }
  }

  async setProjectArchived(id: string, archived: boolean): Promise<void> {
    const p = this.projects.find((x) => x.id === id)
    if (p) p.archived = archived
  }

  async deleteProject(id: string): Promise<number> {
    const affected = this.records.filter((r) => r.project === id).length
    this.records.forEach((r) => { if (r.project === id) { r.project = ''; r.projectSource = '' } })
    this.projects = this.projects.filter((p) => p.id !== id)
    if (this.settings.goalProject === id) this.settings.goalProject = ''
    return affected
  }

  async bulkAssign(ids: number[], project: string): Promise<number> {
    if (!ids.length) throw new Error('Select at least one activity.')
    this.snapshot()
    this.records.forEach((r) => {
      if (ids.includes(r.id) && r.cat !== 'idle' && r.cat !== 'gap') {
        r.project = project
        r.projectSource = project ? 'user' : ''
      }
    })
    return ids.length
  }

  /* ---- manual entries (dev mock: minutes-since-midnight view model) ---- */

  private manualConflict(date: string, start: number, end: number, excludeId?: number): string | null {
    for (const r of this.records) {
      if (r.date !== date) continue
      if (r.id === excludeId || r.cat === 'gap' || r.cat === 'idle') continue
      if (r.start < end && r.end > start) {
        const kind = r.manual ? 'manual entry' : 'recorded activity'
        return `Overlaps ${kind} “${r.app}”. Adjust the times or edit that record first.`
      }
    }
    return null
  }

  private writeManual(existingId: number | null, dateRaw: string, start: number, end: number, label: string, cat: string, project: string): void {
    const date = mapDate(dateRaw)
    const conflict = this.manualConflict(date, start, end, existingId ?? undefined)
    if (conflict) throw new Error(conflict)
    this.snapshot()
    this.records = this.records.filter((r) => {
      if (r.date !== date) return true
      if (r.cat === 'gap' || r.cat === 'idle') return !(r.start < end && r.end > start)
      return true
    })
    if (existingId != null) this.records = this.records.filter((r) => r.id !== existingId)
    this.records.push({
      id: this.nextId++, date, start, end, app: label, detail: label, cat: cat as Category, project,
      focus: 0, kind: 'manual', source: 'Manual entry', corrected: true, manual: true, projectSource: project ? 'user' : '',
    })
  }

  async manualEntryAdd(startMs: number, endMs: number, label: string, cat: string, project: string): Promise<void> {
    const date = localDateStr(new Date(startMs))
    const dayStart = new Date(startMs); dayStart.setHours(0, 0, 0, 0)
    this.writeManual(null, date, (startMs - dayStart.getTime()) / 60000, (endMs - dayStart.getTime()) / 60000, label, cat, project)
  }

  async manualEntryUpdate(id: number, startMs: number, endMs: number, label: string, cat: string, project: string): Promise<void> {
    const r = this.records.find((x) => x.id === id)
    if (!r?.manual) throw new Error('Only manual entries can be edited here.')
    const dayStart = new Date(startMs); dayStart.setHours(0, 0, 0, 0)
    this.writeManual(id, r.date, (startMs - dayStart.getTime()) / 60000, (endMs - dayStart.getTime()) / 60000, label, cat, project)
  }

  async manualEntryDelete(id: number): Promise<void> {
    const r = this.records.find((x) => x.id === id)
    if (!r?.manual) throw new Error('Only manual entries can be removed here.')
    this.snapshot()
    this.records = this.records.filter((x) => x.id !== id)
  }

  /* ---- work blocks (grouping): mirrors the core's 7-minute chaining ---- */

  async getWorkBlocks(date: string): Promise<{ date: string; blocks: WorkBlock[]; breakCount: number }> {
    const rows = this.records.filter((r) => r.date === mapDate(date)).slice().sort((a, b) => a.start - b.start)
    const activity = rows.filter((r) => ['productive', 'distracting', 'neutral', 'unclassified'].includes(r.cat))
    type MockBlock = WorkBlock & { spanEnd: number; lastCat: string }
    const blocks: MockBlock[] = []
    const GAP_MAX = 7
    for (const r of activity) {
      const last = blocks[blocks.length - 1]
      const breakHere = this.groupBreaks.some((b) => b > r.start - 1440 && b <= r.start)
      const chains = last && r.start - last.spanEnd <= GAP_MAX && !breakHere && !(last.project && r.project && last.project !== r.project)
      if (chains && last) {
        last.spanEnd = r.end
        last.minutes += r.end - r.start
        if (!last.apps.includes(r.app)) last.apps.push(r.app)
        if (r.project && !last.project) last.project = r.project
        if (r.cat !== last.lastCat) last.switches++
        last.lastCat = r.cat
        last.intervalIds.push(r.id)
      } else {
        blocks.push({
          startMs: 0, endMs: (r.end - r.start) * 60000, apps: [r.app], dominant: r.app, project: r.project,
          minutes: r.end - r.start, spanMinutes: r.end - r.start, internalAwayMinutes: 0, switches: 0,
          intervalIds: [r.id], userBoundary: breakHere, spanEnd: r.end, lastCat: r.cat,
        })
      }
    }
    return { date: mapDate(date), blocks, breakCount: this.groupBreaks.length }
  }

  async groupBreakAdd(atTs: number): Promise<void> { if (!this.groupBreaks.includes(atTs)) this.groupBreaks.push(atTs) }
  async groupBreakRemove(atTs: number): Promise<void> { this.groupBreaks = this.groupBreaks.filter((b) => b !== atTs) }

  async getIdleExceptions(): Promise<{ id: number; label: string }[]> { return this.idleExceptions.map((e) => ({ ...e })) }
  async idleExceptionAdd(label: string): Promise<void> { this.idleExceptions.push({ id: Date.now(), label: label.trim() }) }
  async idleExceptionRemove(id: number): Promise<void> { this.idleExceptions = this.idleExceptions.filter((e) => e.id !== id) }

  async getBlockList(): Promise<BlockListEntry[]> { return this.blockList.map((b) => ({ ...b })) }
  async blockListAdd(kind: 'app' | 'domain', target: string, label: string): Promise<void> {
    if (this.blockList.some((b) => b.kind === kind && b.target.toLowerCase() === target.toLowerCase())) throw new Error('That target is already on the block list.')
    this.blockList.push({ id: Date.now(), kind, target: target.trim(), label: label.trim() || target.trim(), enabled: true })
  }
  async blockListRemove(id: number): Promise<void> { this.blockList = this.blockList.filter((b) => b.id !== id) }
  async blockListSetEnabled(id: number, enabled: boolean): Promise<void> {
    const b = this.blockList.find((x) => x.id === id)
    if (b) b.enabled = enabled
  }

  async retentionStatus(): Promise<RetentionStatus> {
    const cutoffDays = this.settings.retention === '30 days' ? 30 : this.settings.retention === '3 months' ? 90 : this.settings.retention === '6 months' ? 180 : this.settings.retention === '1 year' ? 365 : null
    return { active: this.settings.retentionActive && cutoffDays !== null, preference: this.settings.retention, cutoffDays, lastRunMs: this.retentionLastRunMs }
  }

  async retentionSetActive(active: boolean): Promise<number> {
    const cutoffDays = this.settings.retention === '30 days' ? 30 : this.settings.retention === '3 months' ? 90 : this.settings.retention === '6 months' ? 180 : this.settings.retention === '1 year' ? 365 : null
    if (active && cutoffDays === null) throw new Error('Choose a retention period before activating automatic deletion.')
    this.settings.retentionActive = active
    return active ? this.retentionRunNow() : 0
  }

  async retentionRunNow(): Promise<number> {
    const status = await this.retentionStatus()
    if (!status.active || status.cutoffDays === null) return 0
    const cutoff = addDays(realToday(), -status.cutoffDays)
    const before = this.records.length
    this.records = this.records.filter((r) => r.date >= cutoff)
    this.retentionLastRunMs = Date.now()
    return before - this.records.length
  }

  async pauseTracking(): Promise<void> {
    if (this.tracking.state === 'active' || this.tracking.state === 'degraded') {
      this.tracking.state = 'paused'
      this.emit('tracking-changed', { ...this.tracking })
    }
  }

  async resumeTracking(): Promise<void> {
    if (this.tracking.state === 'paused') {
      this.tracking.state = 'active'
      this.emit('tracking-changed', { ...this.tracking })
    }
  }

  async enableTracking(): Promise<void> {
    this.tracking.state = 'active'
    this.emit('tracking-changed', { ...this.tracking })
  }

  async disableTracking(): Promise<void> {
    this.tracking.state = 'disabled'
    this.emit('tracking-changed', { ...this.tracking })
  }

  async simulateUnknownRecovery(): Promise<void> {
    if (['paused', 'privacy', 'disabled'].includes(this.tracking.state)) return
    this.tracking.state = 'confirmation'
    this.emit('tracking-changed', { ...this.tracking })
  }

  async setPrivacy(on: boolean): Promise<void> {
    if (on) {
      this.tracking.privacyReturn = this.tracking.state
      this.tracking.state = 'privacy'
    } else if (this.tracking.state === 'privacy') {
      this.tracking.state = this.tracking.privacyReturn
    }
    this.emit('tracking-changed', { ...this.tracking })
  }

  async setDnd(on: boolean): Promise<void> { this.dnd = on }

  async updateSettings(patch: Partial<Settings>): Promise<void> {
    this.settings = { ...this.settings, ...patch }
    if (patch.platform) {
      // platform selection in the dev mock only switches the illustrative capability table
      this.settings.platform = patch.platform
    }
    if (patch.displayName !== undefined) {
      const trimmed = patch.displayName.trim()
      this.profileName = trimmed || 'Alex Morgan'
    }
    // Mirrors the Tauri backend: settings mutations confirm via state-changed so the
    // store refreshes and toggle UIs reflect the stored value.
    this.emit('state-changed', {})
  }

  async completeOnboarding(consent: boolean): Promise<void> {
    this.onboarded = true
    if (!consent) {
      if (this.tracking.state === 'privacy') this.tracking.privacyReturn = 'disabled'
      else this.tracking.state = 'disabled'
    } else if (this.tracking.state === 'confirmation') {
      this.tracking.state = 'active'
    }
    this.emit('tracking-changed', { ...this.tracking })
  }

  async createBackup(dir: string | null): Promise<BackupManifest> {
    void dir
    const t = this.totals(this.records)
    return {
      createdAt: new Date().toISOString(), intervals: this.records.length, projects: this.projects.length,
      rules: this.rules.length, trackedMinutes: t.tracked, rangeStart: '2026-10-12', rangeEnd: TODAY,
      sizeBytes: 0, integrity: 'passed', path: '(dev mock — no file created)',
    }
  }

  async inspectBackup(path: string): Promise<RestorePlanSummary> {
    void path
    return { strategy: 'merge', preservePrivacy: true, allowDeletedReintroduction: false, backupIntervalCount: 84, backupProjectCount: 3, conflicts: 2, previouslyDeletedRecords: 0 }
  }

  async restoreBackup(path: string, plan: RestorePlanSummary): Promise<{ restored: number; conflictsSkipped: number }> {
    void path, plan
    return { restored: 0, conflictsSkipped: 0 }
  }

  async exportData(format: 'CSV' | 'JSON', req: ReportRequest, dir: string | null): Promise<string> {
    void req, dir
    return `(dev mock — ${format} export preview only)`
  }

  async quitApp(): Promise<void> { /* no-op in browser */ }

  on<E extends AppEvent>(event: E, handler: (payload: AppEventMap[E]) => void): () => void {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set())
    const set = this.listeners.get(event)!
    const fn = handler as (p: unknown) => void
    set.add(fn)
    return () => set.delete(fn)
  }
}

export const mockBackend: Backend = new MockBackend()

// keep fmt referenced for parity with prototype helpers used by views
void fmt
