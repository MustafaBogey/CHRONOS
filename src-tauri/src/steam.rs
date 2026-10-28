//! Steam gameplay detection — local evidence only, no credentials, no online APIs.
//!
//! A foreground executable counts as Steam gameplay only when its install path lies
//! under a Steam library's `steamapps\common\` directory. Friendly game names come
//! from the local `appmanifest_*.acf` files (InstallDir → name) when parseable;
//! otherwise the `common\<folder>` folder name is used. The Steam *client* itself
//! (steam.exe / steamwebhelper.exe) stays Neutral via the recognition catalog.

#[cfg(windows)]
pub fn detect_game(full_image_path: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    static CACHE: OnceLock<Mutex<Option<HashMap<String, String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    // Evidence gate: the executable must live under steamapps\common\.
    let p = full_image_path.to_lowercase().replace('/', "\\");
    let marker = "\\steamapps\\common\\";
    let idx = p.find(marker)?;
    let after = &full_image_path[idx + marker.len()..];
    let folder = after.split(['\\', '/']).next()?.trim();
    if folder.is_empty() {
        return None;
    }

    let mut guard = cache.lock().unwrap();
    let map = guard.get_or_insert_with(load_manifests);
    if let Some(name) = map.get(&folder.to_lowercase()) {
        return Some(name.clone());
    }
    Some(folder.split(['_', '-']).map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }).collect::<Vec<_>>().join(" "))
}

/// Parse Steam library folders and app manifests into install-folder → friendly name.
#[cfg(windows)]
fn load_manifests() -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    let candidates: Vec<String> = vec![
        r"C:\Program Files (x86)\Steam".to_string(),
        r"C:\Program Files\Steam".to_string(),
    ];
    // Read libraryfolders.vdf for extra libraries, then appmanifest_*.acf in each.
    let mut libraries: Vec<String> = candidates.clone();
    for root in &candidates {
        let vdf = std::path::Path::new(root).join("steamapps\\libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("\"path\"") {
                    let path = rest.trim().trim_matches('"').replace("\\\\", "\\");
                    if !path.is_empty() && !libraries.contains(&path) {
                        libraries.push(path);
                    }
                }
            }
        }
    }
    for lib in libraries {
        let apps_dir = std::path::PathBuf::from(&lib).join("steamapps");
        let Ok(entries) = std::fs::read_dir(&apps_dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("appmanifest_") && name.ends_with(".acf") {
                let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
                let mut install_dir: Option<String> = None;
                let mut app_name: Option<String> = None;
                for line in text.lines() {
                    let t: &str = line.trim();
                    if let Some(rest) = t.strip_prefix("\"installdir\"") {
                        install_dir = Some(rest.trim().trim_matches('"').to_string());
                    } else if let Some(rest) = t.strip_prefix("\"name\"") {
                        app_name = Some(rest.trim().trim_matches('"').to_string());
                    }
                }
                if let (Some(dir), Some(n)) = (install_dir, app_name) {
                    if !dir.is_empty() && !n.is_empty() {
                        map.insert(dir.to_lowercase(), n);
                    }
                }
            }
        }
    }
    map
}

#[cfg(not(windows))]
pub fn detect_game(_full_image_path: &str) -> Option<String> {
    None // Steam detection is Windows-only in this round
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn path_gate_requires_steamapps_common() {
        // No marker → no detection, regardless of Steam running.
        assert!(super::detect_game(r"C:\Games\EldenRing\eldenring.exe").is_none());
        assert!(super::detect_game(r"C:\Program Files\Mozilla Firefox\firefox.exe").is_none());
        // Marker present → detected; friendly name at minimum the folder name.
        let d = super::detect_game(r"C:\Program Files (x86)\Steam\steamapps\common\Elden Ring\eldenring.exe");
        assert!(d.is_some());
        assert!(d.unwrap().len() > 0);
    }
}
