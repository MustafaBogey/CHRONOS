# CHRONOS — Tester Guide

A concise handbook for hands-on testing by additional users. Windows first;
macOS and Linux builds are not yet produced (see Limitations).

## 1. Install

1. Run `CHRONOS-Windows-Setup.exe` (per-machine install to
   `C:\Program Files\CHRONOS`). An existing 1.0/1.1 install upgrades in place.
2. Launch CHRONOS. The short introduction explains the model; tracking starts
   only after you choose to enable it (consent is explicit).

## 2. Confirm tracking is active

- Sidebar box (bottom-left) shows **Tracking active**.
- System tray icon → right-click → status line; the menu can pause/resume.
- Open anything for ~10 seconds, then Timeline → Today: an interval appears.

## 3. Enable the browser extension (website attribution + blocking)

1. Settings & Integrations → **Connect** → pick your browser → follow the
   load steps (Developer mode → Load unpacked on Chromium browsers;
   Temporary Add-on on Firefox/Zen).
2. The pairing page pairs automatically — no token copying.
3. Status turns **Communicating** when you browse; Timeline then shows
   "Chrome/Edge · SiteName" rows.

Note after upgrades: when the extension gains permissions, the browser
disables it until you re-enable it once on the extensions page
(`chrome://extensions` / `edge://extensions`). That is normal browser
behavior, not a defect.

## 4. Exclusions vs. blocking — the one distinction that matters

| | Exclusions | Distraction blocking |
|---|---|---|
| Purpose | Privacy: stop *recording* | Attention: discourage *access* |
| Where | Settings → Tracking & Classification | Settings → Focus & Notifications |
| Effect on the app/site | Fully usable, simply not tracked | Websites redirect to a block page; Windows apps get minimized |
| History | Never touched | Never touched |
| Lists | Independent — nothing is copied between them | Independent |

Exclusions accept any form (`youtube.com`, `www.youtube.com`, or a full URL)
and cover subdomains; they can be added **and removed** in the manager.

## 5. Find the version

Settings → **Help & About** → "Version x.y.z" (read from the installed
application itself, so it is always accurate). The same section holds short
explanations of tracking, classifications, projects/work blocks, focus
sessions, privacy, and troubleshooting.

## 6. Known platform limitations (honest)

- Application blocking (minimize) is Windows-only in this release.
- Linux: X11 is supported; Wayland compositors without a foreground-window
  protocol are honestly reported as unsupported. Linux/macOS packages are
  **not yet built** — do not assume availability.
- Website blocking requires the extension; without it, website entries wait
  quietly (the UI says so — it never claims enforcement that is not real).
- Blocking is discouragement: it does not prevent launching a process.

## 7. Reporting problems

Report with: CHRONOS version (Help & About), OS, what you did, what you
expected, what happened, and — for tracking issues — whether the tray status
showed active/paused/degraded at the time. Reproduction steps matter more
than screenshots. Keep in mind: gaps in the Timeline are honest records of
unobserved time (paused, asleep, tracking off), not lost data.

## 8. If website blocking is not enforced (1.1.2+)

CHRONOS shows "Enforced" only when a browser extension confirms the rules -
never from the toggle alone. If status says configured-but-unconfirmed:

1. Check the browser's extensions page. A broken/errored CHRONOS extension
   (e.g. loaded from a deleted folder) must be removed.
2. Load the current extension: Settings -> Integrations -> Connect, follow
   the browser steps (Chrome: chrome://extensions -> Developer mode ->
   Load unpacked; the command-line shortcut does not work in branded
   Chrome).
3. If the browser shows the extension was disabled after an update,
   re-enable it once.
4. Browse briefly; status should change to "Enforced - the browser
   extension confirmed...".

Blocking can also be verified in seconds: add a harmless site (example.com),
enable blocking, and open it - the CHRONOS block page should appear within
about a minute.
