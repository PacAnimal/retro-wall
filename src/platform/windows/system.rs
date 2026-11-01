//! The machine, from the system's own counters.

use super::{unwide, wide};
use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::sync::Mutex;
use std::time::Instant;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetBestRoute2, GetExtendedTcpTable, GetIfTable2, MIB_IF_TABLE2, MIB_IPFORWARD_ROW2, MIB_TCP_STATE_ESTAB, MIB_TCP6ROW_OWNER_PID,
    MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_INET};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW, GetVolumePathNameW, OPEN_EXISTING};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

/// The battery's charge, and whether mains is off. A machine with no battery (flag 128) or an unknown
/// one (255): no charge, on mains.
pub fn power() -> crate::platform::Power {
    // SAFETY: zeroed is a valid SYSTEM_POWER_STATUS, filled in by the call.
    let mut s: SYSTEM_POWER_STATUS = unsafe { zeroed() };
    // SAFETY: a valid out-pointer.
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 || s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 {
        return Default::default();
    }
    crate::platform::Power {
        charge: if s.BatteryLifePercent <= 100 { s.BatteryLifePercent as f64 } else { 0.0 },
        on_battery: s.ACLineStatus == 0,
        battery: true,
    }
}

/// A table GetExtendedTcpTable fills: asked for its size first, then again while it grows.
fn tcp_table(family: u16) -> Option<Vec<u32>> {
    let mut size = 0u32;
    let mut buf: Vec<u32> = vec![];
    for _ in 0..4 {
        // SAFETY: the buffer holds `size` bytes (none on the first call, which only sizes it).
        let r = unsafe { GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, family as u32, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) };
        if r == NO_ERROR {
            return Some(buf);
        }
        buf = vec![0; (size as usize).div_ceil(4) + 64];
        size = (buf.len() * 4) as u32;
    }
    None
}

/// A TCP table's rows: a count, then that many rows; no more than the buffer holds.
fn rows_of<R: TcpRow>(t: &[u32]) -> &[R] {
    let Some((&count, rest)) = t.split_first() else { return &[] };
    let n = (count as usize).min(size_of_val(rest) / size_of::<R>());
    // SAFETY: the rows start 4 bytes in, which suits their 4-byte alignment; `n` rows lie within
    // `rest`; every bit pattern is a valid row (TcpRow); the slice borrows `t`.
    unsafe { std::slice::from_raw_parts(rest.as_ptr().cast::<R>(), n) }
}

/// A row of a TCP table: plain integers and bytes, aligned to 4 at most.
///
/// # Safety
/// Every bit pattern is a valid value, and the alignment is 4 or less.
unsafe trait TcpRow {}
// SAFETY: u32 fields only.
unsafe impl TcpRow for MIB_TCPROW_OWNER_PID {}
// SAFETY: u32 fields and byte arrays only.
unsafe impl TcpRow for MIB_TCP6ROW_OWNER_PID {}

/// Whether a TCP connection to local `port` is established, over IPv4 or IPv6.
pub fn vnc_connected(port: u16) -> bool {
    let local = |p: u32| u16::from_be(p as u16) == port;
    let v4 = tcp_table(AF_INET).is_some_and(|t| {
        let rows = rows_of::<MIB_TCPROW_OWNER_PID>(&t);
        rows.iter().any(|r| r.dwState == MIB_TCP_STATE_ESTAB as u32 && local(r.dwLocalPort))
    });
    v4 || tcp_table(AF_INET6).is_some_and(|t| {
        let rows = rows_of::<MIB_TCP6ROW_OWNER_PID>(&t);
        rows.iter().any(|r| r.dwState == MIB_TCP_STATE_ESTAB as u32 && local(r.dwLocalPort))
    })
}

/// (idle, total) in 100 ns units, all processors: the kernel time includes the idle time.
pub fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (0u64, 0u64, 0u64);
    // SAFETY: FILETIME is two u32s, the same layout as a little-endian u64.
    let ok = unsafe { GetSystemTimes((&raw mut idle).cast(), (&raw mut kernel).cast(), (&raw mut user).cast()) };
    (ok != 0).then_some((idle, kernel + user))
}

/// SYSTEM_PAGEFILE_INFORMATION, as NtQuerySystemInformation's class 18 lists them.
#[repr(C)]
struct PageFile {
    next: u32,
    total: u32,
    in_use: u32,
    peak: u32,
    name: [usize; 2],
}

/// (memory used, page files used), in percent.
pub fn memory() -> Option<(f64, f64)> {
    // SAFETY: zeroed is a valid MEMORYSTATUSEX; its length is set before the call.
    let mut m: MEMORYSTATUSEX = unsafe { zeroed() };
    m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a valid, sized out-pointer.
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 || m.ullTotalPhys == 0 {
        return None;
    }
    let used = 100.0 * (1.0 - m.ullAvailPhys as f64 / m.ullTotalPhys as f64);
    let mut buf = vec![0u64; 512];
    let (mut total, mut in_use) = (0u64, 0u64);
    // SAFETY: the call writes at most the buffer's 4 KB; entries are walked by their own offsets, each
    // checked to lie inside the buffer, and copied out unaligned (PageFile is plain integers).
    unsafe {
        let r =
            windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation(18, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, std::ptr::null_mut());
        if r == 0 {
            let base = buf.as_ptr() as *const u8;
            let mut at = 0usize;
            while at + size_of::<PageFile>() <= buf.len() * 8 {
                let p = base.add(at).cast::<PageFile>().read_unaligned();
                total += p.total as u64;
                in_use += p.in_use as u64;
                if p.next == 0 {
                    break;
                }
                at += p.next as usize;
            }
        }
    }
    Some((used, if total > 0 { 100.0 * in_use as f64 / total as f64 } else { 0.0 }))
}

/// The physical drives' handles, opened for no access (all IOCTL_DISK_PERFORMANCE needs, and no
/// administrator), with the time they were found.
struct Drives(Instant, Vec<HANDLE>);

// SAFETY: kernel handles are valid on every thread of the process; these are opened, used and closed
// only under DRIVES's lock.
unsafe impl Send for Drives {}

static DRIVES: Mutex<Option<Drives>> = Mutex::new(None);

/// Reads and writes completed so far, over the physical drives (looked for again every 30 s). One
/// small ioctl a drive: cheaper than a PDH query, with nothing to set up.
pub fn disk_ios() -> u64 {
    let mut d = DRIVES.lock().unwrap_or_else(|e| e.into_inner());
    if d.as_ref().is_none_or(|Drives(at, _)| at.elapsed().as_secs() > 30) {
        if let Some(Drives(_, old)) = d.take() {
            for h in old {
                // SAFETY: a handle we opened and no longer use.
                unsafe { CloseHandle(h) };
            }
        }
        let mut found = vec![];
        for n in 0..32 {
            let name = wide(format!(r"\\.\PhysicalDrive{n}"));
            // SAFETY: a NUL-terminated name; no security attributes or template.
            let h = unsafe { CreateFileW(name.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
            if h != INVALID_HANDLE_VALUE && !h.is_null() {
                found.push(h);
            }
        }
        *d = Some(Drives(Instant::now(), found));
    }
    let Some(Drives(_, handles)) = d.as_ref() else { return 0 };
    handles
        .iter()
        .map(|&h| {
            // SAFETY: zeroed is a valid DISK_PERFORMANCE; the call writes at most its size.
            let mut p: DISK_PERFORMANCE = unsafe { zeroed() };
            let mut got = 0u32;
            // SAFETY: an open handle and a sized out-buffer, synchronous.
            let ok = unsafe {
                DeviceIoControl(
                    h,
                    IOCTL_DISK_PERFORMANCE,
                    std::ptr::null(),
                    0,
                    (&raw mut p).cast(),
                    size_of::<DISK_PERFORMANCE>() as u32,
                    &mut got,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 { p.ReadCount as u64 + p.WriteCount as u64 } else { 0 }
        })
        .sum()
}

/// (used, free) of the volume holding `path`. "/" is the system drive.
pub fn drive_space(path: &str) -> Option<(f64, f64)> {
    let path = if path == "/" { std::env::var("SystemDrive").unwrap_or("C:".into()) + "\\" } else { path.to_string() };
    let name = wide(&path);
    let mut volume = [0u16; 261];
    // SAFETY: a NUL-terminated path and a buffer of the length given.
    if unsafe { GetVolumePathNameW(name.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) } == 0 {
        return None;
    }
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-terminated volume path and valid out-pointers; the third is optional.
    if unsafe { GetDiskFreeSpaceExW(volume.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) } == 0 || total == 0 {
        return None;
    }
    let (total, free) = (total as f64, free as f64);
    Some((100.0 * (total - free) / total, 100.0 * free / total))
}

/// {interface alias: (received, sent)}: of the physical interfaces (hardware, and not a filter driver
/// stacked on one, which counts the same traffic again), or of the aliases in `only`.
pub fn net_counts(only: &[String]) -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table, freed below; its rows follow the count.
    unsafe {
        if GetIfTable2(&mut table) != NO_ERROR || table.is_null() {
            return out;
        }
        let rows = std::slice::from_raw_parts(
            (&raw const (*table).Table).cast::<windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2>(),
            (*table).NumEntries as usize,
        );
        for r in rows {
            let name = unwide(&r.Alias);
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let physical = flags & 1 != 0 && flags & 2 == 0;
            if if only.is_empty() { physical } else { only.contains(&name) } {
                let e = out.entry(name).or_insert((0, 0));
                e.0 += r.InOctets;
                e.1 += r.OutOctets;
            }
        }
        FreeMibTable(table.cast());
    }
    out
}

fn primary_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:53").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// The next hop of the route Windows takes out (to 1.1.1.1); none on a route without one (a tunnel).
fn router() -> Option<String> {
    // SAFETY: zeroed SOCKADDR_INET and MIB_IPFORWARD_ROW2 are valid; the destination is a full IPv4
    // address; the outputs are valid pointers.
    unsafe {
        let mut dest: SOCKADDR_INET = zeroed();
        dest.Ipv4.sin_family = AF_INET;
        dest.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes([1, 1, 1, 1]);
        let mut row: MIB_IPFORWARD_ROW2 = zeroed();
        let mut source: SOCKADDR_INET = zeroed();
        if GetBestRoute2(std::ptr::null(), 0, std::ptr::null(), &dest, 0, &mut row, &mut source) != NO_ERROR || row.NextHop.si_family != AF_INET {
            return None;
        }
        let hop = row.NextHop.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes();
        (hop != [0; 4]).then(|| std::net::Ipv4Addr::from(hop).to_string())
    }
}

pub fn addresses() -> (Option<String>, Option<String>) {
    (primary_address(), router())
}

/// The DNS host name, without its domain.
pub fn hostname() -> String {
    let mut buf = [0u16; 256];
    let mut n = buf.len() as u32;
    // SAFETY: a buffer of n characters.
    if unsafe { GetComputerNameExW(ComputerNameDnsHostname, buf.as_mut_ptr(), &mut n) } == 0 {
        return std::env::var("COMPUTERNAME").unwrap_or_default().to_lowercase();
    }
    unwide(&buf).split('.').next().unwrap_or("").to_string()
}
