//! The machine: Mach host statistics, sysctl (memory, swap, the routing table, interfaces), IOKit
//! (power sources, disk statistics) and netstat (TCP connections).

use crate::platform::Route;
use objc2::Message;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSCopyPowerSourcesList(blob: *const c_void) -> *const c_void;
    fn IOPSGetPowerSourceDescription(blob: *const c_void, source: *const c_void) -> *const c_void;
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(main_port: u32, matching: *mut c_void, iterator: *mut u32) -> c_int;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: *const c_void, allocator: *const c_void, options: u32) -> *const c_void;
    fn IOObjectRelease(object: u32) -> c_int;
}

unsafe extern "C" {
    fn mach_host_self() -> libc::mach_port_t;
}

/// The host port, asked for once: each mach_host_self adds a reference to the port name.
fn host() -> libc::mach_port_t {
    static HOST: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
    // SAFETY: mach_host_self takes nothing and touches no memory.
    *HOST.get_or_init(|| unsafe { mach_host_self() })
}

/// A CF object we own (a Copy or Create result), as its bridged Foundation class.
///
/// # Safety
/// `p` is null or a +1 reference to an object of class `T` (or a toll-free bridged CF type).
unsafe fn owned<T: Message>(p: *const c_void) -> Option<Retained<T>> {
    // SAFETY: as the caller promises.
    unsafe { Retained::from_raw(p as *mut T) }
}

/// A CF object we do not own (a Get result), retained for as long as we hold it.
///
/// # Safety
/// `p` is null or a valid reference to an object of class `T`.
unsafe fn borrowed<T: Message>(p: *const c_void) -> Option<Retained<T>> {
    // SAFETY: as the caller promises.
    unsafe { Retained::retain(p as *mut T) }
}

/// `obj` as a dictionary, if it is one.
pub fn dictionary(obj: Retained<AnyObject>) -> Option<Retained<NSDictionary<NSString, AnyObject>>> {
    let class = AnyClass::get(c"NSDictionary")?;
    // SAFETY: every object, CF ones included, takes isKindOfClass: with a class and answers a BOOL.
    let is: bool = unsafe { msg_send![&*obj, isKindOfClass: class] };
    // SAFETY: an NSDictionary. Its key and value types are not checked: it is only looked up by
    // NSString keys, and its values only read as objects, then downcast.
    is.then(|| unsafe { Retained::cast_unchecked(obj) })
}

/// A dictionary's value for `key`, if it is a number.
pub fn number(d: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<f64> {
    d.objectForKey(&NSString::from_str(key))?.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

/// A dictionary's value for `key`, if it is a string.
pub fn string(d: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<String> {
    d.objectForKey(&NSString::from_str(key))?.downcast::<NSString>().ok().map(|s| s.to_string())
}

/// The internal batteries' charge, and whether the machine runs on battery.
pub fn power() -> crate::platform::Power {
    // SAFETY: the Copy functions return +1 references or null, owned here; the blob outlives the list
    // and the descriptions read from it; a description is a CFDictionary or null, as IOKit documents,
    // retained while read.
    unsafe {
        let blob = IOPSCopyPowerSourcesInfo();
        let Some(blob_owned) = owned::<AnyObject>(blob) else { return Default::default() };
        let Some(list) = owned::<NSArray<AnyObject>>(IOPSCopyPowerSourcesList(blob)) else { return Default::default() };
        let (mut charges, mut on_battery) = (vec![], false);
        for source in list.iter() {
            let Some(d) = borrowed::<NSDictionary<NSString, AnyObject>>(IOPSGetPowerSourceDescription(blob, Retained::as_ptr(&source) as *const c_void)) else {
                continue;
            };
            if string(&d, "Type").as_deref() != Some("InternalBattery") {
                continue;
            }
            if let (Some(now), Some(max)) = (number(&d, "Current Capacity"), number(&d, "Max Capacity"))
                && max > 0.0
            {
                charges.push(100.0 * now / max);
            }
            on_battery |= string(&d, "Power Source State").as_deref() == Some("Battery Power");
        }
        drop(blob_owned);
        let charge = if charges.is_empty() { 0.0 } else { charges.iter().sum::<f64>() / charges.len() as f64 };
        crate::platform::Power { charge, on_battery: !charges.is_empty() && on_battery }
    }
}

/// A sysctl's value by MIB, as bytes. Tables may grow between asking their size and reading them.
fn sysctl_mib(mib: &mut [c_int]) -> Option<Vec<u8>> {
    sysctl_with(|buf, len| {
        // SAFETY: the MIB is `mib.len()` integers; `buf` is null or points to `*len` writable bytes.
        unsafe { libc::sysctl(mib.as_mut_ptr(), mib.len() as u32, buf, len, null_mut(), 0) }
    })
}

fn sysctl_with(mut call: impl FnMut(*mut c_void, &mut usize) -> c_int) -> Option<Vec<u8>> {
    for _ in 0..4 {
        let mut len = 0usize;
        if call(null_mut(), &mut len) != 0 {
            return None;
        }
        let mut buf = vec![0u8; len + len / 4 + 256];
        let mut got = buf.len();
        if call(buf.as_mut_ptr() as *mut c_void, &mut got) == 0 {
            buf.truncate(got);
            return Some(buf);
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return None;
        }
    }
    None
}

/// A fixed-size sysctl value by name.
fn sysctl_value<T: Plain>(name: &CStr) -> Option<T> {
    let mut v = std::mem::MaybeUninit::<T>::zeroed();
    let mut len = size_of::<T>();
    // SAFETY: a NUL-terminated name; `v` has room for `len` bytes, and sysctlbyname writes at most that.
    let ok = unsafe { libc::sysctlbyname(name.as_ptr(), v.as_mut_ptr() as *mut c_void, &mut len, null_mut(), 0) } == 0;
    // SAFETY: zeroed, then written by the kernel; every bit pattern is a T (Plain).
    (ok && len == size_of::<T>()).then(|| unsafe { v.assume_init() })
}

/// A type the kernel may fill with any bytes.
///
/// # Safety
/// Every bit pattern is a valid value: integers only, no padding read as anything else.
unsafe trait Plain: Copy {}
// SAFETY: an integer.
unsafe impl Plain for u64 {}
// SAFETY: integers only (boolean_t is an int).
unsafe impl Plain for libc::xsw_usage {}

/// Whether netstat was found to see no connections: the kernel hides other programs' sockets from
/// programs it does not trust (an ad-hoc signed one, and what it runs).
static TABLE_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a viewer is connected to local TCP `port`: netstat's table where it is visible; else, for
/// 5900, whether Screen Sharing's server runs (launchd starts it when a viewer connects).
pub fn vnc_connected(port: u16) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if !TABLE_HIDDEN.load(Relaxed) {
        if let Ok(o) = std::process::Command::new("/usr/sbin/netstat").args(["-anp", "tcp"]).stderr(std::process::Stdio::null()).output() {
            let table = String::from_utf8_lossy(&o.stdout);
            if table.lines().any(|l| l.starts_with("tcp")) {
                return established_to(&table, port);
            }
        }
        TABLE_HIDDEN.store(true, Relaxed);
    }
    port == 5900 && root_runs("screensharingd")
}

/// Whether a process of root's has this command name (as ps shows it, at most 16 bytes).
fn root_runs(name: &str) -> bool {
    // struct kinfo_proc's size, and where its kp_proc.p_comm (17 bytes) lies
    const SIZE: usize = 648;
    const COMM: usize = 243;
    let Some(t) = sysctl_mib(&mut [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_RUID, 0]) else { return false };
    t.as_chunks::<SIZE>().0.iter().any(|k| {
        let comm = &k[COMM..COMM + 17];
        comm.split(|&b| b == 0).next() == Some(name.as_bytes())
    })
}

/// Whether `netstat -anp tcp` shows an established connection to local `port` (its local address
/// ends in .PORT).
pub fn established_to(table: &str, port: u16) -> bool {
    let end = format!(".{port}");
    table.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() >= 6 && f[0].starts_with("tcp") && f[5] == "ESTABLISHED" && f[3].ends_with(&end)
    })
}

/// (idle, total) CPU ticks so far, over every core.
pub fn cpu_times() -> Option<(u64, u64)> {
    let mut info = [0u32; libc::CPU_STATE_MAX as usize];
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: `info` holds HOST_CPU_LOAD_INFO_COUNT integers, as the flavour fills.
    let r = unsafe { libc::host_statistics(host(), libc::HOST_CPU_LOAD_INFO, info.as_mut_ptr() as *mut _, &mut count) };
    (r == 0).then(|| (info[libc::CPU_STATE_IDLE as usize] as u64, info.iter().map(|&t| t as u64).sum()))
}

/// (memory used, swap used) in percent. Used memory as Activity Monitor's "Memory Used": app memory
/// (internal less purgeable), wired and compressed. Swap is measured against the memory: macOS adds
/// swap files as they fill, so against its own total it is always nearly full.
pub fn memory() -> Option<(f64, f64)> {
    let total = sysctl_value::<u64>(c"hw.memsize")? as f64;
    // SAFETY: plain integers; zero is a valid value of each.
    let mut vm: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `vm` is a vm_statistics64 and `count` its size in integers, as HOST_VM_INFO64 fills.
    if unsafe { libc::host_statistics64(host(), libc::HOST_VM_INFO64, &mut vm as *mut _ as *mut _, &mut count) } != 0 || total <= 0.0 {
        return None;
    }
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as f64;
    let pages = (vm.internal_page_count as f64 - vm.purgeable_count as f64) + vm.wire_count as f64 + vm.compressor_page_count as f64;
    let swap = sysctl_value::<libc::xsw_usage>(c"vm.swapusage").map_or(0.0, |s| s.xsu_used as f64);
    Some(((100.0 * pages * page / total).clamp(0.0, 100.0), (100.0 * swap / total).min(100.0)))
}

/// Reads and writes completed so far, over IOKit's block storage drivers (each disk's).
pub fn disk_ios() -> u64 {
    let stats = NSString::from_str("Statistics");
    let mut total = 0u64;
    // SAFETY: IOServiceMatching's dictionary is consumed by IOServiceGetMatchingServices; each object
    // the iterator gives, and the iterator, is released once; the key is a CFString (toll-free bridged);
    // the property is a +1 CF object or null, owned here and read only once it is found a dictionary.
    unsafe {
        let mut it = 0u32;
        if IOServiceGetMatchingServices(0, IOServiceMatching(c"IOBlockStorageDriver".as_ptr()), &mut it) != 0 {
            return 0;
        }
        loop {
            let entry = IOIteratorNext(it);
            if entry == 0 {
                break;
            }
            let p = IORegistryEntryCreateCFProperty(entry, Retained::as_ptr(&stats) as *const c_void, std::ptr::null(), 0);
            if let Some(d) = owned::<AnyObject>(p).and_then(dictionary) {
                for key in ["Operations (Read)", "Operations (Write)"] {
                    total += number(&d, key).unwrap_or(0.0) as u64;
                }
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(it);
    }
    total
}

pub fn drive_space(path: &str) -> Option<(f64, f64)> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: plain integers; zero is a valid value of each.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a statvfs to fill.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 || st.f_blocks == 0 {
        return None;
    }
    let (total, free) = (st.f_blocks as f64 * st.f_frsize as f64, st.f_bavail as f64 * st.f_frsize as f64);
    Some((100.0 * (total - free) / total, 100.0 * free / total))
}

/// An interface's 64-bit counters (if_data64), from its IFMIB_IFDATA sysctl: (type, received, sent).
fn interface_data(index: u32) -> Option<(u8, u64, u64)> {
    const PF_LINK: libc::c_int = 18;
    const IFMIB_IFDATA: libc::c_int = 2;
    const IFDATA_GENERAL: libc::c_int = 1;
    // struct ifmibdata: the name (16 bytes), nine 32-bit fields, then the if_data64
    const DATA: usize = 16 + 9 * 4;
    let t = sysctl_mib(&mut [libc::CTL_NET, PF_LINK, 0, IFMIB_IFDATA, index as libc::c_int, IFDATA_GENERAL])?;
    if t.len() < DATA + size_of::<libc::if_data64>() {
        return None;
    }
    // SAFETY: the bytes hold a whole if_data64 at DATA; read unaligned.
    let d: libc::if_data64 = unsafe { std::ptr::read_unaligned(t[DATA..].as_ptr() as *const libc::if_data64) };
    Some((d.ifi_type, d.ifi_ibytes, d.ifi_obytes))
}

/// {interface: (received, sent)} bytes so far: of the physical interfaces (Ethernet-type and named
/// en*: Wi-Fi, Ethernet, USB and Thunderbolt adapters, not bridges, tunnels or AirDrop's), or of
/// `only`. From the per-interface sysctl: the routing socket's interface list wraps them at 4 GiB.
pub fn net_counts(only: &[String]) -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();
    // SAFETY: the list is read up to its zero entry and freed once.
    unsafe {
        let list = libc::if_nameindex();
        if list.is_null() {
            return out;
        }
        let mut i = 0;
        while (*list.add(i)).if_index != 0 {
            let entry = &*list.add(i);
            i += 1;
            let name = CStr::from_ptr(entry.if_name).to_string_lossy().into_owned();
            if if only.is_empty() { !name.starts_with("en") } else { !only.contains(&name) } {
                continue;
            }
            // 6: IFT_ETHER
            if let Some((kind, rx, tx)) = interface_data(entry.if_index)
                && (!only.is_empty() || kind == 6)
            {
                out.insert(name, (rx, tx));
            }
        }
        libc::if_freenameindex(list);
    }
    out
}

fn primary_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:53").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

/// The IPv4 default routes in a routing-table dump (NET_RT_FLAGS): rt_msghdr records, each followed by
/// the socket addresses its rtm_addrs names, each padded to 4 bytes. The route the system uses is the
/// unscoped one (metric 0); the per-interface scoped ones get 1. A gateway that is not an IPv4 address
/// (an interface: a tunnel) is no next hop.
pub fn default_routes(t: &[u8]) -> Vec<Route> {
    let mut out = vec![];
    let head = size_of::<libc::rt_msghdr>();
    let mut at = 0;
    while at + 4 <= t.len() {
        let len = u16::from_ne_bytes([t[at], t[at + 1]]) as usize;
        if len == 0 || at + len > t.len() {
            break;
        }
        let msg = &t[at..at + len];
        at += len;
        if len < head {
            continue;
        }
        // SAFETY: `msg` holds a whole rt_msghdr; read unaligned.
        let h: libc::rt_msghdr = unsafe { std::ptr::read_unaligned(msg.as_ptr() as *const libc::rt_msghdr) };
        if h.rtm_flags & libc::RTF_UP == 0 {
            continue;
        }
        // the addresses, by RTA_ bit
        let mut addrs: [Option<&[u8]>; 8] = [None; 8];
        let mut o = head;
        for (bit, slot) in addrs.iter_mut().enumerate() {
            if h.rtm_addrs & (1 << bit) == 0 {
                continue;
            }
            let Some(&sa_len) = msg.get(o) else { break };
            let sa_len = sa_len as usize;
            *slot = msg.get(o..o + sa_len.max(2));
            o += if sa_len == 0 { 4 } else { sa_len.div_ceil(4) * 4 };
        }
        let v4 = |a: &[u8]| (a.len() >= 8 && a[1] as c_int == libc::AF_INET).then(|| std::net::Ipv4Addr::new(a[4], a[5], a[6], a[7]));
        let Some(dst) = addrs[0].and_then(v4) else { continue };
        // the netmask's length counts its significant bytes: none, or all zero, is 0.0.0.0/0
        let mask_zero = addrs[2].is_none_or(|m| m.iter().skip(4).all(|&b| b == 0));
        if !dst.is_unspecified() || !mask_zero {
            continue;
        }
        let gateway = if h.rtm_flags & libc::RTF_GATEWAY != 0 { addrs[1].and_then(v4).filter(|g| !g.is_unspecified()).map(|g| g.to_string()) } else { None };
        out.push(Route { gateway, metric: i64::from(h.rtm_flags & libc::RTF_IFSCOPE != 0) });
    }
    out
}

pub fn addresses() -> (Option<String>, Option<String>) {
    let table = sysctl_mib(&mut [libc::CTL_NET, libc::PF_ROUTE, 0, libc::AF_INET, libc::NET_RT_FLAGS, libc::RTF_UP]);
    (primary_address(), table.and_then(|t| crate::platform::gateway(&default_routes(&t))))
}

pub fn hostname() -> String {
    let mut buf = [0 as c_char; 256];
    // SAFETY: `buf` has room for the name and its NUL; the last byte stays 0.
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len() - 1) } != 0 {
        return String::new();
    }
    // SAFETY: NUL-terminated above.
    let name = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned();
    name.split('.').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_read() {
        for _ in 0..3 {
            assert!(cpu_times().is_some_and(|(idle, total)| idle <= total && total > 0));
            assert!(memory().is_some_and(|(m, s)| (0.0..=100.0).contains(&m) && (0.0..=100.0).contains(&s)));
        }
        assert!(disk_ios() > 0, "the boot disk has done some reads");
    }

    #[test]
    fn tcp_table() {
        let t = "Active Internet connections (including servers)\n\
            Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)\n\
            tcp4       0      0  192.168.1.5.5900       192.168.1.9.50122      ESTABLISHED\n\
            tcp6       0      0  *.5900                 *.*                    LISTEN\n\
            tcp4       0      0  192.168.1.5.50200      192.168.1.9.5901       ESTABLISHED\n";
        assert!(established_to(t, 5900));
        assert!(!established_to(t, 5901));
        assert!(!established_to(t, 22));
        assert!(!established_to("", 5900));
    }

    #[test]
    fn this_machine() {
        assert!(cpu_times().is_some_and(|(idle, total)| idle <= total && total > 0));
        assert!(memory().is_some_and(|(m, s)| (0.0..=100.0).contains(&m) && (0.0..=100.0).contains(&s)));
        assert!(drive_space("/").is_some());
        assert!(!hostname().is_empty());
    }
}
