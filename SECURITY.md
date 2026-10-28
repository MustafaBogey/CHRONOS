# Security Policy

CHRONOS is a local-first application: your activity data stays on your
machine, and the project takes both security and privacy seriously.

## Reporting a vulnerability

Please use GitHub's **private vulnerability reporting** on this repository
(Report a vulnerability → Security Advisories) so reports stay private until
a fix is available. Include what you found, affected versions, and
reproduction steps if possible.

Please do not open public issues for suspected vulnerabilities.

## Scope

- The CHRONOS desktop application (`src/`, `src-tauri/`).
- The optional browser extension (`browser-extension/`,
  `browser-extension-firefox/`).
- Released installers published on the Releases page.

## Privacy expectations

- No accounts, no cloud sync, no telemetry.
- The only network listener is a localhost pairing endpoint used by the
  optional browser extension; it reports the active tab's domain to the
  local app and nothing else.
- Activity data lives in a local SQLite database you can inspect, export,
  back up, or delete at any time.
