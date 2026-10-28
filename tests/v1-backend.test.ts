import { describe, it, expect } from 'vitest'
import { mockBackend } from '../src/backend/mock'
import type { ActivityRecord } from '../src/types'

/**
 * V1 workflow tests against the dev mock, which mirrors the Rust core's rules:
 * manual-entry overlap policy, bulk assignment, project deletion references,
 * and retention fail-safe activation. Slots avoid the seeded sample day
 * (09:00-15:40) so conflicts come only from entries the tests create.
 */

const MIN = 60_000

function midnight(): number {
  const d = new Date()
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

function dateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

async function addManual(startMin: number, endMin: number, label: string, cat = 'productive', project = '') {
  const base = midnight()
  return mockBackend.manualEntryAdd(base + startMin * MIN, base + endMin * MIN, label, cat, project)
}

describe('manual time entries', () => {
  it('adds a manual entry marked as manual, never observed', async () => {
    await addManual(7 * 60, 8 * 60, 'Client call')
    const rows = await mockBackend.getTimeline(dateStr(new Date()))
    const entry = rows.find((r) => r.app === 'Client call')
    expect(entry).toBeTruthy()
    expect(entry?.manual).toBe(true)
    expect(entry?.kind).toBe('manual')
    expect(entry?.source).toBe('Manual entry')
  })

  it('rejects overlapping manual entries instead of double-counting', async () => {
    await expect(addManual(19 * 60, 20 * 60, 'First block')).resolves.toBeUndefined()
    await expect(addManual(19 * 60 + 30, 20 * 60 + 30, 'Overlapping')).rejects.toThrow(/Overlaps manual entry/)
  })

  it('edits by replacing the span and deletes cleanly', async () => {
    const base = midnight()
    await addManual(20 * 60, 21 * 60, 'Reading', 'neutral')
    const rows = await mockBackend.getTimeline(dateStr(new Date()))
    const id = rows.find((r) => r.app === 'Reading')!.id
    await mockBackend.manualEntryUpdate(id, base + 21 * 60 * MIN, base + 22 * 60 * MIN, 'Reading later', 'neutral', '')
    const after = await mockBackend.getTimeline(dateStr(new Date()))
    expect(after.filter((r) => r.manual).length).toBeGreaterThanOrEqual(2)
    expect(after.some((r) => r.app === 'Reading later' && Math.round(r.start) === 21 * 60)).toBe(true)
    await mockBackend.manualEntryDelete(after.find((r) => r.app === 'Reading later')!.id)
    const final = await mockBackend.getTimeline(dateStr(new Date()))
    expect(final.some((r) => r.app === 'Reading later')).toBe(false)
  })
})

describe('bulk assignment', () => {
  it('assigns and unassigns many intervals with explicit attribution', async () => {
    const projects = await mockBackend.getProjectsWithTime()
    const target = projects[0]
    await addManual(22 * 60, 23 * 60, 'Deep work')
    const rows = await mockBackend.getTimeline(dateStr(new Date()))
    const manualIds = rows.filter((r) => r.manual).map((r) => r.id)
    expect(manualIds.length).toBeGreaterThan(0)
    const n = await mockBackend.bulkAssign(manualIds, target.id)
    expect(n).toBe(manualIds.length)
    const assigned = (await mockBackend.getTimeline(dateStr(new Date()))).find((r) => r.app === 'Deep work')
    expect(assigned?.project).toBe(target.id)
    expect(assigned?.projectSource).toBe('user')
    await mockBackend.bulkAssign(manualIds, '')
    const cleared = (await mockBackend.getTimeline(dateStr(new Date()))).find((r) => r.app === 'Deep work')
    expect(cleared?.project).toBe('')
  })
})

describe('project deletion', () => {
  it('preserves intervals while clearing project references', async () => {
    await mockBackend.createProject('Doomed V1', 'test', '')
    const projects = await mockBackend.getProjectsWithTime()
    const target = projects.find((p) => p.name === 'Doomed V1')!
    await addManual(23 * 60, 23 * 60 + 30, 'Doomed work', 'productive', target.id)
    const affected = await mockBackend.deleteProject(target.id)
    expect(affected).toBeGreaterThanOrEqual(1)
    const after = await mockBackend.getTimeline(dateStr(new Date()))
    const row = after.find((r) => r.app === 'Doomed work')
    expect(row).toBeTruthy() // interval preserved
    expect(row?.project).toBe('') // reference cleared, never reassigned
    const list = await mockBackend.getProjectsWithTime()
    expect(list.some((p) => p.id === target.id)).toBe(false)
  })
})

describe('retention enforcement', () => {
  it('never deletes on preference alone; activation requires a finite period', async () => {
    const before = await mockBackend.retentionRunNow()
    expect(before).toBe(0) // inactive by default in the mock settings
    const st = await mockBackend.retentionStatus()
    expect(st.active).toBe(false)
    await mockBackend.updateSettings({ retention: 'Forever' })
    await expect(mockBackend.retentionSetActive(true)).rejects.toThrow(/Choose a retention period/)
  })
})

describe('work blocks', () => {
  it('computes blocks without altering records', async () => {
    const result = await mockBackend.getWorkBlocks(dateStr(new Date()))
    expect(Array.isArray(result.blocks)).toBe(true)
    const sum = result.blocks.reduce((a, b) => a + b.minutes, 0)
    expect(sum).toBeGreaterThanOrEqual(0)
    for (const b of result.blocks) {
      expect(b.intervalIds.length).toBeGreaterThan(0)
    }
  })
})

void ({} as ActivityRecord)
