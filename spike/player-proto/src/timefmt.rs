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

#[cfg(test)]
mod tests {
    use super::*;

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
