//! The panels' words for times (Mac `When` in `Recall.swift`):
//! a day's heading — `Today`, `Yesterday`, `12 October`, never a year — and
//! how long ago something was, the way `RelativeDateTimeFormatter`'s
//! abbreviated style says it (`5 min. ago`, `2 hr. ago`, `3 days ago`).
//! Platform-free; the calendar comes in through `format::Calendar`.

use copper_shell_core::format::{self, Calendar, Civil};

/// A day group's heading (`When.day`): the Mac's `d MMMM`, with no year.
pub fn day<C: Calendar>(unix: f64, now: f64) -> String {
    let d = format::day_number::<C>(unix);
    if d == format::day_number::<C>(now) {
        return "Today".into();
    }
    if d == format::day_number::<C>(now - 86_400.0) {
        return "Yesterday".into();
    }
    C::civil(unix)
        .map(|t| format!("{} {}", t.day, C::month_name(t.month)))
        .unwrap_or_default()
}

/// One local day, known from one moment in it: enough to group a
/// newest-first list by day while asking the calendar (a time-zone
/// conversion on Windows) only near each day's start, not once per entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DaySpan {
    /// `format::day_number` of the day (0 when the calendar had none).
    pub day: i64,
    /// The day's start as measured from that moment's wall clock: off by
    /// as much as a clock change that day (an hour) either way.
    start: f64,
}

impl DaySpan {
    /// The day `unix` is on, `civil` being its local time.
    pub fn of(unix: f64, civil: Option<&Civil>) -> DaySpan {
        match civil {
            Some(c) => DaySpan {
                day: i64::from(c.year) * 1000 + i64::from(c.day_of_year),
                start: unix - f64::from(c.hour * 3600 + c.minute * 60) - unix.rem_euclid(60.0),
            },
            None => DaySpan {
                day: 0,
                start: f64::INFINITY,
            },
        }
    }

    /// Whether `unix` — no later than the moment this was made from — is
    /// surely on the same day: at least an hour after the measured start,
    /// which no clock change of up to an hour can move across midnight.
    pub fn surely_holds(&self, unix: f64) -> bool {
        unix >= self.start + 3600.0
    }
}

/// How long ago `unix` was at `now` (`When.said`): the largest whole unit.
pub fn said(unix: f64, now: f64) -> String {
    let s = (now - unix).max(0.0);
    let (n, one, many) = if s < 60.0 {
        (s, "sec.", "sec.")
    } else if s < 3_600.0 {
        (s / 60.0, "min.", "min.")
    } else if s < 86_400.0 {
        (s / 3_600.0, "hr.", "hr.")
    } else if s < 7.0 * 86_400.0 {
        (s / 86_400.0, "day", "days")
    } else if s < 30.0 * 86_400.0 {
        (s / (7.0 * 86_400.0), "wk.", "wk.")
    } else if s < 365.0 * 86_400.0 {
        (s / (30.0 * 86_400.0), "mo.", "mo.")
    } else {
        (s / (365.0 * 86_400.0), "yr.", "yr.")
    };
    let n = n.floor() as u64;
    format!("{n} {} ago", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_shell_core::format::Civil;

    /// UTC with English month names.
    enum Utc {}

    impl Calendar for Utc {
        fn civil(unix: f64) -> Option<Civil> {
            let days = (unix / 86_400.0).floor() as i64;
            let secs = (unix - days as f64 * 86_400.0) as u32;
            // Howard Hinnant's civil_from_days.
            let z = days + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
            let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
            let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
            Some(Civil {
                year: y,
                month: m,
                day: d,
                day_of_year: day_of_year(y, m, d),
                hour: secs / 3600,
                minute: secs % 3600 / 60,
            })
        }

        fn month_name(month: u32) -> String {
            [
                "January", "February", "March", "April", "May", "June", "July",
            ]
            .get(month as usize - 1)
            .unwrap_or(&"Later")
            .to_string()
        }
    }

    fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
        const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        BEFORE[month as usize - 1] + day + u32::from(leap && month > 2)
    }

    /// 2025-03-10 12:00 UTC.
    const NOW: f64 = 1_741_608_000.0;

    #[test]
    fn day_headings_have_no_year() {
        assert_eq!(day::<Utc>(NOW - 60.0, NOW), "Today");
        assert_eq!(day::<Utc>(NOW - 86_400.0, NOW), "Yesterday");
        assert_eq!(day::<Utc>(NOW - 3.0 * 86_400.0, NOW), "7 March");
        // A year back still says only the day and month, as the Mac does.
        assert_eq!(day::<Utc>(NOW - 365.0 * 86_400.0, NOW), "10 March");
    }

    /// New York-like: UTC−5, then UTC−4 from `SPRING` (2025-03-09 07:00
    /// UTC, 02:00 local) and UTC−5 again from `FALL` (an hour change each
    /// way, a 23-hour and a 25-hour day).
    enum Shifty {}
    const SPRING: f64 = 1_741_503_600.0;
    const FALL: f64 = SPRING + 3.0 * 86_400.0;

    impl Calendar for Shifty {
        fn civil(unix: f64) -> Option<Civil> {
            let offset = if (SPRING..FALL).contains(&unix) {
                -4.0
            } else {
                -5.0
            };
            Utc::civil(unix + offset * 3600.0)
        }

        fn month_name(month: u32) -> String {
            Utc::month_name(month)
        }
    }

    /// Day numbers of a newest-first run, through [`DaySpan`] as the
    /// History list does it.
    fn spans<C: Calendar>(times: &[f64]) -> Vec<i64> {
        let mut out = Vec::new();
        let mut span: Option<DaySpan> = None;
        for &t in times {
            if !span.is_some_and(|s| s.surely_holds(t)) {
                span = Some(DaySpan::of(t, C::civil(t).as_ref()));
            }
            out.push(span.map_or(0, |s| s.day));
        }
        out
    }

    #[test]
    fn day_spans_group_like_asking_each_time() {
        // Every 7 minutes, newest first, over five days round both changes.
        let times: Vec<f64> = (0..1_100)
            .map(|i| FALL + 86_400.0 - f64::from(i) * 420.0)
            .collect();
        let naive: Vec<i64> = times
            .iter()
            .map(|&t| format::day_number::<Shifty>(t))
            .collect();
        assert_eq!(spans::<Shifty>(&times), naive);
        let naive: Vec<i64> = times
            .iter()
            .map(|&t| format::day_number::<Utc>(t))
            .collect();
        assert_eq!(spans::<Utc>(&times), naive);
        // And it does not ask once per entry.
        let asked = times
            .windows(2)
            .filter(|w| {
                let s = DaySpan::of(w[0], Shifty::civil(w[0]).as_ref());
                !s.surely_holds(w[1])
            })
            .count();
        assert!(asked < 200, "{asked}");
    }

    #[test]
    fn ago_in_the_largest_unit() {
        assert_eq!(said(NOW - 20.0, NOW), "20 sec. ago");
        assert_eq!(said(NOW - 300.0, NOW), "5 min. ago");
        assert_eq!(said(NOW - 7_200.0, NOW), "2 hr. ago");
        assert_eq!(said(NOW - 86_400.0, NOW), "1 day ago");
        assert_eq!(said(NOW - 3.0 * 86_400.0, NOW), "3 days ago");
        assert_eq!(said(NOW - 14.0 * 86_400.0, NOW), "2 wk. ago");
        assert_eq!(said(NOW - 90.0 * 86_400.0, NOW), "3 mo. ago");
        assert_eq!(said(NOW - 800.0 * 86_400.0, NOW), "2 yr. ago");
        assert_eq!(said(NOW + 5.0, NOW), "0 sec. ago");
    }
}
