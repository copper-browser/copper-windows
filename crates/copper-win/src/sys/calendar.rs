//! The user's calendar for the panels' dates (`Today`, `12 October`) and
//! times (`3:41 PM`): Windows' local time zone, the user's month names and
//! time style.

use super::from_wide;
use copper_shell_core::format::{Calendar, Civil};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Globalization::GetDateFormatEx;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::core::{PCWSTR, w};

/// Windows' calendar (`format::Calendar`).
pub struct WinCalendar;

/// Seconds from 1601-01-01 (FILETIME's epoch) to 1970-01-01.
const EPOCH_GAP: i64 = 11_644_473_600;

impl Calendar for WinCalendar {
    fn civil(unix: f64) -> Option<Civil> {
        if !unix.is_finite() {
            return None;
        }
        let ticks = (unix as i64)
            .checked_add(EPOCH_GAP)?
            .checked_mul(10_000_000)?;
        let ticks = u64::try_from(ticks).ok()?;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
        // SAFETY: conversions between locals; `None` is the current zone.
        unsafe {
            FileTimeToSystemTime(&ft, &mut utc).ok()?;
            SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
        }
        let (y, m, d) = (
            i32::from(local.wYear),
            u32::from(local.wMonth),
            u32::from(local.wDay),
        );
        Some(Civil {
            year: y,
            month: m,
            day: d,
            day_of_year: day_of_year(y, m, d),
            hour: u32::from(local.wHour),
            minute: u32::from(local.wMinute),
        })
    }

    fn month_name(month: u32) -> String {
        if !(1..=12).contains(&month) {
            return String::new();
        }
        let date = SYSTEMTIME {
            wYear: 2000,
            wMonth: month as u16,
            wDay: 1,
            ..Default::default()
        };
        let mut buf = [0u16; 64];
        // SAFETY: the user's locale, a valid date, a buffer of its length.
        let n = unsafe {
            GetDateFormatEx(
                PCWSTR::null(),
                Default::default(),
                Some(&date),
                w!("MMMM"),
                Some(&mut buf),
                PCWSTR::null(),
            )
        };
        if n > 0 {
            from_wide(&buf)
        } else {
            ENGLISH[month as usize - 1].to_string()
        }
    }
}

const ENGLISH: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The time of day in the user's short style (`When.clock`: `3:41 PM`,
/// `15:41`), in Windows' local time zone.
pub fn clock(unix: f64) -> String {
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    let Some(t) = WinCalendar::civil(unix) else {
        return String::new();
    };
    let time = SYSTEMTIME {
        wYear: t.year.clamp(1601, 30827) as u16,
        wMonth: t.month as u16,
        wDay: t.day as u16,
        wHour: t.hour as u16,
        wMinute: t.minute as u16,
        ..Default::default()
    };
    let mut buf = [0u16; 32];
    // SAFETY: the user's locale, a valid time, a buffer of its length.
    let n = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            Some(&time),
            PCWSTR::null(),
            Some(&mut buf),
        )
    };
    if n > 1 {
        String::from_utf16_lossy(&buf[..n as usize - 1])
    } else {
        format!("{:02}:{:02}", t.hour, t.minute)
    }
}

/// 1–366.
pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let m = month.clamp(1, 12) as usize;
    BEFORE[m - 1] + day + u32::from(leap && m > 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_of_the_year() {
        assert_eq!(day_of_year(2025, 1, 1), 1);
        assert_eq!(day_of_year(2025, 3, 1), 60);
        assert_eq!(day_of_year(2024, 3, 1), 61);
        assert_eq!(day_of_year(2024, 12, 31), 366);
        assert_eq!(day_of_year(1900, 3, 1), 60);
        assert_eq!(day_of_year(2000, 3, 1), 61);
    }
}
