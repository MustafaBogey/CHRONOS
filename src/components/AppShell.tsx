import { actions, useStore } from '../store'
import { backend } from '../backend'
import { Icon } from './icons'
import { Button, statusMeta, withToast } from './ui'
import { Overview } from '../views/Overview'
import { Timeline } from '../views/Timeline'
import { Focus } from '../views/Focus'
import { Reports } from '../views/Reports'
import { Settings } from '../views/Settings'
import type { Page } from '../types'
import { dateLabel, localDateStr } from '../format'

const navItems: [Page, string, string][] = [
  ['overview', 'home', 'Overview'],
  ['timeline', 'timeline', 'Timeline'],
  ['focus', 'focus', 'Focus & Goals'],
  ['reports', 'reports', 'Reports'],
  ['settings', 'settings', 'Settings & Integrations'],
]

export function AppShell() {
  const page = useStore((s) => s.page)
  // NOTE: body[data-page] is applied during App render (before paint) to avoid a hero
  // flash; this effect only confirms it after mount.

  return (
    <div className="shell">
      <Sidebar />
      <main className="main" id="main">
        <Topbar />
        <div className="content" id="content">
          {page === 'overview' ? <Overview /> : null}
          {page === 'timeline' ? <Timeline /> : null}
          {page === 'focus' ? <Focus /> : null}
          {page === 'reports' ? <Reports /> : null}
          {page === 'settings' ? <Settings /> : null}
          <footer className="footer">
            <div className="row"><Icon name="shield" />Private by design. No account. No cloud.</div>
            <span>Local data · {dateLabel(localDateStr(), { month: 'short', year: 'numeric' })}</span>
          </footer>
        </div>
      </main>
    </div>
  )
}

function Sidebar() {
  const page = useStore((s) => s.page)
  const appState = useStore((s) => s.appState)
  const isMock = backend.kind === 'dev-mock'

  const tracking = appState?.tracking.state ?? 'confirmation'
  const meta = statusMeta(tracking)
  const running = tracking === 'active' || tracking === 'degraded'
  const dnd = appState?.dnd ?? false

  return (
    <aside className="sidebar" aria-label="Application sidebar">
      <div className="brand"><div className="logomark" aria-hidden="true" /><div className="wordmark">CHRONOS</div></div>
      <p className="brand-sub">Where Every Second Counts.</p>
      <nav className="nav" aria-label="Primary navigation" id="nav">
        {navItems.map(([id, icon, label]) => (
          <button key={id} onClick={() => actions.navigate(id)} className={page === id ? 'active' : ''}
            aria-current={page === id ? 'page' : undefined} title={label}>
            <Icon name={icon} /><span className="nav-label">{label}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-bottom">
        <div className="local-note"><Icon name="shield" />Local first. Yours alone.</div>
        <div>
          <div className="tracking-box">
            <div className="status-line" style={{ color: meta.color }}><span className="dot" />{meta.label}</div>
            <div className="small muted">{meta.description}</div>
            <button className="sidebar-action" onClick={() => {
              if (running) withToast(backend.pauseTracking(), 'Manually paused. Recovery will preserve this choice.')
              else if (tracking === 'paused') withToast(backend.resumeTracking(), 'Tracking resumed.')
              else if (tracking === 'privacy') actions.openModal({ kind: 'privacy-leave' })
              else actions.openModal({ kind: 'tracking-enable' })
            }}>
              <Icon name={running ? 'pause' : 'play'} /><span>{running ? 'Pause tracking' : 'Review / resume'}</span>
            </button>
            <button className="sidebar-action" onClick={() => withToast(backend.setDnd(!dnd), `Do Not Disturb ${!dnd ? 'on' : 'off'}.`)}>
              <Icon name="moon" /><span>Do Not Disturb</span>
              <span style={{ marginLeft: 'auto', color: dnd ? 'var(--violet)' : 'var(--muted)' }}>{dnd ? 'On' : 'Off'}</span>
            </button>
          </div>
        </div>
        {isMock ? (
          <button className="sidebar-action" onClick={() => actions.toast('In the desktop app, CHRONOS lives in your system tray.')}
            title="System tray" aria-label="System tray"><Icon name="tray" /><span className="nav-label">System tray</span></button>
        ) : null}
        <button className="sidebar-action" onClick={() => actions.openModal({ kind: 'onboarding' })}
          title="Revisit introduction" aria-label="Revisit introduction"><Icon name="spark" /><span className="nav-label">A little introduction</span></button>
        <div className="profile">
          <div className="avatar">{appState?.profile.initials ?? '··'}</div>
          <div><strong>{appState?.profile.name ?? 'Personal workspace'}</strong><small>{appState?.profile.workspace ?? ''}</small></div>
          <span className="green" style={{ marginLeft: 'auto', fontSize: 12 }}>✧</span>
        </div>
      </div>
    </aside>
  )
}

function Topbar() {
  const dnd = useStore((s) => s.appState?.dnd ?? false)
  return (
    <header className="topbar">
      <div className="top-actions">
        <span className="quiet-badge" id="quiet-badge">{dnd ? '☾ Do Not Disturb' : ''}</span>
      </div>
    </header>
  )
}

export function QuitMenuItem() {
  return <Button label="Quit CHRONOS" icon="power" onClick={() => actions.openModal({ kind: 'quit' })} />
}
