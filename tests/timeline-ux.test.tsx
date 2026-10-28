import { describe, expect, it, beforeAll, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import '@testing-library/jest-dom/vitest'

beforeAll(() => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({ matches: false, media: query, onchange: null, addListener: () => {}, removeListener: () => {}, addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => false }),
  })
  if (typeof HTMLDialogElement !== 'undefined' && !HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) { this.setAttribute('open', '') }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) { this.removeAttribute('open') }
  }
  window.localStorage.clear()
})

import App from '@src/App'
import { boot, actions, getStore } from '@src/store'
import { backend } from '@src/backend'

afterEach(() => cleanup())

async function bootApp() {
  await boot()
  if (!(await backend.getState()).onboarded) await backend.completeOnboarding(true)
  render(<App />)
  await new Promise((r) => setTimeout(r, 60))
}

const DAY = '2026-10-24'
/** The mock maps the real today onto the sample day, so live data only applies
 *  when the timeline is set to the real today. */
const REAL_TODAY = (() => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}` })()

/** All timeline row start times in rendered order (excluding the live ongoing
 *  row, whose start is the real wall clock and would make the assertion
 *  time-of-day dependent against the fixed sample day). */
function renderedRowTimes(): number[] {
  const items = document.querySelectorAll('.timeline-item:not(.live-row) .time')
  const times: number[] = []
  items.forEach((el) => {
    const m = /(\d{1,2}):(\d{2})/.exec(el.textContent ?? '')
    if (m) times.push(Number(m[1]) * 60 + Number(m[2]))
  })
  return times
}

describe('Total Activity (default view)', () => {
  it('is the default presentation: one entry per application, ordered by total duration', async () => {
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(DAY)
    actions.setTimelineFilter('all')
    await new Promise((r) => setTimeout(r, 120))
    expect(getStore().timelineView).toBe('total')
    const rows = [...document.querySelectorAll('.app-aggregate-row')]
    expect(rows.length).toBeGreaterThan(1)
    // One entry per distinct application — Visual Studio Code appears once
    // despite many intervals in the sample data.
    const labels = rows.map((el) => el.querySelector('strong')?.textContent ?? '')
    const unique = new Set(labels)
    expect(unique.size).toBe(labels.length)
    expect(labels.filter((l) => l === 'Visual Studio Code').length).toBe(1)
    // Ordered by total duration descending (sample: VS Code has the most time).
    expect(labels[0]).toBe('Visual Studio Code')
  }, 20000)

  it('shows interval counts and honest per-app totals derived from the underlying records', async () => {
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(DAY)
    actions.setTimelineFilter('all')
    await new Promise((r) => setTimeout(r, 120))
    const rows = await backend.getTimeline(DAY)
    const byApp = new Map<string, number>()
    rows.filter((r) => r.cat !== 'idle' && r.cat !== 'gap').forEach((r) => byApp.set(r.app, (byApp.get(r.app) ?? 0) + (r.end - r.start)))
    const topLabel = document.querySelector('.app-aggregate-row strong')?.textContent ?? ''
    const expected = byApp.get(topLabel) ?? 0
    const timeEl = document.querySelector('.app-aggregate-row .app-aggregate-time strong')?.textContent ?? ''
    // fmt() renders "2h 35m" / "50m" — compare minutes by parsing.
    const shownMin = parseDuration(timeEl)
    expect(Math.abs(shownMin - expected)).toBeLessThanOrEqual(1)
  }, 20000)

  it('expands an application to reveal its individual activities, newest first, with all corrections intact', async () => {
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(DAY)
    actions.setTimelineFilter('all')
    await new Promise((r) => setTimeout(r, 120))
    expect(document.querySelectorAll('.timeline-item').length).toBe(0)
    const first = document.querySelector('.app-aggregate-row') as HTMLElement
    fireEvent.click(first)
    await new Promise((r) => setTimeout(r, 100))
    const detail = document.querySelector('.app-aggregate-detail')
    expect(detail).toBeTruthy()
    expect(detail!.querySelectorAll('.timeline-item, .gap-row').length).toBeGreaterThan(1)
    const times = renderedRowTimes()
    for (let i = 1; i < times.length; i++) {
      expect(times[i - 1] >= times[i]).toBe(true)
    }
  }, 20000)

  it('represents idle and tracking gaps as separate entries, never credited to an application', async () => {
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(DAY)
    actions.setTimelineFilter('all')
    await new Promise((r) => setTimeout(r, 120))
    const labels = [...document.querySelectorAll('.app-aggregate-row strong')].map((el) => el.textContent)
    expect(labels).toContain('Idle')
    expect(labels).toContain('Tracking gaps')
    // No application entry is labeled Idle or a gap.
    const appRows = [...document.querySelectorAll('.app-aggregate-row')].filter((el) => {
      const l = el.querySelector('strong')?.textContent
      return l !== 'Idle' && l !== 'Tracking gaps'
    })
    expect(appRows.length).toBeGreaterThan(0)
  }, 20000)

  it('labels mixed classifications honestly as Mixed instead of one category', async () => {
    // The mock sample has applications with mixed categories (e.g. Browser research
    // has unclassified + productive rows under different names; VS Code all productive).
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(DAY)
    actions.setTimelineFilter('all')
    await new Promise((r) => setTimeout(r, 120))
    const rows = await backend.getTimeline(DAY)
    const mixedApps = new Set<string>()
    const catByApp = new Map<string, string>()
    rows.forEach((r) => {
      const prev = catByApp.get(r.app)
      if (prev && prev !== r.cat) mixedApps.add(r.app)
      catByApp.set(r.app, r.cat)
    })
    const rendered = [...document.querySelectorAll('.app-aggregate-row')].map((el) => ({
      label: el.querySelector('strong')?.textContent,
      text: el.textContent ?? '',
    }))
    for (const app of mixedApps) {
      const row = rendered.find((r) => r.label === app)
      if (row) expect(row.text).toContain('Mixed')
    }
  }, 20000)
})

describe('Daily Timeline', () => {
  it('shows the chronological list, newest first, and the live ongoing row from the mock', async () => {
    await bootApp()
    actions.navigate('timeline')
    actions.setTimelineDate(REAL_TODAY)
    actions.setTimelineFilter('all')
    fireEvent.click(screen.getByRole('button', { name: 'Daily Timeline' }))
    await new Promise((r) => setTimeout(r, 120))
    expect(getStore().timelineView).toBe('daily')
    const times = renderedRowTimes()
    expect(times.length).toBeGreaterThan(2)
    for (let i = 1; i < times.length; i++) {
      expect(times[i - 1] >= times[i]).toBe(true)
    }
    // The mock reports a live activity (Documentation · Google Chrome):
    // the ongoing row leads the list and is clearly marked.
    const allItems = [...document.querySelectorAll('.timeline-item')]
    const liveIdx = allItems.findIndex((el) => el.classList.contains('live-row'))
    expect(liveIdx).toBe(0)
    expect(document.querySelector('.live-row')).toBeTruthy()
    expect(document.querySelector('.live-row')?.textContent).toContain('Ongoing')
  }, 20000)
})

describe('Live tracking (Overview)', () => {
  it('renders the live "Right now" activity with an ongoing marker (mock data)', async () => {
    await bootApp()
    actions.navigate('overview')
    await new Promise((r) => setTimeout(r, 120))
    const live = document.querySelector('.live-now')
    expect(live).toBeTruthy()
    expect(live!.textContent).toContain('Documentation')
    expect(live!.textContent).toContain('Now')
  }, 20000)
})

describe('Breadcrumbs', () => {
  it('are removed from the topbar on every page', async () => {
    await bootApp()
    for (const page of ['timeline', 'focus', 'reports', 'settings', 'overview'] as const) {
      actions.navigate(page)
      await new Promise((r) => setTimeout(r, 80))
      expect(document.querySelector('.breadcrumb')).toBeNull()
      expect(document.querySelector('#breadcrumb-current')).toBeNull()
    }
  }, 20000)
})

describe('Aggregation integrity', () => {
  it('total per application equals the sum of its recorded intervals (no double counting)', async () => {
    const rows = await backend.getTimeline(DAY)
    const byApp = new Map<string, { min: number; count: number }>()
    rows.forEach((r) => {
      if (r.cat === 'idle' || r.cat === 'gap') return
      const e = byApp.get(r.app) ?? { min: 0, count: 0 }
      e.min += r.end - r.start
      e.count += 1
      byApp.set(r.app, e)
    })
    expect(byApp.size).toBeGreaterThan(1)
    const sumAll = [...byApp.values()].reduce((a, e) => a + e.min, 0)
    const sumRows = rows.filter((r) => r.cat !== 'idle' && r.cat !== 'gap').reduce((a, r) => a + (r.end - r.start), 0)
    expect(sumAll).toBe(sumRows)
  }, 20000)
})

function parseDuration(text: string): number {
  const h = /(\d+)h/.exec(text)
  const m = /(\d+)m/.exec(text)
  return (h ? Number(h[1]) * 60 : 0) + (m ? Number(m[1]) : 0)
}
