import { actions, useStore } from '../store'
import { backend } from '../backend'
import { Icon } from '../components/icons'
import { Button, CardHead, PageTitle, Setting, Tabs, Toggle, withToast } from '../components/ui'
import { GoalRow, QualityOrbit, WeekChart, ChartLegend, qualityPercent, sessionControls, SessionPill } from './Overview'
import { cats } from '../types'
import { dateLabel, fmt, localDateStr, timerText } from '../format'
import { useState } from 'react'
import { useBlockingSummary, blockingSummaryText } from '../components/blocking'

export function Focus() {
  const tab = useStore((s) => s.focusTab)
  return (
    <>
      <PageTitle h="Attention is your superpower." p="Create the conditions for your best work, one session at a time." />
      <Tabs items={[['sessions', 'Focus sessions'], ['goals', 'Goals'], ['projects', 'Projects'], ['distractions', 'Distraction management']]}
        active={tab} onSelect={(id) => actions.setFocusTab(id as never)} label="Focus views" />
      {tab === 'sessions' ? <SessionsTab /> : null}
      {tab === 'goals' ? <GoalsTab /> : null}
      {tab === 'projects' ? <ProjectsTab /> : null}
      {tab === 'distractions' ? <DistractionsTab /> : null}
    </>
  )
}

function SessionsTab() {
  const appState = useStore((s) => s.appState)
  const history = useStore((s) => s.focusHistory)
  if (!appState) return null
  const session = appState.session
  const quality = qualityPercent(session.qualified, session.elapsed)
  const projects = appState.projects

  return (
    <div className="two-col">
      <section className="card focus-stage">
        <div className="row" style={{ justifyContent: 'center' }}><SessionPill status={session.status} phase={session.phase} breakExpired={session.breakExpired} idlePaused={session.focusIdlePaused} /></div>
        <div className="large-timer">{timerText(session.elapsed)}</div>
        <p className="muted small">Elapsed time · {session.intention || 'Deep work'}{session.project ? ` · ${projects.find((p) => p.id === session.project)?.name ?? ''}` : ''}</p>
        <div className="session-controls">{sessionControls(session.status, session.breakExpired, session.focusIdlePaused)}</div>
        <div className="focus-measures">
          <div>Qualified focus<strong className="green">{fmt(session.qualified)}</strong></div>
          <div>Idle time<strong>{fmt(session.idle)}</strong></div>
          <div>Interruptions<strong>{session.interruptions}</strong></div>
        </div>
        <p className="session-note">Qualified focus counts observed productive activity — sustained stretches of 5+ minutes, or any productive time inside a focus session. Idle time and interruptions never qualify.</p>
      </section>
      <section className="card">
        <CardHead h="A history of showing up" right={<span className="muted">Today</span>} />
        {history && history.length ? history.map((s) => (
          <div className="history-row" key={s.id}>
            <div>{s.intention}<small>{dateLabel(localDateStr(), { month: 'short', day: 'numeric' })} · {fmt(s.elapsed)} elapsed</small></div>
            <span className="green">{fmt(s.qualified)} qualified</span>
          </div>
        )) : <div className="empty">No sessions yet today.<br /><span className="small">Sessions you complete will appear here.</span></div>}
        <div className="inset" style={{ marginTop: 24 }}>
          <span className="eyebrow">FOCUS QUALITY</span>
          <div className="quality-stage-number" style={{ width: 136, height: 136 }}>
            <QualityOrbit percent={quality} />
          </div>
          <p>Qualified focused minutes ÷ elapsed session minutes. Idle time and interruptions are never credited as focus.</p>
        </div>
        <p className="section-caption" style={{ margin: '18px 0 0' }}>Qualified focus is measured from observed productive intervals, never from the timer alone.</p>
      </section>
    </div>
  )
}

function GoalsTab() {
  const appState = useStore((s) => s.appState)
  const overview = useStore((s) => s.overview)
  if (!appState || !overview) return null
  const t = overview.totals
  const goals = appState.goals
  const weekFocused = overview.week.reduce((a, d) => a + d.totals.focused, 0)
  const weekMax = Math.max(480, ...overview.week.map((d) => d.totals.tracked))
  const goalProject = appState.projects.find((p) => p.id === 'p1') ?? appState.projects[0]

  return (
    <div className="two-col">
      <section className="card">
        <CardHead h="Your intentions" right={<Button label="Configure goals" icon="settings" onClick={() => actions.openModal({ kind: 'goals' })} />} />
        <div className="goals-stack">
          <GoalRow type="focus" label="Daily focus" value={t.focused} target={goals.daily} color="linear-gradient(90deg,#8654ee,#ba8afc)" />
          <GoalRow type="project" label={`Project time · ${goalProject?.name ?? 'Project'}`} value={overview.projectMinutesToday} target={goals.project} color="var(--cyan)" />
          <GoalRow type="limit" label="Distraction limit" value={t.distracting} target={goals.limit} color="var(--pink)" limit />
        </div>
        <div style={{ marginTop: 25, borderTop: '1px solid var(--line)', paddingTop: 5 }}>
          <GoalRow type="focus" label="Weekly focused time" value={weekFocused} target={goals.weekly} color="var(--green)" />
        </div>
        <p className="section-caption" style={{ marginBottom: 0 }}>Goals are guides, not grades. Focus targets use qualified activity, not the running timer.</p>
      </section>
      <section className="card">
        <CardHead h="A sustainable rhythm" />
        <WeekChart days={overview.week} selected={overview.date} maxMinutes={weekMax} />
        <ChartLegend />
        <div className="inset" style={{ marginTop: 25 }}>
          <p>Leave room for breaks. Your distraction limit is optional and never blocks an application unless you explicitly enable blocking controls.</p>
        </div>
      </section>
    </div>
  )
}

function ProjectsTab() {
  const projects = useStore((s) => s.projectsWithTime)
  const appState = useStore((s) => s.appState)
  if (!appState) return null
  return (
    <>
      <div className="row between" style={{ marginBottom: 20 }}>
        <p className="muted small">Personal projects, naturally connected to your time.</p>
        <Button label="Create project" variant="primary" icon="plus" onClick={() => actions.openModal({ kind: 'project' })} />
      </div>
      <div className="project-grid">
        {(projects ?? []).map((p) => (
          <section className="card project-card" key={p.id}>
            <div className="row between">
              <span style={{ color: p.color }}><Icon name="folder" /></span>
              <span className={`pill ${p.archived ? 'gray' : 'purple'}`}>{p.archived ? 'Archived' : 'Active'}</span>
            </div>
            <h2>{p.name}</h2>
            <p className="small muted" style={{ marginTop: -6, minHeight: 18, visibility: p.client ? 'visible' : 'hidden' }}>Client · {p.client || '—'}</p>
            <p>{p.desc}</p>
            <div className="project-hours">{fmt(p.minutes)}</div>
            <p>{p.intervalCount} assigned intervals</p>
            <div className="project-actions">
              <Button label="Rename" onClick={() => actions.openModal({ kind: 'project', projectId: p.id })} />
              <Button label="Assign activities" variant="primary" onClick={() => actions.openModal({ kind: 'assign-activities', projectId: p.id })} />
              <Button label={p.archived ? 'Unarchive' : 'Archive'}
                onClick={() => withToast(backend.setProjectArchived(p.id, !p.archived), `${p.name} ${!p.archived ? 'archived. Historical assignments are preserved.' : 'is active again.'}`)} />
              <Button label="Delete…" onClick={() => actions.openModal({ kind: 'project-delete', projectId: p.id })} />
            </div>
          </section>
        ))}
      </div>
    </>
  )
}

function DistractionsTab() {
  const appState = useStore((s) => s.appState)
  const overview = useStore((s) => s.overview)
  const blocking = useBlockingSummary()
  if (!appState || !overview) return null
  const s = appState.settings
  const t = overview.totals
  const [dismissedLimit, setDismissedLimit] = useState(false)
  const overLimit = s.blocking === false && appState.goals.limit > 0 && t.distracting >= appState.goals.limit && !dismissedLimit

  return (
    <div className="two-col">
      <section className="card">
        <CardHead h="Protect your attention" />
        {overLimit ? (
          <div className="info-strip" style={{ marginBottom: 16 }}>
            <Icon name="info" />
            <span>Daily distraction limit reached — {fmt(t.distracting)} of {fmt(appState.goals.limit)}. A little awareness goes a long way.</span>
            <button className="text-btn" onClick={() => setDismissedLimit(true)}>Dismiss</button>
          </div>
        ) : null}
        <Setting h="Distraction blocking" p={blockingSummaryText(blocking)}
          control={<div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <Button label="Block list" onClick={() => actions.openModal({ kind: 'blocklist' })} />
            <Toggle checked={s.blocking} label="Distraction blocking"
              onChange={(v) => withToast(backend.updateSettings({ blocking: v }), v ? 'Blocking enabled — the block list is now enforced.' : 'Blocking off. Saved targets stay configured.')} />
          </div>} />
        <Setting h="Break reminders" p={`A gentle invitation to step away every ${s.breakEveryMinutes} minutes.`}
          control={<Toggle checked={s.breaks} label="Break reminders" onChange={(v) => void backend.updateSettings({ breaks: v })} />} />
        <Setting h="Do Not Disturb" p="Suppress nonessential notifications."
          control={<Toggle checked={appState.dnd} label="Do Not Disturb" onChange={(v) => withToast(backend.setDnd(v), `Do Not Disturb ${v ? 'on' : 'off'}.`)} />} />
        <Setting h="Quiet hours" p={`Keep the evening quiet, ${String(s.quietStart).padStart(2, '0')}:00–${String(s.quietEnd).padStart(2, '0')}:00.`}
          control={<Toggle checked={s.quiet} label="Quiet hours" onChange={(v) => void backend.updateSettings({ quiet: v })} />} />
        <p className="section-caption" style={{ marginBottom: 0 }}>
          Blocking is separate from tracking exclusions and always reversible from the block list.
          Idle time and tracking gaps stay distinct from breaks; reminders respect quiet hours and Do Not Disturb.
        </p>
      </section>
      <section className="card">
        <CardHead h="A quieter kind of nudge" />
        <p className="section-caption">Take a breath. Look away from the screen. Your work will still be here.</p>
        <Button label="Preview a break reminder" icon="bell" onClick={() => actions.openModal({ kind: 'notifications' })} />
        <div className="inset" style={{ marginTop: 22 }}>
          <span className="eyebrow">QUIET MODE</span>
          <p style={{ marginTop: 8 }}>Do Not Disturb and quiet hours suppress break and completion notices. Significant tracking degradation remains visible.</p>
        </div>
        <div className="inset" style={{ marginTop: 16 }}>
          <span className="eyebrow">TODAY</span>
          <div className="row between" style={{ marginTop: 8 }}><span className="muted small">Distracting time</span><span style={{ color: cats.distracting.color }}>{fmt(t.distracting)}</span></div>
          {appState.goals.limit > 0 ? (
            <div className="bar" style={{ marginTop: 8 }}><span style={{ width: `${Math.min(100, (t.distracting / appState.goals.limit) * 100)}%`, background: 'var(--pink)' }} /></div>
          ) : null}
        </div>
      </section>
    </div>
  )
}
