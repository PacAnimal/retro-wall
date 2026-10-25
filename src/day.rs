//! The time of day, for themes that follow it: sun and moon positions, light, and which look (day or
//! night) to show. --demo HH:mm holds the time.

use std::sync::atomic::{AtomicI64, Ordering};

static TIME: AtomicI64 = AtomicI64::new(-1); // held time in seconds after midnight; -1 follows the local clock

/// HH:mm or HH:mm:ss as seconds after midnight.
pub fn parse(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty() || p.len() > 2 || !p.bytes().all(|b| b.is_ascii_digit())) || parts[1].len() != 2 {
        return None;
    }
    let n: Vec<i64> = parts.iter().map(|p| p.parse().unwrap_or(99)).collect();
    if n[0] > 23 || n[1] > 59 || n.get(2).is_some_and(|&s| s > 59) {
        return None;
    }
    Some(n[0] * 3600 + n[1] * 60 + n.get(2).copied().unwrap_or(0))
}

/// Hold the time of day at HH:mm[:ss]; false if it is not one.
pub fn hold(value: &str) -> bool {
    parse(value).inspect(|&t| TIME.store(t, Ordering::Relaxed)).is_some()
}

pub fn held() -> bool {
    TIME.load(Ordering::Relaxed) >= 0
}

/// The time of day as a fraction of the local day.
pub fn daytime() -> f64 {
    let t = TIME.load(Ordering::Relaxed);
    if t >= 0 {
        return t as f64 / 86400.0;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let off = local_offset(now);
    (((now + off).rem_euclid(86400)) as f64) / 86400.0
}

/// The local zone's current UTC offset in seconds, from /etc/localtime (TZif); 0 when unreadable.
fn local_offset(now: i64) -> i64 {
    std::fs::read("/etc/localtime").ok().and_then(|d| tz_offset(&d, now)).unwrap_or(0)
}

/// The UTC offset at `now` in a TZif file; None for a file that is not one or is cut short.
pub fn tz_offset(d: &[u8], now: i64) -> Option<i64> {
    if d.get(..4)? != b"TZif" {
        return None;
    }
    let be = |o: usize| Some(i32::from_be_bytes(d.get(o..o.checked_add(4)?)?.try_into().ok()?) as i64);
    let count = |o: usize| be(o).and_then(|v| usize::try_from(v).ok());
    let (isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt) = (count(20)?, count(24)?, count(28)?, count(32)?, count(36)?, count(40)?);
    let v1 = 44usize
        .checked_add(timecnt.checked_mul(5)?)?
        .checked_add(typecnt.checked_mul(6)?)?
        .checked_add(charcnt)?
        .checked_add(leapcnt.checked_mul(8)?)?
        .checked_add(isstdcnt)?
        .checked_add(isutcnt)?;
    // a version 2+ file repeats the data with 64-bit times after the version 1 block
    let (h, w) = if *d.get(4)? >= b'2' && v1.checked_add(44)? <= d.len() { (v1, 8) } else { (0, 4) };
    let tc = count(h + 32)?;
    let times = h + 44;
    let idx = times.checked_add(tc.checked_mul(w)?)?;
    let types = idx.checked_add(tc)?;
    let mut which = 0usize;
    for i in 0..tc {
        let at = times + i * w;
        let t = if w == 8 { i64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?) } else { be(at)? };
        if t <= now {
            which = *d.get(idx + i)? as usize;
        }
    }
    be(types.checked_add(which.checked_mul(6)?)?)
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// At fraction f of the day, with rise and set in hours: (sun, moon, elevation, light, dusk).
pub fn at(rise: f64, set: f64, f: f64) -> (f64, f64, f64, f64, f64) {
    let h = f.rem_euclid(1.0) * 24.0;
    let day = set - rise;
    let sun = (h - rise) / day;
    let moon = ((h - set + 2.0).rem_euclid(24.0) - 2.0) / (24.0 - day);
    let elevation = if (0.0..=1.0).contains(&sun) { (std::f64::consts::PI * sun).sin() } else { -(std::f64::consts::PI * moon.clamp(0.0, 1.0)).sin() };
    (sun, moon, elevation, smooth(-0.1, 0.22, elevation), (-(elevation / 0.13).powi(2)).exp())
}

/// "day" or "night" by the sun now, for a theme with looks; None for one without.
pub fn lighting_phase(l: &crate::config::Loaded) -> Option<String> {
    l.looks.as_ref()?;
    let [rise, set] = l.plain.tw.day;
    Some(if at(rise, set, daytime()).3 > 0.5 { "day" } else { "night" }.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version 1 zone file: one change, at `change`, from UTC+1 to UTC+2.
    fn zone(change: i32) -> Vec<u8> {
        let mut d = b"TZif\0".to_vec();
        d.resize(20, 0);
        for count in [0i32, 0, 0, 1, 2, 0] {
            d.extend_from_slice(&count.to_be_bytes());
        }
        d.extend_from_slice(&change.to_be_bytes());
        d.push(1); // after the change: the second type
        for offset in [3600i32, 7200] {
            d.extend_from_slice(&offset.to_be_bytes());
            d.extend_from_slice(&[0, 0]);
        }
        d
    }

    #[test]
    fn times_of_day() {
        assert_eq!(parse("00:00"), Some(0));
        assert_eq!(parse("7:05"), Some(7 * 3600 + 5 * 60));
        assert_eq!(parse("23:59:59"), Some(86_399));
        for bad in ["", "12", "12:5", "12:005", "24:00", "12:60", "12:00:60", "a:00", "12:00:"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_zone_file_gives_the_offset_at_a_time() {
        let d = zone(1000);
        assert_eq!(tz_offset(&d, 999), Some(3600));
        assert_eq!(tz_offset(&d, 1000), Some(7200));
        for n in 0..d.len() {
            assert!(tz_offset(&d[..n], 2000).is_none_or(|o| o == 7200), "cut at {n}");
        }
        assert_eq!(tz_offset(b"NotAZoneFile", 0), None);
    }

    #[test]
    fn the_sun_rises_and_sets() {
        let (sun, _, elevation, light, _) = at(6.0, 18.0, 0.5);
        assert!((sun - 0.5).abs() < 1e-9 && (elevation - 1.0).abs() < 1e-9 && light == 1.0);
        assert!(at(6.0, 18.0, 0.0).2 < 0.0, "below the horizon at midnight");
        assert!(at(6.0, 18.0, 0.0).3 < 1e-9, "no daylight at midnight");
    }
}
