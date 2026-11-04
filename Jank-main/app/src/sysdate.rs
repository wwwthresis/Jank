//! Today's date in local time (month, day). The standard library has no time zones, so on Linux we
//! call `localtime_r` from libc (already linked into every Rust binary); on other systems we fall back to UTC.

#[cfg(target_os = "linux")]
pub fn today() -> (u32, u32) {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const u8,
    }
    extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: `time(NULL)` only reads the clock; `localtime_r` is thread-safe and writes only into the struct we pass,
    // whose layout matches glibc/musl `struct tm` on 64-bit Linux.
    let ok = unsafe {
        let t = time(std::ptr::null_mut());
        !localtime_r(&t, &mut tm).is_null()
    };
    if ok && (1..=12).contains(&(tm.mon + 1)) && (1..=31).contains(&tm.mday) {
        (tm.mon as u32 + 1, tm.mday as u32)
    } else {
        utc_today()
    }
}
#[cfg(not(target_os = "linux"))]
pub fn today() -> (u32, u32) {
    utc_today()
}

/// Month and day in UTC (civil calendar derived from the day count since 1970-01-01).
pub fn utc_today() -> (u32, u32) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil((secs / 86_400) as i64)
}
fn civil(days: i64) -> (u32, u32) {
    // Howard Hinnant's `civil_from_days` algorithm
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let _ = era;
    (m, d)
}

#[cfg(test)]
mod tests {
    use super::civil;
    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1, 1)); // 1970-01-01
        assert_eq!(civil(19_723), (1, 1)); // 2024-01-01
        assert_eq!(civil(19_776), (2, 23)); // 2024-02-23
        assert_eq!(civil(19_782), (2, 29)); // 2024-02-29 (leap day)
        assert_eq!(civil(20_447), (12, 25)); // 2025-12-25
    }
}
