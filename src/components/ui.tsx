import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Icon } from './icons'
import { actions, useStore } from '../store'
import { backend } from '../backend'
import type { TrackingState } from '../types'

export function Button({ label, children, onClick, variant = '', icon, ariaLabel, disabled, danger }: {
  label?: ReactNode
  children?: ReactNode
  onClick?: () => void
  variant?: '' | 'primary' | 'ghost' | 'danger'
  icon?: string
  ariaLabel?: string
  disabled?: boolean
  danger?: boolean
}) {
  return (
    <button
      className={`btn ${variant || ''} ${danger ? 'danger' : ''}`.trim()}
      onClick={onClick}
      aria-label={ariaLabel}
      title={ariaLabel}
      disabled={disabled}
    >
      {icon ? <Icon name={icon} /> : null}
      {children ?? label}
    </button>
  )
}

export function TextButton({ children, onClick, ariaLabel }: { children: ReactNode; onClick?: () => void; ariaLabel?: string }) {
  return <button className="text-btn" onClick={onClick} aria-label={ariaLabel}>{children}</button>
}

export function Tabs({ items, active, onSelect, label }: {
  items: [string, string][]
  active: string
  onSelect: (id: string) => void
  label: string
}) {
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {items.map(([id, l]) => (
        <button key={id} role="tab" aria-selected={active === id} className={active === id ? 'active' : ''} onClick={() => onSelect(id)}>{l}</button>
      ))}
    </div>
  )
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button className="toggle" role="switch" aria-checked={checked} aria-label={label}
      onClick={() => onChange(!checked)} />
  )
}

export function PageTitle({ h, p, action }: { h: ReactNode; p: ReactNode; action?: ReactNode }) {
  return (
    <div className="page-head">
      <div><h1>{h}</h1><p>{p}</p></div>
      {action}
    </div>
  )
}

export function CardHead({ h, right }: { h: ReactNode; right?: ReactNode }) {
  return <div className="card-head"><h2>{h}</h2>{right}</div>
}

/* ---------- discreet contextual help (hover, click, and keyboard) ---------- */

/** Small "?" affordance. The explanation appears on hover, on keyboard focus,
 *  and on click (click toggles so it stays readable; Escape or blur closes).
 *
 *  Placement: `below` hints (card headings) open beneath via CSS. Every other
 *  hint measures the viewport and opens to the RIGHT of its icon — close to
 *  and vertically aligned with it — falling back to below, above, then left
 *  (whichever keeps the popover fully in the viewport and, when `avoid` names
 *  a selector such as the session dial, off that element). */
export function HelpHint({ label, below = false, avoid, children }: { label: string; below?: boolean; avoid?: string; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const wrapRef = useRef<HTMLSpanElement>(null)
  const popRef = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    if (below) return // heading hints keep their CSS placement
    const wrap = wrapRef.current
    const pop = popRef.current
    if (!wrap || !pop) return
    const place = () => {
      const icon = wrap.getBoundingClientRect()
      const vw = window.innerWidth
      const vh = window.innerHeight
      const w = pop.offsetWidth || 228
      const h = pop.offsetHeight || 96
      const GAP = 9
      const M = 8 // viewport margin
      const avoidRect = avoid
        ? (document.querySelector(avoid) as HTMLElement | null)?.getBoundingClientRect()
        : null
      const inVp = (l: number, t: number) => l >= M && l + w <= vw - M && t >= M && t + h <= vh - M
      const hitsAvoid = (l: number, t: number) => {
        if (!avoidRect) return false
        return l < avoidRect.right + 1 && l + w > avoidRect.left - 1 && t < avoidRect.bottom + 1 && t + h > avoidRect.top - 1
      }
      // Candidate placements in viewport coordinates, each clamped into view.
      // Order: right (preferred), below, above, left. For below/above, the
      // centered box is tried first and, when it would intersect the avoided
      // element, a box slid to the avoided element's right.
      const besideY = Math.min(Math.max(icon.top + icon.height / 2 - h / 2, M), vh - M - h)
      const centeredX = Math.min(Math.max(icon.left + icon.width / 2 - w / 2, M, icon.left), vw - M - w)
      type Cand = { side: string; l: number; t: number }
      const cands: Cand[] = [{ side: 'right', l: icon.right + GAP, t: besideY }]
      for (const s of ['below', 'above']) {
        const t = s === 'below' ? icon.bottom + GAP : icon.top - GAP - h
        cands.push({ side: s, l: centeredX, t })
        if (avoidRect) cands.push({ side: s, l: avoidRect.right + 2, t })
      }
      cands.push({ side: 'left', l: icon.left - GAP - w, t: besideY })
      const best =
        cands.find((c) => inVp(c.l, c.t) && !hitsAvoid(c.l, c.t))
        ?? cands.find((c) => inVp(c.l, c.t))
      const side = best?.side ?? (vw - icon.right >= icon.left ? 'right' : 'left')
      const vx = best?.l ?? icon.right + GAP
      const vy = best?.t ?? besideY
      // Safety clamp for viewports narrower than the popover itself.
      const left = Math.min(Math.max(vx, M), vw - M - w)
      const top = Math.min(Math.max(vy, M), vh - M - h)
      wrap.dataset.place = side
      if (side === 'right' || side === 'left') {
        wrap.style.setProperty('--arrow-x', '50%')
      } else {
        const arrowX = Math.min(Math.max(icon.left + icon.width / 2 - left, 12), w - 12)
        wrap.style.setProperty('--arrow-x', `${Math.round(arrowX)}px`)
      }
      pop.style.top = `${Math.round(top - icon.top)}px`
      pop.style.left = `${Math.round(left - icon.left)}px`
    }
    place()
    const onHover = () => place()
    // Viewport clamps are computed against the current scroll position, so
    // re-measure when the window is resized or scrolled (and on hover/focus,
    // which open the popover without a React state change).
    window.addEventListener('resize', onHover)
    window.addEventListener('scroll', onHover, { passive: true, capture: true })
    wrap.addEventListener('mouseenter', onHover)
    wrap.addEventListener('focusin', onHover)
    return () => {
      window.removeEventListener('resize', onHover)
      window.removeEventListener('scroll', onHover, { capture: true })
      wrap.removeEventListener('mouseenter', onHover)
      wrap.removeEventListener('focusin', onHover)
    }
  }, [below, avoid, open])

  return (
    <span ref={wrapRef} className={`help-wrap${below ? ' below' : ''}${open ? ' open' : ''}`}>
      <button type="button" className="help-hint" aria-label={`Help: ${label}`} aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        onBlur={() => setOpen(false)}
        onKeyDown={(e) => { if (e.key === 'Escape') setOpen(false) }}>
        ?
      </button>
      <span ref={popRef} className="help-pop" role="tooltip">{children}</span>
    </span>
  )
}

export function Setting({ h, p, control }: { h: string; p: ReactNode; control?: ReactNode }) {
  return (
    <div className="setting-row">
      <div><h3>{h}</h3><p>{p}</p></div>
      {control}
    </div>
  )
}

export function SelectField({ id, values, current, onChange, ariaLabel }: {
  id?: string
  values: string[]
  current: string
  onChange: (v: string) => void
  ariaLabel: string
}) {
  return (
    <select className="field" id={id} aria-label={ariaLabel} value={current} onChange={(e) => onChange(e.target.value)}>
      {values.map((v) => <option key={v} value={v}>{v}</option>)}
    </select>
  )
}

/* ---------- tracking status metadata (approved copy) ---------- */

export function statusMeta(s: TrackingState): { label: string; color: string; description: string } {
  switch (s) {
    case 'active': return { label: 'Tracking active', color: 'var(--green)', description: 'Observing local activity · stored on this device' }
    case 'paused': return { label: 'Manually paused', color: 'var(--amber)', description: 'Stays paused until you resume' }
    case 'privacy': return { label: 'Privacy mode', color: 'var(--violet)', description: 'Your privacy choice stays in place' }
    case 'disabled': return { label: 'Tracking disabled', color: 'var(--muted)', description: 'Explicit enable required' }
    case 'degraded': return { label: 'Tracking degraded', color: 'var(--amber)', description: 'Limited context · inspect diagnostics' }
    case 'confirmation': return { label: 'Awaiting confirmation', color: 'var(--amber)', description: 'Unknown previous state · review first' }
  }
}

/* ---------- modal host ---------- */

export function ModalHost({ children, title }: { children: ReactNode; title: string }) {
  const modalOpen = useStore((s) => s.modal !== null)
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = ref.current
    if (!d) return
    if (modalOpen && !d.open) d.showModal()
    if (!modalOpen && d.open) d.close()
  }, [modalOpen])
  return (
    <dialog id="modal" aria-labelledby="modal-title" ref={ref}
      onClick={(e) => { if (e.target === ref.current) actions.closeModal() }}
      onCancel={(e) => { e.preventDefault(); actions.closeModal() }}>
      {modalOpen ? (
        /* The visual card is an inner element: clicks anywhere on it — content,
           labels, fields, or its own padding — can never reach the <dialog>
           element, so only genuine backdrop clicks dismiss the modal. */
        <div className="modal-body">
          <div className="dialog-head">
            <h2 id="modal-title">{title}</h2>
            <button className="icon-btn" onClick={() => actions.closeModal()} aria-label="Close dialog"><Icon name="close" /></button>
          </div>
          {children}
        </div>
      ) : null}
    </dialog>
  )
}

/* ---------- helpers used across views ---------- */

export async function withToast(p: Promise<unknown>, message: string, undoable = false) {
  try {
    await p
    actions.toast(message, undoable)
    await refreshAfterMutation()
  } catch (err) {
    actions.toast(err instanceof Error ? err.message : 'Something went wrong.')
  }
}

export async function refreshAfterMutation() {
  const { refreshView } = await import('../store')
  await refreshView()
}

export const backendRef = backend
