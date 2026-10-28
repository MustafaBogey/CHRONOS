import { useEffect, useRef, useState } from 'react'
import { actions, useStore } from '../store'
import { backend } from '../backend'
import { Icon } from './icons'
import { tauri } from '../browserStatus'
import { cats, type Category } from '../types'

/**
 * Mini CHRONOS — optional slim floating companion (its own always-on-top frameless
 * window created by the Rust core). One horizontal row: classification indicator,
 * current activity + classification, elapsed time, pause/resume, hide. Reuses the
 * existing tracking engine and events; never invents activity.
 */

type CurrentActivity = { tracking: string; activity: { name: string; cat: string; startMs: number; isSite: boolean } | null; nowMs: number }

function elapsedLabel(startMs: number, now: number): string {
  const mins = Math.max(0, Math.floor((now - startMs) / 60000))
  if (mins < 1) return 'now'
  if (mins < 60) return `${mins}m`
  const h = Math.floor(mins / 60)
  const m = mins % 60
  return m ? `${h}h ${m}m` : `${h}h`
}

export function MiniBar() {
  const tracking = useStore((s) => s.appState?.tracking.state)
  const [snap, setSnap] = useState<CurrentActivity | null>(null)
  const [now, setNow] = useState(Date.now())
  const saveTimer = useRef<number | null>(null)

  useEffect(() => {
    document.documentElement.classList.add('mini')
    document.title = 'Mini CHRONOS'
  }, [])

  const refresh = () => {
    void tauri()?.invoke('get_current_activity').then((raw) => setSnap(raw as CurrentActivity)).catch(() => undefined)
  }

  useEffect(() => {
    refresh()
    const offA = backend.on('activity-changed', refresh)
    const offT = backend.on('tracking-changed', refresh)
    const t = window.setInterval(refresh, 15_000)
    const tick = window.setInterval(() => setNow(Date.now()), 10_000)
    return () => { offA(); offT(); window.clearInterval(t); window.clearInterval(tick) }
  }, [])

  // Persist the dragged position (debounced).
  useEffect(() => {
    let off: (() => void) | undefined
    void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      const win = getCurrentWindow()
      const handler = () => {
        if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
        saveTimer.current = window.setTimeout(() => {
          void win.outerPosition().then((pos) => {
            void tauri()?.invoke('mini_save_position', { x: pos.x, y: pos.y }).catch(() => undefined)
          }).catch(() => undefined)
        }, 600)
      }
      void win.onMoved(() => handler()).then((fn) => { off = fn })
    }).catch(() => undefined)
    return () => { off?.(); if (saveTimer.current !== null) window.clearTimeout(saveTimer.current) }
  }, [])

  const running = tracking === 'active' || tracking === 'degraded'
  const act = running ? snap?.activity ?? null : null
  const meta = act ? cats[(act.cat as Category) ?? 'unclassified'] ?? cats.unclassified : null
  const statusColor = !running
    ? tracking === 'paused' ? 'var(--amber)' : 'var(--muted)'
    : meta ? meta.color : 'var(--muted)'

  const togglePause = () => {
    if (running) void backend.pauseTracking().then(refresh)
    else if (tracking === 'paused') void backend.resumeTracking().then(refresh)
    else actions.toast('Enable tracking from the main window first.')
  }

  return (
    <div className="mini-bar" data-tauri-drag-region>
      <span className="mini-dot" style={{ background: statusColor }} aria-hidden="true" />
      <button className="mini-activity" onClick={() => { void tauri()?.invoke('mini_open_main').catch(() => undefined) }}
        onMouseDown={(e) => e.stopPropagation()} /* keep click-to-open; the bar itself drags */
        title="Open CHRONOS">
        <span className="mini-name">{act ? act.name : !running ? (tracking === 'paused' ? 'Paused' : 'Tracking off') : 'Observing…'}</span>
        {act && meta ? <span className="mini-cat" style={{ color: meta.color }}>{meta.label}</span> : null}
        {act ? <span className="mini-timer mono">{elapsedLabel(act.startMs, snap?.nowMs ?? now)}</span> : null}
      </button>
      <button className="mini-btn" onMouseDown={(e) => e.stopPropagation()} onClick={togglePause} aria-label={running ? 'Pause tracking' : 'Resume tracking'}
        title={running ? 'Pause tracking' : 'Resume tracking'}>
        <Icon name={running ? 'pause' : 'play'} />
      </button>
      <button className="mini-btn" onMouseDown={(e) => e.stopPropagation()} onClick={() => { void tauri()?.invoke('mini_hide').catch(() => undefined) }}
        aria-label="Hide Mini CHRONOS" title="Hide Mini CHRONOS (tracking continues)">
        <Icon name="close" />
      </button>
    </div>
  )
}
