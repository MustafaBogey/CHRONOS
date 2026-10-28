import { describe, expect, it } from 'vitest'
import { serviceFor } from '@src/components/icons'
import type { ActivityRecord } from '@src/types'

const row = (over: Partial<ActivityRecord>): ActivityRecord => ({
  id: 1, date: '2026-10-26', start: 0, end: 1, app: '', detail: '', cat: 'unclassified',
  project: '', focus: 0, kind: 'app', source: '', corrected: false, confidence: 0.5,
  ...over,
})

describe('recognizable service icons', () => {
  it('resolves attributed website rows by their domain label', () => {
    expect(serviceFor(row({ kind: 'website', detail: 'ChatGPT', app: 'Google Chrome', cat: 'productive' }))).toBe('chatgpt')
    expect(serviceFor(row({ kind: 'website', detail: 'youtube.com', app: 'Google Chrome' }))).toBe('youtube')
    expect(serviceFor(row({ kind: 'website', detail: 'instagram.com', app: 'Firefox' }))).toBe('instagram')
    expect(serviceFor(row({ kind: 'website', detail: 'Twitch', app: 'Google Chrome' }))).toBe('twitch')
  })

  it('resolves curated title-evidence browser rows and plain browser rows', () => {
    expect(serviceFor(row({ kind: 'app', detail: 'YouTube', app: 'Google Chrome' }))).toBe('youtube')
    expect(serviceFor(row({ kind: 'app', detail: '', app: 'Google Chrome' }))).toBe('chrome')
    expect(serviceFor(row({ kind: 'app', detail: '', app: 'Microsoft Edge' }))).toBe('edge')
  })

  it('resolves known native applications by name', () => {
    expect(serviceFor(row({ app: 'Visual Studio Code', detail: '' }))).toBe('vscode')
    expect(serviceFor(row({ app: 'Discord', detail: 'Discord' }))).toBe('discord')
    expect(serviceFor(row({ app: 'Steam', detail: 'Steam' }))).toBe('steam')
  })

  it('falls back to null (generic/native icon path) for unknown activities', () => {
    expect(serviceFor(row({ app: 'mpc-hc64', detail: 'movie.mkv' }))).toBeNull()
    expect(serviceFor(row({ app: 'Terminal', detail: 'cargo build' }))).toBeNull()
  })

  it('never mistakes a service mention inside unrelated text for an identity', () => {
    // "Steam" surrounded by separators matches; "steamapps" path fragment does not.
    expect(serviceFor(row({ app: ' explorer.exe', detail: 'C:\\Program Files (x86)\\Steam\\steamapps\\common' }))).toBeNull()
  })
})
