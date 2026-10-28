import { useEffect, useState } from 'react'
import { actions, useStore, refreshState } from '../store'
import { backend } from '../backend'
import { Button, PageTitle, Setting, SelectField, Toggle, withToast, statusMeta } from '../components/ui'
import type { SettingsTab } from '../types'
import { fmt } from '../format'
import { DATE_FORMATS } from '../format'
import { fetchPairingInfo, hintAgeSeconds, browserStatus } from '../browserStatus'
import { useBlockingSummary, blockingSummaryText } from '../components/blocking'

const groups: SettingsTab[] = ['General', 'Tracking & Classification', 'Privacy & Data', 'Focus & Notifications', 'Integrations', 'Advanced & Diagnostics', 'Help & About']

export function Settings() {
  const tab = useStore((s) => s.settingsTab)
  // Reflect the selected section in the URL fragment so a refresh (or a direct link to
  // #settings/<section>) returns to the same section; invalid values fall back safely
  // to General at boot (store.ts).
  useEffect(() => {
    const slug = settingsSlug(tab)
    if (window.location.hash !== `#settings/${slug}`) {
      window.history.replaceState(null, '', `#settings/${slug}`)
    }
  }, [tab])
  return (
    <>
      <PageTitle h="Make CHRONOS your own." p="Thoughtful defaults. Clear controls. Your privacy, always." />
      <div className="settings-layout">
        <nav className="settings-nav" aria-label="Settings groups">
          {groups.map((g) => (
            <button key={g} className={tab === g ? 'active' : ''} onClick={() => actions.setSettingsTab(g)}>{g}</button>
          ))}
        </nav>
        <section>
          <h2>{tab}</h2>
          <p className="section-caption">Preferences are stored locally and persist across restarts.</p>
          <div className="card settings-card">
            {tab === 'General' ? <GeneralGroup /> : null}
            {tab === 'Tracking & Classification' ? <TrackingGroup /> : null}
            {tab === 'Privacy & Data' ? <PrivacyGroup /> : null}
            {tab === 'Focus & Notifications' ? <FocusGroup /> : null}
            {tab === 'Integrations' ? <IntegrationsGroup /> : null}
            {tab === 'Advanced & Diagnostics' ? <AdvancedGroup /> : null}
            {tab === 'Help & About' ? <HelpAboutGroup /> : null}
          </div>
        </section>
      </div>
    </>
  )
}

function GeneralGroup() {
  const s = useStore((st) => st.appState?.settings)
  const profile = useStore((st) => st.appState?.profile)
  const [name, setName] = useState(profile?.name ?? '')
  if (!s) return null
  const apply = (patch: Parameters<typeof backend.updateSettings>[0]) =>
    backend.updateSettings(patch)
      .then(() => refreshState())
      .catch((err) => actions.toast(err instanceof Error ? err.message : 'Could not save this setting.'))
  const saveName = () => {
    const trimmed = name.trim()
    if (trimmed === (profile?.name ?? '')) return
    void apply({ displayName: trimmed }).then(() => actions.toast(trimmed ? `CHRONOS will call you ${trimmed}.` : 'Using your system name.'))
  }
  return (
    <>
      <Setting h="Display name" p="What CHRONOS calls you. Leave empty to use your system account name."
        control={
          <span className="row" style={{ gap: 8 }}>
            <input className="field" aria-label="Display name" maxLength={40} value={name}
              placeholder={profile?.name ?? 'Your name'}
              onChange={(e) => setName(e.target.value)}
              onBlur={saveName}
              onKeyDown={(e) => { if (e.key === 'Enter') saveName() }}
              style={{ minWidth: 170 }} />
          </span>
        } />
      <Setting h="Appearance" p="A softer surface or a deeper midnight."
        control={<SelectField values={['Midnight', 'Soft midnight']} current={s.appearance} ariaLabel="appearance"
          onChange={(v) => void apply({ appearance: v as never })} />} />
      <Setting h="Launch at startup" p="Register CHRONOS to start with your operating system."
        control={<Toggle checked={s.startup} label="Launch at startup"
          onChange={(v) => withToast(backend.updateSettings({ startup: v }), v ? 'Startup registration requested.' : 'Startup registration removed.')} />} />
      <Setting h="Minimize to system tray" p="Keep CHRONOS out of your way while it keeps counting."
        control={<Toggle checked={s.tray} label="Minimize to system tray" onChange={(v) => void backend.updateSettings({ tray: v })} />} />
      <Setting h="Mini CHRONOS" p="A slim floating bar showing the current activity, its classification, and pause/resume. Position is remembered; hide it from the bar itself or the tray menu."
        control={<Toggle checked={s.miniBar} label="Mini CHRONOS"
          onChange={(v) => withToast(backend.updateSettings({ miniBar: v }), v ? 'Mini CHRONOS opened.' : 'Mini CHRONOS hidden. Background tracking continues.')} />} />
      <Setting h="Time format" p="Applied to activity timestamps throughout CHRONOS."
        control={<SelectField values={['24-hour', '12-hour']} current={s.time} ariaLabel="time format"
          onChange={(v) => void backend.updateSettings({ time: v as never })} />} />
      <Setting h="Date format" p="How dates are presented throughout CHRONOS. Stored history is unchanged."
        control={<SelectField values={DATE_FORMATS.map((d) => d.label)} current={DATE_FORMATS.find((d) => d.value === s.dateFormat)?.label ?? DATE_FORMATS[0].label}
          ariaLabel="date format" onChange={(label) => {
            const pick = DATE_FORMATS.find((d) => d.label === label)
            if (pick) void apply({ dateFormat: pick.value } as never)
          }} />} />
      <Setting h="Quick guided setup" p="Revisit the privacy-first introduction."
        control={<Button label="Preview onboarding" icon="spark" onClick={() => actions.openModal({ kind: 'onboarding' })} />} />
    </>
  )
}

function TrackingGroup() {
  const st = useStore((s) => s.appState)
  if (!st) return null
  const tracking = st.tracking.state
  return (
    <>
      <Setting h="Tracking state" p="Explicit state changes only. Recovery never silently overrides a pause."
        control={
          <select className="field" aria-label="Tracking state" value={tracking}
            onChange={(e) => {
              const requested = e.target.value
              if (requested === 'active' && (tracking === 'disabled' || tracking === 'confirmation')) { actions.openModal({ kind: 'tracking-enable' }); return }
              if (requested === 'privacy') { void backend.setPrivacy(true); return }
              if (requested === 'paused') { void backend.pauseTracking(); return }
              if (requested === 'disabled') { withToast(backend.disableTracking(), 'Tracking disabled. Explicit enable required.'); return }
              void backend.updateSettings({})
            }}>
            {(['active', 'paused', 'privacy', 'disabled', 'degraded', 'confirmation'] as const).map((v) => (
              <option key={v} value={v}>{statusMeta(v).label}</option>
            ))}
          </select>
        } />
      <Setting h="Idle detection" p={`After ${st.settings.idleMinutes} minutes without input, time is recorded as idle rather than active.`}
        control={
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input type="number" className="field" min={1} max={120} step={1} style={{ width: 84 }}
              aria-label="Idle threshold in minutes" defaultValue={st.settings.idleMinutes}
              onBlur={(e) => {
                const v = Math.round(Number(e.target.value))
                if (!Number.isFinite(v) || v < 1 || v > 120) { e.target.value = String(st.settings.idleMinutes); return }
                e.target.value = String(v)
                if (v !== st.settings.idleMinutes) void withToast(backend.updateSettings({ idleMinutes: v }), `Idle threshold set to ${v} minutes.`)
              }} />
            <span className="small muted">min</span>
            <Toggle checked={st.settings.autoIdle} label="Idle detection" onChange={(v) => void backend.updateSettings({ autoIdle: v })} />
          </div>
        } />
      <Setting h="Idle exceptions" p="Meetings, presentations, media — apps assumed engaged without input. Your explicit assumption, never a claim of observation."
        control={<Button label="Manage exceptions" onClick={() => actions.openModal({ kind: 'idle-exceptions' })} />} />
      <Setting h="Tracking schedule" p={st.settings.scheduleEnabled
        ? `Observation window ${st.settings.scheduleStart || '—'}–${st.settings.scheduleEnd || '—'} on ${st.settings.scheduleDays || 'selected days'}. Outside it, honest gaps.`
        : 'Optional observation window. Outside it, CHRONOS records an honest gap instead of activity.'}
        control={<Button label="Configure schedule" onClick={() => actions.openModal({ kind: 'schedule' })} />} />
      <Setting h="Classification rules" p={`${st.rules.length} user-defined rules. New rules apply to future activity by default.`}
        control={<Button label="Manage rules" onClick={() => actions.openModal({ kind: 'rules' })} />} />
      <Setting h="Application & website exclusions" p={`${st.exclusions.length} exclusion${st.exclusions.length === 1 ? '' : 's'}. Excluded activity is not recorded from now on; history is never rewritten. Open the manager to add or remove entries.`}
        control={<Button label="Manage exclusions" onClick={() => actions.openModal({ kind: 'exclusions' })} />} />
      <Setting h="Uncertain-activity review" p="Unclassified time stays unclassified until there is enough context."
        control={<Button label="Open review queue" onClick={() => actions.openModal({ kind: 'review' })} />} />
      <div className="inset" style={{ margin: '20px 0' }}>
        <span className="eyebrow">CLASSIFICATION PRIORITY</span>
        <p style={{ marginTop: 9 }}>Your corrections → your rules → contextual classification → built-in defaults → unclassified.</p>
      </div>
    </>
  )
}

function PrivacyGroup() {
  const st = useStore((s) => s.appState)
  if (!st) return null
  const tracking = st.tracking.state
  return (    <>
      <Setting h="Pause tracking" p="A manual pause persists until you explicitly resume."
        control={<Button label={tracking === 'paused' ? 'Resume tracking' : 'Pause tracking'} icon="pause"
          onClick={() => {
            if (tracking === 'paused') withToast(backend.resumeTracking(), 'Tracking resumed.')
            else if (tracking === 'active' || tracking === 'degraded') withToast(backend.pauseTracking(), 'Manually paused. Recovery will preserve this choice.')
            else actions.openModal({ kind: 'tracking-enable' })
          }} />} />
      <Setting h="Privacy mode" p="Suspends collection entirely. Resume never overrides it; leaving it restores your prior state."
        control={<Toggle checked={tracking === 'privacy'} label="Privacy mode"
          onChange={(v) => withToast(backend.setPrivacy(v), v ? 'Privacy mode on. Recovery cannot override this choice.' : 'Privacy mode off. Your previous tracking state was restored.')} />} />
      <RetentionSetting />
      <Setting h="Historical deletion" p="Deliberately delete a selected date range. Unrelated dates are preserved."
        control={<Button label="Delete history…" onClick={() => actions.openModal({ kind: 'deletion' })} />} />
      <Setting h="Manual backup" p="Create a verified local backup of all CHRONOS data."
        control={<Button label="Back up now" icon="shield" onClick={() => actions.openModal({ kind: 'backup' })} />} />
      <Setting h="Scheduled backup" p="Keep an automatic local backup each day CHRONOS runs."
        control={<Toggle checked={st.settings.scheduled} label="Scheduled backup" onChange={(v) => void backend.updateSettings({ scheduled: v })} />} />
      <Setting h="Restore" p="Inspect a backup, choose merge or replace, and restore safely."
        control={<Button label="Restore…" onClick={() => actions.openModal({ kind: 'restore' })} />} />
      <Setting h="Export" p="Export the selected report range as CSV or JSON."
        control={<Button label="Export…" icon="download" onClick={() => actions.openModal({ kind: 'export' })} />} />
    </>
  )
}

/** Retention: preference + explicit activation. Destructive automation never
 *  starts from a stored preference alone — the user flips the switch. */
function RetentionSetting() {
  const st = useStore((s) => s.appState)
  const [status, setStatus] = useState<{ active: boolean; preference: string; cutoffDays: number | null; lastRunMs: number | null } | null>(null)
  const [confirming, setConfirming] = useState(false)
  const refresh = () => { void backend.retentionStatus().then(setStatus) }
  useEffect(refresh, [st?.settings.retention, st?.settings.retentionActive])
  if (!st) return null
  const lastRun = status?.lastRunMs ? new Date(status.lastRunMs).toLocaleString() : null
  return (
    <Setting h="Data retention" p={
      status?.active
        ? `Active — records older than ${status.cutoffDays} days are deleted automatically (${status.preference}). Last sweep: ${lastRun ?? 'just now'}. Backups on disk are never touched.`
        : 'Choose a period, then explicitly activate. Until activated, nothing is ever deleted automatically.'}
      control={
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
          <SelectField values={['Forever', '30 days', '3 months', '6 months', '1 year']} current={st.settings.retention} ariaLabel="retention"
            onChange={(v) => withToast(backend.updateSettings({ retention: v }).then(refresh), 'Retention preference updated. Automatic deletion stays as configured.')} />
          {status?.active ? (
            <>
              <Button label="Run now" variant="ghost" onClick={() => { void withToast(backend.retentionRunNow().then(refresh), 'Retention sweep complete.') }} />
              <Button label="Deactivate" onClick={() => { void withToast(backend.retentionSetActive(false).then(refresh), 'Automatic deletion deactivated.') }} />
            </>
          ) : confirming ? (
            <>
              <Button label="Keep my history" variant="ghost" onClick={() => setConfirming(false)} />
              <Button label="Confirm activation" danger icon="shield"
                onClick={() => {
                  setConfirming(false)
                  void withToast(backend.retentionSetActive(true).then(refresh), 'Retention enforcement activated. One sweep has run.')
                }} />
            </>
          ) : (
            <Button label="Activate automatic deletion" onClick={() => setConfirming(true)}
              disabled={st.settings.retention === 'Forever'} />
          )}
        </div>
      } />
  )
}

function FocusGroup() {
  const st = useStore((s) => s.appState)
  const blocking = useBlockingSummary()
  if (!st) return null
  const s = st.settings
  return (
    <>
      <Setting h="Focus defaults" p="Configure your focus targets and distraction limit."
        control={<Button label="Edit targets" onClick={() => actions.openModal({ kind: 'goals' })} />} />
      <Setting h="Break reminders" p={`A gentle nudge after ${s.breakEveryMinutes} minutes of continuous work.`}
        control={<Toggle checked={s.breaks} label="Break reminders" onChange={(v) => void backend.updateSettings({ breaks: v })} />} />
      <Setting h="Quiet hours" p={`Evenings stay quiet, ${String(s.quietStart).padStart(2, '0')}:00–${String(s.quietEnd).padStart(2, '0')}:00, using your real local clock.`}
        control={<Toggle checked={s.quiet} label="Quiet hours" onChange={(v) => void backend.updateSettings({ quiet: v })} />} />
      <Setting h="Do Not Disturb" p="Suppress break and completion notices."
        control={<Toggle checked={st.dnd} label="Do Not Disturb" onChange={(v) => withToast(backend.setDnd(v), `Do Not Disturb ${v ? 'on' : 'off'}.`)} />} />
      <Setting h="Distraction blocking" p={blockingSummaryText(blocking)}
        control={
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <Button label="Block list" onClick={() => actions.openModal({ kind: 'blocklist' })} />
            <Toggle checked={s.blocking} label="Distraction blocking" onChange={(v) => withToast(backend.updateSettings({ blocking: v }), v ? 'Blocking enabled — the block list is now enforced.' : 'Blocking off. Saved targets stay configured.')} />
          </div>
        } />
      <Setting h="Notification design" p="Preview break, completion, and significant degradation notices."
        control={<Button label="Preview notices" icon="bell" onClick={() => actions.openModal({ kind: 'notifications' })} />} />
    </>
  )
}

/** Stable URL slug per settings section (kebab-case); used by refresh-safe navigation. */
const SETTINGS_SLUGS: Record<SettingsTab, string> = {
  'General': 'general',
  'Tracking & Classification': 'tracking-classification',
  'Privacy & Data': 'privacy-data',
  'Focus & Notifications': 'focus-notifications',
  'Integrations': 'integrations',
  'Advanced & Diagnostics': 'advanced-diagnostics',
  'Help & About': 'help-about',
}

export function settingsSlug(tab: SettingsTab): string {
  return SETTINGS_SLUGS[tab] ?? SETTINGS_SLUGS['General']
}

/* ---------------- Help & About ---------------- */

function HelpAboutGroup() {
  const [version, setVersion] = useState<string | null>(null)
  const platform = useStore((s) => s.capabilities?.platform) ?? null
  useEffect(() => { void backend.getAppVersion().then(setVersion).catch(() => setVersion(null)) }, [])
  return (
    <>
      <div className="inset" style={{ margin: '18px 0' }}>
        <div className="row" style={{ gap: 14, alignItems: 'center' }}>
          <span className="logomark" style={{ width: 40, height: 40 }} aria-hidden="true" />
          <div>
            <strong style={{ fontSize: 15, letterSpacing: 1 }}>CHRONOS</strong>
            <span className="pill purple" style={{ marginLeft: 10 }}>Version {version ?? '…'}</span>
            {platform ? <span className="pill gray" style={{ marginLeft: 6 }}>{platform}</span> : null}
          </div>
        </div>
        <p style={{ marginTop: 10 }}>
          A local-first personal time tracker: CHRONOS observes which application is in the foreground, classifies
          activity into categories, and turns honest observation into reports you can trust. Everything is stored
          in a local database on this machine — CHRONOS works fully offline, and no activity data ever leaves your
          computer. The optional browser extension talks to CHRONOS over a loopback connection on this machine only.
        </p>
      </div>

      <Setting h="How automatic tracking works" p="CHRONOS records which application is in the foreground and for how long, using a 2-second observation loop. Very short appearances (under 5 seconds) are treated as noise, time without keyboard or mouse input can be marked idle, and anything not observed — pauses, sleep, tracking off — shows as an honest tracking gap rather than invented activity. After an unexpected shutdown, CHRONOS asks before resuming." />
      <Setting h="Activity classifications" p="Each interval is classified Productive, Distracting, Neutral, or Unclassified. The order of authority is: your corrections → your rules → patterns learned from your consistent corrections → built-in defaults. Ambiguous services (like AI assistants or discussion sites) are flagged for your review instead of silently judged. You can correct any interval from the Timeline." />
      <Setting h="Projects and Work Sessions" p="Projects collect assigned activity for goals and client reports; you can assign activities in bulk, archive, or delete a project without losing tracked time. Work Sessions (Timeline → Work Sessions) are automatically inferred groups of contiguous related activity — collapsed summaries you can expand; they are separate from Focus Sessions, which you start manually. Sessions never merge or double-count your records." />
      <Setting h="Manual focus sessions" p="A Focus Session is a deliberate timer you start with an intention. Elapsed time is just time: only observed productive activity during the session (or a sustained 5-minute productive stretch) counts as qualified focus. (Work Sessions are automatic summaries; Focus Sessions are yours.)" />
      <Setting h="Tracking exclusions vs. distraction blocking" p="Exclusions (Tracking & Classification) are privacy: matching future activity is not recorded — the app or site stays fully usable and history is untouched. Distraction blocking (Focus & Notifications) is attention: listed websites redirect to a block page inside browsers with the CHRONOS extension, and listed Windows applications are minimized when they take the foreground — discouragement, not prevention. The two lists are independent." />
      <Setting h="Privacy and local data" p="All data lives in a local SQLite database. Back up, inspect, and restore it from Privacy & Data; deletion tools operate on explicit date ranges and are undoable. Browsers, Discord, and File Explorer never store window titles — only app identity or a curated service label." />
      <Setting h="Troubleshooting browser attribution" p="Websites appear as 'Chrome · some site' only when the optional browser extension is installed and paired. If the Integrations status shows Not connected: open Settings → Integrations → Connect, load the extension in your browser as guided, and the local pairing page pairs it automatically. After upgrading the extension, browsers may disable it until you re-enable it once in the extensions page." />
    </>
  )
}

function IntegrationsGroup() {
  return (
    <>
      <BrowserIntegrationSetting />
      <IntegrationSetting name="Google Calendar" icon="calendar"
        p="Optional read-only calendar context. No account required to use CHRONOS." />
      <IntegrationSetting name="Microsoft Outlook / Microsoft 365" icon="mail"
        p="Optional calendar context, connected only with your permission." />
      <div className="setting-row">
        <div>
          <p className="setting-h">AI Providers</p>
          <p className="setting-p">Optional enhancement being evaluated for a future release.</p>
          <p className="setting-p" style={{ marginTop: 4 }}><span className="pill gray">Not available in V1</span></p>
        </div>
      </div>
      <div className="inset" style={{ margin: '20px 0' }}>
        <p>CHRONOS tracks and classifies activity locally without AI. Optional AI integration may be introduced in a future version. Integrations are optional — CHRONOS works fully offline without any of them.</p>
      </div>
    </>
  )
}

/**
 * The browser-integration summary derives its state from the same authoritative
 * evidence as the integration dialog (the last valid authenticated browser signal),
 * polled only while this section is visible, so the two views can never contradict
 * each other.
 */
function BrowserIntegrationSetting() {
  const [status, setStatus] = useState(browserStatus(null))
  useEffect(() => {
    let alive = true
    const refresh = () => {
      void fetchPairingInfo().then((info) => {
        if (alive) setStatus(browserStatus(hintAgeSeconds(info)))
      })
    }
    refresh()
    const t = window.setInterval(refresh, 5000)
    return () => { alive = false; window.clearInterval(t) }
  }, [])
  return (
    <Setting h="Browser integration" p="Improves website attribution on supported browsers. Nothing is collected without it."
      control={
        <span className="row" style={{ gap: 8 }}>
          <span className="small" style={{ color: status.color, whiteSpace: 'nowrap' }} aria-live="polite">● {status.short}</span>
          <Button label="Manage" icon="monitor" onClick={() => actions.openModal({ kind: 'integration', name: 'Browser integration' })} />
        </span>
      } />
  )
}

function IntegrationSetting({ name, icon, p }: { name: string; icon: string; p: string }) {
  return <Setting h={name} p={p} control={<Button label="Not connected" icon={icon} onClick={() => actions.openModal({ kind: 'integration', name })} />} />
}

function AdvancedGroup() {
  const st = useStore((s) => s.appState)
  const caps = useStore((s) => s.capabilities)
  const diagnostics = useStore((s) => s.diagnostics)
  const [showPerf, setShowPerf] = useState(false)
  useEffect(() => {
    void actions.loadDiagnostics()
  }, [])
  if (!st) return null
  const tracking = st.tracking.state
  const measured = diagnostics?.measured ?? null
  const value = (v: number | null | undefined, render: (n: number) => string) =>
    v == null ? (diagnostics ? 'not available on this platform' : 'measuring…') : render(v)

  return (
    <>
      <Setting h="Tracking diagnostics" p={`Current state: ${statusMeta(tracking).label}. ${statusMeta(tracking).description}`}
        control={<Button label="Inspect status" onClick={() => actions.openModal({ kind: 'diagnostics' })} />} />
      <Setting h="Platform capabilities" p={`${caps?.platform ?? 'Unknown platform'} — live capability detection.`} control={<span />} />
      <CapabilityTable />
      <Setting h="Recovery behavior" p="Unknown prior state requires confirmation before resuming."
        control={<Button label="Simulate recovery" onClick={() => actions.openModal({ kind: 'recovery' })} />} />
      <Setting h="Performance" p="Measured on this machine, where measurement is available."
        control={<Button label={showPerf ? 'Hide measurements' : 'View measurements'} onClick={async () => {
          setShowPerf(!showPerf)
          if (!showPerf) await actions.loadDiagnostics()
        }} />} />
      {showPerf ? (
        <div className="inset" style={{ marginBottom: 20 }}>
          <span className="eyebrow">MEASURED</span>
          <p style={{ marginTop: 8 }}>
            Tracker CPU (recent average): {value(measured?.trackerThreadCpuPercent, (n) => `${n.toFixed(2)}%`)}<br />
            Process memory: {value(measured?.processMemoryMb, (n) => `${n.toFixed(0)} MB`)}<br />
            Uptime: {fmt((measured?.uptimeSeconds ?? 0) / 60)} · Intervals: {diagnostics?.db.intervals ?? '—'}<br />
            Database: {diagnostics ? (diagnostics.db.healthy ? 'healthy' : 'check needed') : 'measuring…'} · schema v{diagnostics?.db.schemaVersion ?? '—'}
          </p>
          <p className="section-caption" style={{ marginTop: 8 }}>
            Engineering targets (not guarantees): tracker CPU ≈ ≤1% average idle; tracker RAM ≈ ≤100 MB; whole-app RAM ≈ ≤250 MB.
          </p>
        </div>
      ) : null}
      <Setting h="Database" p={diagnostics ? `${diagnostics.db.path} · ${(diagnostics.db.sizeBytes / 1024).toFixed(0)} KB` : 'Local SQLite database.'} control={<span />} />
    </>
  )
}

function CapabilityTable() {
  const caps = useStore((s) => s.capabilities)
  if (!caps) return null
  return (
    <div style={{ overflow: 'auto', margin: '5px 0 18px' }}>
      <table className="cap-table">
        <caption className="small muted" style={{ textAlign: 'left', padding: '12px 0' }}>Live capabilities · {caps.platform}</caption>
        <thead><tr><th>Capability</th><th>Status</th></tr></thead>
        <tbody>
          {caps.capabilities.map((c) => (
            <tr key={c.name}>
              <td>{c.name}</td>
              <td>
                <span className={c.status === 'available' ? 'green' : c.status === 'unsupported' ? 'pink' : 'amber'}>
                  {c.status === 'available' ? '✓' : '◌'} {statusLabel(c.status)}
                </span>
                <div className="small muted">{c.detail}</div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function statusLabel(s: string): string {
  switch (s) {
    case 'available': return 'Available'
    case 'limited': return 'Limited'
    case 'permission-required': return 'Permission required'
    case 'unsupported': return 'Unsupported'
    case 'temporarily-unavailable': return 'Temporarily unavailable'
    default: return s
  }
}
