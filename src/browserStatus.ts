/* Browser-integration connection status — one authoritative derivation shared by the
   Settings → Integrations summary and the browser-integration dialog so the two views
   can never contradict each other.

   Evidence is the timestamp of the last valid token-authenticated domain hint received
   by the local listener (loopback-only). It proves real communication, never mere
   installation intent: clicking Connect changes nothing here. */

export type PairingInfo = { port: number; token: string; lastHintMs: number; nowMs: number }

type Internals = { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> } | undefined

export function tauri(): Internals {
  return (window as unknown as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__
}

export async function fetchPairingInfo(): Promise<PairingInfo | null> {
  try {
    const raw = await tauri()?.invoke('browser_integration_info')
    return (raw as PairingInfo) ?? null
  } catch {
    return null
  }
}

/** Seconds since the last valid browser signal, or null when none was ever received. */
export function hintAgeSeconds(info: PairingInfo | null): number | null {
  if (!info || info.lastHintMs <= 0) return null
  return Math.max(0, Math.round((info.nowMs - info.lastHintMs) / 1000))
}

export function ageLabel(ageS: number): string {
  if (ageS < 60) return `${ageS}s`
  if (ageS < 3600) return `${Math.round(ageS / 60)}m`
  return `${Math.round(ageS / 3600)}h`
}

export interface BrowserStatus {
  /** Dialog phrasing. */
  label: string
  note: string
  /** Short phrasing for the Settings summary button. */
  short: string
  color: string
}

/** Derives the one status from hint age: no guessing, no contradiction between views. */
export function browserStatus(ageS: number | null): BrowserStatus {
  if (ageS === null) {
    return {
      label: 'Not connected yet',
      note: 'No browser signal received — nothing is paired until a browser extension talks to CHRONOS.',
      short: 'Not connected',
      color: 'var(--muted)',
    }
  }
  if (ageS < 30) {
    return {
      label: 'Communicating',
      note: `Browser signal ${ageLabel(ageS)} ago — website attribution is flowing.`,
      short: 'Connected',
      color: 'var(--green)',
    }
  }
  return {
    label: 'Awaiting communication',
    note: `Last browser signal ${ageLabel(ageS)} ago — the browser may simply be idle.`,
    short: 'Awaiting signal',
    color: 'var(--amber)',
  }
}
