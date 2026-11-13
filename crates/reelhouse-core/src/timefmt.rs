//! Player time labels and seek-bar maths.

pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn seek_fraction(fraction: f64, duration: f64) -> f64 {
    fraction.clamp(0.0, 1.0) * duration
}

/// "20:30" for a Unix time. `offset_min` is JavaScript's `getTimezoneOffset()`: minutes to add to
/// local time to reach UTC, so UTC+4 is -240.
pub fn clock(unix_s: i64, offset_min: i64) -> String {
    let local = (unix_s - offset_min * 60).rem_euclid(86_400);
    format!("{:02}:{:02}", local / 3600, local / 60 % 60)
}

/// "Today", "Yesterday", "Tomorrow", or "Tue 10 Nov": the local day `unix_s` falls on, seen from
/// `now`. `offset_min` as for `clock`.
pub fn day(now: i64, unix_s: i64, offset_min: i64) -> String {
    let local_day = |t: i64| (t - offset_min * 60).div_euclid(86_400);
    let d = local_day(unix_s);
    match d - local_day(now) {
        0 => "Today".into(),
        -1 => "Yesterday".into(),
        1 => "Tomorrow".into(),
        _ => {
            // Days since 1970-01-01, a Thursday, to day and month (Howard Hinnant's civil_from_days).
            const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let z = d + 719_468;
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let dom = doy - (153 * mp + 2) / 5 + 1;
            let month = if mp < 10 { mp + 3 } else { mp - 9 };
            format!(
                "{} {dom} {}",
                DAYS[d.rem_euclid(7) as usize],
                MONTHS[(month - 1) as usize]
            )
        }
    }
}

/// "32 min left" or "1h 35m left" until `stop`, never less than a minute.
pub fn left(now: i64, stop: i64) -> String {
    let minutes = ((stop - now).max(0) + 59) / 60;
    let minutes = minutes.max(1);
    if minutes < 60 {
        format!("{minutes} min left")
    } else {
        format!("{}h {}m left", minutes / 60, minutes % 60)
    }
}

/// "just now", "5 min ago", "3 hours ago", "2 days ago": how long before `now` `then` was.
pub fn ago(now: i64, then: i64) -> String {
    let s = (now - then).max(0);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..7200 => "an hour ago".into(),
        7200..86400 => format!("{} hours ago", s / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", s / 86400),
    }
}

/// "45 min", "an hour", "5 hours", "3 days": how far ahead `until` is from `now`.
pub fn ahead(now: i64, until: i64) -> String {
    let s = (until - now).max(0);
    match s {
        0..3600 => format!("{} min", (s / 60).max(1)),
        3600..7200 => "an hour".into(),
        7200..172800 => format!("{} hours", s / 3600),
        _ => format!("{} days", s / 86400),
    }
}

/// Whether seeking to `target` in a live HLS conversion that began at `offset` and has
/// `converted` seconds so far needs a fresh conversion starting at `target`.
pub fn needs_restart(target: f64, offset: f64, converted: f64) -> bool {
    // Segments are 6 s; stay clear of the one still being written.
    const MARGIN: f64 = 2.0;
    target < offset || target > offset + converted - MARGIN
}

#[cfg(test)]
mod tests {

    #[test]
    fn day_names_the_local_day() {
        // 2026-11-13 12:00 UTC, a Friday.
        let noon = 1_794_571_200;
        assert_eq!(day(noon, noon - 3600, 0), "Today");
        assert_eq!(day(noon, noon - 86_400, 0), "Yesterday");
        assert_eq!(day(noon, noon + 86_400, 0), "Tomorrow");
        assert_eq!(day(noon, noon - 3 * 86_400, 0), "Tue 10 Nov");
        assert_eq!(day(noon, noon + 20 * 86_400, 0), "Thu 3 Dec");
        // 23:30 UTC is already tomorrow at UTC+1 (offset -60).
        assert_eq!(day(noon, noon + 11 * 3600 + 1800, 0), "Today");
        assert_eq!(day(noon, noon + 11 * 3600 + 1800, -60), "Tomorrow");
    }

    #[test]
    fn ahead_reads_naturally() {
        assert_eq!(ahead(1000, 1010), "1 min");
        assert_eq!(ahead(0, 45 * 60), "45 min");
        assert_eq!(ahead(0, 3600), "an hour");
        assert_eq!(ahead(0, 5 * 3600), "5 hours");
        assert_eq!(ahead(0, 3 * 86400), "3 days");
    }

    #[test]
    fn ago_reads_naturally() {
        assert_eq!(ago(1000, 990), "just now");
        assert_eq!(ago(1000, 1010), "just now");
        assert_eq!(ago(10_000, 10_000 - 5 * 60), "5 min ago");
        assert_eq!(ago(10_000, 10_000 - 3600), "an hour ago");
        assert_eq!(ago(100_000, 100_000 - 3 * 3600), "3 hours ago");
        assert_eq!(ago(200_000, 200_000 - 86400), "yesterday");
        assert_eq!(ago(900_000, 900_000 - 3 * 86400), "3 days ago");
    }

    use super::*;

    #[test]
    fn restart_only_outside_what_is_converted() {
        // Stream started at 600 s and has 30 s converted: 600..630 is reachable.
        assert!(!needs_restart(615.0, 600.0, 30.0));
        assert!(needs_restart(1200.0, 600.0, 30.0));
        assert!(needs_restart(100.0, 600.0, 30.0));
        // The last couple of seconds may not be playable yet.
        assert!(needs_restart(629.5, 600.0, 30.0));
    }

    #[test]
    fn clock_applies_the_local_offset() {
        // 2026-11-10 20:30 UTC; JavaScript reports UTC+4 as an offset of -240 minutes.
        assert_eq!(clock(1_794_342_600, -240), "00:30");
        assert_eq!(clock(1_794_342_600, 0), "20:30");
        assert_eq!(clock(1_794_342_600, 60), "19:30");
    }

    #[test]
    fn left_reads_in_minutes_or_hours() {
        assert_eq!(left(1000, 1000 + 32 * 60), "32 min left");
        assert_eq!(left(1000, 1000 + 95 * 60), "1h 35m left");
        assert_eq!(left(1000, 1030), "1 min left");
    }

    #[test]
    fn formats_hours() {
        assert_eq!(format_time(3725.0), "1:02:05");
    }

    #[test]
    fn formats_minutes() {
        assert_eq!(format_time(65.4), "1:05");
    }

    #[test]
    fn formats_nan_as_zero() {
        assert_eq!(format_time(f64::NAN), "0:00");
    }

    #[test]
    fn seek_fraction_clamps() {
        assert_eq!(seek_fraction(-0.2, 100.0), 0.0);
        assert_eq!(seek_fraction(1.5, 100.0), 100.0);
        assert_eq!(seek_fraction(0.25, 100.0), 25.0);
    }
}
