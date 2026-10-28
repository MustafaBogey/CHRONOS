import { actions, useStore } from '../store'
import { backend } from '../backend'
import { Icon, AppIcon } from '../components/icons'
import { Button, PageTitle } from '../components/ui'
import { cats, type ActivityRecord } from '../types'
import { clock, dateLabel, fmt } from '../format'
import type { CurrentActivity } from '../backend'

export function Timeline() {
  const records = useStore((s) => s.timelineRecords)
  const appState = useStore((s) => s.appState)
  const date = useStore((s) => s.timelineDate)
  const filter = useStore((s) => s.timelineFilter)
  const search = useStore((s) => s.timelineSearch)
  const view = useStore((s) => s.timelineView)
  const expanded = useStore((s) => s.timelineExpanded)
  const liveSnap = useStore((s) => s.currentActivity)
  const nowMs = useStore((s) => s.now)

  if (!records || !appState) return null
  const twelve = appState.settings.time === '12-hour'
  const projects = appState.projects
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? 'No project'

  const matches = (r: ActivityRecord) =>
    (filter === 'all' || r.cat === filter || r.project === filter) &&
    `${r.app} ${r.detail} ${projectName(r.project)}`.toLowerCase().includes(search.toLowerCase())
  const filtered = records.filter(matches)
  const totals = computeTotals(records)
  const unclassifiedCount = records.filter((r) => r.cat === 'unclassified').length
  // Live data only ever applies to today's date — never invent ongoing
  // activity on a historical date.
  const live = date === todayStr() && liveSnap && liveSnap.tracking === 'active' ? liveSnap : null

  return (
    <>
      <PageTitle h="Your day, in perspective." p="A truthful record of your time. Refine it when context matters." />
      <div className="toolbar">
        <button className="icon-btn" onClick={() => actions.shiftTimelineDay(-1)} aria-label="Previous day"><Icon name="left" /></button>
        <input className="field" type="date" aria-label="Timeline date" value={date}
          onChange={(e) => { if (e.target.value) actions.setTimelineDate(e.target.value) }} />
        <button className="icon-btn" onClick={() => actions.shiftTimelineDay(1)} aria-label="Next day"><Icon name="chevron" /></button>
        <Button label="Today" variant="ghost" onClick={() => actions.setTimelineDate(todayStr())} />
        <span className="spacer" />
        <SearchInput />
        <select className="field" aria-label="Filter activities" value={filter} onChange={(e) => actions.setTimelineFilter(e.target.value)}>
          <option value="all">All activities</option>
          {Object.entries(cats).map(([k, v]) => <option key={k} value={k}>{v.label}</option>)}
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <button className={`btn ${view === 'total' ? 'primary' : ''}`} aria-pressed={view === 'total'} onClick={() => actions.setTimelineView('total')}>Total Activity</button>
        <button className={`btn ${view === 'daily' ? 'primary' : ''}`} aria-pressed={view === 'daily'} onClick={() => actions.setTimelineView('daily')}>Daily Timeline</button>
        <button className="btn" onClick={() => actions.openModal({ kind: 'review' })}>
          <Icon name="spark" />Review · {unclassifiedCount}
        </button>
        <Button label="Add time" variant="ghost" icon="plus" onClick={() => actions.openModal({ kind: 'manual-entry' })} />
      </div>
      <div className="info-strip">
        <Icon name="info" />
        <span>
          <strong>{fmt(totals.tracked)} tracked</strong> · {fmt(totals.active)} active · {fmt(totals.idle)} idle · {fmt(totals.gap)} tracking gaps.
          {view === 'total'
            ? ' Applications are aggregated for the selected date; expand one to see its individual activities.'
            : ' Newest activity first.'}
          {' '}Gaps are not observed activity.
        </span>
      </div>
      <section className="card timeline-list">
        {records.length || live ? (
          view === 'total' ? (
            <TotalActivityView filtered={filtered} filter={filter} expanded={expanded} twelve={twelve}
              projectName={projectName} live={live} nowMs={nowMs} />
          ) : (
            <DailyTimelineView filtered={filtered} twelve={twelve} projectName={projectName}
              live={live} nowMs={nowMs} />
          )
        ) : (
          <EmptyTimeline tracking={appState.tracking.state} />
        )}
      </section>
      <div className="row between" style={{ marginTop: 17 }}>
        <span className="muted small">
          {filtered.length} authoritative intervals · {dateLabel(date, { month: 'short', day: 'numeric' })}
        </span>
        <button className="text-btn" disabled={appState.undoDepth === 0}
          onClick={() => void backend.undoLast().then((ok) => actions.toast(ok ? 'Correction undone. Dependent totals restored.' : 'No correction to undo.'))}>
          <Icon name="undo" /> Undo last correction
        </button>
      </div>
    </>
  )
}

/* ---------------- Total Activity: one entry per application ---------------- */

interface AppAggregate {
  app: string
  minutes: number
  intervals: ActivityRecord[]
  /** Single category when every interval agrees; null when mixed. */
  cat: keyof typeof cats | null
  /** Honest project summary: the one project, "Multiple projects", or "No project". */
  projectLabel: string
}

function aggregateByApp(rows: ActivityRecord[], projectName: (id: string) => string): AppAggregate[] {
  const byApp = new Map<string, AppAggregate & { projectIds: Set<string> }>()
  for (const r of rows) {
    if (r.cat === 'idle' || r.cat === 'gap') continue // represented separately, never credited to an app
    const a = byApp.get(r.app) ?? { app: r.app, minutes: 0, intervals: [], cat: r.cat, projectLabel: 'No project', projectIds: new Set<string>() }
    a.minutes += r.end - r.start
    a.intervals.push(r)
    if (a.cat !== null && a.cat !== r.cat) a.cat = null // mixed classification: no single label
    if (r.project) a.projectIds.add(r.project)
    byApp.set(r.app, a)
  }
  return [...byApp.values()]
    .map(({ projectIds, ...a }) => ({
      ...a,
      projectLabel: projectIds.size === 1 ? projectName([...projectIds][0]) : projectIds.size > 1 ? 'Multiple projects' : 'No project',
    }))
    .sort((a, b) => b.minutes - a.minutes)
}

function TotalActivityView({ filtered, filter, expanded, twelve, projectName, live, nowMs }: {
  filtered: ActivityRecord[]
  filter: string
  expanded: Record<string, boolean>
  twelve: boolean
  projectName: (id: string) => string
  live: CurrentActivity | null
  nowMs: number
}) {
  const apps = aggregateByApp(filtered, projectName)
  const idleRows = filtered.filter((r) => r.cat === 'idle')
  const gapRows = filtered.filter((r) => r.cat === 'gap')
  const idleMinutes = idleRows.reduce((a, r) => a + (r.end - r.start), 0)
  const gapMinutes = gapRows.reduce((a, r) => a + (r.end - r.start), 0)

  // The live activity attaches to its application's entry (matched by app
  // identity) and is shown as a separate, clearly marked ongoing amount —
  // never merged into the completed total.
  const liveActivity = live?.activity ?? null
  const liveMinutes = liveActivity ? Math.max(0, (nowMs - liveActivity.startMs) / 60000) : 0
  const liveCatOk = filter === 'all' || (liveActivity ? filter === liveActivity.cat : false)
  const liveBelongsToApp = (app: string) => liveActivity?.app === app && liveCatOk

  if (!apps.length && !idleRows.length && !gapRows.length && !(liveActivity && liveCatOk)) {
    return <div className="empty">No activity matches this view.<br /><span className="small">Try another date or clear your filters.</span></div>
  }

  return (
    <div>
      {apps.map((a) => {
        const isOpen = expanded[a.app] === true
        const meta = a.cat ? cats[a.cat] : null
        return (
          <div key={a.app} className="app-aggregate">
            <button className="app-aggregate-row" aria-expanded={isOpen} onClick={() => actions.toggleAppExpanded(a.app)}
              aria-label={`${a.app}, ${fmt(a.minutes)} total, ${a.intervals.length} intervals`}>
              <AppIcon record={a.intervals[0]} />
              <span className="activity-name">
                <strong>{a.app}</strong>
                <small>
                  {meta
                    ? <><i className="dot" style={{ color: meta.color }} /><span style={{ color: meta.color }}>{meta.label}</span>{' · '}</>
                    : <><i className="dot" style={{ color: 'var(--muted)' }} /><span className="muted">Mixed</span>{' · '}</>}
                  {a.intervals.length} interval{a.intervals.length === 1 ? '' : 's'} · {a.projectLabel}
                </small>
              </span>
              <span className="app-aggregate-time">
                <strong className="mono">{fmt(a.minutes)}</strong>
                {liveBelongsToApp(a.app) ? <span className="live-addition mono">+ {fmt(liveMinutes)} now</span> : null}
              </span>
              <Icon name={isOpen ? 'down' : 'chevron'} />
            </button>
            {isOpen ? (
              <div className="app-aggregate-detail">
                {liveBelongsToApp(a.app) && live ? <LiveRow live={live} nowMs={nowMs} twelve={twelve} /> : null}
                {[...a.intervals].reverse().map((r) => (
                  <TimelineRow key={r.id} r={r} twelve={twelve} projectName={projectName(r.project)} manual={r.manual === true} />
                ))}
              </div>
            ) : null}
          </div>
        )
      })}
      {liveActivity && liveCatOk && !apps.some((a) => a.app === liveActivity.app) ? (
        <div className="app-aggregate">
          <button className="app-aggregate-row" aria-expanded={expanded[`live:${liveActivity.app}`] === true}
            onClick={() => actions.toggleAppExpanded(`live:${liveActivity.app}`)}
            aria-label={`${liveActivity.app}, ongoing`}>
            <span className="live-dot" />
            <span className="activity-name">
              <strong>{liveActivity.app}</strong>
              <small>Currently active — no completed intervals match this view yet</small>
            </span>
            <span className="app-aggregate-time"><strong className="mono">{fmt(liveMinutes)}</strong><span className="live-addition mono">ongoing</span></span>
            <Icon name={expanded[`live:${liveActivity.app}`] === true ? 'down' : 'chevron'} />
          </button>
          {expanded[`live:${liveActivity.app}`] === true && live ? (
            <div className="app-aggregate-detail"><LiveRow live={live} nowMs={nowMs} twelve={twelve} /></div>
          ) : null}
        </div>
      ) : null}
      {idleRows.length ? (
        <div className="app-aggregate">
          <button className="app-aggregate-row" aria-expanded={expanded['@idle'] === true} onClick={() => actions.toggleAppExpanded('@idle')}
            aria-label={`Idle, ${fmt(idleMinutes)} total`}>
            <span className="dot" style={{ color: cats.idle.color }} />
            <span className="activity-name"><strong>Idle</strong><small>{idleRows.length} period{idleRows.length === 1 ? '' : 's'} away from keyboard — not credited to any application</small></span>
            <span className="app-aggregate-time"><strong className="mono">{fmt(idleMinutes)}</strong></span>
            <Icon name={expanded['@idle'] === true ? 'down' : 'chevron'} />
          </button>
          {expanded['@idle'] === true ? (
            <div className="app-aggregate-detail">
              {[...idleRows].reverse().map((r) => <TimelineRow key={r.id} r={r} twelve={twelve} projectName="—" />)}
            </div>
          ) : null}
        </div>
      ) : null}
      {gapRows.length ? (
        <div className="app-aggregate">
          <button className="app-aggregate-row" aria-expanded={expanded['@gap'] === true} onClick={() => actions.toggleAppExpanded('@gap')}
            aria-label={`Tracking gaps, ${fmt(gapMinutes)} total`}>
            <span className="dot" style={{ color: 'var(--muted)' }} />
            <span className="activity-name"><strong>Tracking gaps</strong><small>{gapRows.length} unobserved period{gapRows.length === 1 ? '' : 's'} — never invented as activity</small></span>
            <span className="app-aggregate-time"><strong className="mono">{fmt(gapMinutes)}</strong></span>
            <Icon name={expanded['@gap'] === true ? 'down' : 'chevron'} />
          </button>
          {expanded['@gap'] === true ? (
            <div className="app-aggregate-detail">
              {[...gapRows].reverse().map((r) => <TimelineRow key={r.id} r={r} twelve={twelve} projectName="—" />)}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

/* ---------------- Daily Timeline: chronological, newest first ---------------- */

function DailyTimelineView({ filtered, twelve, projectName, live, nowMs }: {
  filtered: ActivityRecord[]
  twelve: boolean
  projectName: (id: string) => string
  live: CurrentActivity | null
  nowMs: number
}) {
  const newestFirst = [...filtered].reverse()
  if (!newestFirst.length && !live?.activity) {
    return <div className="empty">No activity matches this view.<br /><span className="small">Try another date or clear your filters.</span></div>
  }
  return (
    <div>
      {live?.activity ? <LiveRow live={live} nowMs={nowMs} twelve={twelve} /> : null}
      {newestFirst.map((r) => (
        <TimelineRow key={r.id} r={r} twelve={twelve} projectName={projectName(r.project)} manual={r.manual === true} />
      ))}
    </div>
  )
}

/** The ongoing interval, clearly marked as not yet a finalized record. */
function LiveRow({ live, nowMs, twelve }: { live: CurrentActivity; nowMs: number; twelve: boolean }) {
  const a = live.activity!
  const catKey = (['productive', 'distracting', 'neutral', 'unclassified', 'idle', 'gap'] as const).includes(a.cat as never)
    ? (a.cat as keyof typeof cats) : 'unclassified'
  const meta = cats[catKey]
  const elapsedMin = Math.max(0, (nowMs - a.startMs) / 60000)
  const midnight = new Date(a.startMs); midnight.setHours(0, 0, 0, 0)
  const startMin = (a.startMs - midnight.getTime()) / 60000
  return (
    <div className="timeline-item live-row" title="This activity is ongoing — not yet a finalized record">
      <span className="time">{clock(startMin, twelve)} – now</span>
      <span className={a.isSite ? 'app-icon web' : 'app-icon'} aria-hidden="true">{a.isSite ? '◈' : '▶'}</span>
      <span className="activity-name">
        <strong>{a.isSite ? a.name : a.app}</strong>
        <small><i className="dot" style={{ color: meta.color }} /><span style={{ color: meta.color }}>{meta.label}</span><span className="muted"> · {a.isSite ? a.app : 'ongoing'}</span></small>
      </span>
      <span className="category" style={{ color: meta.color }}><span className="dot" />{meta.label}</span>
      <span className="project-label live-addition">Ongoing</span>
      <span className="text-btn mono live-elapsed">{fmt(elapsedMin)}</span>
    </div>
  )
}

/* ---------------- shared row (finalized intervals) ---------------- */

function SearchInput() {
  const search = useStore((s) => s.timelineSearch)
  return (
    <input type="search" className="field" value={search} placeholder="Search activities…"
      aria-label="Search activities" onChange={(e) => actions.setTimelineSearch(e.target.value)} />
  )
}

function todayStr() {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function EmptyTimeline({ tracking }: { tracking: string }) {
  if (tracking === 'paused') {
    return <div className="empty">Tracking is paused.<br /><span className="small">Resume tracking to record new activity.</span></div>
  }
  if (tracking === 'privacy') {
    return <div className="empty">Privacy mode is on.<br /><span className="small">Nothing is collected while privacy mode is active.</span></div>
  }
  return <div className="empty">No recorded activity on this date.<br /><span className="small">A new day begins the moment CHRONOS observes it.</span></div>
}

export function TimelineRow({ r, twelve, projectName, manual = false }: {
  r: ActivityRecord; twelve: boolean; projectName: string; manual?: boolean
}) {
  if (r.cat === 'gap') {
    return (
      <div className="gap-row">
        {clock(r.start, twelve)} – {clock(r.end, twelve)} &nbsp;·&nbsp; {fmt(r.end - r.start)} tracking gap &nbsp;—&nbsp; {r.detail}
      </div>
    )
  }
  if (manual) {
    return (
      <div className="timeline-item">
        <span className="time">{clock(r.start, twelve)} – {clock(r.end, twelve)}</span>
        <span className="mono" title="Manually entered time">✎</span>
        <button className="timeline-open activity-name" onClick={() => actions.openModal({ kind: 'manual-entry', recordId: r.id })}>
          <strong>{r.app}</strong>
          <small>Manual entry · {r.source}</small>
        </button>
        <span className="category" style={{ color: cats[r.cat].color }}><span className="dot" />{cats[r.cat].label}</span>
        <span className="project-label">{projectName}</span>
        <button className="text-btn mono" onClick={() => actions.openModal({ kind: 'manual-entry', recordId: r.id })}
          aria-label={`Edit manual entry ${r.app}`}>{fmt(r.end - r.start)}</button>
      </div>
    )
  }
  if (r.cat === 'idle') {
    return (
      <div className="timeline-item">
        <span className="time">{clock(r.start, twelve)} – {clock(r.end, twelve)}</span>
        <span className="dot" style={{ color: cats.idle.color }} />
        <span className="activity-name"><strong>Idle</strong><small>Away from keyboard</small></span>
        <span className="category" style={{ color: cats.idle.color }}><span className="dot" />Idle</span>
        <span className="project-label">—</span>
        <span className="text-btn mono">{fmt(r.end - r.start)}</span>
      </div>
    )
  }
  return (
    <div className="timeline-item">
      <span className="time">{clock(r.start, twelve)} – {clock(r.end, twelve)}</span>
      <AppIcon record={r} />
      <button className="timeline-open activity-name" onClick={() => actions.openModal({ kind: 'activity', recordId: r.id })}>
        <strong>{r.app}{r.corrected ? <> <span className="violet">✓</span></> : ''}</strong>
        {r.detail ? <small>{r.detail}</small> : null}
      </button>
      <span className="category" style={{ color: cats[r.cat].color }}><span className="dot" />{cats[r.cat].label}</span>
      <span className="project-label">{projectName}</span>
      <button className="text-btn mono" onClick={() => actions.openModal({ kind: 'activity', recordId: r.id })}
        aria-label={`Inspect ${r.app} at ${clock(r.start, twelve)}`}>{fmt(r.end - r.start)}</button>
    </div>
  )
}

export function computeTotals(rows: ActivityRecord[]) {
  const t = { tracked: 0, active: 0, productive: 0, distracting: 0, neutral: 0, unclassified: 0, idle: 0, gap: 0, focused: 0, classified: 0, coverage: 0, productivity: 0 }
  rows.forEach((r) => {
    const d = r.end - r.start
    t[r.cat] += d
    if (r.cat !== 'gap') { t.tracked += d; if (r.cat !== 'idle') t.active += d }
    if (r.cat === 'productive' || r.cat === 'distracting' || r.cat === 'neutral') t.classified += d
    if (r.cat === 'productive') t.focused += Math.min(r.focus, d)
  })
  t.coverage = t.active ? Math.round((t.classified / t.active) * 100) : 0
  t.productivity = t.classified ? Math.round((t.productive / t.classified) * 100) : 0
  return t
}
