/** Formatting helpers matching the approved prototype behavior. */

export function fmt(m: number): string {
  if (!Number.isFinite(m)) return '0m'
  if (m > 0 && m < 1) return '<1m'
  m = Math.round(m)
  return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m` : `${m}m`
}

export function clock(minutes: number, twelve = false): string {
  // Display formatting: whole minutes, never fractional values from the view model.
  const total = Math.floor(minutes)
  const h = Math.floor(total / 60)
  const mm = String(total % 60).padStart(2, '0')
  if (twelve) return `${h % 12 || 12}:${mm} ${h < 12 ? 'AM' : 'PM'}`
  return `${String(h).padStart(2, '0')}:${mm}`
}

/* Date presentation: the user-selected format (Settings → General → Date format).
   Stored timestamps and all calculations stay ISO (YYYY-MM-DD); only presentation changes. */
export type DateFormat = 'medium' | 'long' | 'd-mmm-yyyy' | 'dd/mm/yyyy' | 'mm/dd/yyyy' | 'iso'

export const DATE_FORMATS: { value: DateFormat; label: string }[] = [
  { value: 'medium', label: 'Oct 27, 2026' },
  { value: 'long', label: 'October 27, 2026' },
  { value: 'd-mmm-yyyy', label: '27 Oct 2026' },
  { value: 'dd/mm/yyyy', label: '27/10/2026' },
  { value: 'mm/dd/yyyy', label: '10/27/2026' },
  { value: 'iso', label: '2026-10-27' },
]

let selectedFormat: DateFormat = 'medium'

export function configureDateFormat(f: string) {
  if (DATE_FORMATS.some((d) => d.value === f)) selectedFormat = f as DateFormat
}

/** Formatted date for the user-selected presentation (e.g. "Oct 27, 2026"). */
export function fmtDate(d: string): string {
  const t = new Date(d + 'T12:00:00')
  switch (selectedFormat) {
    case 'long': return t.toLocaleDateString('en-US', { month: 'long', day: 'numeric', year: 'numeric' })
    case 'd-mmm-yyyy': return `${t.getDate()} ${t.toLocaleDateString('en-US', { month: 'short' })} ${t.getFullYear()}`
    case 'dd/mm/yyyy': return `${String(t.getDate()).padStart(2, '0')}/${String(t.getMonth() + 1).padStart(2, '0')}/${t.getFullYear()}`
    case 'mm/dd/yyyy': return `${String(t.getMonth() + 1).padStart(2, '0')}/${String(t.getDate()).padStart(2, '0')}/${t.getFullYear()}`
    case 'iso': return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(t.getDate()).padStart(2, '0')}`
    default: return t.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })
  }
}

export function dateLabel(d: string, options: Intl.DateTimeFormatOptions = { weekday: 'long', month: 'long', day: 'numeric' }): string {
  // Full labels (weekday context) keep their long form; bare dates use the selected format.
  if (options.weekday) return new Date(d + 'T12:00:00').toLocaleDateString('en-US', options)
  if (options.month || options.day || options.year) return fmtDate(d)
  return fmtDate(d)
}

export function addDays(d: string, n: number): string {
  const t = new Date(d + 'T12:00:00')
  t.setDate(t.getDate() + n)
  return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(t.getDate()).padStart(2, '0')}`
}

export function localDateStr(t: Date = new Date()): string {
  return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(t.getDate()).padStart(2, '0')}`
}

export function minutesSinceLocalMidnight(t: Date = new Date()): number {
  return t.getHours() * 60 + t.getMinutes() + t.getSeconds() / 60
}

export function timerText(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds))
  return `${String(Math.floor(s / 3600)).padStart(2, '0')}:${String(Math.floor(s / 60) % 60).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
}

export function weekStart(d: string): string {
  const t = new Date(d + 'T12:00:00')
  const dow = t.getDay() // 0 = Sunday
  const shift = dow === 0 ? -6 : 1 - dow // Monday-start weeks
  return addDays(d, shift)
}
