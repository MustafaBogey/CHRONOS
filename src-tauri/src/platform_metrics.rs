//! Real resource measurement for diagnostics. Windows uses native APIs; other platforms
//! report process memory via /proc when available and null otherwise (honestly).

#[cfg(windows)]
pub fn process_memory_mb() -> Option<f64> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let handle = GetCurrentProcess();
        let ok = GetProcessMemoryInfo(handle, &mut counters, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32).is_ok();
        if ok {
            Some(counters.WorkingSetSize as f64 / (1024.0 * 1024.0))
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
pub fn process_memory_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(kb) = line.strip_prefix("VmRSS:") {
            let kb: f64 = kb.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb / 1024.0);
        }
    }
    None
}

/// Average CPU of the whole process measured across a short sampling window.
#[cfg(windows)]
pub fn tracker_cpu_percent() -> Option<f64> {
    use windows::Win32::System::Threading::{GetProcessTimes, GetCurrentProcess};
    use windows::Win32::Foundation::FILETIME;
    unsafe {
        let handle = GetCurrentProcess();
        let mut creation = FILETIME::default();
        let mut exit_t = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // Sample CPU time, wait briefly, sample again — process-wide recent usage.
        if !GetProcessTimes(handle, &mut creation, &mut exit_t, &mut kernel, &mut user).is_ok() {
            return None;
        }
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64; // 100ns units
        let first = ft(kernel) + ft(user);
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut kernel2 = FILETIME::default();
        let mut user2 = FILETIME::default();
        let mut c2 = FILETIME::default();
        let mut e2 = FILETIME::default();
        if !GetProcessTimes(handle, &mut c2, &mut e2, &mut kernel2, &mut user2).is_ok() {
            return None;
        }
        let second = ft(kernel2) + ft(user2);
        let delta_100ns = second - first;
        let window_100ns = 250.0 * 10_000.0;
        let cores = std::thread::available_parallelism().map(|n| n.get() as f64).unwrap_or(1.0);
        let cpu_fraction = (delta_100ns / window_100ns).min(1.0);
        Some(cpu_fraction * 100.0 / cores)
    }
}

#[cfg(not(windows))]
pub fn tracker_cpu_percent() -> Option<f64> {
    // Parse /proc/self/stat utime+stime deltas across a short window.
    fn cpu_ticks() -> Option<(u64, u64)> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let fields: Vec<&str> = stat.split_whitespace().collect();
        let utime: u64 = fields.get(13)?.parse().ok()?;
        let stime: u64 = fields.get(14)?.parse().ok()?;
        Some((utime, stime))
    }
    let (u1, s1) = cpu_ticks()?;
    std::thread::sleep(std::time::Duration::from_millis(250));
    let (u2, s2) = cpu_ticks()?;
    let hz = 100.0;
    let cores = std::thread::available_parallelism().map(|n| n.get() as f64).unwrap_or(1.0);
    let ticks = ((u2 - u1) + (s2 - s1)) as f64;
    Some((ticks / (0.25 * hz)) * 100.0 / cores)
}
