//! Linux observation adapter.
//!
//! - Hyprland (Omarchy): event-driven foreground tracking through the compositor's IPC
//!   (`socket2.sock` for `activewindow>>` events, `socket.sock` for state queries).
//!   Reconnects with backoff when the compositor restarts.
//! - X11: `_NET_ACTIVE_WINDOW` polling every 2 seconds plus X screen-saver idle time.
//! - Other Wayland compositors: no foreground-window protocol is assumed; the tracker
//!   reports the limitation honestly instead of fabricating data.
//! - Sleep/wake on logind systems: wall-clock discontinuity gaps (service-layer watchdog).

use super::PlatformEvent;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

const POLL_MS: u64 = 2000;

pub fn run(_app: Option<tauri::AppHandle>, tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>) {
    if let Some(sock_dir) = hyprland_dir() {
        if let Some(event_path) = sock_dir.clone().join("socket2.sock").canonicalize().ok() {
            if let Some(request_path) = sock_dir.join(".socket.sock").canonicalize().ok() {
                run_hyprland(tx.clone(), stop.clone(), event_path, request_path);
            }
        }
    }
    if try_x11(tx.clone(), stop.clone()) {
        return;
    }
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        let _ = tx.send(PlatformEvent::SourceLost(
            "Wayland compositor without a foreground-window protocol (Hyprland not detected)".into(),
        ));
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }
    } else {
        let _ = tx.send(PlatformEvent::SourceLost("No desktop session detected".into()));
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn hyprland_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = PathBuf::from(std::env::var("XDG_RUNTIME_DIR").ok()?);
    let dir = runtime.join("hypr").join(&sig);
    dir.exists().then_some(dir)
}

fn run_hyprland(tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>, event_path: PathBuf, request_path: PathBuf) {
    // Initial state query.
    if let Some((class, title)) = hypr_query_active(&request_path) {
        let _ = tx.send(PlatformEvent::Foreground { exe: class, title, full_path: None });
    }
    let mut backoff = 1u64;
    while !stop.load(Ordering::Relaxed) {
        let Ok(stream) = UnixStream::connect(&event_path) else {
            let _ = tx.send(PlatformEvent::SourceLost("Hyprland event socket unavailable; retrying".into()));
            std::thread::sleep(Duration::from_secs(backoff.min(15)));
            backoff = (backoff * 2).min(15);
            // Retry initial query on reconnect.
            if let Some((class, title)) = hypr_query_active(&request_path) {
                let _ = tx.send(PlatformEvent::Foreground { exe: class, title, full_path: None });
                backoff = 1;
            }
            continue;
        };
        backoff = 1;
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let Ok(line) = line else { break }; // socket closed → reconnect
            if let Some(payload) = line.strip_prefix("activewindow>>") {
                if let Some((class, title)) = parse_activewindow(payload) {
                    let _ = tx.send(PlatformEvent::Foreground { exe: class, title, full_path: None });
                }
            }
        }
        // Compositor went away or restarted.
        let _ = tx.send(PlatformEvent::SourceLost("Hyprland compositor disconnected".into()));
        std::thread::sleep(Duration::from_secs(2));
        if let Some((class, title)) = hypr_query_active(&request_path) {
            let _ = tx.send(PlatformEvent::Foreground { exe: class, title, full_path: None });
        }
    }
}

fn parse_activewindow(payload: &str) -> Option<(String, String)> {
    // Format: "Class,Title"
    let (class, title) = payload.split_once(',')?;
    let class = class.trim();
    if class.is_empty() {
        return None;
    }
    Some((class.to_string(), title.trim().to_string()))
}

fn hypr_query_active(request_path: &PathBuf) -> Option<(String, String)> {
    let mut stream = UnixStream::connect(request_path).ok()?;
    stream.write_all(b"j/activewindow").ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_str(&buf).ok()?;
    let class = v.get("class")?.as_str()?.to_string();
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if class.is_empty() { None } else { Some((class, title)) }
}

fn try_x11(tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>) -> bool {
    // X11 path (only when DISPLAY exists and Hyprland is absent).
    let Ok((conn, screen)) = x11rb::connect() else { return false };
    let screen = conn.setup().roots.get(screen).cloned();
    let Some(screen) = screen else { return false };
    let root = screen.root;
    let active_atom = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let class_atom = conn.intern_atom(false, b"WM_CLASS").ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let net_name_atom = conn.intern_atom(false, b"_NET_WM_NAME").ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
    let (Some(active_atom), Some(class_atom), Some(net_name_atom)) = (active_atom, class_atom, net_name_atom) else { return false };

    while !stop.load(Ordering::Relaxed) {
        let fg = conn.get_property(false, root, active_atom, x11rb::protocol::xproto::AtomEnum::WINDOW, 0, 1).ok()
            .and_then(|c| c.reply().ok())
            .and_then(|reply| {
                let win = reply.value32()?.next()?;
                let class = conn.get_property(false, win, class_atom, x11rb::protocol::xproto::AtomEnum::STRING, 0, 64).ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).split('\0').next().unwrap_or("").to_string())
                    .unwrap_or_default();
                let title = conn.get_property(false, win, net_name_atom, x11rb::protocol::xproto::AtomEnum::UTF8_STRING, 0, 128).ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).trim_end_matches('\0').to_string())
                    .unwrap_or_default();
                if class.is_empty() { None } else { Some((class, title)) }
            });
        let idle = x11_screensaver_idle(&conn, root);
        let _ = tx.send(PlatformEvent::Tick { idle_ms: idle, foreground: fg.map(|(e, t)| (e, t, None)) });
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
    true
}

fn x11_screensaver_idle(conn: &x11rb::rust_connection::RustConnection, root: u32) -> Option<u64> {
    let reply = conn.screensaver_query_info(root).ok()?.reply().ok()?;
    Some(reply.idle as u64)
}

/// Distraction discouragement is Windows-only in V1; other platforms no-op.
pub fn discourage_foreground() {}
