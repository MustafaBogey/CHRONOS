import { describe, expect, it, beforeEach, beforeAll } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
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

import '@src/styles/chronos.css'
import App from '@src/App'
import { boot, actions, getStore } from '@src/store'

describe('navigation persistence after refresh', () => {
  it('restores the persisted page on boot', async () => {
    window.localStorage.setItem('chronos.page', 'timeline')
    await boot()
    expect(getStore().page).toBe('timeline')
    render(<App />)
    expect(await screen.findByText(/Your day, in perspective/)).toBeInTheDocument()
  }, 15000)

  it('persists navigation changes', async () => {
    window.localStorage.clear()
    await boot()
    actions.navigate('reports')
    expect(getStore().page).toBe('reports')
    expect(window.localStorage.getItem('chronos.page')).toBe('reports')
    // A simulated refresh (re-boot) keeps Reports active.
    await boot()
    expect(getStore().page).toBe('reports')
  }, 15000)

  it('ignores invalid stored pages', async () => {
    window.localStorage.setItem('chronos.page', 'nonsense')
    await boot()
    expect(getStore().page).toBe('overview')
  }, 15000)
})

describe('settings section persistence after refresh', () => {
  const clearHash = () => window.history.replaceState(null, '', window.location.pathname)

  it('restores the section from the #settings/<section> fragment on boot', async () => {
    clearHash()
    window.localStorage.setItem('chronos.page', 'settings')
    window.location.hash = '#settings/integrations'
    await boot()
    expect(getStore().page).toBe('settings')
    expect(getStore().settingsTab).toBe('Integrations')
    clearHash()
  }, 15000)

  it('deep-links directly to a settings section from any stored page', async () => {
    clearHash()
    window.localStorage.setItem('chronos.page', 'timeline')
    window.location.hash = '#settings/tracking-classification'
    await boot()
    expect(getStore().page).toBe('settings')
    expect(getStore().settingsTab).toBe('Tracking & Classification')
    expect(window.localStorage.getItem('chronos.page')).toBe('settings')
    clearHash()
  }, 15000)

  it('falls back to General for an unknown section slug', async () => {
    clearHash()
    window.location.hash = '#settings/not-a-real-section'
    await boot()
    expect(getStore().page).toBe('settings')
    expect(getStore().settingsTab).toBe('General')
    clearHash()
  }, 15000)

  it('keeps the fragment in sync while sections change, and clears it when leaving Settings', async () => {
    clearHash()
    window.localStorage.setItem('chronos.page', 'settings')
    await boot()
    actions.setSettingsTab('Privacy & Data')
    render(<App />)
    expect(window.location.hash).toBe('#settings/privacy-data')
    actions.navigate('overview')
    expect(getStore().page).toBe('overview')
    expect(window.location.hash).toBe('')
    clearHash()
  }, 15000)
})

beforeEach(() => {
  cleanup()
})
