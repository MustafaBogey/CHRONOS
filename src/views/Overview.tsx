import { actions, useStore } from '../store'
import { backend } from '../backend'
import { Icon, AppIcon, MetricGlyph, GoalGlyph } from '../components/icons'
import { Button, CardHead, HelpHint, TextButton, withToast } from '../components/ui'
import { cats, type ActivityRecord, type SessionSnapshot, type Totals } from '../types'
import { clock, dateLabel, fmt, timerText } from '../format'

function greeting(now: Date): string {
  const h = now.getHours()
  if (h >= 5 && h < 12) return 'Good morning'
  if (h >= 12 && h < 18) return 'Good afternoon'
  if (h >= 18 && h < 23) return 'Good evening'
  return 'Up late'
}

function HeadClock({ now, twelve }: { now: Date; twelve: boolean }) {
  const min = String(now.getMinutes()).padStart(2, '0')
  const h = now.getHours()
  const time = twelve ? `${h % 12 || 12}:${min} ${h < 12 ? 'AM' : 'PM'}` : `${String(h).padStart(2, '0')}:${min}`
  return (
    <div className="head-clock-inline" aria-label="Local device time, updates live">
      <span className="head-clock-inline-date">{now.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' })}</span>
      <span className="head-clock-inline-row">
        <span className="head-clock-inline-time mono">{time}</span>
        <span className="head-clock-inline-note">Local time · live</span>
      </span>
    </div>
  )
}

function Metrics({ t }: { t: Totals }) {
  const rows: [string, number, string, string, string][] = [
    ['Tracked time', t.tracked, 'clock', 'violet', 'Observed intervals only'],
    ['Active time', t.active, 'activity', 'cyan', 'Tracked, excluding idle'],
    ['Productive time', t.productive, 'leaf', 'green', `${t.productivity}% of classified active time`],
    ['Distracting time', t.distracting, 'distraction', 'pink', 'A little awareness goes a long way'],
  ]
  const card = ([l, v, _i, c, n]: [string, number, string, string, string], index: number) => (
    <section className="card metric" key={l} title={n}>
      <div className="metric-top"><span className={c}><MetricGlyph index={index} /></span>{l}</div>
      <div className="metric-value">{fmt(v)}</div>
      <div className="metric-bottom">{n}</div>
    </section>
  )
  // Paired to the dashboard columns: [Tracked, Active] above Today's Focus,
  // [Productive, Distracting] above Today's activity, sharing the same column gap.
  return (
    <div className="metrics metrics-paired">
      <div className="metric-pair">{card(rows[0], 0)}{card(rows[1], 1)}</div>
      <div className="metric-pair">{card(rows[2], 2)}{card(rows[3], 3)}</div>
    </div>
  )
}

export function Ring({ percent, qualifiedMinutes, targetMinutes }: { percent: number; qualifiedMinutes: number; targetMinutes: number }) {
  const p = Math.min(100, percent)
  return (
    <div className="ring" role="img" aria-label={`${p}% of daily focus target`}>
      <svg viewBox="0 0 164 164">
        <circle className="track" cx="82" cy="82" r="67" />
        <circle className="progress" cx="82" cy="82" r="67" stroke="url(#ringGradient)" strokeDasharray={`${p * 4.21} 421`} />
      </svg>
      <div className="ring-center">
        <strong>{p}<span style={{ fontSize: 20 }}>%</span></strong>
        <small>
          <span className="daily-ring-time">{fmt(qualifiedMinutes)} / {fmt(targetMinutes)}</span>
          <span className="daily-ring-label">Daily target</span>
        </small>
      </div>
    </div>
  )
}

export function GoalRow({ label, value, target, color, limit = false, type }: {
  label: string; value: number; target: number; color: string; limit?: boolean; type: 'project' | 'limit' | 'focus'
}) {
  const percent = target ? Math.round((value / target) * 100) : 0
  return (
    <div className="goal-row reference-goal">
      <span className={`goal-emblem goal-${type}`} style={{ color: type === 'project' ? 'var(--cyan)' : type === 'limit' ? 'var(--pink)' : 'var(--violet)' }}>
        <GoalGlyph type={type} />
      </span>
      <div className="goal-detail">
        <span className="goal-title"><span>{label}</span></span>
        <span className="goal-value">{target ? `${fmt(value)} / ${fmt(target)}` : 'No limit'}</span>
      </div>
      <div className="bar"><span style={{ width: `${Math.min(percent, 100)}%`, background: color }} /></div>
      <span className="goal-percent">{limit ? (target ? `${fmt(Math.max(0, target - value))} remaining` : 'Optional · off') : `${percent}%`}</span>
    </div>
  )
}

export function WeekChart({ days, selected, maxMinutes }: { days: { date: string; totals: Totals }[]; selected?: string; maxMinutes: number }) {
  const max = Math.max(1, maxMinutes)
  const yLabels = max >= 480 ? ['8h', '4h', '0'] : ['4h', '2h', '0']
  return (
    <div className="weekly-chart" role="img" aria-label="Weekly time distribution, productive, distracting, unclassified, neutral, and idle.">
      <div className="chart-y"><span>{yLabels[0]}</span><span>{yLabels[1]}</span><span>{yLabels[2]}</span></div>
      {days.map((d) => (
        <div key={d.date} className={`day-column ${d.date === selected ? 'selected' : ''}`} title={`${dateLabel(d.date)}: ${fmt(d.totals.tracked)} tracked`}>
          <div className="day-stack" style={{ height: `${(d.totals.tracked / max) * 100}%` }}>
            {(['productive', 'distracting', 'neutral', 'unclassified', 'idle'] as const).map((c) => (
              <span key={c} style={{ height: `${d.totals.tracked ? (d.totals[c] / d.totals.tracked) * 100 : 0}%`, background: cats[c].color }} />
            ))}
          </div>
          <span className="day-label">{dateLabel(d.date, { weekday: 'short' })}</span>
        </div>
      ))}
    </div>
  )
}

export function ChartLegend() {
  return (
    <div className="chart-legend">
      {(['productive', 'distracting', 'unclassified', 'idle'] as const).map((c) => (
        <span key={c}><i className="dot" style={{ color: cats[c].color }} />{cats[c].label}</span>
      ))}
      <span><i className="dot" style={{ color: 'var(--cyan)' }} />Neutral</span>
    </div>
  )
}

export function QualityOrbit({ percent }: { percent: number }) {
  const p = Math.min(100, percent)
  return (
    <div className="quality-ring" role="img" aria-label={`Focus quality: ${p}% of elapsed session time`}>
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <circle cx="50" cy="50" r="41" fill="none" stroke="#164454" strokeWidth="7" />
        <circle cx="50" cy="50" r="41" fill="none" stroke="url(#qualityGradient)" strokeWidth="7" strokeLinecap="round"
          strokeDasharray={`${p * 2.576} 258`} style={{ filter: 'drop-shadow(0 0 4px #00aeb566)' }} />
      </svg>
      <strong>{p}%</strong>
    </div>
  )
}

export function sessionControls(status: 'running' | 'paused' | 'ended', breakExpired = false, idlePaused = false) {
  if (status === 'ended') {
    return <Button label="Start a focus session" variant="primary" icon="play" onClick={() => actions.openModal({ kind: 'session-start' })} />
  }
  if (breakExpired) {
    return (
      <>
        <Button label="Resume Focus" variant="primary" icon="play" onClick={() => void withToast(backend.resumeFocus(), 'Focus resumed. Next interval started.')} />
        <Button label="Another 5 Minutes" icon="pause" onClick={() => void withToast(backend.extendBreak(), 'Break extended by five minutes.')} />
      </>
    )
  }
  if (idlePaused) {
    // The focus countdown froze when the user went idle; it continues from the
    // preserved remaining time only when explicitly resumed.
    return (
      <>
        <Button label="Resume Focus" variant="primary" icon="play" onClick={() => void withToast(backend.resumeFocus(), 'Focus countdown resumed.')} />
        <Button label="End session" icon="stop" onClick={() => void backend.endSession()} />
      </>
    )
  }
  return (
    <>
      <Button label={status === 'running' ? 'Pause' : 'Resume'} variant={status === 'running' ? '' : 'primary'}
        icon={status === 'running' ? 'pause' : 'play'}
        onClick={() => void backend.toggleSession()} />
      <Button label="End session" icon="stop" onClick={() => void backend.endSession()} />
    </>
  )
}

export function SessionPill({ status, phase, breakExpired, idlePaused }: { status: 'running' | 'paused' | 'ended'; phase?: string; breakExpired?: boolean; idlePaused?: boolean }) {
  // No badge when no session is running — the Start button tells that story;
  // every active-session state keeps its meaningful indicator.
  if (status === 'ended') return null
  const cls = status === 'running' ? (idlePaused ? 'gray' : '') : 'purple'
  const label = status === 'running'
    ? idlePaused && phase !== 'break'
      ? 'Focus paused — away'
      : breakExpired ? 'Break ended' : phase === 'break' ? 'On break' : 'In progress'
    : 'Paused'
  return (
    <span className={`pill ${cls}`}>
      {status === 'running' && !(idlePaused && phase !== 'break') ? <span className="dot" /> : null}
      {label}
    </span>
  )
}

export function qualityPercent(qualified: number, elapsedSeconds: number): number {
  const minutes = elapsedSeconds / 60
  return Math.round(qualified / (minutes || 1) * 100)
}

/** The circular session dial shows the time relevant to the current phase —
 *  never the overall session elapsed time. Values come from the engine's
 *  authoritative phaseRemainingMs (frozen while idle-paused or manually
 *  paused). With scheduled breaks off there is no phase countdown; the dial
 *  falls back to the elapsed session time, clearly labeled. */
export function phaseTimer(s: SessionSnapshot): { text: string; label: string } {
  const mmss = (ms: number) => {
    const total = Math.max(0, Math.round(ms / 1000))
    return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
  }
  if (s.phase === 'break') {
    if (s.breakExpired) {
      // Overdue time since the (extended) allowance ended; focus has NOT resumed.
      return { text: `+${mmss(-(s.phaseRemainingMs ?? 0))}`, label: 'Break overdue' }
    }
    return { text: mmss(Math.max(0, s.phaseRemainingMs ?? 0)), label: 'Break remaining' }
  }
  if (s.phase === 'focus' && s.breaksEnabled) {
    return { text: mmss(Math.max(0, s.phaseRemainingMs ?? 0)), label: 'Focus remaining' }
  }
  // No countdown exists: breaks-disabled uninterrupted focus, or no live session.
  return { text: timerText(s.elapsed), label: 'Elapsed' }
}

function QualityTrend({ intervals }: { intervals: ActivityRecord[] }) {
  if (!intervals.length) return <p className="session-note" style={{ display: 'block', marginTop: 8 }}>No new observed activity.</p>
  const n = intervals.length
  const points = intervals.map((r, i) => {
    const dur = r.end - r.start
    const share = r.cat === 'productive' ? Math.min(r.focus, dur) / (dur || 1) : 0
    return `${4 + (i / Math.max(1, n - 1)) * 232},${27 - share * 23}`
  }).join(' ')
  return (
    <svg className="quality-trend" viewBox="0 0 240 32" preserveAspectRatio="none" role="img"
      aria-label="Qualified share of each observed session interval">
      <path d="M4 30H236" stroke="#342849" fill="none" />
      <polygon points={`${points} 236,30 4,30`} fill="url(#qualityArea)" stroke="none" />
      <polyline points={points} stroke="#c24fff" strokeWidth="1.5" fill="none" />
    </svg>
  )
}

export function Overview() {
  const overview = useStore((s) => s.overview)
  const appState = useStore((s) => s.appState)
  const nowMs = useStore((s) => s.now)
  const liveSnap = useStore((s) => s.currentActivity)
  if (!overview || !appState) return null

  const t = overview.totals
  const goals = appState.goals
  const session = appState.session
  const projects = appState.projects
  const twelve = appState.settings.time === '12-hour'
  const now = new Date(nowMs)
  const pct = goals.daily ? Math.round((t.focused / goals.daily) * 100) : 0
  const quality = qualityPercent(session.qualified, session.elapsed)
  const dial = phaseTimer(session)
  const goalProject = projects.find((p) => p.id === 'p1') ?? projects[0]
  const projectName = goalProject?.name ?? 'Project'
  const weekMax = Math.max(480, ...overview.week.map((d) => d.totals.tracked))
  const liveActivity = liveSnap && liveSnap.tracking === 'active' ? liveSnap.activity : null

  return (
    <>
      <div className="page-head">
        <div>
          <HeadClock now={now} twelve={twelve} />
          <h1>{greeting(now)}, <span className="violet">{appState.profile.name.split(' ')[0]}.</span></h1>
          <p>Your time, with intention. Here's how today is unfolding.</p>
        </div>
      </div>
      <Metrics t={t} />
      <div className="dashboard-grid">
        <section className="card focus-card">
          <CardHead h="Today’s Focus" right={<span className="eyebrow">DAILY INTENTION</span>} />
          <div className="focus-head-link">
            <TextButton onClick={() => actions.openModal({ kind: 'goals' })}>Adjust your target <Icon name="arrow" /></TextButton>
          </div>
          <div className="focus-content">
            <Ring percent={pct} qualifiedMinutes={t.focused} targetMinutes={goals.daily} />
            <div className="focus-copy">
              <h3>“Discipline today<br />creates freedom tomorrow.”</h3>
              <p>Qualified focus. Meaningful progress.</p>
            </div>
          </div>
        </section>

        <section className="card">
          <CardHead h="Today’s activity" right={<span className="muted mono">{fmt(t.tracked)} total</span>} />
          {t.tracked > 0 ? (
            <div className="activity-mix">
              {(['productive', 'distracting', 'unclassified', 'idle'] as const).map((c) => (
                <div className="mix-row" key={c}>
                  <span className="mix-label"><i className="dot" style={{ color: cats[c].color }} />{cats[c].label}</span>
                  <div className="bar"><span style={{ width: `${(t[c] / t.tracked) * 100}%`, background: cats[c].color }} /></div>
                  <span className="mono">{fmt(t[c])}</span>
                  <span className="muted small mono">{Math.round((t[c] / t.tracked) * 100)}%</span>
                </div>
              ))}
              <div className="row between muted" style={{ fontSize: 10, marginTop: 18 }}>
                <span>+ {fmt(t.neutral)} neutral</span>
                <span>{t.coverage}% classification coverage</span>
              </div>
            </div>
          ) : (
            <div className="empty">No observed activity yet today.<br /><span className="small">When tracking is active, CHRONOS records it here — locally.</span></div>
          )}
        </section>

        <section className="card recent-card">
          <CardHead h="Recent activity" right={<TextButton onClick={() => actions.navigate('timeline')}>View timeline <Icon name="arrow" /></TextButton>} />
          <LiveNowCard twelve={twelve} />
          {overview.recent.length ? overview.recent.map((r) => <RecentRow key={r.id} r={r} twelve={twelve} />) : (
            overview.recent.length === 0 && !liveActivity ? (
              <div className="empty">Nothing yet today.<br /><span className="small">Your recent applications will appear here.</span></div>
            ) : null
          )}
        </section>

        <section className="card session-card">
          <CardHead h={<>Focus Session<HelpHint label="Focus Session" below>A deliberate focus period. CHRONOS observes your real activity through tracking — the timer alone never counts as focus. The focus countdown freezes automatically while you are away and continues only when you choose to resume.</HelpHint></>} right={<SessionPill status={session.status} phase={session.phase} breakExpired={session.breakExpired} idlePaused={session.focusIdlePaused} />} />
          <div className="session-body">
            <div className="session-orbit-col">
              <div className="session-orbit" role="img" aria-label={`Session dial: ${dial.label} ${dial.text}`}>
                <div className="eyebrow">{(session.intention || 'Deep work').toUpperCase()}</div>
                <div className="session-timer">{dial.text}</div>
                <div className="session-phase-label">{dial.label}</div>
              </div>
              <div className="session-caption">{session.intention || 'Deep work'}{session.project ? ` · ${projectNameById(projects, session.project)}` : ''}</div>
              {session.status !== 'ended' && session.phase === 'break' ? (
                <div className="session-phase-note">
                  <strong className="violet">Break{session.breakExpired ? ' ended' : ''}</strong>
                  {!session.breakExpired && session.phaseRemainingMs != null && session.phaseRemainingMs > 0 ? (
                    <span className="muted small"> · {fmt(session.phaseRemainingMs / 60000)} remaining</span>
                  ) : null}
                  {session.breakExtensionsMin ? <span className="muted small"> · +{fmt(session.breakExtensionsMin)} extended</span> : null}
                </div>
              ) : session.status !== 'ended' && session.phase === 'focus' && session.breaksEnabled && !session.focusIdlePaused && session.phaseRemainingMs != null && session.phaseRemainingMs > 0 ? (
                <div className="session-phase-note">
                  <strong>Focus</strong>
                  <span className="muted small"> · break at {fmt(session.phaseRemainingMs / 60000)}</span>
                  {session.focusIntervals ? <span className="muted small"> · {session.focusIntervals} completed</span> : null}
                </div>
              ) : session.status !== 'ended' && session.focusIntervals ? (
                <div className="session-phase-note"><span className="muted small">{session.focusIntervals} focus intervals completed</span></div>
              ) : null}
            </div>
            <div className="session-stats">
              <div className="quality-label">Focus quality <HelpHint label="Focus quality" avoid=".session-orbit">Qualified focus ÷ elapsed session time. Because elapsed includes breaks and idle time, a session with long breaks shows a lower percentage even when its focus time was fully productive.</HelpHint></div>
              <div className="quality-readout">
                <QualityOrbit percent={quality} />
                <div className="quality-notes">
                  <p><span className="green">✓</span><strong>{fmt(session.qualified)}</strong> qualified focus</p>
                  <p><span className="green">✓</span><strong>{session.interruptions}</strong> interruptions <HelpHint label="Interruptions" avoid=".session-orbit">Counted each time you go idle (away from keyboard) or open a distracting application while the session runs.</HelpHint></p>
                  <p><strong>{fmt(session.idle)}</strong> idle · excluded</p>
                </div>
              </div>
              <QualityTrend intervals={overview.sessionIntervals} />
              <div className="break-stats">
                <p><strong className="mono">{fmt(session.breakTotalMin ?? 0)}</strong> total break time</p>
                <p><strong className="mono">{session.breakCount ?? 0}</strong> breaks taken</p>
                <p><strong className="mono">{fmt(session.breakOverrunTotalMin ?? 0)}</strong> break overrun <HelpHint label="Break overrun" avoid=".session-orbit">Break time beyond the scheduled allowance plus any extensions you requested. The break clock keeps running until you choose Resume Focus.</HelpHint></p>
              </div>
            </div>
          </div>
          {session.breakExpired ? (
            <div className="info-strip" style={{ marginBottom: 12 }}>
              <Icon name="info" />
              <span>Your break has ended. Choose when to resume — CHRONOS waits for you.</span>
            </div>
          ) : session.status === 'running' && session.focusIdlePaused ? (
            <div className="info-strip" style={{ marginBottom: 12 }}>
              <Icon name="info" />
              <span>You were away. Resume when you're ready.</span>
            </div>
          ) : null}
          <div className="session-controls">{sessionControls(session.status, session.breakExpired, session.focusIdlePaused)}</div>
          <p className="session-note">Elapsed time is not all focused time. Qualified activity is measured separately.</p>
        </section>

        <section className="card goals-card">
          <CardHead h="Today’s Goals" right={<TextButton onClick={() => actions.openModal({ kind: 'goals' })}><Icon name="settings" /> Edit goals</TextButton>} />
          <div className="goals-stack">
            <GoalRow type="focus" label="Daily focus" value={t.focused} target={goals.daily} color="linear-gradient(90deg,#8654ee,#ba8afc)" />
            <GoalRow type="project" label={`Project time · ${projectName}`} value={overview.projectMinutesToday} target={goals.project} color="var(--cyan)" />
            <GoalRow type="limit" label="Distraction limit" value={t.distracting} target={goals.limit} color="var(--pink)" limit />
          </div>
        </section>

        <section className="card weekly-card">
          <CardHead h="Weekly Overview" right={<span className="muted">{dateLabel(overview.week[0]?.date ?? '', { month: 'short', day: 'numeric' })} – {dateLabel(overview.week[6]?.date ?? '', { month: 'short', day: 'numeric' })}</span>} />
          <WeekChart days={overview.week} selected={overview.date} maxMinutes={weekMax} />
          <ChartLegend />
        </section>
      </div>
    </>
  )
}

function projectNameById(projects: { id: string; name: string }[], id: string): string {
  return projects.find((p) => p.id === id)?.name ?? 'No project'
}

/**
 * The live "Right now" strip inside Recent activity. Shows the currently open
 * interval — explicitly marked ongoing, never as a finalized record — with the
 * elapsed duration ticking locally from the engine's authoritative start time.
 * Idle, paused, privacy, and unobserved states are labeled honestly.
 */
function LiveNowCard({ twelve }: { twelve: boolean }) {
  const snap = useStore((s) => s.currentActivity)
  const nowMs = useStore((s) => s.now)
  if (!snap) return null
  if (snap.tracking === 'paused' || snap.tracking === 'disabled' || snap.tracking === 'privacy' || snap.tracking === 'confirmation') {
    return null // the sidebar status box already tells this story
  }
  if (!snap.activity) {
    return (
      <div className="live-now" aria-label="Current activity">
        <span className="live-dot live-dot-idle" />
        <span className="activity-name">
          <strong>{snap.idle ? 'Idle — away from keyboard' : 'Observing…'}</strong>
          <small>{snap.idle ? 'No current activity while you are away' : 'Waiting for the first observed activity'}</small>
        </span>
        <span className="pill gray">Now</span>
      </div>
    )
  }
  const a = snap.activity
  const meta = cats[(a.cat as keyof typeof cats) in cats ? (a.cat as keyof typeof cats) : 'unclassified'] ?? cats.unclassified
  const elapsedMin = Math.max(0, (nowMs - a.startMs) / 60000)
  return (
    <div className="live-now" aria-label={`Current activity: ${a.name}`}>
      <span className="live-dot" />
      <span className="activity-name">
        <strong>{a.isSite ? a.name : a.app}</strong>
        <small>
          <i className="dot" style={{ color: meta.color }} />
          <span style={{ color: meta.color }}>{meta.label}</span>
          <span className="muted"> · {a.isSite ? a.app : 'in progress'} · since {clock(minutesOfDay(a.startMs), twelve)}</span>
        </small>
      </span>
      <span className="activity-duration mono live-elapsed">{fmt(elapsedMin)}</span>
      <span className="pill purple">Now</span>
    </div>
  )
}

function minutesOfDay(ms: number): number {
  const d = new Date(ms)
  return d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60
}

/** Browser display names from the tracker (tracker/mod.rs display_name) — their rows
 *  always carry a privacy-safe service/domain label in `detail`, whichever path
 *  (domain hint or curated title evidence) produced it. */
const BROWSER_APPS = /^(Google Chrome|Microsoft Edge|Firefox|Brave|Zen Browser|Chromium|Opera|Vivaldi|Arc)$/

function RecentRow({ r, twelve }: { r: ActivityRecord; twelve: boolean }) {
  // Browser rows lead with the attributed domain/service label (e.g. "ChatGPT",
  // "youtube.com"); the browser stays visible as the parent application identity.
  // The category indicator and label always use the existing category palette.
  const isSite = (r.kind === 'website' || BROWSER_APPS.test(r.app)) && !!r.detail
  const meta = cats[r.cat]
  return (
    <button className="activity-row" onClick={() => actions.openModal({ kind: 'activity', recordId: r.id })}>
      <AppIcon record={r} />
      <span className="activity-name">
        <strong>{isSite ? r.detail : r.app}</strong>
        <small>
          <i className="dot" style={{ color: meta.color }} />
          <span style={{ color: meta.color }}>{meta.label}</span>
          <span className="muted"> · {isSite ? r.app : projectNameById(useStoreStaticProjects(), r.project)}</span>
        </small>
      </span>
      <span className="activity-time">{clock(r.start, twelve)} – {clock(r.end, twelve)}</span>
      <span className="activity-duration mono">{fmt(r.end - r.start)}</span>
    </button>
  )
}

function useStoreStaticProjects() {
  return useStore((s) => s.appState?.projects ?? [])
}
