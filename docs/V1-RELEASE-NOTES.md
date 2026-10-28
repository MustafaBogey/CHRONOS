# CHRONOS V1 (1.1.0) - Release Notes

## Upgrade and migration

- Upgrading from 1.0.0 is silent and in-place (same installer). The database
  migrates automatically on first launch: schema v4 -> v5, additive only
  (a `manual` flag on intervals, new `idle_exceptions`, `group_breaks`, and
  `block_list` tables). No records are rewritten or deleted. The migration was
  validated against a copy of a real 1.0.0 database before release, and
  integrity-checked after the live upgrade.
- If CHRONOS is running during upgrade, the installer closes it; the next
  launch shows the honest recovery confirmation and one click resumes tracking.

## What is new

- Tracking schedules: an optional observation window with overnight support;
  outside it CHRONOS writes honest gaps instead of activity. Manual Pause and
  Privacy always take precedence.
- Idle exceptions: apps you name (meetings, presentations, media) are assumed
  engaged without input - your explicit assumption, never claimed as observed.
- Work blocks: read-time grouping of contiguous related activity (7-minute
  chaining, project-change splits, reversible user boundaries) in Timeline and
  Reports. Records are never merged or double-counted.
- Manual time entries: record real unobserved time with a strict no-double-
  counting overlap policy; entries are always marked Manual.
- Projects: bulk activity assignment, permanent deletion with type-to-confirm
  (intervals preserved, references cleared), intention editing, and a
  selectable project for the project-time goal everywhere.
- Distraction blocking (real): websites redirect to a calm block page in
  browsers with the extension; blocked Windows applications are minimized.
  Honest per-target status; always reversible.
- Retention: explicit activation with confirmation; sweeps run automatically
  once activated. A stored preference alone never deletes anything.
- Intelligence: ambiguous services (AI assistants, discussion platforms) are
  marked for review instead of silently judged; learning is conflict-aware;
  multipurpose apps are never forced into one project.
- AI: honestly labeled "Not available in V1".

## Browser extension 1.2.0

- Adds optional website blocking via declarativeNetRequest (block list synced
  from the desktop app over loopback).
- New permissions: `declarativeNetRequest`, `<all_urls>` host permissions.
  Existing unpacked installs must be re-enabled once in chrome://extensions
  after upgrading (Chrome disables extensions whose permissions grew).
- Firefox requires 128+.

## Known limitations (honest)

- Application blocking (minimize) is Windows-only in V1.
- Linux: X11 tracking supported; Wayland compositors without a foreground
  protocol are honestly reported unsupported. Not runtime-tested in this
  release - see the V1 report.

## 1.1.1 (correction release)

- Exclusions: URL-shaped input now normalizes to the domain (the stored
  https://www.youtube.com/ style entries match from now on); full add/remove
  management in the dialog; counts refresh immediately.
- Distraction blocking: honest status everywhere (off / on-empty /
  on-configured with counts), live extension status, per-entry pause,
  block-list domains normalize like exclusions.
- Extension 1.2.1: alarm-based block-list sync (reliable across service-worker
  suspension) plus catch-up on tab changes. New "alarms" permission - unpacked
  installs are disabled by the browser once after upgrade; re-enable them.
- Help & About section in Settings with the authoritative installed version.
- Modal dialogs: interior clicks never dismiss; only backdrop, X, Cancel, Esc.

## 1.1.2 (blocking fix + Timeline UX)

- Website blocking: extensions acknowledge every sync with the applied rule
  count, and CHRONOS reports "Enforced" only from a fresh matching
  acknowledgment - configured targets without confirmation are labeled
  exactly that, with recovery guidance. Full-domain requirement for website
  targets (twitch.tv, not twitch).
- Timeline: newest activity first (rows, groups, filters, search), groups
  Evening -> Afternoon -> Morning, "Blocks" renamed "Work Sessions", and
  sessions render as collapsed expandable summary cards. Fixed the inverted
  refresh condition that made the session view show only the plain timeline.
- Known browser notes: branded Google Chrome ignores command-line extension
  loading, so load the extension via chrome://extensions -> Load unpacked
  (Edge also supports automatic guided loading). After any extension update
  that adds permissions, browsers disable it once until re-enabled.

## 1.1.3 (Overview layout fix)

- Overview no longer scrolls far beyond its content: the unified artwork
  band is now height-bounded (natural scale, top-anchored, same pixels in
  the hero region) instead of growing with window width, which had made an
  absolutely-positioned box as tall as window-width/1.6 create a huge
  artwork-only scrollable area.
- The unintended horizontal scrollbar is fixed: the closed modal <dialog>
  no longer renders as a full-viewport box (author CSS had overridden the
  browser's closed-dialog display:none), and the open dialog now sizes
  with inset:0 instead of 100vw so it never includes the scrollbar width.
- Artwork, theme, and all other screens unchanged.

## 1.1.4 (fresh-install toggle fix)

- External tester report: on a fresh installation every settings toggle
  appeared dead. Reproduced and root-caused on a clean sandbox profile:
  creating the Mini CHRONOS webview while the main window is fully
  running breaks the main webview's IPC bridge on Windows - invoke
  replies and events stop arriving, so toggles (and any data refresh)
  freeze while pure-frontend buttons keep working. A fresh install hits
  this on the first Mini toggle click; established installations were
  unaffected because their mini window opens during startup.
- Fix: the Mini CHRONOS window is now always pre-created at application
  startup (hidden when disabled). The toggle - and the tray menu - only
  show or hide an existing window; a webview is never created
  mid-session. Verified on a fresh sandbox: every toggle responds
  visually, persists across restarts, and page data keeps loading after
  the mini is shown; verified on the upgraded installation as well.

## 1.2.0 (unified artwork, Total Activity + Daily Timeline, live tracking)

- Unified artwork: the established full-width "Neon Night Workspace
  Gradient" band now spans every primary page (Overview, Timeline, Focus
  & Goals, Reports, Settings), extending behind the sidebar. The 1.1.3
  containment corrections (bounded band height, closed-dialog
  display:none) are preserved - verified no artwork-only overflow and no
  horizontal overflow across five viewports on all five pages.
- Breadcrumbs ("Personal workspace / ...") removed from the topbar on
  every page.
- Timeline redesigned around two views (replacing Group and Work
  Sessions):
  - Total Activity (default): one expandable entry per application with
    total observed duration, interval count, honest Mixed labels for
    ambiguous classification/project, plus separate Idle and Tracking
    gaps entries that are never credited to an application. Expanding
    reveals the individual recorded intervals (newest first) with all
    correction workflows intact.
  - Daily Timeline: the chronological sequence, newest first, with the
    live ongoing interval at the top when viewing today.
- Genuine live tracking: the engine now emits activity-changed when an
  ordinary interval closes (the missing notification fixed) and a new
  current-activity event on every transition (begin, close, idle
  start/end, pause/privacy/sleep). Overview shows a "Right now" live
  card - current app, classification, start time, and a per-second
  ticking elapsed duration computed locally from the authoritative
  start (no database writes per second). Returning from idle with the
  same foreground app now re-opens its interval (a pre-existing engine
  gap) so resumed activity is recorded and shown live.
- Mini CHRONOS IPC correction (1.1.4) preserved and re-verified:
  toggles respond before and after mini interactions; Overview data
  loads after toggling.

88 Rust + 60 frontend tests green; tsc, production build, installed
Windows app, and the layout matrix (5 viewports x 5 pages) verified.

## 1.3.0 (Timeline controls fix, Reports label, website cleanup, Focus Session rework)

- Timeline toolbar fix: the unified artwork band's stacking context
  (position:relative + isolation:isolate on .page-head) painted above the
  statically-positioned toolbar. Fixed by adding position:relative;
  z-index:1 to .toolbar, .info-strip, and .tabs. All controls (date
  navigation, search, filter, Total Activity / Daily Timeline views,
  Review, Add time) are now visible on the installed app.
- Reports tab label corrected: "Focus & daily detail" -> "Focus & Daily
  Detail".
- Website reporting: browser-internal destinations (newtab, extensions,
  32-char extension-origin identifiers, letterless fragments like "0.1")
  are excluded from the Websites report. Real domains and catalog service
  labels are preserved. Applications report unchanged.
- Focus Session rework:
  - Setup dialog with focus duration (20/25/30/45 min), break duration
    (5/10/15 min), and a breaks toggle (default 25/5 with breaks).
  - Automatic focus-to-break transitions with notifications.
  - Break expiry prompts the user with Resume Focus / Another 5 Minutes -
    never auto-resumes.
  - Repeated extensions without limit, each recorded.
  - Honest break accounting: planned allowance vs requested extensions
    vs actual wall-clock duration vs unplanned overrun, all from
    timestamps.
  - Session summary and history retain break records for later review.
  - Break time never counts as qualified focus; ordinary tracking
    continues independently during breaks.
  - Pause freezes both the session timer and the phase countdown.
  - Schema v6 adds focus/break configuration and accounting columns to
    focus_sessions (additive migration).

91 Rust + 60 frontend tests; tsc, production build, and installed
Windows app verified (controls visible, data intact, Mini CHRONOS
stable).

## 1.3.1 (Focus Session idle pause, Settings nav visibility, packaged phase-UI serialization fix)

- Focus Session idle pause: when the tracking engine detects idle, the focus
  countdown now freezes at the authoritative last-input moment (not detection
  time, and never before the interval began) and stays frozen after the user
  returns. The session itself stays running and recoverable; ordinary idle
  rows continue to be recorded; no focus time is fabricated and no completed
  time is lost. The countdown continues only on an explicit Resume Focus
  (same interval, preserved remaining time) or a manual pause/resume. While
  idle-paused the focus interval can never expire or trigger a scheduled
  break. A tracker outage (>15 min missed ticks) freezes the countdown the
  same way. Already-started scheduled breaks keep their existing duration,
  expiry, extension, and overrun accounting during idle; break expiry still
  waits for the user's explicit choice. The UI states the reason: pill
  "Focus paused - away", phase note with the preserved remaining time, an
  info strip, and a Resume Focus action (the manual Pause toggle is replaced
  while idle-paused).
- Manual pause phase freeze corrected: the 1.3.0 shift-at-pause refunded the
  entire run segment, so repeated pause/resume cycles inside one focus
  interval restored time that had already been consumed. The freeze is now
  applied at resume as the actual pause duration; countdowns are preserved
  exactly across cycles.
- Settings & Integrations nav visibility: the statically-positioned
  .settings-layout painted below the unified artwork band (same stacking
  mechanism as the 1.3.0 Timeline toolbar fix), leaving the selected General
  item illegible until hover (a hover filter promotes the button above the
  band). Fixed by adding .settings-layout to the existing
  position:relative; z-index:1 rule. Verified on the installed app with the
  cursor parked away: General and every section render legibly with their
  selected styling; switching sections works.
- Packaged-app phase UI fix: SessionSnapshot serialized snake_case
  (phase_remaining_ms, break_expired) while the frontend reads camelCase, so
  the 1.3.0 phase countdown, break-expiry actions, and break notes never
  rendered from the real backend (the dev mock masked this). SessionSnapshot
  now serializes camelCase; BreakRecord does too, with snake_case
  deserialize aliases so 1.3.0-stored break history keeps parsing. Verified
  end-to-end on the installed app ("Focus - 25m to break" renders from the
  live engine).
- Test repair (disclosed): a Daily Timeline test compared the mock's live
  row (real wall clock) against the fixed sample day, so it failed before
  ~17:00 local. Ordering is now asserted on closed rows with the live row's
  lead position checked separately.

95 Rust + 63 frontend tests; tsc, production build, silent upgrade
(integrity ok, all 554 intervals and 4 real sessions preserved), and
installed-app verification (Settings no-hover, section switching, Timeline,
focus session start/end flow with live phase rendering).

## 1.3.2 (Focus Session dial shows the phase countdown; no text over the ring)

- The circular Focus Session dial now displays the time relevant to the
  current phase, never the overall session elapsed time: remaining focus
  countdown while focusing (and while idle-paused or manually paused - the
  frozen remaining value), remaining break countdown while on a scheduled
  break, and the overdue duration (prefixed "+") labeled "Break overdue"
  after a break expires. A short phase label sits beneath the countdown
  inside the ring ("Focus remaining" / "Break remaining" / "Break overdue").
  Sessions with scheduled breaks disabled - and ended sessions - have no
  phase countdown, so the dial falls back to the elapsed session time,
  clearly labeled "Elapsed". Overall elapsed time remains available in the
  Focus & Goals sessions tab and the quality stats.
- Engine support (presentation only, no accounting change): the session
  snapshot now exposes the frozen phase countdown while manually paused,
  anchored at the stored pause moment; the value does not drift while
  paused and the resumed countdown continues from it (covered by a new
  engine test). All state-machine, idle-pause, and break-accounting logic
  is untouched.
- Dial layout: the intention caption and the detailed phase explanations
  moved below the circle (new .session-orbit-col); only the countdown and
  the short phase label remain inside the ring, so nothing can overlap the
  circular border. The idle-pause info strip was simplified to the agreed
  wording ("You were away. Resume when you're ready."). Ring, theme, and
  card layout unchanged.
- Verified on the installed app (real engine): running 25:00 counting
  down; manual pause frozen (pill "Paused", dial unchanged, Resume/End
  buttons); resume continuing 1:1 from the frozen value; ended session
  showing "Elapsed"; maximized and 1000x700 windows with no text crossing
  the ring border and no clipping. 96 Rust + 71 frontend tests.

## 1.3.3 (Focus Session break statistics + contextual help)

- Break statistics in the Focus Session card (beneath Focus Quality):
  total break time (actual wall-clock break duration including the running
  break, extensions, and overruns), breaks taken (scheduled breaks started,
  completed + running), and break overrun (time beyond allowances plus
  requested extensions, live while a break runs over). Values update every
  session tick, show clear zeros before the first break, never double-count
  completed and ongoing breaks, and never count idle pauses as breaks. The
  engine exposes them as derived snapshot fields (breakCount,
  breakTotalMin, breakOverrunTotalMin) computed from the existing 1.3.0
  accounting - no new tracking, tables, or accounting changes.
- Discreet "?" contextual help on five Focus Session concepts (Focus
  Session incl. automatic idle pause, Focus quality, Interruptions, Total
  break time, Break overrun): hover, keyboard focus, or click toggles;
  Escape/blur closes; aria-expanded/role=tooltip for accessibility.
  Popovers beneath the heading and leftward for labels near the window's
  right side, so they never overflow the viewport (verified at 1000x700).
- Circular timer and Focus Quality display unchanged; no other UI changes.

97 Rust + 73 frontend tests; packaged app verified (maximized and
1000x700): zeros render from the live engine, help popovers readable and
contained, dial intact; data preserved.

## 1.3.4 (UI cleanup: help-popover positioning shipped, two elements removed)

- Ships the verified help-popover positioning correction (c7bb18d): metric-
  label popovers open to the RIGHT of their "?" icons with measured viewport
  placement, falling back to below/above/left, never overlapping the session
  dial when an in-view placement avoids it, re-measuring on resize/scroll/
  hover/focus, with slimmer popovers in compact tiers.
- Removed the "?" help icon beside "Total break time" (statistic, calculation,
  and presentation unchanged; the other four help icons unchanged).
- Removed the "Ready when you are" badge shown when no session runs; active-
  session indicators (In progress, Paused, On break, Break ended, Focus
  paused - away) are preserved.
- Presentation-only. 73 frontend tests, 97 Rust tests, tsc, and production
  build pass; installed app verified (idle without badge, running/paused
  states with indicators, popover right of its icon, dial unobstructed);
  data preserved (integrity ok).
