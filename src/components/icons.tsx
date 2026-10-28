import { useEffect, useState } from 'react'
import type { ActivityRecord } from '../types'
import { tauri } from '../browserStatus'

/* Ported verbatim from the approved prototype icon system (local SVGs only). */

const icons: Record<string, string> = {
  home: 'm3 10 9-7 9 7v10H6V10m4 10v-6h4v6',
  timeline: 'M4 4v16m5-14h11v5H9zm0 9h7v5H9z',
  focus: 'M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6z M9 12l2 2 4-5',
  reports: 'M4 21V4h16v17M8 16v-4m4 4V7m4 9v-6',
  settings: 'm9 3-1 3-3 1-2 3 2 2v3l2 3 3-1 2 2 3-1 1-3 3-1 1-3-2-2V7l-3-1-2-3z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
  clock: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M12 7v5l3 2',
  activity: 'M3 12h4l3-7 4 14 3-7h4',
  leaf: 'M20 4C8 2 3 9 6 16s15 5 14-12M6 19 16 9',
  distraction: 'M7 8h10l4 9c1 3-3 4-5 0H8c-2 4-6 3-5 0zM6 12h4m-2-2v4m8-2h.1m2 2h.1',
  arrow: 'M4 12h15m-5-5 5 5-5 5',
  chevron: 'm9 5 7 7-7 7',
  down: 'm5 9 7 7 7-7',
  left: 'm15 5-7 7 7 7',
  pause: 'M8 5v14m8-14v14',
  play: 'm8 4 12 8-12 8z',
  stop: 'M5 5h14v14H5z',
  moon: 'M20 15A9 9 0 0 1 9 3a9 9 0 1 0 11 12',
  bell: 'M6 8a6 6 0 0 1 12 0v7l2 3H4l2-3zm4 13h4',
  tray: 'M3 14h5l2 3h4l2-3h5M4 6h16l2 14H2z',
  shield: 'M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6zM9 12l2 2 4-5',
  spark: 'm12 2 2.5 7.5L22 12l-7.5 2.5L12 22l-2.5-7.5L2 12l7.5-2.5z',
  plus: 'M12 5v14M5 12h14',
  close: 'm6 6 12 12M18 6 6 18',
  check: 'm5 12 4 4L19 6',
  download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
  calendar: 'M4 5h16v16H4zM8 3v5m8-5v5M4 10h16',
  info: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M12 11v6m0-10h.01',
  undo: 'M8 4 3 9l5 5M3 9h10a6 6 0 0 1 0 12',
  folder: 'M3 6h7l2 3h9v11H3z',
  search: 'M17 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0m-2 5 6 6',
  monitor: 'M3 4h18v13H3zm5 17h8m-4-4v4',
  privacy: 'M3 3 21 21M9 5c5-2 10 2 13 7l-3 4M6 7l-4 5c5 8 13 9 18 3M10 10a3 3 0 0 0 4 4',
  target: 'M21 12a9 9 0 1 1-9-9m5 0v4h4m-5 5a4 4 0 1 1-4-4m0 4 7-7',
  power: 'M12 2v10M6 5a9 9 0 1 0 12 0',
  mail: 'M3 5h18v14H3zm0 0 9 8 9-8',
}

const expressive: Record<string, string> = {
  home: '<path d="m3 10 9-7 9 7M5.5 8.5V21h13V8.5M9.5 21v-7h5v7"/><path d="M6 11h12v9H6z" fill="currentColor" fill-opacity=".06" stroke="none"/>',
  timeline: '<path d="M8 4H4v5m12-5h4v5M8 20H4v-5m12 5h4v-5M8 7h8v4H8zm0 8h8v2H8z"/><path d="M5 12h14" opacity=".25"/>',
  focus: '<path d="m12 3-8 3v6c0 4.6 8 9 8 9s8-4.4 8-9V6z" fill="currentColor" fill-opacity=".05"/><path d="m12 7 1.1 3.5 3.4 1.1-3.4 1.1L12 16l-1.1-3.3-3.4-1.1 3.4-1.1z" stroke-width="1.4"/>',
  reports: '<rect x="3.5" y="3" width="17" height="18" rx="1.7"/><path d="M8 3v18" opacity=".6"/><path d="M11 16v-4m4 4V7m3 9v-6" stroke-width="2"/>',
  settings: '<path d="m9.4 3-.6 2.5-2 .9L4.5 6l-1.6 2.8 1.8 1.9-.1 2.3-1.7 1.9L4.5 18l2.5-.5 2 .9.5 2.6h3.3l.6-2.6 2-.9 2.5.5 1.6-3.1-1.7-1.9v-2.3l1.7-1.9L18 6l-2.5.4-2-.9-.6-2.5z"/><circle cx="11.1" cy="12" r="3.1"/>',
  pause: '<rect x="6.5" y="5.5" width="3.5" height="13" rx=".8" fill="currentColor" stroke="none"/><rect x="14" y="5.5" width="3.5" height="13" rx=".8" fill="currentColor" stroke="none"/>',
  play: '<path d="M8 4.5v15l11-7.5z" fill="currentColor" stroke="none"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="1.4" fill="currentColor" stroke="none"/>',
}

export function Icon({ name }: { name: string }) {
  const body = expressive[name] ?? `<path d="${icons[name] ?? icons.info}"/>`
  // eslint-disable-next-line react/no-danger
  return <svg className="icon" viewBox="0 0 24 24" aria-hidden="true" dangerouslySetInnerHTML={{ __html: body }} />
}

/* --- colorful activity app icons (local SVG alternatives, no remote brand assets) --- */

const appGlyphs: Record<string, string> = {
  code: '<path d="m17 3 7 3v16l-7 3-12-9v-4z" fill="#1a96ec"/><path d="m17 3 7 3v16l-7 3z" fill="#37c5ff"/><path d="m10.5 8-6 6 6 6 2.6-2.7L9.8 14l3.3-3.3" fill="none" stroke="#e3faff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/><path d="m15 8 4 6-4 6" fill="none" stroke="#074b89" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>',
  document: '<path d="M7 3h10l5 5v17H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2" fill="#edf6ff"/><path d="M17 3v6h5" fill="#77c6ff"/><rect x="8" y="10" width="8" height="6" rx="1" fill="#2b7def"/><path d="M9 19h10M9 22h7" fill="none" stroke="#498ce8" stroke-width="1.7" stroke-linecap="round"/>',
  research: '<circle cx="14" cy="14" r="10.5" fill="#228de9"/><path d="M4.7 9.5A10.5 10.5 0 0 1 23 7.9L14 14z" fill="#9c70ff"/><path d="M23 7.9A10.5 10.5 0 0 1 21 22l-7-8z" fill="#20dcb7"/><path d="M21 22A10.5 10.5 0 0 1 4.7 9.5L14 14z" fill="#35b9f8"/><circle cx="14" cy="14" r="5.7" fill="#ecf9ff"/><circle cx="14" cy="14" r="3.9" fill="#2d6ed1"/><path d="m15.7 10.7-1 4-2.6 2.5 1-4z" fill="#dcf7ff"/>',
  video: '<rect x="3" y="6" width="22" height="16" rx="5" fill="#ff477e"/><path d="m11 10 8 4-8 4z" fill="#fff1fb"/><path d="M7 7.5h13" stroke="#ffb5d1" stroke-opacity=".5" stroke-linecap="round"/>',
  design: '<path d="M14 14H8.5a5.5 5.5 0 1 1 5.5-5.5z" fill="#ff6b83"/><path d="M14 14V8.5a5.5 5.5 0 1 1 5.5 5.5z" fill="#a770ff"/><path d="M14 14h5.5a5.5 5.5 0 1 1-5.5 5.5z" fill="#2ed7d0"/><path d="M14 14v5.5A5.5 5.5 0 1 1 8.5 14z" fill="#299ef5"/><path d="m14 10 4 4-4 4-4-4z" fill="#f1ddff"/>',
  communication: '<path d="M3 5h13a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3H9l-4 4v-4H3a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2" fill="#29d9cc"/><path d="M13 11h11a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3h-1v3l-4-3h-6a3 3 0 0 1-3-3v-6a3 3 0 0 1 3-3" fill="#fa5b9a"/><path d="M6 10h8" stroke="#e5fffc" stroke-width="1.8" stroke-linecap="round"/><circle cx="15" cy="17" r="1.1" fill="#fff3db"/><circle cx="19" cy="17" r="1.1" fill="#fff3db"/><circle cx="23" cy="17" r="1.1" fill="#fff3db"/>',
  idle: '<circle cx="14" cy="14" r="10" fill="#8e9ddc"/><path d="M17 5a8 8 0 1 0 6 12.5A9 9 0 0 1 17 5" fill="#d3ddff"/><path d="M11 10v5l4 2" fill="none" stroke="#344f89" stroke-width="1.8" stroke-linecap="round"/>',
  break: '<path d="M5 10h15v8a6 6 0 0 1-6 6h-3a6 6 0 0 1-6-6z" fill="#ffc784"/><path d="M20 12h2a4 4 0 0 1 0 8h-3" fill="none" stroke="#ffe1ba" stroke-width="2.2"/><path d="M9 7V4m5 3V3m4 4V4" stroke="#ffa4c4" stroke-width="1.7" stroke-linecap="round"/>',
}

function recordKindToGlyph(r: ActivityRecord): string {
  // Real application records carry kind 'app'/'website'; map by category + app-name
  // context so distinct tools get distinct artwork (prototype fixtures keep their kinds).
  const name = `${r.app} ${r.detail}`.toLowerCase()
  if (r.kind === 'development' || /code|studio|idea|pycharm|webstorm|rustrover|cursor|opencode|vim|nvim|sublime|emacs/.test(name)) return 'code'
  if (r.kind === 'website' || /docs|documentation|readthedocs|mdn/.test(name)) return r.kind === 'website' ? 'research' : 'document'
  if (/youtube|netflix|twitch|video|twitch|prime video|hulu/.test(name)) return 'video'
  if (r.kind === 'design' || /figma|photoshop|illustrator|inkscape|gimp|blender|design/.test(name)) return 'design'
  if (r.kind === 'communication' || /messages|slack|teams|discord|telegram|whatsapp|mail|outlook|chat/.test(name)) return 'communication'
  if (r.app === 'Lunch break' || r.app === 'Idle' || r.cat === 'idle') return 'idle'
  if (r.kind === 'research' || /github|stack overflow|stackoverflow|search|research/.test(name)) return 'research'
  if (r.cat === 'productive') return 'code'
  if (r.cat === 'distracting') return 'video'
  return 'research'
}

/* --- recognizable service marks (offline, simplified, local SVG only) ---
   Order matters: domain services match before parent browsers. */
const SERVICE_MATCHERS: [string, RegExp][] = [
  ['chatgpt', /chatgpt|openai\.com/],
  ['claude', /claude\.ai|\bclaude\b/],
  ['youtube', /youtube|youtu\.be/],
  ['twitch', /twitch/],
  ['instagram', /instagram/],
  ['reddit', /reddit/],
  ['github', /github/],
  ['spotify', /spotify/],
  ['discord', /discord/],
  ['steam', /(^|\s|·)steam(\s|$|·)/],
  ['netflix', /netflix/],
  ['slack', /slack/],
  ['telegram', /telegram/],
  ['whatsapp', /whatsapp/],
  ['x', /x\.com|twitter/],
  ['wikipedia', /wikipedia/],
  ['vscode', /visual studio code|vscode/],
  ['chrome', /google chrome/],
  ['edge', /microsoft edge/],
  ['firefox', /firefox/],
]

/** Resolves a recognizable offline service mark for known websites/apps, else null. */
export function serviceFor(r: ActivityRecord): string | null {
  const name = `${r.detail ?? ''} ${r.app}`.toLowerCase()
  for (const [id, re] of SERVICE_MATCHERS) {
    if (re.test(name)) return id
  }
  return null
}

const serviceGlyphs: Record<string, string> = {
  chatgpt: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0d1240"/><g fill="none" stroke="#e9e5ff" stroke-width="1.7" stroke-linecap="round"><path d="M14 6.2a4.6 4.6 0 0 1 4.2 2.8 4.4 4.4 0 0 1-.8 5 4.5 4.5 0 0 1-2.5 7.2 4.7 4.7 0 0 1-5-1.9 4.6 4.6 0 0 1-5.3-2.5 4.4 4.4 0 0 1 .8-5 4.5 4.5 0 0 1 2.5-7.2 4.7 4.7 0 0 1 5 1.6"/><path d="M11.4 9.3 16 12l4.4 2.6M11.4 9.3v5.4m0 0 3.2 5.4m0-10.8v5.4"/></g>',
  claude: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#d97757"/><g stroke="#fdf4ee" stroke-width="1.9" stroke-linecap="round"><path d="M14 7v14M9.8 9.8 18.2 18M7 14h14M9.8 18.2 18.2 9.8M14 7l-2 4m2-4 2 4M7 14l4-2m-4 2 4 2m10-2-4-2m4 2-4 2"/></g>',
  youtube: '<rect x="2" y="5" width="24" height="18" rx="5.5" fill="#ff0033"/><path d="m11.5 10 7 3.5-7 3.5z" fill="#fff"/>',
  twitch: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#9146ff"/><path d="M8 8h13v9l-4 4h-3l-3 3v-3H8z" fill="#fff"/><path d="M13 12v4m5-4v4" stroke="#9146ff" stroke-width="2" stroke-linecap="round"/>',
  instagram: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#f2a43a"/><rect x="2.5" y="2.5" width="23" height="23" rx="6.5" fill="none" stroke="#e1306c" stroke-width="2.4"/><rect x="7.5" y="7.5" width="13" height="13" rx="4" fill="none" stroke="#fff" stroke-width="1.9"/><circle cx="14" cy="14" r="3.4" fill="none" stroke="#fff" stroke-width="1.9"/><circle cx="18.7" cy="9.3" r="1.2" fill="#fff"/>',
  reddit: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#ff4500"/><ellipse cx="14" cy="16.5" rx="8" ry="5.5" fill="#fff"/><circle cx="10.8" cy="15.8" r="1.4" fill="#ff4500"/><circle cx="17.2" cy="15.8" r="1.4" fill="#ff4500"/><path d="M11.5 19c1.6 1 3.4 1 5 0" stroke="#ff4500" stroke-width="1.4" stroke-linecap="round" fill="none"/><path d="m18.5 10.5 3-3" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/><circle cx="22" cy="7" r="1.8" fill="#fff"/><path d="M21.4 8.4a4 4 0 0 1-2.4 3.4" stroke="#fff" stroke-width="1.4" stroke-linecap="round" fill="none"/>',
  github: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#24292f"/><path d="M14 6.5c-4.1 0-7.3 3.1-7.3 7 0 3 1.9 5.6 4.7 6.6.2 0 .4-.1.4-.4v-1.6c-1.7.3-2.2-.6-2.4-1.2-.2-.5-.7-1.4-1.3-1.6-.5-.2-1.1-.8 0-.8.8 0 1.5 1 1.8 1.5.6 1 1.5 1.2 2.2.8.1-.8.4-1.4.8-1.7-2-.3-3.6-1.4-3.6-3.9 0-1 .4-1.8 1-2.4-.1-.3-.4-1.2.1-2 0 0 .9 0 1.9 1a6.5 6.5 0 0 1 4.6 0c1-1 1.9-1 1.9-1 .5.8.2 1.7.1 2 .6.6 1 1.4 1 2.4 0 2.5-1.6 3.6-3.6 3.9.5.5.8 1.3.8 2.3v2.1c0 .3.2.4.4.4a7.2 7.2 0 0 0 4.3-6.7c0-3.9-3.2-7-7.3-7z" fill="#fff"/>',
  spotify: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#1db954"/><circle cx="14" cy="14" r="8.6" fill="#0f1524"/><g stroke="#1db954" stroke-linecap="round" fill="none"><path d="M9.6 11.2a8 8 0 0 1 8.8 1.6" stroke-width="1.9"/><path d="M10.3 14.4a6 6 0 0 1 6.4 1.3" stroke-width="1.9"/><path d="M10.9 17.3a3.7 3.7 0 0 1 3.9.9" stroke-width="1.9"/></g>',
  steam: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#171d25"/><circle cx="17.5" cy="10.5" r="4.4" fill="none" stroke="#66c0f4" stroke-width="2.4"/><circle cx="17.5" cy="10.5" r="1.6" fill="#66c0f4"/><path d="M13.4 13.4 8 19.6m0 0a2.6 2.6 0 1 0 2.9 2.6l5.2-6.2" stroke="#66c0f4" stroke-width="2.2" stroke-linecap="round" fill="none"/><circle cx="8" cy="22.2" r="2.4" fill="#c7e5f9"/>',
  discord: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#5865f2"/><path d="M9.5 8.5C11 8 17 8 18.5 8.5c1.6 2.4 2.4 5.5 2 8.8-1.5 1.1-3 1.7-4.4 2l-.9-1.6c-1.9.5-3.5.5-5.4 0L8.9 19.3c-1.5-.3-2.9-.9-4.4-2-.4-3.3.4-6.4 2-8.8z" fill="#fff"/><circle cx="11" cy="13" r="1.5" fill="#5865f2"/><circle cx="17" cy="13" r="1.5" fill="#5865f2"/>',
  netflix: '<rect x="2" y="2" width="24" height="24" rx="6" fill="#0b0b0f"/><path d="M8.5 5.5h3.8l4.7 11V5.5h3v17h-3.6l-4.9-11.4v11.4h-3z" fill="#e50914"/>',
  slack: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0f1524"/><rect x="6" y="6" width="6.6" height="6.6" rx="2" fill="#36c5f0"/><rect x="15.4" y="6" width="6.6" height="6.6" rx="2" fill="#ecb22e"/><rect x="6" y="15.4" width="6.6" height="6.6" rx="2" fill="#2eb67d"/><rect x="15.4" y="15.4" width="6.6" height="6.6" rx="2" fill="#e01e5a"/>',
  telegram: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#2aabee"/><path d="m6.5 13.7 13-5.2c.6-.2 1.2.2 1 .9l-2.2 10.6c-.2.6-.8.8-1.3.4l-3.3-2.5-1.8 1.8c-.3.3-.8.2-.9-.3l-.9-3.6z" fill="#fff"/><path d="m11.3 16.4-.4 3.4c.3 0 .5-.1.7-.4l1.2-1.2" fill="#2aabee"/>',
  whatsapp: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#25d366"/><path d="M14 6.5a7.4 7.4 0 0 0-6.3 11.3L6.5 21.5l3.8-1.1A7.4 7.4 0 1 0 14 6.5z" fill="#fff"/><path d="M10.8 9.8c.3-.6.6-.6.9-.6h.6c.2 0 .5 0 .7.6l.8 1.9c.1.3.1.5-.1.8l-.5.7c-.2.2-.2.4-.1.6.6 1 1.6 1.9 2.7 2.4.3.1.5.1.7-.1l.7-.8c.2-.3.5-.3.8-.1l1.8 1c.3.2.4.4.3.7-.2.9-1.1 1.7-2 1.7-2.7 0-8-3.3-8.6-7.5-.1-.7.1-1.4.5-2z" fill="#25d366"/>',
  x: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0b0b0f"/><path d="m7.5 7 4 5.4-4.3 5.6h2.3l3.2-4.1 3 4.1h2.9l-4.5-6 4-5h-2.3l-2.9 3.7-2.6-3.7z" fill="#fff"/>',
  wikipedia: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0f1524"/><path d="M6.5 9h3.6l1.6 3.7L13.3 9h3.5l-4.4 9.5h-1.2z" fill="none" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/><path d="M6.5 9h2m7.9 0h1.8" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>',
  vscode: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0f2a3d"/><path d="m19 6.2 3 1.5v12.6l-3 1.5L10 14.6l-4.2 3.2-2-1v-5.6l2-1L10 13.4z" fill="#24c1f0"/><path d="m19 10.5-9 6.1-3.4-2.5m12.4 1.9-9-6.1-3.4 2.6" fill="none" stroke="#0f2a3d" stroke-width="1.6" stroke-linejoin="round"/>',
  chrome: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#fff"/><circle cx="14" cy="14" r="9" fill="#4285f4"/><path d="M14 5a9 9 0 0 1 7.8 4.5h-6.3a4.5 4.5 0 0 0-4.3 3.2L7.7 6.9A9 9 0 0 1 14 5z" fill="#ea4335"/><path d="M21.4 15.5a9 9 0 0 1-13.2 4.5l3.2-5.4a4.5 4.5 0 0 0 3.7 2z" fill="#fbbc05"/><circle cx="14" cy="14" r="3.6" fill="#fff"/><circle cx="14" cy="14" r="2.2" fill="#4285f4"/>',
  edge: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#0b3b57"/><path d="M6 12.2C7 7.6 10.2 5 14.2 5c4.4 0 7 3.2 7.5 6.3-2.3-1.6-5-2.3-8.4-2-3 .2-5.5 1.2-7.3 2.9z" fill="#35c1f1"/><path d="M5.4 17.7c-1.3-3.5.6-7.4 4.6-9 .3 2.6 2 5 4.6 6.3 2.4 1.2 4.8 1.4 6.8.8-1.4 3-4.6 5-8.2 4.6-3.2-.3-5.9-1.2-7.8-2.7z" fill="#4dd6ff"/><path d="M21.4 15.8c-.8 3.6-3.9 6.2-7.7 6.2-3 0-5.6-1.3-7.3-3.3 2.5 1 5.6 1.4 8.4.6 2.7-.7 5-2 6.6-3.5z" fill="#0f8ce8"/>',
  firefox: '<rect x="2" y="2" width="24" height="24" rx="7" fill="#1c1030"/><path d="M20.6 10.8c-.3-1.6-1-2.9-2-4-.6-.6-1.2-1.1-2-1.5.4.7.6 1.4.5 2-.7-.9-1.7-1.6-2.9-2a8.9 8.9 0 0 0-5.4-.3c1 .3 1.8.8 2.3 1.5-2.6-.5-5 .3-6.7 2.3A8.8 8.8 0 0 0 5 15.6C5.6 20 9.1 23 13.4 23c4.4 0 8-3.2 8.3-7.5.1-1.6-.6-3.4-1.1-4.7z" fill="#ff9500"/><path d="M12.2 21c-3.2-.4-5.5-3-5.4-6.2 0-2.4 1.4-4.4 3.5-5.2-.3.8-.3 1.6.1 2.3.4-1.9 1.8-3.2 3.8-3.5 1.6-.2 3 .3 4.2 1.4-.6-.2-1.2-.2-1.7 0 .9.5 1.6 1.2 2 2.2.5 1.3.5 2.6 0 3.9-.4-2.5-2.2-4.2-4.6-4.4-2.6-.2-4.8 1.5-5.3 4-.4 2.1.6 4.2 2.5 5.2.3.1.3.3 0 .3z" fill="#ffdf6a"/><circle cx="13.6" cy="14.6" r="3" fill="#8d5cf6"/>',
}

export function ServiceGlyph({ id }: { id: string }) {
  const body = serviceGlyphs[id]
  if (!body) return null
  return <svg className="app-glyph" viewBox="0 0 28 28" fill="none" aria-hidden="true" dangerouslySetInnerHTML={{ __html: body }} />
}

/* --- native application icons (offline one-shot cache from the Rust core) --- */

const nativeIconCache = new Map<string, string | null>() // undefined = in flight
const nativeListeners = new Map<string, Set<() => void>>()
let nativeQueue = new Set<string>()
let nativeTimer: number | null = null

function flushNativeQueue() {
  nativeTimer = null
  const names = [...nativeQueue]
  nativeQueue = new Set()
  const invoke = tauri()?.invoke as ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | undefined
  if (!invoke || !names.length) {
    names.forEach((n) => nativeIconCache.set(n, null))
    return
  }
  void invoke('get_app_icons', { names }).then((raw) => {
    const map = (raw ?? {}) as Record<string, string>
    for (const n of names) {
      nativeIconCache.set(n, map[n] ?? null)
      nativeListeners.get(n)?.forEach((l) => l())
    }
  }).catch(() => {
    for (const n of names) {
      nativeIconCache.set(n, null)
      nativeListeners.get(n)?.forEach((l) => l())
    }
  })
}

/** Subscribes to the offline native icon for one display name; null = use fallback. */
export function useNativeAppIcon(name: string | null): string | null {
  const [, force] = useState(0)
  useEffect(() => {
    if (!name || !tauri()) return
    if (nativeIconCache.has(name)) return
    const listener = () => force((v) => v + 1)
    let set = nativeListeners.get(name)
    if (!set) {
      set = new Set()
      nativeListeners.set(name, set)
    }
    set.add(listener)
    nativeQueue.add(name)
    if (nativeTimer === null) {
      nativeTimer = window.setTimeout(flushNativeQueue, 24)
    }
    return () => {
      set.delete(listener)
    }
  }, [name])
  const value = name ? nativeIconCache.get(name) : undefined
  return value === undefined ? null : value
}

export function AppIcon({ record }: { record: ActivityRecord }) {
  const service = serviceFor(record)
  const native = useNativeAppIcon(service ? null : record.app)
  if (service && serviceGlyphs[service]) {
    return (
      <span className="app-icon app-service" aria-hidden="true">
        <ServiceGlyph id={service} />
      </span>
    )
  }
  if (native) {
    return (
      <span className="app-icon app-native" aria-hidden="true">
        <img src={native} alt="" draggable={false} />
      </span>
    )
  }
  const type = recordKindToGlyph(record)
  return (
    <span className={`app-icon app-${type}`} aria-hidden="true">
      <svg className="app-glyph" viewBox="0 0 28 28" fill="none" dangerouslySetInnerHTML={{ __html: appGlyphs[type] }} />
    </span>
  )
}

/* --- distinct filled metric symbols --- */

const metricGlyphs = [
  '<rect x="3" y="17" width="5" height="10" rx="1.2" fill="#8056ff"/><rect x="12" y="11" width="5" height="16" rx="1.2" fill="#a274ff"/><rect x="21" y="4" width="5" height="23" rx="1.2" fill="#be89ff"/><path d="M4 18h3m6-6h3m6-7h3" stroke="#ecd8ff" stroke-opacity=".65" stroke-width=".7"/>',
  '<circle cx="16" cy="16" r="10.5" fill="none" stroke="#174b75" stroke-width="4"/><path d="M16 5.5a10.5 10.5 0 1 1-9.8 6.7" fill="none" stroke="#04b8ff" stroke-width="4" stroke-linecap="round"/><path d="M16 2v6" stroke="#69e2ff" stroke-width="2" stroke-linecap="round"/><path d="M22 8a10.5 10.5 0 0 1 3.8 4" fill="none" stroke="#66e7ff" stroke-width="1" stroke-linecap="round"/>',
  '<path d="M27 4C11 2 3 10 6 21c8 6 21-1 21-17" fill="#00d8a4"/><path d="M7 26 22 10M12 20l-.5-6m.5 6 6-.5" fill="none" stroke="#064c4e" stroke-width="1.8" stroke-linecap="round"/><path d="M11 10c4-3 8-4 12-4" fill="none" stroke="#60ffd2" stroke-opacity=".65" stroke-linecap="round"/>',
  '<path d="M9 8h14c3 0 5 4 6 10l.5 4c.4 5-4.5 7-8-1h-11c-3.5 8-8.4 6-8-1l.5-3C4 12 6 8 9 8" fill="#f63675"/><path d="M8 15h6m-3-3v6" stroke="#661d45" stroke-width="2.2" stroke-linecap="round"/><circle cx="21" cy="14" r="1.6" fill="#651d46"/><circle cx="24" cy="18" r="1.6" fill="#651d46"/><path d="M9 9h13" stroke="#ff9ec5" stroke-width=".8" stroke-linecap="round"/>',
]

export function MetricGlyph({ index }: { index: number }) {
  return <svg className="metric-symbol" viewBox="0 0 32 32" fill="none" aria-hidden="true" dangerouslySetInnerHTML={{ __html: metricGlyphs[index % 4] }} />
}

/* --- goal emblems --- */

function goalGlyph(type: 'project' | 'limit' | 'focus'): string {
  if (type === 'project')
    return '<path d="M11 6V3h10v3M5 9h22v18H5z" fill="#0b9df3"/><path d="M10 6h12v4H10z" fill="#52d4ff"/><path d="M5 16c6 4 16 4 22 0" fill="none" stroke="#075795" stroke-width="2"/><rect x="13" y="14" width="6" height="6" rx="1" fill="#9ceeff"/>'
  if (type === 'limit')
    return '<circle cx="16" cy="16" r="11" fill="none" stroke="#ff4287" stroke-width="3"/><path d="m8 24 16-16" stroke="#ff4287" stroke-width="3" stroke-linecap="round"/>'
  return '<path d="M13 3h6M16 3v5m7 0 3-3" fill="none" stroke="#c677ff" stroke-width="2.6" stroke-linecap="round"/><circle cx="16" cy="19" r="10" fill="#7c33d32b" stroke="#ad5cff" stroke-width="2.7"/><path d="M16 12v7l4 2" fill="none" stroke="#d99aff" stroke-width="2.4" stroke-linecap="round"/>'
}

export function GoalGlyph({ type }: { type: 'project' | 'limit' | 'focus' }) {
  return <svg className="goal-symbol" viewBox="0 0 32 32" fill="none" aria-hidden="true" dangerouslySetInnerHTML={{ __html: goalGlyph(type) }} />
}
