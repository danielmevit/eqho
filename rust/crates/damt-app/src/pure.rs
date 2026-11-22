//! Pure helpers for the app shell: UTC timestamps and log-line formatting.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Format `time` as `yyyy-MM-ddTHH:mm:ss.fffZ` in UTC.
///
/// Times before the Unix epoch are clamped to the epoch. The log only ever passes the current time.
pub(crate) fn format_timestamp_utc(time: SystemTime) -> String {
    let elapsed = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let seconds = elapsed.as_secs();
    let milliseconds = elapsed.subsec_millis();

    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milliseconds:03}Z",
        remainder / 3_600,
        (remainder % 3_600) / 60,
        remainder % 60,
    )
}

/// Format one `[timestamp] context: message` line (no trailing newline).
pub(crate) fn format_line(time: SystemTime, context: Option<&str>, message: &str) -> String {
    let timestamp = format_timestamp_utc(time);
    match context.map(str::trim).filter(|value| !value.is_empty()) {
        Some(context) => format!("[{timestamp}] {context}: {message}"),
        None => format!("[{timestamp}] {message}"),
    }
}

/// Convert days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year + i64::from(month <= 2);

    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64, milliseconds: u32) -> SystemTime {
        UNIX_EPOCH + Duration::new(seconds, milliseconds * 1_000_000)
    }

    #[test]
    fn epoch_formats_as_expected() {
        assert_eq!(format_timestamp_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn milliseconds_are_three_digits() {
        assert_eq!(format_timestamp_utc(at(0, 7)), "1970-01-01T00:00:00.007Z");
        assert_eq!(format_timestamp_utc(at(0, 123)), "1970-01-01T00:00:00.123Z");
    }

    #[test]
    fn known_utc_instants_format_correctly() {
        assert_eq!(
            format_timestamp_utc(at(1_700_000_000, 0)),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(
            format_timestamp_utc(at(1_709_164_800, 0)),
            "2024-02-29T00:00:00.000Z"
        );
        assert_eq!(
            format_timestamp_utc(at(1_735_689_599, 999)),
            "2024-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn pre_epoch_is_clamped() {
        let before = UNIX_EPOCH - Duration::from_secs(5);
        assert_eq!(format_timestamp_utc(before), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn line_without_context_has_no_colon() {
        assert_eq!(
            format_line(UNIX_EPOCH, None, "boom"),
            "[1970-01-01T00:00:00.000Z] boom"
        );
        assert_eq!(
            format_line(UNIX_EPOCH, Some("   "), "boom"),
            "[1970-01-01T00:00:00.000Z] boom"
        );
    }

    #[test]
    fn line_with_context_has_context_and_message() {
        assert_eq!(
            format_line(UNIX_EPOCH, Some("startup"), "ready"),
            "[1970-01-01T00:00:00.000Z] startup: ready"
        );
    }
}
