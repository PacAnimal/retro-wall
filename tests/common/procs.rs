//! Processes, for the tests of plugins on every Unix: their state and process groups (from /proc on
//! Linux, from ps elsewhere), scratch directories, waiting for a condition.

use std::path::PathBuf;

/// A scratch directory of the test's own, empty.
pub fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("retro-wall-x-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A process's state letter (R, S, T, ...), None once it is gone.
#[cfg(target_os = "linux")]
pub fn state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.trim_start().chars().next()
}

/// The processes in a process group, but zombies.
#[cfg(target_os = "linux")]
pub fn group(pgid: u32) -> Vec<u32> {
    let mut out = vec![];
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
        if rest.split_whitespace().nth(2).and_then(|g| g.parse::<u32>().ok()) == Some(pgid) && state(pid) != Some('Z') {
            out.push(pid);
        }
    }
    out
}

/// ps's lines of the given fields, for every process.
#[cfg(not(target_os = "linux"))]
fn ps(fields: &str) -> Vec<Vec<String>> {
    let out = std::process::Command::new("ps").args(["-A", "-o", fields]).output().expect("ps runs");
    String::from_utf8_lossy(&out.stdout).lines().map(|l| l.split_whitespace().map(String::from).collect()).collect()
}

/// A process's state letter (R, S, T, U, ...), None once it is gone.
#[cfg(not(target_os = "linux"))]
pub fn state(pid: u32) -> Option<char> {
    let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().chars().next()
}

/// The processes in a process group, but zombies.
#[cfg(not(target_os = "linux"))]
pub fn group(pgid: u32) -> Vec<u32> {
    ps("pid=,pgid=,stat=").iter().filter(|f| f.len() == 3 && f[1].parse() == Ok(pgid) && !f[2].starts_with('Z')).filter_map(|f| f[0].parse().ok()).collect()
}

/// Wait up to `secs` for `ok`, checking every 50 ms.
pub fn eventually(secs: f64, mut ok: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + retro_wall::util::seconds(secs);
    loop {
        if ok() {
            return true;
        }
        if std::time::Instant::now() >= end {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
