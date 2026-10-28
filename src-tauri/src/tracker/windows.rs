//! Windows observation adapter.
//!
//! - Event-driven foreground changes via `SetWinEventHook` on a message-only window.
//! - Lock/unlock via `WTSRegisterSessionNotification`.
//! - Sleep/wake via `WM_POWERBROADCAST`.
//! - Idle via `GetLastInputInfo`, polled on a 2-second timer that also reconciles
//!   foreground changes the hook may have missed.

use super::PlatformEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT};
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, PostMessageW, RegisterClassW, SetTimer, ShowWindow, TranslateMessage,
    CW_USEDEFAULT, HWND_MESSAGE, MSG, SW_MINIMIZE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT, WM_TIMER, WM_WTSSESSION_CHANGE,
    WNDCLASSW, WM_POWERBROADCAST,
};

const EVENT_SYSTEM_FOREGROUND_RAW: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT_RAW: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS_RAW: u32 = 0x0002;
const PBT_APMSUSPEND_RAW: usize = 4;
const PBT_APMRESUMEAUTOMATIC_RAW: usize = 18;
const WTS_SESSION_LOCK_RAW: usize = 0x7;
const WTS_SESSION_UNLOCK_RAW: usize = 0x8;
const TIMER_POLL_ID: usize = 1;
const TIMER_POLL_MS: u32 = 2000;

static TX: OnceLock<Sender<PlatformEvent>> = OnceLock::new();
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// Last known fullscreen-foreground state, so the mini bar only reacts to changes.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);

/// Distraction discouragement: minimize the foreground window. Reversible and
/// narrowly scoped — no process termination, no global hooks, no system changes.
pub fn discourage_foreground() {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_invalid() {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
    }
}

fn send(ev: PlatformEvent) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(ev);
    }
}

/// True when the foreground window covers its whole monitor without a title bar
/// (a fullscreen app or game). Maximized windows keep WS_CAPTION and are excluded.
unsafe fn foreground_fullscreen() -> bool {
    use windows::Win32::Foundation::{RECT, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowLongPtrW, GetWindowRect, GWL_STYLE, WS_CAPTION,
    };
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() {
        return false; // our own windows (main + mini) are never "in the way"
    }
    let mut class = [0u16; 24];
    let n = GetClassNameW(hwnd, &mut class) as usize;
    let name = String::from_utf16_lossy(&class[..n]);
    if name == "Progman" || name == "WorkerW" {
        return false; // the desktop
    }
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    if (style as usize) & (WS_CAPTION.0 as usize) != 0 {
        return false; // framed (incl. maximized) windows are not fullscreen
    }
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if monitor.is_invalid() {
        return false;
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut mi as *mut _ as _).as_bool() {
        return false;
    }
    let _ = POINT::default();
    rect.left <= mi.rcMonitor.left
        && rect.right >= mi.rcMonitor.right
        && rect.top <= mi.rcMonitor.top
        && rect.bottom >= mi.rcMonitor.bottom
}

/// Called on each poll tick: lowers the mini bar under fullscreen apps and restores
/// it afterwards. State-change only, so the steady-state cost is one rect check.
unsafe fn reconcile_fullscreen_mini() {
    let fs = foreground_fullscreen();
    if FULLSCREEN.swap(fs, Ordering::Relaxed) == fs {
        return;
    }
    if let Some(app) = APP.get() {
        crate::mini::set_below_fullscreen(app, fs);
    }
}

struct ForegroundInfo {
    exe: String,
    title: String,
    full_path: Option<String>,
}

unsafe fn foreground_info() -> Option<ForegroundInfo> {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == std::process::id() {
        return None; // never observe our own windows
    }
    let title = window_text(hwnd);
    let full = full_image_name(pid);
    let exe = full
        .as_ref()
        .map(|f| f.rsplit(['\\', '/']).next().unwrap_or("").trim_end_matches(".exe").to_string())
        .unwrap_or_default();
    if exe.is_empty() {
        return None;
    }
    Some(ForegroundInfo { exe, title, full_path: full })
}

unsafe fn window_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let copied = GetWindowTextW(hwnd, &mut buf);
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..copied as usize])
}

unsafe fn full_image_name(pid: u32) -> Option<String> {
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return None;
    };
    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buf.as_mut_ptr()), &mut size).is_ok();
    let _ = windows::Win32::Foundation::CloseHandle(handle);
    if !ok || size == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..size as usize]))
}

unsafe fn idle_ms() -> Option<u64> {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if GetLastInputInfo(&mut info).as_bool() {
        let now = GetTickCount();
        Some(now.wrapping_sub(info.dwTime) as u64)
    } else {
        None
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if wparam.0 == TIMER_POLL_ID {
                reconcile_fullscreen_mini();
                let fg = foreground_info();
                let idle = idle_ms();
                send(PlatformEvent::Tick {
                    idle_ms: idle,
                    foreground: fg.map(|f| (f.exe, f.title, f.full_path)),
                });
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            if wparam.0 == WTS_SESSION_LOCK_RAW {
                send(PlatformEvent::Locked);
            } else if wparam.0 == WTS_SESSION_UNLOCK_RAW {
                send(PlatformEvent::Unlocked);
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMSUSPEND_RAW {
                send(PlatformEvent::SystemSleep);
            } else if wparam.0 == PBT_APMRESUMEAUTOMATIC_RAW {
                send(PlatformEvent::SystemWake);
            }
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn winevent_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND_RAW {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return; // never observe our own windows
        }
        let title = window_text(hwnd);
        let full = if pid > 0 { full_image_name(pid) } else { None };
        let exe = full
            .as_ref()
            .map(|f| f.rsplit(['\\', '/']).next().unwrap_or("").trim_end_matches(".exe").to_string())
            .unwrap_or_default();
        if exe.is_empty() {
            return;
        }
        send(PlatformEvent::Foreground { exe, title, full_path: full });
    }
}

pub fn run(app: Option<tauri::AppHandle>, tx: Sender<PlatformEvent>, stop: Arc<AtomicBool>) {
    let _ = TX.set(tx);
    if let Some(a) = app {
        let _ = APP.set(a);
    }
    unsafe {
        let class_name: PCWSTR = w!("ChronosTrackerWindow");
        let wc = WNDCLASSW { lpfnWndProc: Some(wnd_proc), lpszClassName: class_name, ..Default::default() };
        let atom = RegisterClassW(&wc);
        if atom == 0 {
            send(PlatformEvent::SourceLost("window class registration failed".into()));
            return;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!(""),
            WINDOW_STYLE(0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) else {
            send(PlatformEvent::SourceLost("message window creation failed".into()));
            return;
        };

        let _hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND_RAW,
            EVENT_SYSTEM_FOREGROUND_RAW,
            None,
            Some(winevent_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT_RAW | WINEVENT_SKIPOWNPROCESS_RAW,
        );

        let _ = WTSRegisterSessionNotification(hwnd, 0);
        SetTimer(Some(hwnd), TIMER_POLL_ID, TIMER_POLL_MS, None);

        // Watcher: on stop, post WM_QUIT to this thread's message loop.
        let watcher = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                unsafe {
                    let _ = PostMessageW(None, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            })
        };

        let mut msg = MSG::default();
        loop {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                break; // WM_QUIT or error
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = watcher.join();
    }
}
