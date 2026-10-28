import { useEffect, useState } from 'react'
import { actions, useStore, currentReportRequest } from '../store'
import { backend } from '../backend'
import { Icon, MetricGlyph } from '../components/icons'
import { Button, PageTitle, Tabs } from '../components/ui'
import { cats } from '../types'
import { dateLabel, fmt } from '../format'
import { GoalRow, ChartLegend, WeekChart } from './Overview'
import type { Totals } from '../types'

const categoryColors = ['var(--violet)', 'var(--cyan)', 'var(--green)', 'var(--pink)', 'var(--idle)']

export function Reports() {
  const report = useStore((s) => s.report)
  const tab = useStore((s) => s.reportTab)
  const appState = useStore((s) => s.appState)
  const period = useStore((s) => s.reportPeriod)
  const project = useStore((s) => s.reportProject)
  const start = useStore((s) => s.reportStart)
  const end = useStore((s) => s.reportEnd)
  if (!report || !appState) return null

  const { totals: t, prior, breakdowns, daily, range, goalProject: reportGoal } = report
  const rangeDesc = `${dateLabel(range.start, { month: 'short', day: 'numeric' })} – ${dateLabel(range.end, { month: 'short', day: 'numeric', year: 'numeric' })}`
  const projects = appState.projects
  const goals = appState.goals
  // Goal-project progress comes from persisted assignments over the period —
  // the same selection logic the Overview uses (user-selected, validated, fallback).
  const goalProject = reportGoal
    ? { name: reportGoal.name, minutes: reportGoal.minutes, target: reportGoal.targetMinutes }
    : null

  return (
    <>
      <PageTitle h="See the bigger picture." p="Useful patterns. Clearer choices. A more intentional week."
        action={<Button label="Export" icon="download" onClick={() => actions.openModal({ kind: 'export' })} />} />
      <div className="toolbar">
        <select className="field" aria-label="Reporting period" value={period}
          onChange={(e) => actions.setReportPeriod(e.target.value as never)}>
          <option value="day">Daily</option>
          <option value="week">Weekly</option>
          <option value="month">Monthly</option>
          <option value="custom">Custom range</option>
        </select>
        {period === 'custom' ? (
          <>
            <input className="field" type="date" aria-label="Report start date" value={start}
              onChange={(e) => { const ns = e.target.value; if (ns && end && ns <= end) actions.setReportRange(ns, end) }} />
            <span className="muted small">to</span>
            <input className="field" type="date" aria-label="Report end date" value={end}
              onChange={(e) => { const ne = e.target.value; if (ne && start && start <= ne) actions.setReportRange(start, ne) }} />
          </>
        ) : <span className="muted small">{rangeDesc}</span>}
        <span className="spacer" />
        <select className="field" aria-label="Report project" value={project}
          onChange={(e) => actions.setReportProject(e.target.value)}>
          <option value="all">All projects</option>
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
      </div>
      {prior ? (
        <p className="muted small" style={{ margin: '-5px 0 20px' }}>Comparisons use the immediately preceding equal-length period.</p>
      ) : (
        <div className="info-strip"><Icon name="info" />Previous-period comparison is unavailable: there are no matching intervals in the preceding period.</div>
      )}
      <Tabs items={[['summary', 'Summary'], ['applications', 'Applications'], ['websites', 'Websites'], ['categories', 'Categories'], ['projects', 'Projects'], ['trends', 'Focus & Daily Detail']]}
        active={tab} onSelect={(id) => actions.setReportTab(id as never)} label="Report views" />

      {tab === 'summary' ? (
        <>
          <MetricsWithComparison t={t} prior={prior} />
          <div className="two-col">
            <section className="card">
              <div className="card-head"><h2>Time, thoughtfully classified</h2><span className="pill purple">{t.coverage}% coverage</span></div>
              <div className="row" style={{ gap: 28 }}>
                <div><div className="report-number green">{t.productivity}%</div><p className="muted small">productive share</p></div>
                <p className="muted small">
                  Of {fmt(t.classified)} classified active time.<br />
                  {fmt(t.unclassified)} unclassified time is excluded<br />from the productivity denominator.
                </p>
              </div>
              <div className="report-extras">
                <div>Focused<strong>{fmt(t.focused)}</strong></div>
                <div>Idle<strong>{fmt(t.idle)}</strong></div>
                <div>Unclassified<strong>{fmt(t.unclassified)}</strong></div>
                <div>Neutral<strong>{fmt(t.neutral)}</strong></div>
              </div>
            </section>
            <section className="card">
              <div className="card-head"><h2>A rhythm, not a race</h2><span className="muted">{rangeDesc} · {project === 'all' ? 'all projects' : projects.find((p) => p.id === project)?.name}</span></div>
              <WeekChart days={daily} maxMinutes={Math.max(480, ...daily.map((d) => d.totals.tracked))} />
              <ChartLegend />
              <p className="small muted" style={{ marginTop: 16 }}>Unobserved gaps are excluded. An empty day means no tracked activity.</p>
            </section>
            <section className="card">
              <div className="card-head"><h2>Where your time goes</h2>
                <button className="text-btn" onClick={() => actions.setReportTab('applications')}>Explore applications <Icon name="arrow" /></button>
              </div>
              <Breakdown rows={breakdowns.categories} type="categories" />
            </section>
            <section className="card">
              <div className="card-head"><h2>Intentions, in motion</h2>
                <button className="text-btn" onClick={() => actions.openModal({ kind: 'goals' })}><Icon name="settings" /> Edit goals</button>
              </div>
              <div className="goals-stack">
                <GoalRow type="focus" label="Daily focus" value={t.focused} target={goals.daily} color="linear-gradient(90deg,#8654ee,#ba8afc)" />
                <GoalRow type="project" label={`Project time · ${goalProject?.name ?? 'Project'}`} value={goalProject?.minutes ?? 0} target={goalProject?.target ?? goals.project} color="var(--cyan)" />
              </div>
              <p className="muted small" style={{ marginTop: 22 }}>Range goals · computed over the selected reporting period{goalProject ? ` · ${goalProject.name} minutes from persisted assignments` : ''}</p>
            </section>
          </div>
        </>
      ) : null}

      {['applications', 'websites', 'categories', 'projects'].includes(tab) ? (
        <section className="card">
          <div className="card-head">
            <h2>{({ applications: 'Applications & activities', websites: 'Website time', categories: 'Category distribution', projects: 'Time by client & project' } as Record<string, string>)[tab]}</h2>
            <span className="muted">{rangeDesc}</span>
          </div>
          {tab === 'projects' ? <ClientProjectBreakdown rows={breakdowns.projects} /> : <Breakdown rows={breakdowns[tab as 'applications' | 'websites' | 'categories']} type={tab as 'applications' | 'websites' | 'categories'} />}
          <p className="section-caption" style={{ margin: '25px 0 0' }}>
            {tab === 'websites'
              ? 'Only intervals with real website attribution from the optional browser integration are included. No browsing information is inferred or collected without it.'
              : tab === 'projects'
                ? 'Client totals aggregate their projects. Idle time and tracking gaps are excluded; attribution is independent of productivity.'
                : 'Totals use observed intervals. Tracking gaps are excluded.'}
          </p>
        </section>
      ) : null}

      {tab === 'trends' ? <Trends daily={daily} /> : null}
    </>
  )
}

/** Projects grouped by client with client totals (Priority Three reporting). */
function ClientProjectBreakdown({ rows }: { rows: { label: string; minutes: number; client?: string; projectId?: string }[] }) {
  if (!rows.length) return <div className="empty">No matching activity in this period.</div>
  const groups = new Map<string, { total: number; projects: { label: string; minutes: number }[] }>()
  for (const r of rows) {
    const key = r.client || 'Personal & unassigned'
    const g = groups.get(key) ?? { total: 0, projects: [] }
    g.total += r.minutes
    g.projects.push({ label: r.label, minutes: r.minutes })
    groups.set(key, g)
  }
  const ordered = [...groups.entries()].sort((a, b) => b[1].total - a[1].total)
  const max = Math.max(1, ...rows.map((r) => r.minutes))
  return (
    <div className="breakdown">
      {ordered.map(([client, g]) => (
        <div key={client} style={{ marginBottom: 14 }}>
          <div className="row between" style={{ marginBottom: 6 }}>
            <span className="eyebrow">{client.toUpperCase()}</span>
            <span className="mono" style={{ textAlign: 'right' }}>{fmt(g.total)}</span>
          </div>
          {g.projects.map((p) => (
            <div className="breakdown-row" key={p.label}>
              <span>{p.label}</span>
              <div className="bar"><span style={{ width: `${(p.minutes / max) * 100}%`, background: 'var(--violet)' }} /></div>
              <span className="mono" style={{ textAlign: 'right' }}>{fmt(p.minutes)}</span>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function MetricsWithComparison({ t, prior }: { t: Totals; prior: Totals | null }) {
  const rows: [string, number, string, string, string][] = [
    ['Tracked time', t.tracked, 'clock', 'violet', 'Observed intervals only'],
    ['Active time', t.active, 'activity', 'cyan', 'Tracked, excluding idle'],
    ['Productive time', t.productive, 'leaf', 'green', `${t.productivity}% of classified active time`],
    ['Distracting time', t.distracting, 'distraction', 'pink', 'A little awareness goes a long way'],
  ]
  return (
    <div className="metrics">
      {rows.map(([l, v, _i, c, n], index) => {
        const key = (['tracked', 'active', 'productive', 'distracting'] as const)[index]
        const prev = prior ? prior[key] : null
        const diff = prev ? Math.round(((v - prev) / prev) * 100) : null
        return (
          <section className="card metric" key={l} title={n}>
            <div className="metric-top"><span className={c}><MetricGlyph index={index} /></span>{l}</div>
            <div className="metric-value">{fmt(v)}</div>
            <div className="metric-bottom">
              {prior ? (
                <span className={diff === null ? 'muted' : index === 3 ? (diff <= 0 ? 'green' : 'pink') : diff >= 0 ? 'green' : 'muted'}>
                  {diff === null ? '—' : `${diff > 0 ? '+' : ''}${diff}%`} vs previous period
                </span>
              ) : n}
            </div>
          </section>
        )
      })}
    </div>
  )
}

function Breakdown({ rows, type }: { rows: { label: string; minutes: number }[]; type: 'applications' | 'websites' | 'categories' | 'projects' }) {
  if (!rows.length) return <div className="empty">No matching activity in this period.</div>
  const max = Math.max(1, ...rows.map((r) => r.minutes))
  return (
    <div className="breakdown">
      {rows.map((row, i) => {
        const k = row.label
        const v = row.minutes
        let color: string | null = null
        if (type === 'categories') {
          color = Object.entries(cats).find(([key, c]) => key === k || c.label === k)?.[1]?.color ?? null
        }
        if (!color) color = categoryColors[i % 5]
        return (
          <div className="breakdown-row" key={k}>
            <span>{k}</span>
            <div className="bar"><span style={{ width: `${(v / max) * 100}%`, background: color }} /></div>
            <span className="mono" style={{ textAlign: 'right' }}>{fmt(v)}</span>
          </div>
        )
      })}
    </div>
  )
}

function Trends({ daily }: { daily: { date: string; totals: Totals }[] }) {
  if (!daily.length) return <section className="card"><div className="empty">No focus data in this range.</div></section>
  const values = daily.map((d) => d.totals.focused)
  const max = Math.max(60, ...values)
  const points = values.map((v, i) => `${20 + (i / Math.max(1, values.length - 1)) * 660},${120 - (v / max) * 95}`).join(' ')
  return (
    <section className="card">
      <div className="card-head"><h2>The shape of focused work</h2><span className="muted">Qualified activity, never session duration</span></div>
      <svg viewBox="0 0 700 150" className="trend-svg" role="img" aria-label="Daily qualified focus trend">
        <path d="M20 120H680M20 70H680M20 20H680" fill="none" stroke="#283149" strokeDasharray="3 5" />
        <polyline points={points} fill="none" stroke="#aa85ff" strokeWidth="2.5" />
        {values.map((v, i) => (
          <circle key={i} cx={20 + (i / Math.max(1, values.length - 1)) * 660} cy={120 - (v / max) * 95} r="3.5" fill="#cba6ff" />
        ))}
      </svg>
      <div className="report-day-row heading"><span>Day</span><span>Tracked</span><span>Focused</span><span>Productive</span><span>Coverage</span></div>
      {daily.map((d) => (
        <div className="report-day-row" key={d.date}>
          <span>{dateLabel(d.date, { month: 'short', day: 'numeric' })}</span>
          <span>{fmt(d.totals.tracked)}</span>
          <span className="violet">{fmt(d.totals.focused)}</span>
          <span>{fmt(d.totals.productive)}</span>
          <span>{d.totals.coverage}%</span>
        </div>
      ))}
      <WorkBlocksCard dates={daily.map((d) => d.date)} />
    </section>
  )
}

/** Work-session summary for the period: read-time grouping, never double-counted. */
function WorkBlocksCard({ dates }: { dates: string[] }) {
  const [summary, setSummary] = useState<{ count: number; minutes: number; switches: number; longest: number } | null>(null)
  useEffect(() => {
    let alive = true
    ;(async () => {
      let count = 0
      let minutes = 0
      let switches = 0
      let longest = 0
      for (const date of dates.slice(0, 31)) {
        try {
          const { blocks } = await backend.getWorkBlocks(date)
          count += blocks.length
          minutes += blocks.reduce((a, b) => a + b.minutes, 0)
          switches += blocks.reduce((a, b) => a + b.switches, 0)
          longest = Math.max(longest, ...blocks.map((b) => b.minutes), 0)
        } catch { /* keep partial data */ }
      }
      if (alive) setSummary({ count, minutes, switches, longest })
    })()
    return () => { alive = false }
  }, [dates.join(',')])
  return (
    <div className="inset" style={{ marginTop: 24 }}>
      <span className="eyebrow">WORK SESSIONS</span>
      {summary === null ? <p style={{ marginTop: 8 }} className="muted small">Computing Work Sessions…</p> : summary.count === 0 ? (
        <p style={{ marginTop: 8 }} className="muted small">No contiguous Work Sessions in this period. Sessions chain activity separated by short breaks (≤ 7 minutes) and split on project changes.</p>
      ) : (
        <>
          <div className="row" style={{ gap: 28, marginTop: 8 }}>
            <div><div className="report-number" style={{ fontSize: 22 }}>{summary.count}</div><p className="muted small">sessions</p></div>
            <div><div className="report-number" style={{ fontSize: 22 }}>{fmt(summary.longest)}</div><p className="muted small">longest session</p></div>
            <div><div className="report-number" style={{ fontSize: 22 }}>{fmt(summary.count ? summary.minutes / summary.count : 0)}</div><p className="muted small">average session</p></div>
            <div><div className="report-number" style={{ fontSize: 22 }}>{summary.switches}</div><p className="muted small">context switches</p></div>
          </div>
          <p className="muted small" style={{ marginTop: 10 }}>
            Work Sessions summarize observed continuity — contiguous activity with breaks under seven minutes, split when the
            project changes. They are computed from your intervals and are not the same as qualified focus or as manually
            started Focus Sessions.
          </p>
        </>
      )}
    </div>
  )
}

export function currentRequest() { return currentReportRequest() }
