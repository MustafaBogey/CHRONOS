import { describe, expect, it } from 'vitest'
import { phaseTimer } from '@src/views/Overview'
import type { SessionSnapshot } from '@src/types'

/**
 * The circular session dial must show the time relevant to the current phase —
 * never the overall session elapsed time (1.3.2 presentation correction).
 * All six acceptance states, driven by the engine's authoritative snapshot.
 */

function snap(patch: Partial<SessionSnapshot>): SessionSnapshot {
  return {
    status: 'running', elapsed: 435, qualified: 0, idle: 0, interruptions: 0,
    intention: 'Deep work', project: '', ...patch,
  }
}

describe('phaseTimer (session dial)', () => {
  it('active focus shows the remaining focus countdown', () => {
    const t = phaseTimer(snap({ phase: 'focus', breaksEnabled: true, focusMinutes: 20, phaseRemainingMs: 12 * 60000 }))
    expect(t).toEqual({ text: '12:00', label: 'Focus remaining' })
  })

  it('idle-paused focus shows the frozen remaining time', () => {
    const t = phaseTimer(snap({ phase: 'focus', breaksEnabled: true, focusMinutes: 20, phaseRemainingMs: 12 * 60000, focusIdlePaused: true }))
    expect(t).toEqual({ text: '12:00', label: 'Focus remaining' })
  })

  it('manually paused focus shows the frozen remaining time', () => {
    const t = phaseTimer(snap({ status: 'paused', phase: 'focus', breaksEnabled: true, focusMinutes: 25, phaseRemainingMs: 15 * 60000 }))
    expect(t).toEqual({ text: '15:00', label: 'Focus remaining' })
  })

  it('explicitly resumed focus continues from the preserved remaining time', () => {
    // After Resume Focus the engine re-anchors the phase; the remaining value
    // continues down from the frozen amount.
    const t = phaseTimer(snap({ phase: 'focus', breaksEnabled: true, focusMinutes: 20, phaseRemainingMs: 11 * 60000 + 58 * 1000 }))
    expect(t).toEqual({ text: '11:58', label: 'Focus remaining' })
  })

  it('active break shows the remaining break countdown', () => {
    const t = phaseTimer(snap({ phase: 'break', breakExpired: false, phaseRemainingMs: 3 * 60000 + 5000, breakExtensionsMin: 5 }))
    expect(t).toEqual({ text: '3:05', label: 'Break remaining' })
  })

  it('expired break shows the overdue duration, clearly labeled', () => {
    const t = phaseTimer(snap({ phase: 'break', breakExpired: true, phaseRemainingMs: -(90 * 1000) }))
    expect(t).toEqual({ text: '+1:30', label: 'Break overdue' })
  })

  it('breaks-disabled sessions and ended sessions fall back to elapsed, labeled as such', () => {
    expect(phaseTimer(snap({ phase: 'focus', breaksEnabled: false }))).toEqual({ text: '00:07:15', label: 'Elapsed' })
    expect(phaseTimer(snap({ status: 'ended', phase: '' }))).toEqual({ text: '00:07:15', label: 'Elapsed' })
  })

  it('never shows a negative or NaN countdown', () => {
    expect(phaseTimer(snap({ phase: 'focus', breaksEnabled: true, phaseRemainingMs: -5 })).text).toBe('0:00')
    expect(phaseTimer(snap({ phase: 'break', breakExpired: false, phaseRemainingMs: undefined as unknown as number })).text).toBe('0:00')
  })
})
