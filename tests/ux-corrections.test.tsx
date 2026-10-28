import { describe, expect, it, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import '@testing-library/jest-dom/vitest'

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
  window.localStorage.clear()
})

import App from '@src/App'
import { boot, actions, getStore } from '@src/store'
import { backend } from '@src/backend'
import { blockingSummaryText } from '@src/components/blocking'

afterEach(() => cleanup())

async function bootApp() {
  await boot()
  // The mock user starts un-onboarded; App's own boot effect would then keep
  // re-opening the onboarding modal and race these tests. Complete it first.
  if (!(await backend.getState()).onboarded) {
    await backend.completeOnboarding(true)
  }
  render(<App />)
  // Let App's self-boot effect settle before interacting.
  await new Promise((r) => setTimeout(r, 50))
}

function modalElement(): HTMLDialogElement {
  return document.getElementById('modal') as HTMLDialogElement
}

describe('modal dismissal (regression: interior clicks closed dialogs)', () => {
  it('keeps the dialog open when clicking content, labels, fields, and empty interior space', async () => {
    await bootApp()
    actions.openModal({ kind: 'goals' })
    const dlg = modalElement()
    expect(dlg).toBeTruthy()
    expect(getStore().modal?.kind).toBe('goals')
    await screen.findByText('Daily focused time')

    const user = userEvent.setup()
    // A text label near a field (the reported crash spot).
    await user.click(screen.getByText('Daily focused time').closest('label') ?? screen.getByText('Daily focused time'))
    expect(getStore().modal).not.toBeNull()

    // The field itself.
    const input = screen.getByLabelText(/Daily focused time/i) as HTMLInputElement
    await user.click(input)
    await user.type(input, '0')
    expect(getStore().modal).not.toBeNull()

    // Empty interior space: the modal body element between the rows.
    fireEvent.click(dlg.querySelector('.modal-body') as HTMLElement)
    expect(getStore().modal).not.toBeNull()

    // Dropdown interaction keeps the parent open.
    const select = screen.getByRole('combobox') as HTMLSelectElement
    await user.click(select)
    fireEvent.change(select, { target: { value: select.options[1]?.value ?? '' } })
    expect(getStore().modal).not.toBeNull()
    expect(dlg.hasAttribute('open')).toBe(true)
  }, 20000)

  it('closes via the X button, Cancel, and a genuine backdrop click', async () => {
    await bootApp()
    actions.openModal({ kind: 'session-start' })
    await screen.findByText('Session intention')
    expect(getStore().modal).not.toBeNull()

    // Genuine backdrop click (target is the <dialog> element itself).
    fireEvent.click(modalElement())
    expect(getStore().modal).toBeNull()

    // X button.
    actions.openModal({ kind: 'session-start' })
    const x = await screen.findByRole('button', { name: 'Close dialog' })
    fireEvent.click(x)
    expect(getStore().modal).toBeNull()

    // Cancel.
    actions.openModal({ kind: 'session-start' })
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
    expect(getStore().modal).toBeNull()
  }, 20000)
})

describe('exclusion management (regression: URL-shaped exclusions never matched; no removal)', () => {
  it('normalizes URL input to the domain, lists entries, and removes them with feedback', async () => {
    await bootApp()
    actions.openModal({ kind: 'exclusions' })
    const user = userEvent.setup()

    // Remove leftovers from other tests, then add via URL input.
    for (const e of [...getStore().appState?.exclusions ?? []]) {
      await backend.removeExclusion(e)
    }
    const input = await screen.findByPlaceholderText(/private\.example/) as HTMLInputElement
    await user.clear(input)
    await user.type(input, 'https://www.youtube.com/')
    fireEvent.click(screen.getByRole('button', { name: 'Add exclusion' }))

    // The state-changed event must refresh appState (no stale counts)…
    await screen.findByText('youtube.com')
    expect(getStore().appState?.exclusions).toContain('youtube.com')

    // …and the stored identity is the bare domain, which the engine matches.
    expect(await backend.getState().then((s) => s.exclusions)).toEqual(['youtube.com'])

    // Removal via the manager: immediate feedback, entry gone, list empty-state shown.
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    await screen.findByText(/No exclusions configured/i)
    expect((await backend.getState()).exclusions).toEqual([])
  }, 20000)

  it('duplicate exclusions are rejected instead of silently duplicated', async () => {
    await backend.addExclusion('youtube.com')
    await expect(backend.addExclusion('https://www.youtube.com/')).rejects.toThrow(/already exists/)
    await backend.removeExclusion('youtube.com')
  })
})

describe('blocking status text (configured is never reported as enforced)', () => {
  const base = {
    loaded: true,
    extension: 'Communicating',
    extensionColor: 'var(--green)',
    disabled: 0,
    ack: { ms: Date.now() - 60_000, applied: 2, enabled: true },
    ackFresh: true,
  }

  it('off + empty: nothing is blocked', () => {
    expect(blockingSummaryText({ ...base, enabled: false, sites: 0, apps: 0, total: 0, enforced: false })).toBe('Off. Nothing is blocked.')
  })

  it('off + configured: list saved but inactive', () => {
    const text = blockingSummaryText({ ...base, enabled: false, sites: 2, apps: 1, total: 3, enforced: false })
    expect(text).toContain('Off.')
    expect(text).toContain('3 saved targets')
    expect(text).toContain('not enforced')
  })

  it('on + empty: honestly says nothing is being blocked', () => {
    const text = blockingSummaryText({ ...base, enabled: true, sites: 0, apps: 0, total: 0, enforced: false })
    expect(text).toContain('Enabled, but no targets are configured')
    expect(text).toContain('nothing is being blocked')
  })

  it('on + configured WITHOUT acknowledgment: says configured, never enforcing (the Twitch-report regression)', () => {
    const s = { ...base, enabled: true, sites: 1, apps: 0, total: 1, ack: { ms: null, applied: 0, enabled: false }, ackFresh: false, enforced: false }
    const text = blockingSummaryText(s)
    expect(text).toContain('Blocking enabled · 1 website configured')
    expect(text).not.toMatch(/Enforc(ed|ing)\b/i)
    expect(text).toContain('requires a connected CHRONOS extension')
  })

  it('on + stale acknowledgment: reports unconfirmed with the age', () => {
    const s = { ...base, enabled: true, sites: 1, apps: 0, total: 1, ack: { ms: Date.now() - 3600_000, applied: 1, enabled: true }, ackFresh: false, enforced: false }
    const text = blockingSummaryText(s)
    expect(text).toContain('Enforcement unconfirmed')
    expect(text).toContain('last acknowledged')
  })

  it('on + fresh matching acknowledgment: enforced, with count and recency', () => {
    const s = { ...base, enabled: true, sites: 2, apps: 1, total: 3, enforced: true }
    const text = blockingSummaryText(s)
    expect(text).toContain('Enforced — the browser extension confirmed blocking for 2 websites and 1 application')
    expect(text).toContain('min ago')
    expect(text).toContain('Applications are minimized on Windows')
  })

  it('on + acknowledgment reporting fewer rules: flags the mismatch instead of claiming success', () => {
    const s = { ...base, enabled: true, sites: 2, apps: 0, total: 2, ack: { ms: Date.now() - 60_000, applied: 1, enabled: true }, ackFresh: true, enforced: false }
    const text = blockingSummaryText(s)
    expect(text).toContain('reported 1 rule installed')
  })
})

describe('Help & About', () => {
  it('shows the application version from the backend and platform pills', async () => {
    await bootApp()
    actions.navigate('settings')
    actions.setSettingsTab('Help & About')
    expect(await screen.findByText(/Version/)).toBeInTheDocument()
    expect(screen.getByText(/local-first/i)).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'How automatic tracking works' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: /Troubleshooting browser attribution/i })).toBeInTheDocument()
  }, 20000)
})
