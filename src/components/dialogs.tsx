import { useEffect, useState } from 'react'
import { actions, useStore, currentReportRequest } from '../store'
import { backend } from '../backend'
import { Icon, AppIcon } from './icons'
import { Button, ModalHost, Toggle, withToast, statusMeta } from './ui'
import { cats, type BackupManifest, type Category, type RestorePlanSummary, type ActivityRecord } from '../types'
import { clock, fmt } from '../format'
import { fetchPairingInfo, hintAgeSeconds, browserStatus, tauri, type PairingInfo } from '../browserStatus'
import { useBlockingSummary, blockingSummaryText } from './blocking'

export function ModalRouter() {
  const modal = useStore((s) => s.modal)
  if (!modal) return <ModalHost title="">{null}</ModalHost>
  switch (modal.kind) {
    case 'activity': return <ModalHost title="A moment, with context."><ActivityDialog id={modal.recordId} /></ModalHost>
    case 'split': return <ModalHost title="Split, without adding time."><SplitDialog id={modal.recordId} /></ModalHost>
    case 'goals': return <ModalHost title="Set an intention."><GoalsDialog /></ModalHost>
    case 'review': return <ModalHost title="A little context goes a long way."><ReviewDialog /></ModalHost>
    case 'rules': return <ModalHost title="Your context comes first."><RulesDialog /></ModalHost>
    case 'exclusions': return <ModalHost title="Some things stay off the record."><ExclusionsDialog /></ModalHost>
    case 'deletion': return <ModalHost title="Let some history go."><DeletionDialog /></ModalHost>
    case 'backup': return <ModalHost title="Your time, safely kept."><BackupDialog /></ModalHost>
    case 'restore': return <ModalHost title="Restore, with a clear view."><RestoreDialog /></ModalHost>
    case 'export': return <ModalHost title="A clear record, ready to take with you."><ExportDialog /></ModalHost>
    case 'project': return <ModalHost title="Give it a name."><ProjectDialog id={modal.projectId} /></ModalHost>
    case 'project-delete': return <ModalHost title="Delete, for good."><ProjectDeleteDialog id={modal.projectId} /></ModalHost>
    case 'assign-activities': return <ModalHost title="Assign activities."><AssignActivitiesDialog projectId={modal.projectId} /></ModalHost>
    case 'manual-entry': return <ModalHost title="Time you want on the record."><ManualEntryDialog recordId={modal.recordId} /></ModalHost>
    case 'blocklist': return <ModalHost title="Distraction block list."><BlockListDialog /></ModalHost>
    case 'idle-exceptions': return <ModalHost title="Idle exceptions."><IdleExceptionsDialog /></ModalHost>
    case 'schedule': return <ModalHost title="Tracking schedule."><ScheduleDialog /></ModalHost>
    case 'session-start': return <ModalHost title="Make room for deep work."><SessionStartDialog /></ModalHost>
    case 'onboarding': return <ModalHost title="CHRONOS"><OnboardingDialog /></ModalHost>
    case 'notifications': return <ModalHost title="Quietly useful."><NotificationsDialog /></ModalHost>
    case 'integration': return <ModalHost title={`${modal.name} · optional.`}><IntegrationDialog name={modal.name} /></ModalHost>
    case 'diagnostics': return <ModalHost title="Tracking diagnostics"><DiagnosticsDialog /></ModalHost>
    case 'recovery': return <ModalHost title="Recovery behavior"><RecoveryDialog /></ModalHost>
    case 'performance': return <ModalHost title="Performance measurements"><PerformanceDialog /></ModalHost>
    case 'tracking-enable': return <ModalHost title="Enable tracking?"><TrackingEnableDialog /></ModalHost>
    case 'privacy-leave': return <ModalHost title="Privacy stays in your hands."><PrivacyLeaveDialog /></ModalHost>
    case 'quit': return <ModalHost title="Leave things just as you chose."><QuitDialog /></ModalHost>
  }
}

/* ---------------- activity detail / correction ---------------- */

function ActivityDialog({ id }: { id: number }) {
  const records = useStore((s) => s.timelineRecords) ?? []
  const overviewRecent = useStore((s) => s.overview?.recent) ?? []
  const r = records.find((x) => x.id === id) ?? overviewRecent.find((x) => x.id === id)
  const projects = useStore((s) => s.appState?.projects) ?? []
  const twelve = useStore((s) => s.appState?.settings.time === '12-hour')
  const [cat, setCat] = useState<Category>(r?.cat ?? 'unclassified')
  const [project, setProject] = useState(r?.project ?? '')
  const [futureRule, setFutureRule] = useState(false)
  if (!r) return null
  const isIdle = r.cat === 'idle'
  return (
    <>
      <p className="dialog-description">{r.app}{r.detail ? ` · ${r.detail}` : ''}<br />{r.date} · {clock(r.start, twelve)} – {clock(r.end, twelve)}</p>
      <div className="detail-grid">
        <div className="detail-stat"><span className="muted small">Observed duration</span><strong>{fmt(r.end - r.start)}</strong></div>
        <div className="detail-stat"><span className="muted small">Classification source</span><strong style={{ fontSize: 12, lineHeight: 1.6 }}>{r.corrected ? 'Explicit user correction' : r.source}</strong></div>
        <div className="detail-stat"><span className="muted small">Confidence</span><strong style={{ fontSize: 12 }}>
          {r.corrected ? 'Certain — your correction' : r.confidence != null ? `${Math.round(r.confidence * 100)}%` : '—'}
        </strong></div>
        <div className="detail-stat"><span className="muted small">Project attribution</span><strong style={{ fontSize: 12, lineHeight: 1.6 }}>{projectReason(r)}</strong></div>
      </div>
      {!r.corrected && r.cat === 'unclassified' ? (
        <div className="info-strip" style={{ marginTop: 16 }}><Icon name="info" />Not enough context to classify this automatically — it stays unclassified until you decide.</div>
      ) : null}
      {isIdle ? (
        <div className="info-strip" style={{ marginTop: 20 }}><Icon name="info" />Idle intervals remain idle. They can be split, but are not counted as active work.</div>
      ) : r.end - r.start < 2 ? (
        <div className="info-strip" style={{ marginTop: 20 }}><Icon name="info" />This interval is only {fmt(r.end - r.start)} long — too short to split meaningfully, so it keeps its full duration.</div>
      ) : (
        <>
          <div className="form-grid">
            <label>
              <span className="input-label">Category</span>
              <select className="field" value={cat} onChange={(e) => setCat(e.target.value as Category)}>
                {(['productive', 'distracting', 'neutral', 'unclassified'] as const).map((c) => <option key={c} value={c}>{cats[c].label}</option>)}
              </select>
            </label>
            <label>
              <span className="input-label">Project</span>
              <select className="field" value={project} onChange={(e) => setProject(e.target.value)}>
                <option value="">No project</option>
                {projects.filter((p) => !p.archived || p.id === r.project).map((p) => (
                  <option key={p.id} value={p.id}>{p.name}{p.archived ? ' (archived)' : ''}</option>
                ))}
              </select>
            </label>
          </div>
          <label className="check-row">
            <input type="checkbox" checked={futureRule} onChange={(e) => setFutureRule(e.target.checked)} />
            Also create a rule for future activity from {r.app}. Existing history is unchanged unless explicitly reapplied.
          </label>
        </>
      )}
      <p className="section-caption" style={{ margin: '20px 0 0' }}>
        A correction updates this interval and affected totals only. It does not create additional focused minutes. Undo is available.
      </p>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Split interval" disabled={r.end - r.start < 2}
          onClick={() => { if (r.end - r.start >= 2) actions.openModal({ kind: 'split', recordId: r.id }) }} />
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        {!isIdle ? (
          <Button label="Save correction" variant="primary"
            onClick={() => { actions.closeModal(); void withToast(backend.correctRecord(r.id, cat, project, futureRule), 'Interval corrected. Dependent totals recalculated.', true) }} />
        ) : null}
      </div>
    </>
  )
}

function SplitDialog({ id }: { id: number }) {
  const records = useStore((s) => s.timelineRecords) ?? []
  const r = records.find((x) => x.id === id)
  const appState = useStore((s) => s.appState)
  const duration = r ? r.end - r.start : 0
  const eligible = duration >= 2
  const maxMinutes = Math.max(1, Math.floor(duration) - 1)
  const [minutes, setMinutes] = useState(Math.floor(duration / 2))
  if (!r) return null
  const twelve = appState?.settings.time === '12-hour'
  return (
    <>
      <p className="dialog-description">{r.app} · {clock(r.start, twelve)}–{clock(r.end, twelve)} · {fmt(duration)} total<br />
        The two resulting intervals preserve the exact original duration. Focused minutes are allocated proportionally.</p>
      {eligible ? (
        <label>
          <span className="input-label">Minutes in first interval (1–{maxMinutes}m)</span>
          <input className="field" type="number" min={1} max={maxMinutes} value={minutes}
            onChange={(e) => setMinutes(Number(e.target.value))} style={{ width: '100%' }} />
        </label>
      ) : (
        <div className="info-strip" style={{ marginTop: 16 }}>
          <Icon name="info" />
          This interval is only {fmt(duration)} long — too short to split meaningfully. It keeps its full duration.
        </div>
      )}
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        {eligible ? (
          <Button label="Split interval" variant="primary"
            onClick={() => {
              if (!Number.isInteger(minutes) || minutes < 1 || minutes > maxMinutes) { actions.toast('Choose a valid whole-minute split point.'); return }
              actions.closeModal()
              void withToast(backend.splitRecord(r.id, minutes), 'Interval split. Total observed time is unchanged.', true)
            }} />
        ) : null}
      </div>
    </>
  )
}

function projectReason(r: { project: string; projectSource?: string }) {
  if (!r.project) return 'Unassigned'
  switch (r.projectSource) {
    case 'rule': return 'Assignment rule'
    case 'learned': return 'Learned from your assignments'
    case 'user': return 'Your explicit assignment'
    default: return r.project
  }
}

/* ---------------- goals ---------------- */

function GoalsDialog() {
  const goals = useStore((s) => s.appState?.goals)
  const projects = useStore((s) => s.appState?.projects) ?? []
  const settings = useStore((s) => s.appState?.settings)
  const [values, setValues] = useState<{ daily: number; weekly: number; project: number; limit: number }>(
    goals ?? { daily: 240, weekly: 1200, project: 240, limit: 60 })
  const [goalProject, setGoalProject] = useState(settings?.goalProject ?? '')
  if (!goals) return null
  const active = projects.filter((p) => !p.archived)
  const targetName = active.find((p) => p.id === goalProject)?.name ?? active[0]?.name ?? 'Project'
  const fields: [keyof typeof values, string, number, number][] = [
    ['daily', 'Daily focused time', 15, 1440],
    ['weekly', 'Weekly focused time', 15, 10080],
    ['project', `${targetName} project time / day`, 15, 1440],
    ['limit', 'Daily distraction limit (0 = off)', 0, 1440],
  ]
  return (
    <>
      <p className="dialog-description">Enough structure to guide your day. Enough flexibility to live it.<br />Targets are in minutes and update all goal views.</p>
      <div className="form-grid">
        {fields.map(([key, label, min, max]) => (
          <label key={key}>
            <span className="input-label">{key === 'project' ? `${targetName} project time / day` : label}</span>
            <input type="number" className="field" min={min} max={max} step={5} value={values[key]} required
              onChange={(e) => setValues({ ...values, [key]: Number(e.target.value) })} />
          </label>
        ))}
      </div>
      <label style={{ display: 'block', marginTop: 14 }}>
        <span className="input-label">Project measured by the project-time goal</span>
        <select className="field" style={{ width: '100%' }} value={goalProject}
          onChange={(e) => setGoalProject(e.target.value)}>
          <option value="">First active project ({active[0]?.name ?? 'none'})</option>
          {active.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <span className="small muted" style={{ display: 'block', marginTop: 6 }}>
          The Overview, Focus &amp; Goals, and Reports project rows all measure this project.
        </span>
      </label>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Save intentions" variant="primary"
          onClick={() => {
            const valid = fields.every(([key, , min, max]) => Number.isFinite(values[key]) && values[key] >= min && values[key] <= max)
            if (!valid) { actions.toast('Use valid targets: 15-1,440 minutes daily, up to 10,080 weekly. The limit may be 0.'); return }
            actions.closeModal()
            void withToast(
              backend.updateSettings({ goalProject }).then(() => backend.setGoals(values)),
              'Your intentions have been updated.',
            )
          }} />
      </div>
    </>
  )
}

/* ---------------- review queue ---------------- */

function ReviewDialog() {
  const records = useStore((s) => s.timelineRecords) ?? []
  const date = useStore((s) => s.timelineDate)
  const twelve = useStore((s) => s.appState?.settings.time === '12-hour')
  const rows = records.filter((r) => r.cat === 'unclassified')
  return (
    <>
      <p className="dialog-description">Uncertain activity · {date}. These intervals contribute to tracked time, but not productive or distracting totals.</p>
      {rows.length ? rows.map((r) => (
        <button className="activity-row" key={r.id} onClick={() => actions.openModal({ kind: 'activity', recordId: r.id })}>
          <AppIcon record={r} />
          <span className="activity-name"><strong>{r.app}</strong><small>{clock(r.start, twelve)} – {clock(r.end, twelve)} · low confidence</small></span>
          <span className="violet">Review →</span>
        </button>
      )) : <div className="empty">You’re all caught up. No uncertain intervals on this date.</div>}
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Done" variant="primary" onClick={() => actions.closeModal()} />
      </div>
    </>
  )
}

/* ---------------- rules ---------------- */

function RulesDialog() {
  const rules = useStore((s) => s.appState?.rules) ?? []
  const projects = useStore((s) => s.appState?.projects) ?? []
  const [app, setApp] = useState('')
  const [cat, setCat] = useState('productive')
  const [project, setProject] = useState('')
  const [retroIndex, setRetroIndex] = useState<number | null>(null)
  const projectName = (id?: string) => projects.find((p) => p.id === id)?.name ?? ''
  return (
    <>
      <p className="dialog-description">Rules apply to future activity by default. Your explicit corrections always take priority.</p>
      {rules.length ? rules.map((r, i) => (
        <div className="history-row" key={`${r.app}-${i}`}>
          <div>{r.app}<small>Future → {cats[r.cat].label}{r.project ? ` · ${projectName(r.project)}` : ''}</small></div>
          {retroIndex === i ? (
            <div className="inset" style={{ marginTop: 12 }}>
              <p>Explicit retroactive action: matching intervals that were never corrected will become {cats[r.cat].label.toLowerCase()}. User-corrected intervals take priority and remain unchanged.</p>
              <div className="row">
                <Button label="Cancel" variant="ghost" onClick={() => setRetroIndex(null)} />
                <Button label="Reclassify matching history" variant="primary"
                  onClick={() => {
                    setRetroIndex(null)
                    void withToast(backend.applyRuleRetroactive(i).then((n) => n), 'Matching history reclassified. Explicit corrections preserved.', true)
                  }} />
              </div>
            </div>
          ) : (
            <button className="text-btn" onClick={() => setRetroIndex(i)}>Apply to history…</button>
          )}
        </div>
      )) : <div className="inset"><p>No rules yet. Add one here, or create one while correcting an activity.</p></div>}
      <label>
        <span className="input-label">Application or website label</span>
        <input className="field" style={{ width: '100%' }} placeholder="e.g. Browser research" value={app} onChange={(e) => setApp(e.target.value)} />
      </label>
      <label>
        <span className="input-label">Classification</span>
        <select className="field" style={{ width: '100%' }} value={cat} onChange={(e) => setCat(e.target.value)}>
          {(['productive', 'neutral', 'distracting', 'unclassified'] as const).map((c) => <option key={c} value={c}>{cats[c].label}</option>)}
        </select>
      </label>
      <label>
        <span className="input-label">Assign to project (optional)</span>
        <select className="field" style={{ width: '100%' }} value={project} onChange={(e) => setProject(e.target.value)}>
          <option value="">No project</option>
          {projects.filter((p) => !p.archived).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
      </label>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Done" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Add future rule" variant="primary"
          onClick={() => {
            if (!app.trim()) { actions.toast('Enter an application or website label.'); return }
            void withToast(backend.addRule(app.trim(), cat, project), 'Rule saved for future activity only. History is unchanged.').then(() => setApp(''))
          }} />
      </div>
    </>
  )
}

/* ---------------- exclusions ---------------- */

function ExclusionsDialog() {
  const exclusions = useStore((s) => s.appState?.exclusions) ?? []
  const [name, setName] = useState('')
  return (
    <>
      <p className="dialog-description">
        Exclusions are a privacy control: matching <strong>future</strong> activity is simply not recorded — the
        application or website stays fully usable, and existing history is never rewritten. Enter an application
        name (e.g. <code>Steam</code>) or a website in any form (<code>youtube.com</code>, <code>www.youtube.com</code>,
        or a full URL) — it is saved as the site's domain, and covers its subdomains too.
      </p>
      <div style={{ borderTop: '1px solid var(--line)', borderBottom: '1px solid var(--line)', margin: '4px 0' }}>
        {exclusions.length === 0 ? (
          <div className="empty" style={{ padding: '28px 20px' }}>No exclusions configured. Everything observed is recorded.</div>
        ) : exclusions.map((e) => (
          <div className="activity-row" key={e}>
            <span className="dot bg-pink" />
            <span className="activity-name"><strong>{e}</strong><small>Future activity ignored · history untouched</small></span>
            <Button label="Remove" variant="ghost"
              onClick={() => { void withToast(backend.removeExclusion(e), `Exclusion removed. ${e} will be tracked again from now on.`) }} />
          </div>
        ))}
      </div>
      <label>
        <span className="input-label">Application name or website</span>
        <input className="field" style={{ width: '100%' }} placeholder="private.example · Steam · https://www.youtube.com" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Done" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Add exclusion" variant="primary"
          onClick={() => {
            if (!name.trim()) { actions.toast('Enter an application or domain.'); return }
            const op = backend.addExclusion(name.trim())
            void withToast(op, 'Exclusion saved. It applies from now on; existing history is unchanged.').then(() => setName(''))
            void op.catch((err) => actions.toast(String(err)))
          }} />
      </div>
    </>
  )
}

/* ---------------- historical deletion ---------------- */

function DeletionDialog() {
  const [start, setStart] = useState('')
  const [end, setEnd] = useState('')
  const [confirmed, setConfirmed] = useState(false)
  return (
    <>
      <p className="dialog-description">Delete intervals in the selected date range. Unrelated dates are preserved. You can undo this action.</p>
      <div className="form-grid">
        <label><span className="input-label">From</span><input className="field" type="date" value={start} onChange={(e) => setStart(e.target.value)} /></label>
        <label><span className="input-label">Through</span><input className="field" type="date" value={end} onChange={(e) => setEnd(e.target.value)} /></label>
      </div>
      <label className="check-row">
        <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
        Delete this date range from my local history.
      </label>
      <p className="small muted" style={{ marginTop: 16 }}>An older backup could still contain deleted records. Restore always shows an explicit warning.</p>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Delete selected history" danger
          onClick={() => {
            if (!start || !end || start > end) { actions.toast('Choose a valid, ordered date range.'); return }
            if (!confirmed) { actions.toast('Confirm the selected deletion first.'); return }
            actions.closeModal()
            void withToast(backend.deleteHistory(start, end).then((n) => n), 'Selected history deleted. Other dates are unchanged.', true)
          }} />
      </div>
    </>
  )
}

/* ---------------- backup / restore / export ---------------- */

function BackupDialog() {
  const [manifest, setManifest] = useState<BackupManifest | null>(null)
  const [busy, setBusy] = useState(false)
  return (
    <>
      <p className="dialog-description">A local backup contains your intervals, projects, rules, and preferences — verified before it is written.</p>
      {manifest ? (
        <div className="inset">
          <div className="row between">
            <strong>CHRONOS local backup</strong>
            <span className={manifest.integrity === 'passed' ? 'green' : 'pink'}>{manifest.integrity === 'passed' ? '✓ Integrity check passed' : '✕ Integrity check failed'}</span>
          </div>
          <p style={{ marginTop: 12 }}>
            {manifest.intervals} intervals · {fmt(manifest.trackedMinutes)} tracked<br />
            {manifest.projects} projects · rules and preferences included<br />
            Range: {manifest.rangeStart} – {manifest.rangeEnd} · {(manifest.sizeBytes / 1024).toFixed(0)} KB
          </p>
          <p className="small muted" style={{ marginTop: 8 }}>Saved to: {manifest.path}</p>
        </div>
      ) : (
        <div className="inset"><p>Choose a destination for a single-file backup. A real backup may contain application names, website context, and historical activity — keep it somewhere private.</p></div>
      )}
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Close" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label={busy ? 'Backing up…' : manifest ? 'Back up again' : 'Choose location & back up'} variant="primary" disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              const dir = await pickDirectory()
              const m = await backend.createBackup(dir)
              setManifest(m)
              actions.toast(m.integrity === 'passed' ? 'Backup created and verified.' : 'Backup integrity check failed.')
            } catch (e) {
              actions.toast(e instanceof Error ? e.message : 'Backup failed.')
            } finally {
              setBusy(false)
            }
          }} />
      </div>
    </>
  )
}

async function pickDirectory(): Promise<string | null> {
  if (backend.kind !== 'tauri') return null
  const { save } = await import('@tauri-apps/plugin-dialog')
  const now = new Date().toISOString().slice(0, 10)
  const path = await save({ defaultPath: `chronos-backup-${now}.chronosbak`, filters: [{ name: 'CHRONOS backup', extensions: ['chronosbak'] }] })
  return typeof path === 'string' ? path : null
}

async function pickBackupFile(): Promise<string | null> {
  if (backend.kind !== 'tauri') return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const path = await open({ multiple: false, filters: [{ name: 'CHRONOS backup', extensions: ['chronosbak'] }] })
  return typeof path === 'string' ? path : null
}

async function pickExportPath(defaultName: string): Promise<string | null> {
  if (backend.kind !== 'tauri') return null
  const { save } = await import('@tauri-apps/plugin-dialog')
  const path = await save({ defaultPath: defaultName })
  return typeof path === 'string' ? path : null
}

function RestoreDialog() {
  const [plan, setPlan] = useState<RestorePlanSummary | null>(null)
  const [path, setPath] = useState<string | null>(null)
  const [strategy, setStrategy] = useState<'merge' | 'replace'>('merge')
  const [preservePrivacy, setPreservePrivacy] = useState(true)
  const [allowDeleted, setAllowDeleted] = useState(false)
  const [busy, setBusy] = useState(false)
  return (
    <>
      <p className="dialog-description">Inspect the backup before choosing what to bring back. Nothing is changed until you confirm.</p>
      {!plan ? (
        <div className="inset"><p>Select a .chronosbak backup file to inspect its contents, verify integrity, and review conflicts.</p></div>
      ) : (
        <div className="inset">
          <span className="green">✓ Backup integrity verified</span>
          <p style={{ marginTop: 8 }}>
            {plan.backupIntervalCount} intervals · {plan.backupProjectCount} projects · {plan.conflicts} conflicts<br />
            Merge: retain current intervals, skip matching IDs.<br />
            Replace: replace current history with the backup snapshot.
          </p>
        </div>
      )}
      <label>
        <span className="input-label">Restore strategy</span>
        <select className="field" style={{ width: '100%' }} value={strategy} onChange={(e) => setStrategy(e.target.value as never)}>
          <option value="merge">Merge with existing data</option>
          <option value="replace">Replace existing history</option>
        </select>
      </label>
      <label className="check-row">
        <input type="checkbox" checked={preservePrivacy} onChange={(e) => setPreservePrivacy(e.target.checked)} />
        Preserve current privacy, pause, and exclusion preferences (recommended).
      </label>
      {plan && plan.previouslyDeletedRecords > 0 ? (
        <>
          <div className="info-strip" style={{ margin: '18px 0 0', borderColor: '#edc78644', color: 'var(--amber)' }}>
            <Icon name="info" />
            This backup may include up to {plan.previouslyDeletedRecords} activity records you previously deleted. Reintroducing them requires your explicit confirmation.
          </div>
          <label className="check-row">
            <input type="checkbox" checked={allowDeleted} onChange={(e) => setAllowDeleted(e.target.checked)} />
            I explicitly allow previously deleted records to be reintroduced by this restore.
          </label>
        </>
      ) : null}
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        {!plan ? (
          <Button label="Choose backup file…" variant="primary" disabled={busy}
            onClick={async () => {
              setBusy(true)
              try {
                const p = await pickBackupFile()
                if (!p) { setBusy(false); return }
                setPath(p)
                const inspected = await backend.inspectBackup(p)
                setPlan(inspected)
                if (inspected.previouslyDeletedRecords > 0 && !allowDeleted) { /* require below */ }
              } catch (e) {
                actions.toast(e instanceof Error ? e.message : 'Could not inspect backup.')
              } finally {
                setBusy(false)
              }
            }} />
        ) : (
          <Button label={busy ? 'Restoring…' : 'Review restore plan'} variant="primary" disabled={busy}
            onClick={async () => {
              if (plan.previouslyDeletedRecords > 0 && !allowDeleted) { actions.toast('Explicit confirmation is required to reintroduce previously deleted records.'); return }
              setBusy(true)
              try {
                const result = await backend.restoreBackup(path ?? '', { ...plan, strategy, preservePrivacy, allowDeletedReintroduction: allowDeleted })
                actions.closeModal()
                actions.toast(`Restore complete · ${result.restored} intervals restored${result.conflictsSkipped ? ` · ${result.conflictsSkipped} conflicts skipped` : ''}.`)
                await import('../store').then((m) => m.refreshView())
              } catch (e) {
                actions.toast(e instanceof Error ? e.message : 'Restore failed.')
              } finally {
                setBusy(false)
              }
            }} />
        )}
      </div>
    </>
  )
}

function ExportDialog() {
  const [format, setFormat] = useState<'CSV' | 'JSON'>('CSV')
  const req = currentReportRequest()
  return (
    <>
      <p className="dialog-description">Export the selected report range ({req.start} → {req.end}).</p>
      <label>
        <span className="input-label">Format</span>
        <select className="field" style={{ width: '100%' }} value={format} onChange={(e) => setFormat(e.target.value as never)}>
          <option value="CSV">CSV · intervals & categories</option>
          <option value="JSON">JSON · structured report</option>
        </select>
      </label>
      <div className="inset" style={{ marginTop: 18 }}>
        <p>Gaps are excluded. Application and website context may be sensitive — choose a private location.</p>
      </div>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Export…" variant="primary"
          onClick={async () => {
            const ext = format.toLowerCase()
            const suggested = await pickExportPath(`chronos-export-${req.start}_${req.end}.${ext}`)
            if (suggested === null && backend.kind === 'tauri') return
            actions.closeModal()
            void withToast(backend.exportData(format, req, suggested), `Exported to ${suggested ?? 'your chosen location'}.`)
          }} />
      </div>
    </>
  )
}

/* ---------------- projects & session ---------------- */

function ProjectDialog({ id }: { id?: string }) {
  const projects = useStore((s) => s.appState?.projects) ?? []
  const p = projects.find((x) => x.id === id)
  const [name, setName] = useState(p?.name ?? '')
  const [desc, setDesc] = useState(p?.desc ?? '')
  const [client, setClient] = useState(p?.client ?? '')
  return (
    <>
      <p className="dialog-description">Projects connect activities, goals, and reports. An optional client groups projects for reporting.</p>
      <label>
        <span className="input-label">Project name</span>
        <input className="field" maxLength={40} style={{ width: '100%' }} value={name} placeholder="Your next good idea"
          onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span className="input-label">A short intention (optional)</span>
        <input className="field" maxLength={100} style={{ width: '100%' }} value={desc} placeholder="What will you make space for?"
          onChange={(e) => setDesc(e.target.value)} />
      </label>
      <label>
        <span className="input-label">Client (optional)</span>
        <input className="field" maxLength={60} style={{ width: '100%' }} value={client} placeholder="Leave empty for a personal project"
          onChange={(e) => setClient(e.target.value)} />
      </label>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label={p ? 'Save project' : 'Create project'} variant="primary"
          onClick={() => {
            const trimmed = name.trim()
            if (!trimmed) { actions.toast('Give your project a name.'); return }
            if (projects.some((x) => x.id !== id && x.name.toLowerCase() === trimmed.toLowerCase())) { actions.toast('A project with this name already exists.'); return }
            actions.closeModal()
            void withToast(p ? backend.renameProject(p.id, trimmed, desc.trim(), client.trim()) : backend.createProject(trimmed, desc.trim(), client.trim()), 'Project saved across the workspace.')
          }} />
      </div>
    </>
  )
}

function SessionStartDialog() {
  const projects = useStore((s) => s.appState?.projects) ?? []
  const [intention, setIntention] = useState('Deep work')
  const [project, setProject] = useState('')
  const [focusMinutes, setFocusMinutes] = useState(25)
  const [breakMinutes, setBreakMinutes] = useState(5)
  const [breaksEnabled, setBreaksEnabled] = useState(true)
  return (
    <>
      <p className="dialog-description">
        A deliberate focus period with honest accounting. CHRONOS observes your actual activity through the existing
        tracking engine — the timer alone never counts as focus. When breaks are on, you'll get a reminder at each
        interval's end and choose when to resume.
      </p>
      <label>
        <span className="input-label">Session intention</span>
        <input className="field" value={intention} style={{ width: '100%' }} onChange={(e) => setIntention(e.target.value)} />
      </label>
      <label>
        <span className="input-label">Project</span>
        <select className="field" style={{ width: '100%' }} value={project} onChange={(e) => setProject(e.target.value)}>
          <option value="">No project</option>
          {projects.filter((x) => !x.archived).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
        </select>
      </label>
      <div className="form-grid" style={{ marginTop: 8 }}>
        <label>
          <span className="input-label">Focus duration</span>
          <select className="field" style={{ width: '100%' }} value={focusMinutes} onChange={(e) => setFocusMinutes(Number(e.target.value))}>
            {[20, 25, 30, 45].map((m) => <option key={m} value={m}>{m} minutes</option>)}
          </select>
        </label>
        <label>
          <span className="input-label">Break duration</span>
          <select className="field" style={{ width: '100%' }} value={breakMinutes} onChange={(e) => setBreakMinutes(Number(e.target.value))} disabled={!breaksEnabled}>
            {[5, 10, 15].map((m) => <option key={m} value={m}>{m} minutes</option>)}
          </select>
        </label>
      </div>
      <div className="check-row">
        <input type="checkbox" checked={breaksEnabled} onChange={(e) => setBreaksEnabled(e.target.checked)} id="breaks-enabled" />
        <label htmlFor="breaks-enabled">Scheduled breaks — a reminder at each focus interval's end, with your choice of when to resume. Uncheck for one uninterrupted focus period.</label>
      </div>
      <p className="small muted" style={{ marginTop: 12 }}>A running session never generates tracked activity on its own; qualified focus comes only from observed productive intervals.</p>
      <div className="dialog-actions" style={{ marginTop: 16 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Begin session" variant="primary" icon="play"
          onClick={() => { actions.closeModal(); void withToast(backend.startSession(intention.trim() || 'Deep work', project, focusMinutes, breakMinutes, breaksEnabled), 'Focus session started.') }} />
      </div>
    </>
  )
}

/* ---------------- onboarding ---------------- */

const onboardingPages: [string, React.ReactNode][] = [
  ['A better relationship with time.', <>CHRONOS helps you understand your day, without making your day about tracking it. Your activity stays local. No account, calendar, or AI setup required.</>],
  ['Your time. Your permission.', <>CHRONOS observes application names, optional window context, and idle intervals. You decide whether to begin. Pause and privacy controls are always within reach.</>],
  ['A little access, clearly explained.', <>Windows, macOS, and Linux offer different capabilities. macOS may need Accessibility access; some Linux Wayland environments provide limited context. CHRONOS explains limitations before requesting anything.</>],
  ['Start with what feels right.', <>Choose a few essentials. You can change everything later.</>],
  ['Ready, at your own pace.', <>Your workspace is yours. You can change these preferences, revisit privacy controls, or pause at any time. For website recognition (e.g. ChatGPT, YouTube), an optional browser extension can be paired later in Settings → Integrations.</>],
]

function OnboardingDialog() {
  const step = useStore((s) => s.onboardStep)
  const consent = useStore((s) => s.onboardConsent)
  const settings = useStore((s) => s.appState?.settings)
  const profile = useStore((s) => s.appState?.profile)
  const [name, setName] = useState(profile?.name ?? '')
  const [, p] = onboardingPages[step]
  return (
    <>
      <div className="onboard-art"><div className="logomark" aria-hidden="true" /></div>
      <div className="step-dots" aria-label={`Step ${step + 1} of 5`}>
        {onboardingPages.map((_, i) => <span key={i} className={i <= step ? 'active' : ''} />)}
      </div>
      <p className="dialog-description">{p}</p>
      {step === 1 ? (
        <label className="check-row">
          <input type="checkbox" checked={consent} onChange={(e) => actions.setOnboardConsent(e.target.checked)} />
          I choose to enable tracking when I finish setup. Leaving this unchecked keeps tracking disabled.
        </label>
      ) : null}
      {step === 2 ? <div className="inset"><p>No operating-system permission is requested here. Permission-required, limited, and unsupported states are explained in Settings → Advanced & Diagnostics.</p></div> : null}
      {step === 3 && settings ? (
        <>
          <div className="setting-row">
            <div><h3>Launch at startup</h3><p>Register with your operating system.</p></div>
            <Toggle checked={settings.startup} label="Launch at startup" onChange={(v) => void backend.updateSettings({ startup: v })} />
          </div>
          <div className="setting-row">
            <div><h3>Detect idle time</h3><p>Keep breaks separate from active time.</p></div>
            <Toggle checked={settings.autoIdle} label="Detect idle time" onChange={(v) => void backend.updateSettings({ autoIdle: v })} />
          </div>
        </>
      ) : null}
      {step === 4 ? (
        <>
          <label>
            <span className="input-label">What should CHRONOS call you?</span>
            <input className="field" maxLength={40} value={name} placeholder={profile?.name ?? 'Your name or nickname'}
              onChange={(e) => setName(e.target.value)} style={{ width: '100%' }} />
            <span className="small muted" style={{ display: 'block', marginTop: 6 }}>Optional. Leave empty to use your system account name. You can change it later in Settings.</span>
          </label>
          <div className="inset" style={{ marginTop: 14 }}>
            <p>{consent ? 'Tracking consent selected. Existing manual pause and privacy preferences will still be respected.' : 'No tracking consent selected. Finishing keeps tracking disabled.'}</p>
          </div>
        </>
      ) : null}
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        {step > 0
          ? <Button label="Back" variant="ghost" onClick={() => actions.setOnboardStep(step - 1)} />
          : <Button label="Maybe later" variant="ghost" onClick={() => actions.closeModal()} />}
        {step < 4 ? (
          <Button label="Continue" variant="primary" onClick={() => actions.setOnboardStep(step + 1)}>Continue <Icon name="arrow" /></Button>
        ) : (
          <Button label="Enter my workspace" variant="primary"
            onClick={() => {
              actions.closeModal()
              const saveName = name.trim() ? backend.updateSettings({ displayName: name.trim() }) : Promise.resolve()
              void saveName.then(() => withToast(backend.completeOnboarding(consent), 'Setup complete. Your explicit tracking preferences are respected.'))
            }} />
        )}
      </div>
    </>
  )
}

/* ---------------- notifications preview ---------------- */

function NotificationsDialog() {
  const dnd = useStore((s) => s.appState?.dnd ?? false)
  return (
    <>
      <p className="dialog-description">Preview notifications in context. Do Not Disturb suppresses nonessential notices; significant tracking degradation remains visible.</p>
      <div className="stack">
        <Button label="Gentle break reminder" icon="bell" onClick={() => { actions.closeModal(); showNotice('break') }} />
        <Button label="Focus-session completion" icon="check" onClick={() => { actions.closeModal(); showNotice('complete') }} />
        <Button label="Significant tracking degradation" icon="info" onClick={() => { actions.closeModal(); showNotice('degraded') }} />
      </div>
      <div className="setting-row">
        <div><h3>Do Not Disturb</h3><p>Current state: {dnd ? 'On' : 'Off'}</p></div>
        <Toggle checked={dnd} label="Do Not Disturb" onChange={(v) => withToast(backend.setDnd(v), `Do Not Disturb ${v ? 'on' : 'off'}.`)} />
      </div>
    </>
  )
}

function showNotice(type: 'break' | 'complete' | 'degraded') {
  const data = {
    break: ['A moment to reset.', 'You\'ve made space for good work. Stretch, rest your eyes, or take a quiet breath.'],
    complete: ['A little progress worth noticing.', 'Your session is complete. Qualified time and elapsed time are recorded separately.'],
    degraded: ['Some tracking context is unavailable.', 'Foreground context is limited right now. Missing detail stays unknown; observed intervals are not invented.'],
  }[type]
  actions.showNotice(type, data[0], data[1])
}

/* ---------------- integration / diagnostics / recovery / performance ---------------- */

type BrowserId = 'chrome' | 'edge' | 'brave' | 'opera' | 'firefox' | 'zen'

const BROWSER_CHOICES: { id: BrowserId; label: string; family: 'chromium' | 'firefox'; page: string }[] = [
  { id: 'chrome', label: 'Chrome', family: 'chromium', page: 'chrome://extensions' },
  { id: 'edge', label: 'Edge', family: 'chromium', page: 'edge://extensions' },
  { id: 'brave', label: 'Brave', family: 'chromium', page: 'brave://extensions' },
  { id: 'opera', label: 'Opera', family: 'chromium', page: 'opera://extensions' },
  { id: 'firefox', label: 'Firefox', family: 'firefox', page: 'about:debugging' },
  { id: 'zen', label: 'Zen', family: 'firefox', page: 'about:debugging' },
]

/**
 * Official store listings, keyed by browser. Empty until a listing is actually
 * published — CHRONOS never claims one-click installation before a real URL exists.
 * Once published, set the entry here and the Connect flow automatically prefers
 * "Install from <store>" over the local Developer-mode setup.
 */
const STORE_LISTINGS: Partial<Record<BrowserId, string>> = {}

function BrowserIntegrationPanel({ pairing }: { pairing: PairingInfo | null }) {
  const [browser, setBrowser] = useState<BrowserId>('chrome')
  const [hintAge, setHintAge] = useState<number | null>(null)
  const choice = BROWSER_CHOICES.find((b) => b.id === browser) ?? BROWSER_CHOICES[0]
  const storeListing = STORE_LISTINGS[choice.id]
  const refresh = () => {
    void fetchPairingInfo().then((info) => setHintAge(hintAgeSeconds(info)))
  }
  useEffect(() => {
    refresh()
    const t = window.setInterval(refresh, 5000) // only while this dialog is open
    return () => window.clearInterval(t)
  }, [])
  const connect = async () => {
    try {
      await tauri()?.invoke('open_extension_folder', { family: choice.family })
      actions.toast(`${choice.label} extension folder opened — continue with the steps below.`)
    } catch {
      actions.toast('Could not open the extension folder. It lives inside the CHRONOS install folder under browser-extension.')
    }
  }
  const installFromStore = async () => {
    if (!storeListing) return
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    try {
      await openUrl(storeListing)
    } catch {
      actions.toast('Could not open the store listing.')
    }
  }
  const reopenPairPage = async () => {
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    try {
      await openUrl(`http://127.0.0.1:${pairing?.port ?? 53711}/pair`)
    } catch {
      actions.toast('Could not open the pairing page.')
    }
  }
  const copyToken = async () => {
    if (!pairing?.token) return
    try {
      await navigator.clipboard.writeText(pairing.token)
      actions.toast('Pairing token copied.')
    } catch {
      actions.toast('Could not copy — select the token text manually.')
    }
  }
  const status = browserStatus(hintAge)
  return (
    <>
      <p>Website attribution uses the optional CHRONOS browser extension. It reports <strong>only the active tab's domain</strong> — never URLs, page content, or history — to this app on your machine.</p>
      <div className="browser-choices" role="group" aria-label="Choose your browser">
        {BROWSER_CHOICES.map((b) => (
          <button key={b.id} className={`browser-choice ${b.id === browser ? 'active' : ''}`}
            aria-pressed={b.id === browser} onClick={() => setBrowser(b.id)}>{b.label}</button>
        ))}
      </div>
      {storeListing ? (
        <>
          <ol className="connect-steps">
            <li>Click <strong>Install extension</strong> — {choice.label} opens the official listing.</li>
            <li>Approve the installation in {choice.label} — CHRONOS never installs anything silently.</li>
            <li>A CHRONOS pairing tab opens by itself and pairing completes — no codes to copy. The status below turns <strong>Communicating</strong> as soon as the browser reports a website.</li>
          </ol>
          <p className="small muted" style={{ marginTop: 8 }}>Advanced local setup without the store remains available below.</p>
        </>
      ) : choice.family === 'chromium' ? (
        <ol className="connect-steps">
          <li>Click <strong>Connect {choice.label}</strong> — CHRONOS opens the extension folder for you.</li>
          <li>In {choice.label}, open <code>{choice.page}</code> and switch on <strong>Developer mode</strong>.</li>
          <li>Choose <strong>Load unpacked</strong> and select the <code>chromium</code> folder that just opened.</li>
          <li>Approve {choice.label}'s own permission prompt — CHRONOS never installs anything silently.</li>
          <li>A CHRONOS pairing tab opens by itself and pairing completes — no codes to copy. The status below turns <strong>Communicating</strong> as soon as the browser reports a website.</li>
        </ol>
      ) : (
        <ol className="connect-steps">
          <li>Click <strong>Connect {choice.label}</strong> — CHRONOS opens the extension folder for you.</li>
          <li>In {choice.label}, open <code>about:debugging</code> → <strong>This Firefox</strong> → <strong>Load Temporary Add-on…</strong> and pick <code>manifest.json</code> from the <code>firefox</code> folder that just opened.</li>
          <li>Approve the prompt. A CHRONOS pairing tab opens by itself and pairing completes — no codes to copy.</li>
          <li>A temporary add-on is removed when the browser closes; load it again next time. A permanent install needs a Mozilla-signed package — see Advanced below.</li>
        </ol>
      )}
      <div className="inset" style={{ marginTop: 14 }}>
        <span className="eyebrow">STATUS</span>
        <div className="integration-status">
          <span style={{ color: status.color, fontWeight: 550 }}>● {status.label}</span>
          <span className="small muted">{status.note}</span>
        </div>
      </div>
      <div className="row" style={{ marginTop: 12, gap: 10 }}>
        {storeListing ? (
          <Button label={`Install extension for ${choice.label}`} variant="primary" icon="download" onClick={() => { void installFromStore() }} />
        ) : null}
        <Button label={`Connect ${choice.label}`} variant={storeListing ? '' : 'primary'} icon="folder" onClick={() => { void connect() }}
          ariaLabel={storeListing ? `Advanced: local ${choice.label} setup without the store` : undefined} />
        <Button label="Open pairing page" onClick={() => { void reopenPairPage() }} ariaLabel="Open the local CHRONOS pairing page in your default browser" />
      </div>
      <details className="integration-advanced" style={{ marginTop: 14 }}>
        <summary className="small muted">Advanced — token, endpoint, and limits</summary>
        <div className="inset" style={{ marginTop: 10 }}>
          <p style={{ marginTop: 8 }}>
            Endpoint: <code>http://127.0.0.1:{pairing?.port ?? '…'}</code><br />
            Token: <code className="mono">{pairing?.token ?? '…'}</code>
            <span className="small muted"> — stored locally; stays valid across CHRONOS restarts and updates.</span>
          </p>
          <div className="row" style={{ gap: 10, marginTop: 8 }}>
            <Button label="Copy token" icon="arrow" onClick={() => { void copyToken() }} />
          </div>
          <p className="small muted" style={{ marginTop: 10 }}>
            The token pairs only with extensions on this machine. An unpacked extension's identity differs per browser
            and machine, so CHRONOS judges the connection by real signals — never by assumption. Manual fallback: open the
            extension's options page with the token in the address bar (<code>options.html#token=…</code>); it is consumed
            and removed from the URL immediately.
          </p>
        </div>
      </details>
    </>
  )
}

function IntegrationDialog({ name }: { name: string }) {
  const isAi = name.startsWith('Optional AI')
  const isCalendar = /calendar|outlook|microsoft 365/i.test(name)
  const isBrowser = /browser integration/i.test(name)
  const [pairing, setPairing] = useState<PairingInfo | null>(null)
  useEffect(() => {
    if (isBrowser) {
      void fetchPairingInfo().then(setPairing)
    }
  }, [isBrowser])
  return (
    <>
      <p className="dialog-description">{name} is optional and not enabled until you pair it. CHRONOS explains what data is used and asks for your permission before anything is connected.</p>
      <div className="inset">
        {isAi ? (
          <p>AI is disabled. No provider credentials, AI requests, or background processing are used. If configured later, insights would run only on your local activity summary — never a chat, never automatic uploads.</p>
        ) : isCalendar ? (
          <p>Connecting requires OAuth client credentials you provide (a Google or Microsoft developer registration). Without them, this integration honestly remains disconnected. CHRONOS never fakes a connection.</p>
        ) : isBrowser ? (
          <BrowserIntegrationPanel pairing={pairing} />
        ) : (
          <p>This integration is optional and currently disconnected.</p>
        )}
      </div>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Understood" variant="primary" onClick={() => actions.closeModal()} />
      </div>
    </>
  )
}

function DiagnosticsDialog() {
  const capabilities = useStore((s) => s.capabilities)
  const tracking = useStore((s) => s.appState?.tracking)
  const diagnostics = useStore((s) => s.diagnostics)
  const [d, setD] = useState(diagnostics)
  useEffect(() => { void backend.getDiagnostics().then(setD) }, [])
  const meta = tracking ? statusMeta(tracking.state) : null
  return (
    <>
      <p className="dialog-description">Live diagnostics · {d?.platform ?? '…'}.</p>
      <div className="inset">
        {meta ? <><strong style={{ color: meta.color }}>{meta.label}</strong><p style={{ marginTop: 8 }}>{meta.description}<br />Backend: {d?.trackerBackend ?? '…'}<br />Degraded reason: {tracking?.degradedReason ?? 'none'}</p></> : null}
      </div>
      {capabilities ? (
        <div style={{ overflow: 'auto', margin: '5px 0 18px' }}>
          <table className="cap-table">
            <caption className="small muted" style={{ textAlign: 'left', padding: '12px 0' }}>Capabilities · {capabilities.platform}</caption>
            <thead><tr><th>Capability</th><th>Status</th></tr></thead>
            <tbody>
              {capabilities.capabilities.map((c) => (
                <tr key={c.name}><td>{c.name}</td><td className={c.status === 'available' ? 'green' : c.status === 'unsupported' ? 'pink' : 'amber'}>
                  {c.status === 'available' ? '✓' : '◌'} {c.status === 'available' ? 'Available' : c.status === 'unsupported' ? 'Unsupported' : c.status === 'permission-required' ? 'Permission required' : c.status === 'temporarily-unavailable' ? 'Temporarily unavailable' : 'Limited'}
                </td></tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      <div className="dialog-actions" style={{ marginTop: 10 }}>
        <Button label="Close" variant="ghost" onClick={() => actions.closeModal()} />
      </div>
    </>
  )
}

function RecoveryDialog() {
  const tracking = useStore((s) => s.appState?.tracking)
  const meta = tracking ? statusMeta(tracking.state) : null
  return (
    <>
      <p className="dialog-description">Recovery uses your last explicit tracking preference. It never overrides privacy mode, a manual pause, or disabled tracking.</p>
      <div className="inset">
        <p>Current preference: <strong style={{ color: meta?.color }}>{meta?.label}</strong><br />
          Known state → preserved exactly<br />
          Unknown state → await your confirmation<br />
          Degraded state → missing capability clearly reported</p>
      </div>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Recover known state" onClick={() => { actions.closeModal(); actions.toast(`Recovery preserved your preference: ${meta?.label}.`) }} />
        <Button label="Simulate unknown state" variant="primary"
          onClick={() => {
            actions.closeModal()
            if (tracking && ['paused', 'privacy', 'disabled'].includes(tracking.state)) {
              actions.toast('Known explicit stop preference preserved. Unknown-state simulation did not override it.')
            } else {
              void withToast(backend.simulateUnknownRecovery(), 'Previous state unknown. Tracking awaits explicit confirmation.')
            }
          }} />
      </div>
    </>
  )
}

function PerformanceDialog() {
  const d = useStore((s) => s.diagnostics)
  const m = d?.measured ?? null
  useEffect(() => { void actions.loadDiagnostics() }, [])
  const value = (v: number | null | undefined, render: (n: number) => string) =>
    v == null ? (d ? 'not available on this platform' : 'measuring…') : render(v)
  return (
    <>
      <p className="dialog-description">Measured where measurement is available on this machine; targets are engineering goals, not guarantees.</p>
      <div className="inset">
        <p>
          Tracker CPU (recent average): {value(m?.trackerThreadCpuPercent, (n) => `${n.toFixed(2)}%`)}<br />
          Process memory: {value(m?.processMemoryMb, (n) => `${n.toFixed(0)} MB`)}<br />
          Uptime: {fmt((m?.uptimeSeconds ?? 0) / 60)} · Intervals: {d?.db.intervals ?? '—'} · Database: {d ? (d.db.healthy ? 'healthy' : 'check needed') : 'measuring…'}
        </p>
        <p style={{ marginTop: 10 }}>
          Engineering targets: tracker CPU ≈ ≤1% average while idle · tracker RAM ≈ ≤100 MB · whole-app RAM ≈ ≤250 MB · AI disabled = zero AI overhead.
        </p>
      </div>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Done" variant="primary" onClick={() => actions.closeModal()} />
      </div>
    </>
  )
}

/* ---------------- state-change confirmations ---------------- */

function TrackingEnableDialog() {
  const tracking = useStore((s) => s.appState?.tracking.state)
  const disabled = tracking === 'disabled'
  return (
    <>
      <p className="dialog-description">
        {disabled
          ? 'Tracking is explicitly disabled. Enabling requires your choice.'
          : 'The previous tracking state is unknown. CHRONOS stays stopped until you confirm.'}
        <br />Activity observation runs locally on this device only.
      </p>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Keep tracking off" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Enable tracking" variant="primary" onClick={() => { actions.closeModal(); void withToast(backend.enableTracking(), 'Tracking explicitly enabled.') }} />
      </div>
    </>
  )
}

function PrivacyLeaveDialog() {
  return (
    <>
      <p className="dialog-description">Privacy mode cannot be overridden by Resume. Leave privacy mode explicitly to restore your prior tracking preference.</p>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Stay private" variant="primary" onClick={() => actions.closeModal()} />
        <Button label="Leave privacy mode" onClick={() => { actions.closeModal(); void withToast(backend.setPrivacy(false), 'Privacy mode off. Your previous tracking state was restored.') }} />
      </div>
    </>
  )
}

function QuitDialog() {
  return (
    <>
      <p className="dialog-description">Quitting stops the tracker, ends any running focus session, and exits CHRONOS completely. Your data is already saved locally.</p>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        <Button label="Keep CHRONOS running" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Quit CHRONOS" danger icon="power" onClick={() => { void backend.quitApp() }} />
      </div>
    </>
  )
}

/* ---------------- V1: manual entries ---------------- */

/** Local epoch-ms for `date` (YYYY-MM-DD) + "HH:MM". */
function datePlusTime(date: string, hhmm: string): number {
  const [h, m] = hhmm.split(':').map(Number)
  const d = new Date(`${date}T00:00:00`)
  d.setHours(h || 0, m || 0, 0, 0)
  return d.getTime()
}
function minutesToHHMM(min: number): string {
  const m = ((Math.round(min) % 1440) + 1440) % 1440
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`
}

function ManualEntryDialog({ recordId }: { recordId?: number }) {
  const records = useStore((s) => s.timelineRecords) ?? []
  const projects = useStore((s) => s.appState?.projects) ?? []
  const date = useStore((s) => s.timelineDate)
  const editing = records.find((x) => x.id === recordId && x.manual)
  const [label, setLabel] = useState(editing?.app ?? '')
  const [start, setStart] = useState(editing ? minutesToHHMM(editing.start) : '09:00')
  const [end, setEnd] = useState(editing ? minutesToHHMM(editing.end) : '09:30')
  const [cat, setCat] = useState<Category>(editing?.cat ?? 'productive')
  const [project, setProject] = useState(editing?.project ?? '')
  const active = projects.filter((p) => !p.archived || p.id === project)
  return (
    <>
      <p className="dialog-description">
        Manual entries record real time CHRONOS could not observe � a walk, an offline meeting, focused reading.
        They are always marked <strong>Manual</strong> and never appear as observed activity. Overlaps with recorded
        activity or other manual entries are rejected; tracking gaps and idle time are replaced honestly.
      </p>
      <label>
        <span className="input-label">What were you doing?</span>
        <input className="field" maxLength={60} style={{ width: '100%' }} value={label} placeholder="Client walkthrough (offline)"
          onChange={(e) => setLabel(e.target.value)} />
      </label>
      <div className="form-grid">
        <label>
          <span className="input-label">Start ({date})</span>
          <input type="time" className="field" style={{ width: '100%' }} value={start} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label>
          <span className="input-label">End</span>
          <input type="time" className="field" style={{ width: '100%' }} value={end} onChange={(e) => setEnd(e.target.value)} />
        </label>
        <label>
          <span className="input-label">Category</span>
          <select className="field" style={{ width: '100%' }} value={cat} onChange={(e) => setCat(e.target.value as Category)}>
            {(['productive', 'neutral', 'distracting', 'unclassified'] as Category[]).map((c) => <option key={c} value={c}>{cats[c].label}</option>)}
          </select>
        </label>
        <label>
          <span className="input-label">Project (optional)</span>
          <select className="field" style={{ width: '100%' }} value={project} onChange={(e) => setProject(e.target.value)}>
            <option value="">No project</option>
            {active.map((p) => <option key={p.id} value={p.id}>{p.name}{p.archived ? ' (archived)' : ''}</option>)}
          </select>
        </label>
      </div>
      <div className="dialog-actions" style={{ marginTop: 20 }}>
        {editing ? (
          <Button label="Remove entry" danger icon="x"
            onClick={() => {
              actions.closeModal()
              void withToast(backend.manualEntryDelete(editing.id), 'Manual entry removed.')
            }} />
        ) : null}
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label={editing ? 'Save entry' : 'Add to timeline'} variant="primary"
          onClick={() => {
            const trimmed = label.trim()
            if (!trimmed) { actions.toast('Describe what you were doing.'); return }
            const s = datePlusTime(date, start)
            const e = datePlusTime(date, end)
            if (e <= s) { actions.toast('End time must be after start time.'); return }
            actions.closeModal()
            const op = editing
              ? backend.manualEntryUpdate(editing.id, s, e, trimmed, cat, project)
              : backend.manualEntryAdd(s, e, trimmed, cat, project)
            void withToast(op, editing ? 'Manual entry updated.' : 'Manual time added to the timeline.', )
            void op.catch((err) => actions.toast(String(err)))
          }} />
      </div>
    </>
  )
}

/* ---------------- V1: bulk activity assignment ---------------- */

function AssignActivitiesDialog({ projectId }: { projectId: string }) {
  const projects = useStore((s) => s.appState?.projects) ?? []
  const project = projects.find((p) => p.id === projectId)
  const timelineDate = useStore((s) => s.timelineDate)
  const [days, setDays] = useState(7)
  const [unassignedOnly, setUnassignedOnly] = useState(true)
  const [search, setSearch] = useState('')
  const [rows, setRows] = useState<ActivityRecord[] | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const twelve = useStore((s) => s.appState?.settings.time === '12-hour')

  useEffect(() => {
    let alive = true
    setRows(null)
    setSelected(new Set())
    ;(async () => {
      const out: ActivityRecord[] = []
      for (let i = 0; i < days; i++) {
        const d = new Date(`${timelineDate}T12:00:00`)
        d.setDate(d.getDate() - i)
        const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
        for (const r of await backend.getTimeline(iso)) {
          if (r.cat !== 'idle' && r.cat !== 'gap') out.push(r)
        }
      }
      if (alive) setRows(out)
    })()
    return () => { alive = false }
  }, [days, timelineDate])

  if (!project) return <div className="empty">Project not found.</div>
  const q = search.trim().toLowerCase()
  const visible = (rows ?? []).filter((r) =>
    (!unassignedOnly || r.project === '') &&
    (!q || r.app.toLowerCase().includes(q) || r.detail.toLowerCase().includes(q)))
  const toggle = (id: number) => {
    const next = new Set(selected)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    setSelected(next)
  }
  return (
    <>
      <p className="dialog-description">
        Select activities to assign to <strong>{project.name}</strong>. Assignments are explicit and persistent;
        the project's accumulated time and report totals update immediately. Existing per-activity corrections are preserved.
      </p>
      <div className="form-grid" style={{ marginBottom: 10 }}>
        <label>
          <span className="input-label">Period</span>
          <select className="field" style={{ width: '100%' }} value={days} onChange={(e) => setDays(Number(e.target.value))}>
            <option value={1}>Today</option>
            <option value={3}>Last 3 days</option>
            <option value={7}>Last 7 days</option>
            <option value={14}>Last 14 days</option>
          </select>
        </label>
        <label>
          <span className="input-label">Search</span>
          <input className="field" style={{ width: '100%' }} placeholder="App or site name" value={search} onChange={(e) => setSearch(e.target.value)} />
        </label>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, paddingTop: 18 }}>
          <input type="checkbox" checked={unassignedOnly} onChange={(e) => setUnassignedOnly(e.target.checked)} />
          <span className="small">Unassigned only</span>
        </label>
      </div>
      <div style={{ maxHeight: 280, overflowY: 'auto', borderTop: '1px solid var(--line)', borderBottom: '1px solid var(--line)' }}>
        {rows === null ? <div className="empty">Loading activities�</div>
          : visible.length === 0 ? <div className="empty">No matching activities in this period.</div>
          : visible.map((r) => (
            <label key={r.id} className="activity-row" style={{ cursor: 'pointer' }}>
              <input type="checkbox" checked={selected.has(r.id)} onChange={() => toggle(r.id)} />
              <AppIcon record={r} />
              <span className="activity-name"><strong>{r.app}</strong><small>{r.date} � {clock(r.start, twelve)}�{clock(r.end, twelve)} � {fmt(r.end - r.start)}</small></span>
              <span className="small muted">{r.project ? projects.find((p) => p.id === r.project)?.name ?? r.project : 'Unassigned'}</span>
            </label>
          ))}
      </div>
      <div className="dialog-actions" style={{ marginTop: 16 }}>
        <span className="small muted" style={{ marginRight: 'auto' }}>{selected.size} selected</span>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Unassign" disabled={selected.size === 0}
          onClick={() => {
            actions.closeModal()
            void withToast(backend.bulkAssign([...selected], ''), 'Activities unassigned.', )
          }} />
        <Button label={`Assign to ${project.name}`} variant="primary" disabled={selected.size === 0}
          onClick={() => {
            actions.closeModal()
            void withToast(backend.bulkAssign([...selected], project.id), `Assigned ${selected.size} activities to ${project.name}.`)
          }} />
      </div>
    </>
  )
}

/* ---------------- V1: project deletion ---------------- */

function ProjectDeleteDialog({ id }: { id: string }) {
  const projects = useStore((s) => s.appState?.projects) ?? []
  const withTime = useStore((s) => s.projectsWithTime) ?? []
  const project = projects.find((p) => p.id === id)
  const stats = withTime.find((p) => p.id === id)
  const [confirmName, setConfirmName] = useState('')
  if (!project) return <div className="empty">Project not found.</div>
  const matches = confirmName.trim().toLowerCase() === project.name.toLowerCase()
  return (
    <>
      <p className="dialog-description">
        Deleting <strong>{project.name}</strong> is permanent. Archive keeps everything and is reversible �
        deletion does not.
      </p>
      <div className="inset">
        <p style={{ margin: '0 0 6px' }}>What deletion does:</p>
        <ul style={{ margin: 0, paddingLeft: 18, lineHeight: 1.8 }}>
          <li>{stats?.intervalCount ?? 0} tracked intervals ({fmt(stats?.minutes ?? 0)}) are <strong>kept</strong> � their project reference is cleared, never reassigned.</li>
          <li>Rules carrying this project keep their classification but lose the assignment.</li>
          <li>The project disappears from assignment lists and reports; the goal falls back to your first project.</li>
        </ul>
      </div>
      <label style={{ display: 'block', marginTop: 14 }}>
        <span className="input-label">Type �{project.name}� to confirm</span>
        <input className="field" style={{ width: '100%' }} value={confirmName} onChange={(e) => setConfirmName(e.target.value)} />
      </label>
      <div className="dialog-actions" style={{ marginTop: 18 }}>
        <Button label="Keep project" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Delete permanently" danger icon="x" disabled={!matches}
          onClick={() => {
            actions.closeModal()
            void withToast(backend.deleteProject(id), 'Project deleted. Tracked intervals were preserved.', )
          }} />
      </div>
    </>
  )
}

/* ---------------- V1: block list, idle exceptions, schedule ---------------- */

function BlockListDialog() {
  const [entries, setEntries] = useState<{ id: number; label: string; kind: string; target: string; enabled: boolean }[] | null>(null)
  const [kind, setKind] = useState<'app' | 'domain'>('domain')
  const [target, setTarget] = useState('')
  const summary = useBlockingSummary()
  const refresh = () => { void backend.getBlockList().then(setEntries) }
  useEffect(refresh, [])
  return (
    <>
      <p className="dialog-description">
        Two kinds of enforcement, honestly labeled. <strong>Websites</strong> are blocked inside browsers where the
        CHRONOS extension is installed — pages redirect to a calm block page. <strong>Applications</strong> are
        discouraged on Windows by minimizing the blocked window whenever it takes the foreground (this is
        discouragement, not prevention of launch). Blocking is separate from tracking exclusions and is always
        reversible here.
      </p>
      <div className="info-strip" style={{ marginBottom: 14 }}>
        <Icon name="info" />
        <span>
          {blockingSummaryText(summary)}
          {summary.enabled && summary.sites > 0 ? <> Extension: <strong style={{ color: summary.enforced ? 'var(--green)' : summary.extensionColor }}>{summary.enforced ? 'enforcement confirmed' : summary.extension.toLowerCase()}</strong>.</> : null}
        </span>
      </div>
      {summary.enabled && summary.sites > 0 && !summary.enforced ? (
        <div className="inset" style={{ marginBottom: 14 }}>
          <p>
            No browser has confirmed these rules yet. If your browser's extensions page shows a broken or older
            CHRONOS extension (it may have been loaded from a temporary folder during an early setup), remove it
            there, then load the current one via <strong>Settings → Integrations → Connect</strong>. Browsers also
            disable extensions once after they gain permissions — re-enable it if that is shown.
          </p>
        </div>
      ) : null}
      <div className="form-grid" style={{ marginBottom: 10 }}>
        <label>
          <span className="input-label">Kind</span>
          <select className="field" style={{ width: '100%' }} value={kind} onChange={(e) => setKind(e.target.value as 'app' | 'domain')}>
            <option value="domain">Website (browser extension)</option>
            <option value="app">Application (Windows · minimize)</option>
          </select>
        </label>
        <label>
          <span className="input-label">{kind === 'domain' ? 'Website (any form — saved as its domain)' : 'Application name'}</span>
          <input className="field" style={{ width: '100%' }} value={target}
            placeholder={kind === 'domain' ? 'youtube.com or https://www.youtube.com' : 'steam'}
            onChange={(e) => setTarget(e.target.value)} />
        </label>
      </div>
      <Button label="Add to block list" variant="primary" icon="plus"
        onClick={() => {
          const t = target.trim()
          if (!t) { actions.toast(kind === 'domain' ? 'Enter a website domain.' : 'Enter an application name.'); return }
          const op = backend.blockListAdd(kind, t, '')
          void withToast(op.then(refresh), 'Added to the block list.')
          void op.catch((err) => actions.toast(String(err)))
          op.then(() => setTarget(''), () => undefined)
        }} />
      <div style={{ marginTop: 14, borderTop: '1px solid var(--line)', paddingTop: 10 }}>
        {entries === null ? <div className="empty">Loading…</div>
          : entries.length === 0 ? <div className="empty">Nothing on the block list yet. Add a website or application above.</div>
          : entries.map((b) => (
            <div key={b.id} className="activity-row">
              <span className={`dot ${b.kind === 'domain' ? 'bg-violet' : 'bg-cyan'}`} title={b.kind === 'domain' ? 'Website' : 'Application'} />
              <span className="activity-name">
                <strong>{b.target}</strong>
                <small>{b.kind === 'domain' ? 'Website · redirects in browsers with the extension' : 'Application · window minimized (Windows)'}</small>
              </span>
              <label style={{ display: 'flex', alignItems: 'center', gap: 6, marginRight: 8 }}>
                <input type="checkbox" checked={b.enabled}
                  onChange={() => void withToast(backend.blockListSetEnabled(b.id, !b.enabled).then(refresh), b.enabled ? 'Entry paused — kept but not enforced.' : 'Entry enabled.')} />
                <span className="small muted">On</span>
              </label>
              <Button label="Remove" variant="ghost" onClick={() => { void withToast(backend.blockListRemove(b.id).then(refresh), 'Removed from the block list.') }} />
            </div>
          ))}
      </div>
      <div className="inset" style={{ marginTop: 14 }}>
        <p>Exclusions (Settings → Tracking &amp; Classification) are a different, independent control: they stop recording, not access. Blocking never creates exclusions, and disabling it here never deletes your list — entries are kept and simply not enforced.</p>
      </div>
    </>
  )
}

function IdleExceptionsDialog() {
  const [entries, setEntries] = useState<{ id: number; label: string }[] | null>(null)
  const [label, setLabel] = useState('')
  const refresh = () => { void backend.getIdleExceptions().then(setEntries) }
  useEffect(refresh, [])
  return (
    <>
      <p className="dialog-description">
        Some work happens without keyboard or mouse � video meetings, presentations, lectures, media playback.
        An exempted foreground application is assumed engaged while it stays in front: its interval continues and
        no idle row is written. This is an assumption you configure, never something CHRONOS claims to observe.
      </p>
      <div className="form-grid" style={{ marginBottom: 10 }}>
        <label>
          <span className="input-label">Application (process or display name)</span>
          <input className="field" style={{ width: '100%' }} value={label} placeholder="zoom � ms-teams � vlc"
            onChange={(e) => setLabel(e.target.value)} />
        </label>
      </div>
      <Button label="Add exception" variant="primary" icon="plus"
        onClick={() => {
          const t = label.trim()
          if (!t) { actions.toast('Enter an application name.'); return }
          void withToast(backend.idleExceptionAdd(t).then(refresh), 'Idle exception added.')
          setLabel('')
        }} />
      <div style={{ marginTop: 14, borderTop: '1px solid var(--line)', paddingTop: 10 }}>
        {entries === null ? <div className="empty">Loading�</div>
          : entries.length === 0 ? <div className="empty">No exceptions. Idle detection applies to every application.</div>
          : entries.map((e) => (
            <div key={e.id} className="activity-row">
              <span className="dot bg-green" />
              <span className="activity-name"><strong>{e.label}</strong><small>Assumed active while foreground</small></span>
              <Button label="Remove" variant="ghost" onClick={() => { void withToast(backend.idleExceptionRemove(e.id).then(refresh), 'Exception removed.') }} />
            </div>
          ))}
      </div>
    </>
  )
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

function ScheduleDialog() {
  const settings = useStore((s) => s.appState?.settings)
  const [enabled, setEnabled] = useState(settings?.scheduleEnabled ?? false)
  const [start, setStart] = useState(settings?.scheduleStart || '09:00')
  const [end, setEnd] = useState(settings?.scheduleEnd || '17:30')
  const [days, setDays] = useState<string[]>(
    (settings?.scheduleDays || 'Mon,Tue,Wed,Thu,Fri').split(',').map((d) => d.trim()).filter(Boolean))
  if (!settings) return null
  const toggleDay = (d: string) => setDays((prev) => prev.includes(d) ? prev.filter((x) => x !== d) : [...prev, d])
  const overnight = start > end
  return (
    <>
      <p className="dialog-description">
        An optional observation window. Outside it, CHRONOS stops observing and writes an honest
        �Outside tracking schedule� gap � never invented activity. Manual Pause and Privacy always take
        precedence, and no history is rewritten.
      </p>
      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 12 }}>
        <Toggle checked={enabled} onChange={(v) => setEnabled(v)} label="Enable tracking schedule" />
        <span className="small">Enable tracking schedule</span>
      </div>
      <div className="form-grid">
        <label>
          <span className="input-label">Start</span>
          <input type="time" className="field" style={{ width: '100%' }} value={start} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label>
          <span className="input-label">End</span>
          <input type="time" className="field" style={{ width: '100%' }} value={end} onChange={(e) => setEnd(e.target.value)} />
        </label>
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', margin: '12px 0' }}>
        {WEEKDAYS.map((d) => (
          <button key={d} type="button"
            className={`chip ${days.includes(d) ? 'chip-on' : ''}`}
            onClick={() => toggleDay(d)}>{d}</button>
        ))}
      </div>
      <p className="small muted">
        {overnight
          ? `Overnight window: tracking runs ${start}�midnight and midnight�${end}; the late part also counts when the previous evening was selected.`
          : `Tracking runs ${start}�${end} on selected days.`}
        {' '}Unselected days record nothing but gaps labeled with the schedule.
      </p>
      <div className="dialog-actions" style={{ marginTop: 18 }}>
        <Button label="Cancel" variant="ghost" onClick={() => actions.closeModal()} />
        <Button label="Save schedule" variant="primary"
          onClick={() => {
            if (enabled && days.length === 0) { actions.toast('Select at least one day, or disable the schedule.'); return }
            actions.closeModal()
            void withToast(
              backend.updateSettings({ scheduleEnabled: enabled, scheduleStart: enabled ? start : '', scheduleEnd: enabled ? end : '', scheduleDays: days.join(',') }),
              enabled ? 'Tracking schedule saved.' : 'Tracking schedule disabled — observation is always allowed.',
            )
          }} />
      </div>
    </>
  )
}
