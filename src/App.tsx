import { useEffect } from 'react'
import { boot, actions, useStore } from './store'
import { backend } from './backend'
import { AppShell } from './components/AppShell'
import { ModalRouter } from './components/dialogs'
import { MiniBar } from './components/MiniBar'
import { configureDateFormat } from './format'

/** Window label ("mini" renders the companion bar instead of the full shell). */
function windowLabel(): string {
  try {
    // Lazy require-free guard: the Tauri API is only meaningful inside the app.
    const w = window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }
    return w.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? 'main'
  } catch {
    return 'main'
  }
}
const IS_MINI = typeof window !== 'undefined' && windowLabel() === 'mini'

export default function App() {
  const ready = useStore((s) => s.ready)
  const appearance = useStore((s) => s.appState?.settings.appearance)
  const dateFormat = useStore((s) => s.appState?.settings.dateFormat)
  const page = useStore((s) => s.page)

  // Apply the page attribute during render (before first paint) so per-page hero CSS is
  // correct on the very first frame — prevents the full-artwork flash on Timeline refresh.
  if (typeof document !== 'undefined' && !IS_MINI) {
    document.body.dataset.page = page
  }

  useEffect(() => {
    void boot()
  }, [])
  // Soft Midnight is a real, distinct theme (approved CSS layer) toggled on the body.
  useEffect(() => {
    document.body.classList.toggle('appearance-soft', appearance === 'Soft midnight')
  }, [appearance])
  // Date presentation follows the selected format (storage stays ISO).
  useEffect(() => {
    if (dateFormat) configureDateFormat(dateFormat)
  }, [dateFormat])

  if (!ready) {
    return <div className="boot-splash" aria-label="CHRONOS is starting"><div className="logomark" /></div>
  }

  if (IS_MINI) {
    return <MiniBar />
  }

  return (
    <>
      <AppShell />
      <ModalRouter />
      <ToastHost />
      <NoticeHost />
    </>
  )
}

function ToastHost() {
  const toast = useStore((s) => s.toast)
  if (!toast) return null
  return (
    <div id="toast-root">
      <div className="toast">
        <span>{toast.message}</span>
        {toast.undoable ? (
          <button onClick={() => { void backend.undoLast().then((ok) => { actions.toast(ok ? 'Correction undone. Dependent totals restored.' : 'No correction to undo.') }) }}>Undo</button>
        ) : (
          <button onClick={() => actions.clearToast()} aria-label="Dismiss message">✕</button>
        )}
      </div>
    </div>
  )
}

function NoticeHost() {
  const notice = useStore((s) => s.notice)
  if (!notice) return null
  const eyebrow =
    notice.type === 'complete' ? 'FOCUS SESSION COMPLETE'
      : notice.type === 'degraded' ? 'STATUS'
        : notice.type === 'break-end' ? 'BREAK ENDED'
          : notice.type === 'break-schedule' ? 'FOCUS INTERVAL COMPLETE'
            : 'GENTLE REMINDER'
  return (
    <div id="notification-root" aria-live="polite">
      <section className={`notification ${notice.type === 'complete' ? 'notice-complete' : ''}`}>
        <div className="row between">
          <span className="eyebrow">CHRONOS · {eyebrow}</span>
          <button className="icon-btn" onClick={() => actions.dismissNotice()} aria-label="Dismiss notification">✕</button>
        </div>
        <h3 style={{ marginTop: 15 }}>{notice.title}</h3>
        <p>{notice.body}</p>
        <div className="row">
          {notice.type === 'break-end' ? (
            <>
              <button className="btn primary" onClick={() => { actions.dismissNotice(); void backend.resumeFocus() }}>Resume Focus</button>
              <button className="btn" onClick={() => { actions.dismissNotice(); void backend.extendBreak() }}>Another 5 Minutes</button>
            </>
          ) : null}
          {notice.type === 'break' ? <ButtonSnooze /> : null}
          {notice.type !== 'break-end' ? <button className="btn" onClick={() => actions.dismissNotice()}>Dismiss</button> : null}
        </div>
      </section>
    </div>
  )
}

function ButtonSnooze() {
  return <button className="btn ghost" onClick={() => { actions.dismissNotice(); actions.toast('Snoozed for 10 minutes.') }}>Snooze 10 min</button>
}
