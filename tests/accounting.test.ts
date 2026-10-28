import { describe, expect, it } from 'vitest'
import type { ActivityRecord, Category } from '../src/types'

/**
 * Frontend mirror of the approved time-accounting contract. The authoritative implementation
 * lives in the Rust core (src-tauri/src/accounting.rs) with equivalent invariant tests; this
 * copy powers the dev mock and must agree with the Rust definitions.
 */
export function totals(rows: ActivityRecord[]) {
  const t = { tracked: 0, active: 0, productive: 0, distracting: 0, neutral: 0, unclassified: 0, idle: 0, gap: 0, focused: 0, classified: 0, coverage: 0, productivity: 0 }
  for (const r of rows) {
    const d = r.end - r.start
    t[r.cat] += d
    if (r.cat !== 'gap') {
      t.tracked += d
      if (r.cat !== 'idle') t.active += d
    }
    if (r.cat === 'productive' || r.cat === 'distracting' || r.cat === 'neutral') t.classified += d
    if (r.cat === 'productive') t.focused += Math.min(r.focus, d)
  }
  t.coverage = t.active ? Math.round((t.classified / t.active) * 100) : 0
  t.productivity = t.classified ? Math.round((t.productive / t.classified) * 100) : 0
  return t
}

function rec(id: number, start: number, end: number, cat: Category, focus = 0): ActivityRecord {
  return { id, date: '2026-10-24', start, end, app: 'App', detail: '', cat, project: '', focus, kind: 'app', source: 'test', corrected: false }
}

describe('time-accounting contract (frontend mirror)', () => {
  it('handover baseline: 6h20m tracked, 5h30m active, 92% coverage, 87% productivity', () => {
    const rows = [
      rec(1, 540, 805, 'productive', 165), // 265m productive, 165 qualified
      rec(2, 0, 25, 'distracting'),
      rec(3, 0, 15, 'neutral'),
      rec(4, 0, 25, 'unclassified'),
      rec(5, 0, 50, 'idle'),
    ]
    const t = totals(rows)
    expect(t.tracked).toBe(380)
    expect(t.active).toBe(330)
    expect(t.classified).toBe(305)
    expect(t.coverage).toBe(92)
    expect(t.productivity).toBe(87)
    expect(t.focused).toBe(165)
  })

  it('gaps excluded from tracked; idle excluded from active', () => {
    const t = totals([rec(1, 0, 30, 'productive'), rec(2, 30, 50, 'gap'), rec(3, 50, 80, 'idle')])
    expect(t.tracked).toBe(60)
    expect(t.active).toBe(30)
    expect(t.gap).toBe(20)
    expect(t.idle).toBe(30)
  })

  it('unclassified excluded from the productivity denominator', () => {
    const t = totals([rec(1, 0, 60, 'productive'), rec(2, 60, 120, 'unclassified')])
    expect(t.classified).toBe(60)
    expect(t.productivity).toBe(100)
    expect(t.active).toBe(120)
  })

  it('qualified focus bounded to productive records only', () => {
    const t = totals([rec(1, 0, 30, 'productive', 50), rec(2, 30, 60, 'neutral', 30)])
    expect(t.focused).toBe(30)
  })

  it('zero denominators handled honestly', () => {
    const t = totals([])
    expect(t.coverage).toBe(0)
    expect(t.productivity).toBe(0)
    expect(t.tracked).toBe(0)
  })
})
