import { useEffect, useState } from 'react'
import { useStore } from '../store'
import { backend, type BlockListEntry } from '../backend'
import { fetchPairingInfo, hintAgeSeconds, browserStatus } from '../browserStatus'

/** Enforcement-acknowledgment from the browser extension (none received = null). */
export interface BlocklistAck {
  ms: number | null
  applied: number
  enabled: boolean
}

/** Ack counts as fresh while the browser could plausibly be running; the
 *  extension re-acknowledges on every sync cycle (about once a minute). */
const ACK_FRESH_MS = 5 * 60 * 1000

export interface BlockingSummary {
  loaded: boolean
  enabled: boolean
  sites: number
  apps: number
  total: number
  disabled: number
  extension: string
  extensionColor: string
  ack: BlocklistAck | null
  ackFresh: boolean
  /** True only when a fresh acknowledgment confirms the configured targets. */
  enforced: boolean
}

/**
 * One authoritative blocking summary shared by Settings, the Distraction
 * management tab, and the block-list dialog. "Configured" is never reported
 * as "enforced": enforcement is confirmed only by a fresh extension
 * acknowledgment whose applied-rule count matches the configured targets.
 */
export function useBlockingSummary(): BlockingSummary {
  const blocking = useStore((s) => s.appState?.settings.blocking) ?? false
  const [entries, setEntries] = useState<BlockListEntry[] | null>(null)
  const [ext, setExt] = useState<{ label: string; color: string }>({ label: 'unknown', color: 'var(--muted)' })
  const [ack, setAck] = useState<BlocklistAck | null>(null)
  useEffect(() => {
    let alive = true
    const load = () => { void backend.getBlockList().then((e) => { if (alive) setEntries(e) }) }
    const pollExt = () => {
      void fetchPairingInfo().then((info) => {
        if (!alive || !info) return
        const s = browserStatus(hintAgeSeconds(info))
        setExt({ label: s.label, color: s.color })
        const raw = (info as unknown as { blocklistAck?: BlocklistAck }).blocklistAck
        setAck(raw ?? { ms: null, applied: 0, enabled: false })
      })
    }
    load()
    pollExt()
    // Poll while visible: block-list edits, extension signals, and
    // acknowledgments are reflected within seconds without stale-cache risk.
    const t = window.setInterval(() => { load(); pollExt() }, 5000)
    return () => {
      alive = false
      window.clearInterval(t)
    }
  }, [])
  const active = entries ?? []
  const enabledEntries = active.filter((e) => e.enabled)
  const sites = enabledEntries.filter((e) => e.kind === 'domain').length
  const apps = enabledEntries.filter((e) => e.kind === 'app').length
  const total = enabledEntries.length
  const ackFresh = ack?.ms != null && Date.now() - ack.ms < ACK_FRESH_MS
  const enforced = blocking && total > 0 && ackFresh === true && !!ack && ack.enabled && ack.applied >= sites
  return {
    loaded: entries !== null,
    enabled: blocking,
    sites,
    apps,
    total,
    disabled: active.length - enabledEntries.length,
    extension: ext.label,
    extensionColor: ext.color,
    ack: ack ?? { ms: null, applied: 0, enabled: false },
    ackFresh,
    enforced,
  }
}

function ackAgeLabel(ms: number): string {
  const mins = Math.max(1, Math.round((Date.now() - ms) / 60000))
  return mins < 60 ? `${mins} min ago` : `${Math.round(mins / 60)} h ago`
}

/**
 * Honest status text. "Enforced" appears only with a fresh acknowledgment
 * matching the configured targets; every other state says exactly what is
 * known and what is missing.
 */
export function blockingSummaryText(s: BlockingSummary): string {
  if (!s.enabled) {
    return s.total === 0
      ? 'Off. Nothing is blocked.'
      : `Off. ${s.total} saved target${s.total === 1 ? '' : 's'} stay configured but are not enforced until you enable blocking.`;
  }
  if (s.total === 0) {
    return 'Enabled, but no targets are configured yet — nothing is being blocked. Open the block list to add sites or applications.';
  }
  const targets = [
    s.sites > 0 ? `${s.sites} website${s.sites === 1 ? '' : 's'}` : '',
    s.apps > 0 ? `${s.apps} application${s.apps === 1 ? '' : 's'}` : '',
  ].filter(Boolean).join(' and ')
  if (s.enforced) {
    const appNote = s.apps > 0 ? ' Applications are minimized on Windows.' : ''
    return `Enforced — the browser extension confirmed blocking for ${targets}, ${ackAgeLabel(s.ack!.ms!)}.${appNote}`;
  }
  if (s.ack?.ms != null && !s.ackFresh) {
    return `Blocking enabled · ${targets} configured. Enforcement unconfirmed: the extension last acknowledged ${ackAgeLabel(s.ack.ms)} (its browser may be closed).`;
  }
  if (s.ackFresh && s.ack && !s.ack.enabled) {
    return `Blocking enabled · ${targets} configured. The extension acknowledged just now but reports blocking is off — try toggling blocking again.`;
  }
  if (s.ackFresh && s.ack && s.ack.applied < s.sites) {
    return `Blocking enabled · ${targets} configured. The extension responded but reported ${s.ack.applied} rule${s.ack.applied === 1 ? '' : 's'} installed — reload it from Settings → Integrations.`;
  }
  return `Blocking enabled · ${targets} configured. Browser enforcement requires a connected CHRONOS extension — none has confirmed yet. Connect it from Settings → Integrations.`;
}
