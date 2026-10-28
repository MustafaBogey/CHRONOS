# Third-Party Notices

CHRONOS is MIT-licensed software that builds on excellent open-source
projects. This file summarizes the third-party material included in, or
produced by, this repository so redistribution requirements are met. Each
project retains its own license and copyright; the lists below name the
direct dependencies most visible in the shipped application and build.

## Bundled application artwork

- `public/overview-art.webp`, the illustration used across CHRONOS pages, and
  the application icons were created for CHRONOS with AI image-generation
  tools (OpenAI ChatGPT) commissioned by the project owner. They are not
  third-party works; the project owner holds the copyright and distributes
  them with the application under this repository's license.

## Rust core (`src-tauri`)

All direct Rust dependencies are MIT and/or Apache-2.0 / MIT-licensed,
including: `tauri` v2 and the Tauri plugins used by CHRONOS
(single-instance, dialog, notification, autostart, opener), `serde` /
`serde_json`, `rusqlite` (bundling SQLite, public domain / MIT-licensed
code), `chrono`, `parking_lot`, `log`, `png`, and the `windows` crate.
Exact versions and the full transitive list are recorded in
`src-tauri/Cargo.lock`.

## Frontend (`src/`, `tests/`)

All direct npm dependencies are MIT-licensed (TypeScript itself is
Apache-2.0), including: `react`, `react-dom`, `vite`, `typescript`,
`vitest`, and `@testing-library/*`. The full list is recorded in
`package-lock.json`.

## Fonts

CHRONOS uses the `Inter` font family when installed on the user's system and
falls back to system fonts (`Segoe UI`, `system-ui`, …). No font files are
bundled or redistributed by this repository.

## Installer

The Windows installer is produced by Tauri's NSIS bundling (NSIS is
zlib/libpng/MIT-licensed) and relies on Microsoft's WebView2 Runtime, which
is governed by Microsoft's applicable runtime terms when installed by the
bootstrapper.

If you believe any attribution here is missing or incorrect, please open an
issue so it can be corrected.
