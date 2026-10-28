//! CHRONOS application entry: plugins, tray, lifecycle, notifications, integrations.

use chrono::{Local, Timelike};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

mod accounting;
mod app_icons;
mod backup;
pub mod classifier;
pub mod commands;
pub mod db;
pub mod grouping;
mod integrations;
mod mini;
pub mod models;
mod platform_metrics;
mod steam;
pub mod tracker;

use commands::AppState;

fn quiet_hours_active(start: i64, end: i64) -> bool {
    let hour = Local::now().hour() as i64;
    if start == end {
        return false;
    }
    if start < end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end // overnight wrap
    }
}

fn should_suppress(state: &AppState, essential: bool) -> bool {
    if essential {
        return false;
    }
    let conn = state.conn.lock();
    let dnd = conn.query_row("SELECT value FROM settings WHERE key='dnd'", [], |r| r.get::<_, String>(0)).map(|v| v == "1").unwrap_or(false);
    let quiet = conn.query_row("SELECT value FROM settings WHERE key='quiet'", [], |r| r.get::<_, String>(0)).map(|v| v == "1").unwrap_or(false);
    let qs: i64 = conn.query_row("SELECT value FROM settings WHERE key='quiet_start'", [], |r| r.get::<_, String>(0)).map(|v| v.parse::<i64>().unwrap_or(20)).unwrap_or(20);
    let qe: i64 = conn.query_row("SELECT value FROM settings WHERE key='quiet_end'", [], |r| r.get::<_, String>(0)).map(|v| v.parse::<i64>().unwrap_or(8)).unwrap_or(8);
    drop(conn);
    dnd || (quiet && quiet_hours_active(qs, qe))
}

pub fn show_notice(app: &AppHandle, kind: &str, title: &str, body: &str, essential: bool) {
    let state = app.state::<AppState>();
    if should_suppress(&state, essential) {
        return;
    }
    tracker::emit_event("notice", &serde_json::json!({ "type": kind, "title": title, "body": body }).to_string());
    // OS notification when the window is hidden or unfocused.
    use tauri_plugin_notification::NotificationExt;
    if let Some(win) = app.get_webview_window("main") {
        if !win.is_visible().unwrap_or(true) || !win.is_focused().unwrap_or(false) {
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}fn build_tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let tracking = state.tracker.tracking_info();
    let session = state.tracker.snapshot_session();
    let running = matches!(tracking.state, models::TrackingStateKind::Active | models::TrackingStateKind::Degraded);

    let status = MenuItemBuilder::with_id("status", format!("CHRONOS · {}", label_of(tracking.state))).enabled(false).build(app)?;
    let pause = MenuItemBuilder::with_id("tracking", if running { "Pause tracking" } else { "Resume tracking" }).build(app)?;
    let session_item = MenuItemBuilder::with_id(
        "session",
        match session.status.as_str() {
            "running" => "Pause focus session",
            "paused" => "Resume focus session",
            _ => "Start focus session",
        },
    )
    .build(app)?;
    let mini = MenuItemBuilder::with_id("mini", "Show Mini CHRONOS").build(app)?;
    let open = MenuItemBuilder::with_id("open", "Open dashboard").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit CHRONOS").build(app)?;
    MenuBuilder::new(app).items(&[&status, &pause, &session_item, &mini, &open, &quit]).build()
}

fn label_of(s: models::TrackingStateKind) -> &'static str {
    match s {
        models::TrackingStateKind::Active => "Tracking active",
        models::TrackingStateKind::Paused => "Paused",
        models::TrackingStateKind::Privacy => "Privacy mode",
        models::TrackingStateKind::Disabled => "Tracking disabled",
        models::TrackingStateKind::Degraded => "Tracking degraded",
        models::TrackingStateKind::Confirmation => "Awaiting confirmation",
    }
}

fn refresh_tray(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("chronos-tray") {
        if let Ok(menu) = build_tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init());

    builder = builder.setup(|app| {
        // CHRONOS_DATA_DIR (optional): isolated data directory for development and
        // integration testing. The installed application never sets it, so user
        // data resolution is unchanged for real usage.
        let app_data = std::env::var_os("CHRONOS_DATA_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| app.path().app_data_dir().ok().map(|p| p.join("chronos")))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&app_data);
        let db_path = app_data.join("chronos.db");
        let conn = db::open(&db_path).map_err(|e| format!("database open failed: {e}"))?;
        let conn = Arc::new(Mutex::new(conn));

        let handle = app.handle().clone();
        tracker::set_emitter(Arc::new(move |name: &str, payload: serde_json::Value| {
            let event = format!("chronos://{name}");
            let _ = handle.emit(&event, payload.clone());
            if name == "tracking-changed" {
                refresh_tray(&handle);
            }
            // Focus-session break notices also fire an OS notification when the
            // main window is not focused, so the user can act even when CHRONOS
            // is in the background. (The in-app actionable choice is the notice
            // itself; this is the attention nudge.)
            if name == "notice" {
                if let (Some(t), Some(b)) = (payload.get("title").and_then(|v| v.as_str()), payload.get("body").and_then(|v| v.as_str())) {
                    let ty = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if ty == "break-schedule" || ty == "break-end" {
                        if let Some(win) = handle.get_webview_window("main") {
                            if !win.is_focused().unwrap_or(false) {
                                use tauri_plugin_notification::NotificationExt;
                                let _ = handle.notification().builder().title(t).body(b).show();
                            }
                        }
                    }
                }
            }
        }));

        let tracker_handle = tracker::start(conn.clone(), app_data.clone(), Some(app.handle().clone()));
        app.manage(AppState {
            tracker: tracker_handle.clone(),
            conn: conn.clone(),
            app_data: app_data.clone(),
            last_break_ms: std::sync::atomic::AtomicI64::new(0),
        });

        // Initial tracking state application: if we restored into active observing, enter observing.
        {
            let g = tracker_handle.inner.lock();
            // state machine picks Observing on the first poll automatically.
            drop(g);
        }

        // Tray — a single registration created here, with an explicit icon and tooltip.
        // (The config-level `trayIcon` was removed: defining both made Windows show two
        // notification-area entries — the config one with an icon and a second blank
        // icon from this builder — the duplicate reported in UAT.)
        let menu = build_tray_menu(app.handle())?;
        let tray_icon = app
            .default_window_icon()
            .cloned()
            .ok_or("default window icon missing")?;
        TrayIconBuilder::with_id("chronos-tray")
            .icon(tray_icon)
            .tooltip("CHRONOS")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| {
                match event.id().as_ref() {
                    "tracking" => {
                        let state = app.state::<AppState>();
                        let info = state.tracker.tracking_info();
                        let next = if matches!(info.state, models::TrackingStateKind::Active | models::TrackingStateKind::Degraded) {
                            models::TrackingStateKind::Paused
                        } else {
                            models::TrackingStateKind::Active
                        };
                        tracker::set_tracking_state(&state.tracker, next);
                    }
                    "session" => {
                        let state = app.state::<AppState>();
                        let status = state.tracker.snapshot_session().status;
                        if status == "ended" {
                            let _ = tracker::session_start(&state.tracker, "Deep work".into(), String::new(), 25, 5, true);
                        } else {
                            let _ = tracker::session_toggle(&state.tracker);
                        }
                    }
                    "mini" => {
                        // Enable the preference and show the companion window.
                        {
                            let state = app.state::<AppState>();
                            let conn = state.conn.lock();
                            let _ = conn.execute(
                                "INSERT INTO settings (key, value) VALUES ('mini_bar', '1') ON CONFLICT(key) DO UPDATE SET value='1'",
                                [],
                            );
                        }
                        let _ = mini::show(app);
                        let _ = app.emit("chronos://state-refresh", true);
                    }
                    "open" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => {
                        commands::app_quit(app.clone());
                    }
                    _ => {}
                }
                refresh_tray(app);
            })
            .on_tray_icon_event(|tray, event| {
                if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                    let app = tray.app_handle().clone();
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            })
            .build(app)?;

        // Close-to-tray behavior.
        let window = app.get_webview_window("main").expect("main window");
        {
            let app_handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let state = app_handle.state::<AppState>();
                    let tray_pref = {
                        let conn = state.conn.lock();
                        conn.query_row("SELECT value FROM settings WHERE key='tray'", [], |r| r.get::<_, String>(0))
                            .map(|v| v == "1")
                            .unwrap_or(true)
                    };
                    if tray_pref {
                        api.prevent_close();
                        let _ = app_handle.get_webview_window("main").map(|w| w.hide());
                    }
                }
            });
        }

        // Onboarding on first run.
        {
            let state = app.state::<AppState>();
            let conn = state.conn.lock();
            let onboarded = conn
                .query_row("SELECT value FROM settings WHERE key='onboarded'", [], |r| r.get::<_, String>(0))
                .map(|v| v == "1")
                .unwrap_or(false);
            drop(conn);
            if !onboarded {
                tracker::emit_event("notice", &serde_json::json!({
                    "type": "degraded", "title": "Welcome to CHRONOS",
                    "body": "A short, private introduction is ready whenever you are. Tracking stays off until you choose it."
                }).to_string());
            }
        }

        // Periodic housekeeping: scheduled backups and gentle break reminders.
        {
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick: u64 = 0;
                loop {
                    tokio_compat_sleep(30).await;
                    tick += 1;
                    let state = app_handle.state::<AppState>();
                    if state.tracker.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if tick % 20 == 0 {
                        backup::run_scheduled_backup_if_due(&state);
                        // Functional retention: one sweep per housekeeping cycle when
                        // the user has explicitly activated automatic deletion.
                        let _ = commands::retention_sweep(&state);
                    }
                    // Gentle break reminder: continuous active work, no session, reminders on.
                    let now = db::now_ms();
                    let (breaks_on, every_minutes, dnd) = {
                        let conn = state.conn.lock();
                        let breaks_on = conn
                            .query_row("SELECT value FROM settings WHERE key='breaks'", [], |r| r.get::<_, String>(0))
                            .map(|v| v == "1")
                            .unwrap_or(false);
                        let every: i64 = conn
                            .query_row("SELECT value FROM settings WHERE key='break_every_minutes'", [], |r| r.get::<_, String>(0))
                            .map(|v| v.parse::<i64>().unwrap_or(50))
                            .unwrap_or(50);
                        let dnd = conn
                            .query_row("SELECT value FROM settings WHERE key='dnd'", [], |r| r.get::<_, String>(0))
                            .map(|v| v == "1")
                            .unwrap_or(false);
                        (breaks_on, every, dnd)
                    };
                    if breaks_on && !dnd && every_minutes > 0 {
                        let tracking = state.tracker.tracking_info();
                        let session = state.tracker.snapshot_session();
                        let idle_ms = state.tracker.idle_ms_estimate();
                        let active_work = matches!(tracking.state, models::TrackingStateKind::Active)
                            && session.status != "running"
                            && idle_ms.map(|i| i < 60_000).unwrap_or(false);
                        let last = state.last_break_ms.load(Ordering::Relaxed);
                        if active_work && now - last > every_minutes * 60_000 {
                            state.last_break_ms.store(now, Ordering::Relaxed);
                            show_notice(
                                &app_handle,
                                "break",
                                "A moment to reset.",
                                "You've made space for good work. Stretch, rest your eyes, or take a quiet breath.",
                                false,
                            );
                        }
                    }
                }
            });
        }

        // Session-complete and break notices are emitted from command handlers; see commands.rs.
        // Browser integration endpoint (optional).
        {
            let state = app.state::<AppState>();
            let mut conn = state.conn.lock();
            // Pairing token for the optional browser extension. Persisted on first
            // generation so the value shown in Settings stays valid across restarts.
            let token: String = conn
                .query_row("SELECT value FROM settings WHERE key='ext_token'", [], |r| r.get(0))
                .unwrap_or_else(|_| {
                    use rand_like::Token;
                    let t = Token::new().0;
                    let _ = conn.execute(
                        "INSERT INTO settings (key, value) VALUES ('ext_token', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                        [&t],
                    );
                    t
                });
            drop(conn);
            integrations::start(state.tracker.clone(), state.conn.clone(), token);
        }

        // Mini CHRONOS companion: the window is always pre-created at startup —
        // visible immediately when the preference is enabled, hidden otherwise.
        // Creating a second webview only at startup (never mid-session) avoids a
        // Windows/WebView2 defect where building a webview while the main window
        // is fully running breaks the main webview's IPC (dead toggles); showing
        // or hiding the existing window is always safe.
        {
            let state = app.state::<AppState>();
            let enabled = {
                let conn = state.conn.lock();
                conn.query_row("SELECT value FROM settings WHERE key='mini_bar'", [], |r| r.get::<_, String>(0))
                    .map(|v| v == "1")
                    .unwrap_or(false)
            };
            let precreate = if enabled { mini::show } else { mini::precreate };
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                let _ = precreate(&handle);
            });
        }

        Ok(())
    });

    let app = builder
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::get_capabilities,
            commands::get_app_version,
            commands::browser_integration_info,
            commands::browser_extension_dir,
            commands::open_extension_folder,
            commands::get_diagnostics,
            commands::get_overview,
            commands::get_timeline,
            commands::get_report,
            commands::get_focus_history,
            commands::get_projects_with_time,
            commands::correct_record,
            commands::split_record,
            commands::undo_last,
            commands::add_rule,
            commands::apply_rule_retroactive,
            commands::add_exclusion,
            commands::remove_exclusion,
            commands::delete_history,
            commands::session_start,
            commands::session_resume_focus,
            commands::session_extend_break,
            commands::session_toggle,
            commands::session_end,
            commands::set_goals,
            commands::project_create,
            commands::project_rename,
            commands::project_set_archived,
            commands::project_delete,
            commands::bulk_assign,
            commands::manual_entry_add,
            commands::manual_entry_update,
            commands::manual_entry_delete,
            commands::get_work_blocks,
            commands::group_break_add,
            commands::group_break_remove,
            commands::get_idle_exceptions,
            commands::idle_exception_add,
            commands::idle_exception_remove,
            commands::get_block_list,
            commands::block_list_add,
            commands::block_list_remove,
            commands::block_list_set_enabled,
            commands::retention_status,
            commands::retention_set_active,
            commands::retention_run_now,
            commands::tracking_pause,
            commands::tracking_resume,
            commands::tracking_enable,
            commands::tracking_disable,
            commands::tracking_simulate_unknown,
            commands::set_privacy,
            commands::set_dnd,
            commands::update_settings,
            commands::complete_onboarding,
            commands::get_app_icons,
            commands::get_current_activity,
            commands::mini_show,
            commands::mini_hide,
            commands::mini_open_main,
            commands::mini_save_position,
            backup::backup_create,
            backup::backup_inspect,
            backup::backup_restore,
            backup::export_data,
            commands::app_quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building CHRONOS");

    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            let state = app_handle.state::<AppState>();
            state.tracker.stop.store(true, Ordering::Relaxed);
            // Allow the service loop to write the clean-shutdown marker and close open intervals.
            std::thread::sleep(std::time::Duration::from_millis(700));
            let _ = code;
            let _ = api;
        }
        RunEvent::Exit => {
            let state = app_handle.state::<AppState>();
            state.tracker.stop.store(true, Ordering::Relaxed);
        }
        _ => {}
    });
}

async fn tokio_compat_sleep(secs: u64) {
    #[cfg(not(feature = "tokio"))]
    {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    #[cfg(feature = "tokio")]
    {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    }
}

mod rand_like {
    pub struct Token(pub String);
    impl Token {
        pub fn new() -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let pid = std::process::id();
            let raw = format!("{ms:x}{pid:x}");
            let mut out = String::new();
            for (i, c) in raw.chars().enumerate() {
                if i > 0 && i % 8 == 0 && out.len() < 32 {
                    out.push('-');
                }
                if out.len() < 35 {
                    out.push(c);
                }
            }
            Token(out)
        }
    }
}

#[allow(unused)]
fn unused_conn(_: Connection) {}
