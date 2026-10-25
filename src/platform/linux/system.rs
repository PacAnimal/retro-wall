//! The machine, from /proc and /sys.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub fn lid_closed() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc/acpi/button/lid") else { return false };
    dir.flatten().any(|e| std::fs::read_to_string(e.path().join("state")).is_ok_and(|s| s.contains("closed")))
}

/// The machine's batteries (not a mouse's), and whether mains is off.
pub fn power() -> crate::platform::Power {
    let (mut charges, mut mains) = (vec![], false);
    if let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") {
        for e in dir.flatten() {
            let p = e.path();
            let read = |f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).ok();
            if read("scope").as_deref() == Some("Device") {
                continue;
            }
            match read("type").as_deref() {
                Some("Battery") => {
                    if let Some(c) = read("capacity").and_then(|c| c.parse::<f64>().ok()) {
                        charges.push(c);
                    }
                }
                Some("Mains") => mains |= read("online").as_deref() == Some("1"),
                _ => {}
            }
        }
    }
    let charge = if charges.is_empty() { 0.0 } else { charges.iter().sum::<f64>() / charges.len() as f64 };
    crate::platform::Power { charge, on_battery: !charges.is_empty() && !mains }
}

/// Whether a connection to local TCP `port` is established, in /proc/net/tcp or tcp6.
pub fn vnc_connected(port: u16) -> bool {
    ["/proc/net/tcp", "/proc/net/tcp6"].iter().any(|t| std::fs::read_to_string(t).is_ok_and(|s| established_to(&s, port)))
}

/// Whether a /proc/net/tcp table has a connection established to local `port`.
pub fn established_to(table: &str, port: u16) -> bool {
    table.lines().skip(1).any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 3 && f[3] == "01" && f[1].rsplit_once(':').and_then(|(_, p)| u16::from_str_radix(p, 16).ok()) == Some(port)
    })
}

pub fn cpu_times() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let f: Vec<u64> = s.lines().next()?.split_whitespace().skip(1).take(8).filter_map(|v| v.parse().ok()).collect();
    (f.len() == 8).then(|| (f[3] + f[4], f.iter().sum()))
}

pub fn memory() -> Option<(f64, f64)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut m = HashMap::new();
    for l in s.lines() {
        let mut it = l.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            m.insert(k.trim_end_matches(':'), v.parse::<f64>().unwrap_or(0.0));
        }
    }
    let g = |k: &str| *m.get(k).unwrap_or(&0.0);
    let used = if g("MemTotal") > 0.0 { 100.0 * (1.0 - g("MemAvailable") / g("MemTotal")) } else { 0.0 };
    let swap = if g("SwapTotal") > 0.0 { 100.0 * (1.0 - g("SwapFree") / g("SwapTotal")) } else { 0.0 };
    Some((used, swap))
}

static DISKS: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);

/// Reads and writes completed so far, summed over physical disks (rescanned every 30 s), from one
/// read of /proc/diskstats.
pub fn disk_ios() -> u64 {
    let mut d = DISKS.lock().unwrap_or_else(|e| e.into_inner());
    if d.as_ref().is_none_or(|(at, _)| at.elapsed().as_secs() > 30) {
        let names = std::fs::read_dir("/sys/block")
            .map(|r| r.flatten().filter(|e| e.path().join("device").exists()).map(|e| e.file_name().to_string_lossy().replace('!', "/")).collect())
            .unwrap_or_default();
        *d = Some((Instant::now(), names));
    }
    let Some((_, names)) = d.as_ref() else { return 0 };
    let mut ios = 0u64;
    for line in std::fs::read_to_string("/proc/diskstats").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() > 7
            && names.iter().any(|n| n == f[2])
            && let (Ok(r), Ok(w)) = (f[3].parse::<u64>(), f[7].parse::<u64>())
        {
            ios += r + w;
        }
    }
    ios
}

pub fn drive_space(path: &str) -> Option<(f64, f64)> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: statvfs is plain data: zeroed is a valid value.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 || st.f_blocks == 0 {
        return None;
    }
    let (total, free) = (st.f_blocks as f64 * st.f_frsize as f64, st.f_bavail as f64 * st.f_frsize as f64);
    Some((100.0 * (total - free) / total, 100.0 * free / total))
}

pub fn net_counts(only: &[String]) -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/net") else { return out };
    for e in dir.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if if only.is_empty() { !p.join("device").exists() } else { !only.contains(&name) } {
            continue;
        }
        let read = |f: &str| std::fs::read_to_string(p.join("statistics").join(f)).ok()?.trim().parse::<u64>().ok();
        if let (Some(rx), Some(tx)) = (read("rx_bytes"), read("tx_bytes")) {
            out.insert(name, (rx, tx));
        }
    }
    out
}

fn primary_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:53").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// The default routes in a /proc/net/route table.
pub fn default_routes(table: &str) -> Vec<crate::platform::Route> {
    let mut out = vec![];
    for l in table.lines().skip(1) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() <= 7 || f[1] != "00000000" || f[7] != "00000000" {
            continue;
        }
        let (Ok(flags), Ok(gw), Ok(metric)) = (u32::from_str_radix(f[3], 16), u32::from_str_radix(f[2], 16), f[6].parse::<i64>()) else { continue };
        if flags & 1 == 0 {
            continue; // not up
        }
        let gateway = (flags & 2 != 0 && gw != 0).then(|| std::net::Ipv4Addr::from(gw.to_le_bytes()).to_string());
        out.push(crate::platform::Route { gateway, metric });
    }
    out
}

pub fn addresses() -> (Option<String>, Option<String>) {
    (primary_address(), std::fs::read_to_string("/proc/net/route").ok().and_then(|t| crate::platform::gateway(&default_routes(&t))))
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().split('.').next().unwrap_or("").to_string()
}

pub fn name_process(name: &str) {
    let _ = std::fs::write("/proc/self/comm", name.chars().take(15).collect::<String>());
}

/// The names of the programs this user runs, as the kernel has them.
pub fn user_programs() -> std::collections::HashSet<String> {
    // SAFETY: getuid cannot fail and touches no memory.
    let uid = unsafe { libc::getuid() };
    let mut running = std::collections::HashSet::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for e in dir.flatten() {
            use std::os::unix::fs::MetadataExt;
            if e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit())
                && e.metadata().is_ok_and(|m| m.uid() == uid)
                && let Ok(c) = std::fs::read_to_string(e.path().join("comm"))
            {
                running.insert(c.trim().to_string());
            }
        }
    }
    running
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Route, gateway};

    const HEAD: &str = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n";

    #[test]
    fn default_routes_are_up_and_to_anywhere() {
        let table = format!(
            "{HEAD}eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\n\
             eth1\t00000000\t0102A8C0\t0002\t0\t0\t10\t00000000\n\
             eth0\t0001A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n\
             bad\t00000000\tZZZZZZZZ\t0003\t0\t0\t1\t00000000\n\
             short\t00000000\n\
             tun0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\n"
        );
        let r = default_routes(&table);
        assert_eq!(r, [Route { gateway: Some("192.168.1.1".into()), metric: 100 }, Route { gateway: None, metric: 50 }]);
        assert_eq!(gateway(&r), None, "the tunnel's metric is lower");
        assert_eq!(gateway(&r[..1]), Some("192.168.1.1".into()));
        assert!(default_routes("").is_empty() && default_routes(HEAD).is_empty());
    }
}
