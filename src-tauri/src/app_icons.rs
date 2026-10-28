//! Recognizable activity icons, fully offline.
//!
//! Native application icons are extracted once per application (Windows SHGetFileInfo,
//! 32×32 → PNG) the first time the tracker observes the executable, and cached in the
//! local `app_icon_cache` table. The frontend asks `get_app_icons` for the names it is
//! displaying and receives data-URLs; missing entries fall back to the existing generic
//! glyph system. No network access, no icon service, no tracking-data changes.

use parking_lot::Mutex;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn seen() -> &'static Mutex<HashSet<String>> {
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Load the cached-name set at startup so observed apps cost no extra queries.
pub fn warm_cache(conn: &Connection) {
    let mut set = seen().lock();
    if let Ok(mut stmt) = conn.prepare("SELECT app_name FROM app_icon_cache") {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            for name in rows.flatten() {
                set.insert(name);
            }
        }
    }
}

/// Called from the tracker after an interval insert: extracts and caches the icon for a
/// newly observed application. One shot per display name; failures are cached as NULL
/// rows so they are not retried on the hot path.
pub fn cache_app_icon(conn: &Arc<Mutex<Connection>>, display: &str, full_path: Option<&std::path::Path>) {
    let Some(path) = full_path else { return };
    {
        let mut set = seen().lock();
        if !set.insert(display.to_string()) {
            return; // already cached or attempted
        }
    }
    let png = extract_icon_png(path);
    let exe = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    {
        let c = conn.lock();
        let _ = c.execute(
            "INSERT INTO app_icon_cache (app_name, exe, icon_png, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(app_name) DO UPDATE SET exe=?2, icon_png=?3, updated_at=?4",
            rusqlite::params![display, exe, png, crate::db::now_ms()],
        );
    }
}

/// data-URLs for the requested display names; missing or failed entries are absent.
pub fn icon_data_urls(conn: &Connection, names: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for name in names {
        let Ok(png) = conn.query_row(
            "SELECT icon_png FROM app_icon_cache WHERE app_name = ?1 AND icon_png IS NOT NULL",
            [name],
            |r| r.get::<_, Vec<u8>>(0),
        ) else {
            continue;
        };
        if png.is_empty() {
            continue;
        }
        out.insert(name.clone(), format!("data:image/png;base64,{}", base64(&png)));
    }
    out
}

/* Tiny standard base64 (no extra dependency for a few icons). */
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() / 3 + 1) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/* ---------- Windows extraction ---------- */

#[cfg(windows)]
fn extract_icon_png(path: &std::path::Path) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut sfi = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || sfi.hIcon.is_invalid() {
        return None;
    }
    let png = hicon_to_png(sfi.hIcon);
    let _ = unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(sfi.hIcon);
    };
    png
}

#[cfg(windows)]
fn hicon_to_png(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut info = ICONINFO::default();
    if unsafe { GetIconInfo(hicon, &mut info) }.is_err() {
        return None;
    }
    // Always free both bitmap handles.
    struct Guard(windows::Win32::Graphics::Gdi::HBITMAP, bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            if self.1 {
                unsafe {
                    let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(self.0.0));
                }
            }
        }
    }
    let _color_guard = Guard(info.hbmColor, !info.hbmColor.is_invalid());
    let _mask_guard = Guard(info.hbmMask, !info.hbmMask.is_invalid());
    if info.hbmColor.is_invalid() {
        return None; // monochrome icons are not worth the conversion path
    }
    let mut bm = BITMAP::default();
    if unsafe { GetObjectW(windows::Win32::Graphics::Gdi::HGDIOBJ(info.hbmColor.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as _)) } == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return None;
    }
    let hdr = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: -(h as i32), // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        ..Default::default()
    };
    let mut bmi = BITMAPINFO { bmiHeader: hdr, ..Default::default() };
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        return None;
    }
    let got = unsafe {
        GetDIBits(
            hdc,
            info.hbmColor,
            0,
            h,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
    }
    if got == 0 {
        return None;
    }
    // BGRA → RGBA; some legacy icons report zero alpha everywhere — treat as opaque.
    let mut has_alpha = false;
    for px in pixels.chunks_exact_mut(4) {
        px.swap(0, 2);
        if px[3] != 0 {
            has_alpha = true;
        }
    }
    if !has_alpha {
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
    encode_png(w, h, &pixels)
}

#[cfg(windows)]
fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(not(windows))]
fn extract_icon_png(_path: &std::path::Path) -> Option<Vec<u8>> {
    None // other platforms use the existing generic fallbacks in this cycle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn cache_roundtrip_returns_data_url_only_for_valid_icons() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE app_icon_cache (app_name TEXT PRIMARY KEY, exe TEXT NOT NULL DEFAULT '', icon_png BLOB, updated_at INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_icon_cache (app_name, exe, icon_png, updated_at) VALUES ('Discord','Discord.exe',X'DEADBEEF',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_icon_cache (app_name, exe, icon_png, updated_at) VALUES ('Empty','e.exe',NULL,1)",
            [],
        )
        .unwrap();
        let names = vec!["Discord".to_string(), "Empty".to_string(), "Unknown".to_string()];
        let urls = icon_data_urls(&conn, &names);
        assert_eq!(urls.len(), 1, "only the real icon resolves");
        assert!(urls["Discord"].starts_with("data:image/png;base64,3q2+7w=="));
    }
}
