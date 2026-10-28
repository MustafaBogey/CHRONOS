//! Mini CHRONOS — optional slim floating companion window.
//!
//! One extra webview window ("mini") sharing the existing app process, state, and
//! tracking engine. Created lazily, hidden (never destroyed) on close/hide so tray
//! icons and webviews never accumulate; position persists in the settings table.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MINI_LABEL: &str = "mini";
const DEFAULT_W: f64 = 318.0;
const DEFAULT_H: f64 = 46.0;

fn saved_position(app: &AppHandle) -> Option<(f64, f64)> {
    let state = app.state::<crate::commands::AppState>();
    let conn = state.conn.lock();
    let x: Option<f64> = conn
        .query_row("SELECT value FROM settings WHERE key='mini_x'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok());
    let y: Option<f64> = conn
        .query_row("SELECT value FROM settings WHERE key='mini_y'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok());
    drop(conn);
    match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }
}

fn default_position(app: &AppHandle) -> (f64, f64) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let size = monitor.size();
        let sf = monitor.scale_factor();
        let w = size.width as f64 / sf;
        (w - DEFAULT_W - 24.0, 16.0)
    } else {
        (760.0, 16.0)
    }
}

/// Create-or-show the mini window. Idempotent: one window, one webview, ever.
///
/// The webview itself is only ever created at application startup (see
/// `precreate`) — creating a second webview window while the main window is
/// fully running breaks the main webview's IPC bridge on Windows (reproduced:
/// after a mid-session creation, invoke replies and events never arrive, so
/// every settings toggle appears dead). Showing an already-existing window is
/// always safe; `show` therefore only falls back to building the window if
/// startup pre-creation did not run (dev/edge cases).
pub fn show(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(MINI_LABEL) {
        let _ = win.set_always_on_top(true); // re-assert (fullscreen mode may have lowered it)
        let _ = win.show();
        return Ok(());
    }
    build(app, true)
}

/// Build the mini window at startup, hidden until the user enables it.
/// Creating it here — before the main webview completes its own setup — is the
/// proven-safe path; a hidden webview costs little (timers throttle while
/// hidden) and makes the first enable instant.
pub fn precreate(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(MINI_LABEL).is_some() {
        return Ok(());
    }
    build(app, false)
}

fn build(app: &AppHandle, visible: bool) -> Result<(), String> {
    let (x, y) = saved_position(app).unwrap_or_else(|| default_position(app));
    // App root (like the main window): an explicit "index.html" path proved flaky on
    // Windows release builds — the webview could stay at about:blank after creation.
    let builder = WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App(std::path::PathBuf::new()))
        .title("Mini CHRONOS")
        .inner_size(DEFAULT_W, DEFAULT_H)
        .position(x, y)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .visible(visible)
        .focused(false);
    let win = builder.build().map_err(|e| e.to_string())?;
    let w = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            // Never destroy: hide instead so reopening is instant and nothing accumulates.
            api.prevent_close();
            let _ = w.hide();
        }
    });
    Ok(())
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MINI_LABEL) {
        let _ = win.hide();
    }
}

/// Lower the mini bar below a fullscreen application (and restore it afterwards).
/// Called from the Windows platform tick on fullscreen-state changes only.
pub fn set_below_fullscreen(app: &AppHandle, fullscreen: bool) {
    if let Some(win) = app.get_webview_window(MINI_LABEL) {
        let _ = win.set_always_on_top(!fullscreen);
    }
}
