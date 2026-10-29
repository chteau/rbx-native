//! The update time as the user types and reads it: local wall-clock time,
//! `2026-10-30 18:00`, stored as Unix seconds.

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

const FORMAT: &str = "%Y-%m-%d %H:%M";

/// `text` in `tz`, as Unix seconds, if it is a time after `now`.
pub(crate) fn parse_in<Tz: TimeZone>(text: &str, tz: &Tz, now: i64) -> Result<i64, String> {
    let naive = NaiveDateTime::parse_from_str(text.trim(), FORMAT)
        .map_err(|_| "Write it as YYYY-MM-DD HH:MM".to_string())?;
    let at = tz
        .from_local_datetime(&naive)
        .earliest()
        .ok_or("That time is skipped by a clock change")?
        .timestamp();
    if at <= now {
        return Err("That time has already passed".to_string());
    }
    Ok(at)
}

pub(crate) fn parse(text: &str, now: i64) -> Result<i64, String> {
    parse_in(text, &Local, now)
}

/// Tomorrow at 18:00 local, the field's first value.
pub(crate) fn default_text(now: i64) -> String {
    let today = DateTime::from_timestamp(now, 0)
        .unwrap_or_default()
        .with_timezone(&Local)
        .date_naive();
    let tomorrow = today.succ_opt().unwrap_or(today);
    format!("{} 18:00", tomorrow.format("%Y-%m-%d"))
}

/// "Thu 30 Oct, 18:00" in `tz`.
pub(crate) fn label_in<Tz: TimeZone>(at: i64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    DateTime::from_timestamp(at, 0)
        .unwrap_or_default()
        .with_timezone(tz)
        .format("%a %-d %b, %H:%M")
        .to_string()
}

pub(crate) fn label(at: i64) -> String {
    label_in(at, &Local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn local_text_round_trips_through_a_zone() {
        let paris = FixedOffset::east_opt(3600).unwrap();
        let at = parse_in("2026-10-30 18:00", &paris, 0).unwrap();
        assert_eq!(at, 1_793_379_600);
        assert_eq!(label_in(at, &paris), "Fri 30 Oct, 18:00");
    }

    #[test]
    fn past_and_malformed_times_are_refused() {
        let utc = FixedOffset::east_opt(0).unwrap();
        assert!(parse_in("2026-10-30 18:00", &utc, 1_900_000_000)
            .unwrap_err()
            .contains("passed"));
        assert!(parse_in("30/10 6pm", &utc, 0).unwrap_err().contains("YYYY"));
    }

    #[test]
    fn the_default_parses_as_a_future_time() {
        let now = chrono::Utc::now().timestamp();
        assert!(parse(&default_text(now), now).is_ok());
    }
}
