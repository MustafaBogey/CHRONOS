import { useSyncExternalStore } from 'react'
import { backend, type OverviewData, type ReportData, type ProjectWithTime, type ReportRequest, type AppState, type CurrentActivity } from './backend'
import type {
  ActivityRecord, Diagnostics, Page, FocusTab, ReportTab, SettingsTab, SessionHistoryEntry, SessionSnapshot,
} from './types'
import { addDays, localDateStr, weekStart } from './format'

export type ModalState =
  | { kind: 'activity'; recordId: number }
  | { kind: 'split'; recordId: number }
  | { kind: 'goals' }
  | { kind: 'review' }
  | { kind: 'rules' }
  | { kind: 'exclusions' }
  | { kind: 'deletion' }
  | { kind: 'backup' }
  | { kind: 'restore' }
  | { kind: 'export' }
  | { kind: 'project'; projectId?: string }
  | { kind: 'project-delete'; projectId: string }
  | { kind: 'assign-activities'; projectId: string }
  | { kind: 'manual-entry'; recordId?: number }
  | { kind: 'blocklist' }
  | { kind: 'idle-exceptions' }
  | { kind: 'schedule' }
  | { kind: 'session-start' }
  | { kind: 'onboarding' }
  | { kind: 'notifications' }
  | { kind: 'integration'; name: string }
  | { kind: 'diagnostics' }
  | { kind: 'recovery' }
  | { kind: 'performance' }
  | { kind: 'tracking-enable' }
  | { kind: 'privacy-leave' }
  | { kind: 'quit' }

export interface Store {
  ready: boolean
  appState: AppState | null
  capabilities: { platform: string; capabilities: Diagnostics['capabilities'] } | null

  page: Page
  focusTab: FocusTab
  reportTab: ReportTab
  settingsTab: SettingsTab

  timelineDate: string
  timelineFilter: string
  timelineSearch: string
  /** Timeline presentation: aggregated per application, or chronological. */
  timelineView: 'total' | 'daily'
  /** Total Activity: which applications are expanded (keyed by app name). */
  timelineExpanded: Record<string, boolean>

  reportPeriod: ReportRequest['period']
  reportStart: string
  reportEnd: string
  reportProject: string

  overview: OverviewData | null
  timelineRecords: ActivityRecord[] | null
  report: ReportData | null
  projectsWithTime: ProjectWithTime[] | null
  focusHistory: SessionHistoryEntry[] | null
  diagnostics: Diagnostics | null
  /** Live current-activity snapshot (event-driven; durations tick locally). */
  currentActivity: CurrentActivity | null

  modal: ModalState | null
  toast: { message: string; undoable: boolean } | null
  notice: { type: 'break' | 'complete' | 'degraded' | 'block' | 'break-end' | 'break-schedule'; title: string; body: string } | null
  onboardStep: number
  onboardConsent: boolean

  now: number
}

const today = localDateStr()

const initial: Store = {
  ready: false,
  appState: null,
  capabilities: null,
  page: 'overview',
  focusTab: 'sessions',
  reportTab: 'summary',
  settingsTab: 'General',
  timelineDate: today,
  timelineFilter: 'all',
  timelineSearch: '',
  timelineView: 'total',
  timelineExpanded: {},
  reportPeriod: 'week',
  reportStart: weekStart(today),
  reportEnd: today,
  reportProject: 'all',
  overview: null,
  timelineRecords: null,
  report: null,
  projectsWithTime: null,
  focusHistory: null,
  diagnostics: null,
  currentActivity: null,
  modal: null,
  toast: null,
  notice: null,
  onboardStep: 0,
  onboardConsent: false,
  now: Date.now(),
}

type Listener = () => void
const listeners = new Set<Listener>()
let store: Store = initial

function emit() { listeners.forEach((l) => l()) }

function set(patch: Partial<Store>) {
  store = { ...store, ...patch }
  emit()
}

export function getStore(): Store { return store }

export function useStore<T>(selector: (s: Store) => T): T {
  return useSyncExternalStore(
    (cb) => { listeners.add(cb); return () => listeners.delete(cb) },
    () => selector(store),
  )
}

/* ---------------- data loading ---------------- */

export async function refreshState() {
  const appState = await backend.getState()
  set({ appState })
}

export async function refreshView() {
  const s = store
  if (s.page === 'overview') {
    const o = await backend.getOverview(localDateStr())
    set({ overview: o })
  } else if (s.page === 'timeline') {
    const rows = await backend.getTimeline(s.timelineDate)
    set({ timelineRecords: rows })
  } else if (s.page === 'reports') {
    const r = await backend.getReport(currentReportRequest())
    set({ report: r })
  }
  if (s.page === 'focus' && s.focusTab === 'projects') {
    set({ projectsWithTime: await backend.getProjectsWithTime() })
  }
  if (s.page === 'focus' && s.focusTab === 'sessions') {
    set({ focusHistory: await backend.getFocusHistory(localDateStr()) })
  }
  await refreshState()
}

export function currentReportRequest(): ReportRequest {
  const s = store
  if (s.reportPeriod === 'day') return { period: 'day', start: localDateStr(), end: localDateStr(), project: s.reportProject }
  if (s.reportPeriod === 'month') {
    const n = new Date()
    const first = `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-01`
    const lastDay = new Date(n.getFullYear(), n.getMonth() + 1, 0).getDate()
    return { period: 'month', start: first, end: `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-${String(lastDay).padStart(2, '0')}`, project: s.reportProject }
  }
  if (s.reportPeriod === 'custom') return { period: 'custom', start: s.reportStart, end: s.reportEnd, project: s.reportProject }
  const ws = weekStart(localDateStr())
  return { period: 'week', start: ws, end: addDays(ws, 6), project: s.reportProject }
}

/** Persist the selected primary destination so a refresh returns to the same page. */
const PAGE_KEY = 'chronos.page'
const VALID_PAGES: Page[] = ['overview', 'timeline', 'focus', 'reports', 'settings']

function storedPage(): Page {
  try {
    const v = window.localStorage.getItem(PAGE_KEY)
    if (v && (VALID_PAGES as string[]).includes(v)) return v as Page
  } catch { /* storage unavailable */ }
  return 'overview'
}

function persistPage(page: Page) {
  try { window.localStorage.setItem(PAGE_KEY, page) } catch { /* ignore */ }
}

/* Settings deep-links: #settings/<section> survives refresh and supports direct
   navigation; unknown sections fall back to General. */
const SETTINGS_SLUG_TO_TAB: Record<string, SettingsTab> = {
  'general': 'General',
  'tracking-classification': 'Tracking & Classification',
  'privacy-data': 'Privacy & Data',
  'focus-notifications': 'Focus & Notifications',
  'integrations': 'Integrations',
  'advanced-diagnostics': 'Advanced & Diagnostics',
  'help-about': 'Help & About',
}

function settingsTabFromHash(): { tab: SettingsTab; onSettings: boolean } {
  const m = /^#settings\/([a-z-]+)/.exec(window.location.hash)
  if (!m) return { tab: 'General', onSettings: false }
  return { tab: SETTINGS_SLUG_TO_TAB[m[1]] ?? 'General', onSettings: true }
}

export async function boot() {
  // Restore the persisted destination before first render so refresh keeps the page;
  // a #settings/<section> deep link takes precedence and lands on that section.
  const deep = settingsTabFromHash()
  if (deep.onSettings) {
    set({ page: 'settings', settingsTab: deep.tab })
    persistPage('settings')
  } else {
    set({ page: storedPage() })
  }
  await refreshState()
  const caps = await backend.getCapabilities()
  set({ capabilities: caps, ready: true })
  await refreshView()

  const appState = store.appState
  if (appState && !appState.onboarded) {
    set({ onboardStep: 0, onboardConsent: false, modal: { kind: 'onboarding' } })
  }

  // Live activity: seed once, then follow engine events. The visible duration
  // ticks locally from the authoritative startMs (1s clock below).
  try { set({ currentActivity: await backend.getCurrentActivity() }) } catch { /* unavailable until tracking starts */ }
  backend.on('current-activity', (snap) => { set({ currentActivity: snap }) })

  backend.on('tracking-changed', () => { void refreshState() })
  // Settings mutations (update_settings / set_dnd) confirm through this event so every
  // toggle reflects the stored value immediately; without it toggles desync visually.
  backend.on('state-changed', () => { void refreshState() })
  backend.on('session-tick', (session: SessionSnapshot) => {
    if (store.appState) set({ appState: { ...store.appState, session } })
  })
  backend.on('activity-changed', () => { void refreshView() })
  backend.on('notice', (n) => { set({ notice: n }) })

  // One-second clock: drives the live elapsed durations (and the wall clock).
  // Re-renders only components that select `now`.
  window.setInterval(() => set({ now: Date.now() }), 1000)
}

/* ---------------- UI actions ---------------- */

export const actions = {
  navigate(page: Page) {
    set({ page, modal: null })
    persistPage(page)
    // Leaving Settings clears the #settings/<section> fragment so the URL matches the view.
    if (page !== 'settings' && window.location.hash.startsWith('#settings/')) {
      window.history.replaceState(null, '', window.location.pathname + window.location.search)
    }
    void refreshView()
  },
  setFocusTab(tab: FocusTab) { set({ focusTab: tab }); void refreshView() },
  setReportTab(tab: ReportTab) { set({ reportTab: tab }); void refreshView() },
  setSettingsTab(tab: SettingsTab) { set({ settingsTab: tab }) },

  setTimelineDate(date: string) { set({ timelineDate: date }); void refreshView() },
  shiftTimelineDay(delta: number) { set({ timelineDate: addDays(store.timelineDate, delta) }); void refreshView() },
  setTimelineFilter(filter: string) { set({ timelineFilter: filter }); void refreshView() },
  setTimelineSearch(search: string) { set({ timelineSearch: search }) },
  setTimelineView(view: 'total' | 'daily') { set({ timelineView: view }) },
  toggleAppExpanded(key: string) { set({ timelineExpanded: { ...store.timelineExpanded, [key]: store.timelineExpanded[key] !== true } }) },

  setReportPeriod(period: ReportRequest['period']) {
    const todayStr = localDateStr()
    const patch: Partial<Store> = { reportPeriod: period }
    if (period === 'week') { const ws = weekStart(todayStr); patch.reportStart = ws; patch.reportEnd = addDays(ws, 6) }
    if (period === 'day') { patch.reportStart = todayStr; patch.reportEnd = todayStr }
    if (period === 'month') {
      const n = new Date()
      const lastDay = new Date(n.getFullYear(), n.getMonth() + 1, 0).getDate()
      patch.reportStart = `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-01`
      patch.reportEnd = `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-${String(lastDay).padStart(2, '0')}`
    }
    set(patch)
    void refreshView()
  },
  setReportRange(start: string, end: string) { set({ reportStart: start, reportEnd: end }); void refreshView() },
  setReportProject(project: string) { set({ reportProject: project }); void refreshView() },

  openModal(modal: ModalState) { set({ modal }) },
  closeModal() { set({ modal: null }) },
  async loadDiagnostics() {
    try {
      const diagnostics = await backend.getDiagnostics()
      set({ diagnostics })
    } catch {
      // Leave any previous value; the UI shows an explicit unavailable state.
    }
  },
  setOnboardStep(step: number) { set({ onboardStep: step }) },
  setOnboardConsent(consent: boolean) { set({ onboardConsent: consent }) },
  showNotice(type: 'break' | 'complete' | 'degraded' | 'block' | 'break-end' | 'break-schedule', title: string, body: string) { set({ notice: { type, title, body } }) },

  toast(message: string, undoable = false) {
    set({ toast: { message, undoable } })
    window.setTimeout(() => {
      if (store.toast?.message === message) set({ toast: null })
    }, 8000)
  },
  clearToast() { set({ toast: null }) },
  dismissNotice() { set({ notice: null }) },
}
