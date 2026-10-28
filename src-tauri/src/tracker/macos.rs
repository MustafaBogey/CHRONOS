//! macOS observation adapter.
//!
//! - Frontmost application queried on the main thread via NSWorkspace (application identity
//!   needs no Accessibility permission; window titles require Accessibility and are left
//!   empty unless granted — the capability table reports this honestly).
//! - Idle detection via CoreGraphics CGEventSource (combined session state, any thread).
//!
//! NOTE: compiled only on macOS targets; requires a macOS environment to build and test.

use super::PlatformEvent;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::MainThreadMarker;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

const POLL_MS: u64 = 2000;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: i32) -> f64;
}

fn idle_ms() -> Option<u64> {
    unsafe {
        const K_CGEVENT_SOURCE_STATE_COMBINED_SESSION_STATES: i32 = 0;
        const K_CGANY_INPUT_EVENT_TYPE: i32 = -1;
        let s = CGEventSourceSecondsSinceLastEventType(K_CGEVENT_SOURCE_STATE_COMBINED_SESSION_STATES, K_CGANY_INPUT_EVENT_TYPE);
        if s < 0.0 { None } else { Some((s * 1000.0) as u64) }
    }
}

pub fn run(app: Option<AppHandle>, tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>) {
    let Some(app) = app else {
        let _ = tx.send(PlatformEvent::SourceLost("main thread unavailable".into()));
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }
        return;
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let tx_tick = tx.clone();
        let idle = idle_ms();
        let result = app.run_on_main_thread(move || {
            unsafe {
                let Some(mtm) = MainThreadMarker::new() else { return };
                let ws = NSWorkspace::sharedWorkspace(mtm);
                let Some(front) = ws.frontmostApplication() else { return };
                let name = front.localizedName().map(|s| s.to_string()).unwrap_or_default();
                let bundle = front.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
                let exe = if !name.is_empty() { name } else { bundle };
                if !exe.is_empty() {
                    let _ = tx_tick.send(PlatformEvent::Tick {
                        idle_ms: idle,
                        foreground: Some((exe.trim_end_matches(".app").to_string(), String::new(), None)),
                    });
                }
            }
        });
        if result.is_err() {
            let _ = tx.send(PlatformEvent::SourceLost("main-thread dispatch failed".into()));
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
}

/// Distraction discouragement is Windows-only in V1; other platforms no-op.
pub fn discourage_foreground() {}
