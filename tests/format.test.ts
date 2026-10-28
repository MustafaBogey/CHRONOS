import { describe, expect, it } from 'vitest'
import { fmt, clock, dateLabel, addDays, timerText, weekStart } from '../src/format'

describe('formatting helpers', () => {
  it('fmt renders minutes and hours like the prototype', () => {
    expect(fmt(25)).toBe('25m')
    expect(fmt(60)).toBe('1h 00m')
    expect(fmt(380)).toBe('6h 20m')
    expect(fmt(0)).toBe('0m')
  })

  it('fmt renders sub-minute durations honestly and guards NaN', () => {
    expect(fmt(0.18)).toBe('<1m')
    expect(fmt(Number.NaN)).toBe('0m')
  })

  it('clock renders 24h and 12h', () => {
    expect(clock(540)).toBe('09:00')
    expect(clock(810)).toBe('13:30')
    expect(clock(540, true)).toBe('9:00 AM')
    expect(clock(780, true)).toBe('1:00 PM')
    expect(clock(0, true)).toBe('12:00 AM')
  })

  it('addDays crosses month boundaries', () => {
    expect(addDays('2026-10-24', 1)).toBe('2026-10-25')
    expect(addDays('2026-10-31', 1)).toBe('2026-11-01')
    expect(addDays('2026-10-24', -12)).toBe('2026-10-12')
  })

  it('weekStart finds Monday', () => {
    // 2026-10-24 is a Saturday; Monday of that week is 2026-10-19
    expect(weekStart('2026-10-24')).toBe('2026-10-19')
    expect(weekStart('2026-10-19')).toBe('2026-10-19')
    expect(weekStart('2026-10-25')).toBe('2026-10-19') // Sunday belongs to prior Monday-start week
  })

  it('timerText zero-pads', () => {
    expect(timerText(0)).toBe('00:00:00')
    expect(timerText(3661)).toBe('01:01:01')
    expect(timerText(6000)).toBe('01:40:00')
  })

  it('dateLabel formats dates', () => {
    expect(dateLabel('2026-10-24', { weekday: 'long' })).toBe('Saturday')
  })
})
