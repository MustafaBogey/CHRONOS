import { describe, expect, it, beforeEach, beforeAll } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import '@testing-library/jest-dom/vitest'

/**
 * Component tests run against the dev mock backend by rendering the views with a prepared store.
 * The mock reproduces the prototype's illustrative dataset.
 */

// jsdom lacks matchMedia and <dialog>.showModal: provide both before importing components.
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

describe('CHRONOS application shell', () => {
  it('boots and renders the five primary destinations', async () => {
    await boot()
    render(<App />)
    for (const label of ['Overview', 'Timeline', 'Focus & Goals', 'Reports', 'Settings & Integrations']) {
      expect(await screen.findByRole('button', { name: label })).toBeInTheDocument()
    }
  }, 15000)

  it('settings mutations refresh appState so toggles reflect the stored value (regression: silent toggle desync)', async () => {
    await boot()
    const before = (await import('@src/store')).getStore().appState?.settings.miniBar
    const { backend } = await import('@src/backend')
    await backend.updateSettings({ miniBar: !before })
    // The store must learn the new value without any other trigger (the state-changed
    // event wired in boot() is what updates Settings toggles in the real app).
    const after = (await import('@src/store')).getStore().appState?.settings.miniBar
    expect(after).toBe(!before)
  }, 15000)

  it('date format selection is exposed and configurable', async () => {
    const { configureDateFormat, fmtDate, DATE_FORMATS } = await import('@src/format')
    for (const f of DATE_FORMATS) {
      configureDateFormat(f.value)
      expect(fmtDate('2026-10-27')).toBe(f.label)
    }
    configureDateFormat('medium')
  }, 15000)

  it('shows the approved overview composition', async () => {
    await boot()
    render(<App />)
    expect(await screen.findByText(/Today’s Focus/)).toBeInTheDocument()
    expect(await screen.findByText(/Today’s activity/)).toBeInTheDocument()
    expect(await screen.findByText(/Recent activity/)).toBeInTheDocument()
    expect(await screen.findByText(/Focus Session/)).toBeInTheDocument()
    expect(await screen.findByText(/Today’s Goals/)).toBeInTheDocument()
    expect(await screen.findByText(/Weekly Overview/)).toBeInTheDocument()
    // Handover baseline metrics from the sample day.
    expect(await screen.findByText('6h 20m')).toBeInTheDocument()
    expect(await screen.findByText('5h 30m')).toBeInTheDocument()
  }, 15000)

  it('navigates to Timeline and shows sample intervals with filters', async () => {
    await boot()
    render(<App />)
    const timeline = await screen.findByRole('button', { name: 'Timeline' })
    timeline.click()
    expect(await screen.findByText(/Your day, in perspective/)).toBeInTheDocument()
    expect(await screen.findByText(/authoritative intervals/)).toBeInTheDocument()
    const review = await screen.findByRole('button', { name: /^Review · \d+/ })
    expect(review).toBeInTheDocument()
  }, 15000)

  it('navigates to Focus & Goals with its four tabs', async () => {
    await boot()
    render(<App />)
    ;(await screen.findByRole('button', { name: 'Focus & Goals' })).click()
    for (const tab of ['Focus sessions', 'Goals', 'Projects', 'Distraction management']) {
      expect(await screen.findByRole('tab', { name: tab })).toBeInTheDocument()
    }
  }, 15000)

  it('navigates to Reports with its six tabs', async () => {
    await boot()
    render(<App />)
    ;(await screen.findByRole('button', { name: 'Reports' })).click()
    for (const tab of ['Summary', 'Applications', 'Websites', 'Categories', 'Projects']) {
      expect(await screen.findByRole('tab', { name: tab })).toBeInTheDocument()
    }
    expect(await screen.findByRole('tab', { name: 'Focus & Daily Detail' })).toBeInTheDocument()
  }, 15000)

  it('navigates to Settings with its six groups', async () => {
    await boot()
    render(<App />)
    ;(await screen.findByRole('button', { name: 'Settings & Integrations' })).click()
    for (const group of ['General', 'Tracking & Classification', 'Privacy & Data', 'Focus & Notifications', 'Integrations', 'Advanced & Diagnostics']) {
      expect(await screen.findByRole('button', { name: group })).toBeInTheDocument()
    }
  }, 15000)
})

beforeEach(() => {
  cleanup()
})
