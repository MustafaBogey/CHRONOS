import { describe, expect, it, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom/vitest'

/**
 * Focus Session idle pause (1.3.1): when the tracking engine detects idle, the
 * focus countdown freezes at the preserved remaining time, the UI says why, and
 * only an explicit Resume Focus continues it. Verified against the mock backend
 * with an engine-simulated session snapshot (the mock does not model idle).
 */

beforeAll(() => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({ matches: false, media: query, onchange: null, addListener: () => {}, removeListener: () => {}, addEventListener: () => {}, removeEventListener: () => {}, dispatchEvent: () => false }),
  })
  if (typeof HTMLDialogElement !== 'undefined' && !HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) { this.setAttribute('open', '') }
    HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) { this.removeAttribute('open') }
  }
})

import '@src/styles/chronos.css'
import App from '@src/App'
import { boot } from '@src/store'
import { backend } from '@src/backend'
import type { MockBackend } from '@src/backend/mock'

async function renderOverview() {
  await boot()
  render(<App />)
  await screen.findByText(/Focus Session/)
}

/** The break-stat rows split their value into a <strong> and carry a "?" help
 *  affordance; match the meaningful part of the row text. */
const rowOf = (text: string) => (_: string, el: Element | null) =>
  el?.tagName === 'P' && !!el.textContent?.replace(/\s+/g, ' ').includes(text)

afterEach(() => cleanup())

async function simulateIdlePause(min: number) {
  await backend.startSession('Deep work', '', 20, 5, true)
  ;(backend as unknown as MockBackend).debugSetSession({
    status: 'running', phase: 'focus', focusIdlePaused: true, phaseRemainingMs: min * 60000,
  })
}

describe('Focus Session idle pause', () => {
  it('shows why the countdown stopped, the preserved remaining time, and a Resume Focus action', async () => {
    await renderOverview()
    await simulateIdlePause(12)
    expect(await screen.findByText('Focus paused — away')).toBeInTheDocument()
    // The circular dial shows the frozen phase countdown, not session elapsed time.
    expect(await screen.findByText('12:00')).toBeInTheDocument()
    expect(await screen.findByText('Focus remaining')).toBeInTheDocument()
    expect(await screen.findByText(/You were away\. Resume when you're ready\./)).toBeInTheDocument()
    const resume = screen.getAllByRole('button', { name: 'Resume Focus' })
    expect(resume.length).toBeGreaterThan(0)
    // The ordinary Pause toggle is replaced while idle-paused (explicit choice required).
    expect(screen.queryByRole('button', { name: 'Pause' })).not.toBeInTheDocument()
    cleanup()
  }, 15000)

  it('returns to the active focus presentation after an explicit Resume Focus', async () => {
    await renderOverview()
    await simulateIdlePause(12)
    const resume = await screen.findAllByRole('button', { name: 'Resume Focus' })
    resume[0].click()
    await backend.resumeFocus()
    expect(await screen.findByText('In progress')).toBeInTheDocument()
    expect(screen.queryByText('Focus paused — away')).not.toBeInTheDocument()
    cleanup()
  }, 15000)

  it('keeps the break-expiry presentation distinct from the idle pause', async () => {
    await renderOverview()
    await backend.startSession('Deep work', '', 25, 5, true)
    ;(backend as unknown as MockBackend).debugSetSession({
      status: 'running', phase: 'break', breakExpired: true, focusIdlePaused: false, phaseRemainingMs: -60_000,
    })
    expect((await screen.findAllByText('Break ended')).length).toBeGreaterThan(0)
    expect(screen.queryByText('Focus paused — away')).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Another 5 Minutes' }).length).toBeGreaterThan(0)
    cleanup()
  }, 15000)

  it('shows zero break statistics before the first break, updating from session ticks', async () => {
    await renderOverview()
    await backend.startSession('Deep work', '', 25, 5, true)
    expect(await screen.findByText(rowOf('0m total break time'))).toBeInTheDocument()
    expect(screen.getByText(rowOf('0 breaks taken'))).toBeInTheDocument()
    expect(screen.getByText(rowOf('0m break overrun'))).toBeInTheDocument()
    // A running break with an overrun arrives via session-tick: the values update.
    ;(backend as unknown as MockBackend).debugSetSession({
      status: 'running', phase: 'break', breakExpired: false, phaseRemainingMs: -90_000,
      breakCount: 1, breakTotalMin: 6, breakOverrunTotalMin: 2,
    })
    expect(await screen.findByText(rowOf('6m total break time'))).toBeInTheDocument()
    expect(screen.getByText(rowOf('1 breaks taken'))).toBeInTheDocument()
    expect(screen.getByText(rowOf('2m break overrun'))).toBeInTheDocument()
    cleanup()
  }, 15000)

  it('offers discreet, accessible help for the session concepts', async () => {
    await renderOverview()
    await backend.startSession('Deep work', '', 25, 5, true)
    await screen.findByText(rowOf('0m total break time'))
    const hints = screen.getAllByRole('button', { name: /^Help: / })
    // 1.3.4: the Total break time hint was removed (4 remain); "Ready when you
    // are" badge removed when no session runs.
    expect(hints.length).toBe(4)
    expect(hints.some((b) => b.getAttribute('aria-label') === 'Help: Total break time')).toBe(false)
    // Click toggles the popover open (aria-expanded + visible tooltip).
    const quality = hints.find((b) => b.getAttribute('aria-label') === 'Help: Focus quality')!
    quality.click()
    await waitFor(() => expect(quality.getAttribute('aria-expanded')).toBe('true'))
    expect(screen.getAllByText(/Qualified focus ÷ elapsed session time/).length).toBeGreaterThan(0)
    // 1.3.4: no "Ready when you are" badge once the session ends (active-session
    // indicators are preserved).
    await backend.endSession()
    await waitFor(() => expect(screen.queryByText('Ready when you are')).not.toBeInTheDocument())
    expect(await screen.findByRole('button', { name: 'Start a focus session' })).toBeInTheDocument()
    cleanup()
  }, 15000)
})
