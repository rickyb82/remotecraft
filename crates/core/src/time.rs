//! Times. Stored as UTC RFC 3339 stamps (`2026-10-13T09:30:00Z`), which sort as text; accepted
//! in the forms people type (`2026-10-13 09:30`, `13/10/2026 9:30`, `9:30` today) in the local
//! time zone of the machine (`TZ`).

use chrono::{DateTime, Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use crate::{Error, Result};

/// The current time.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// A stamp: `2026-10-13T09:30:00Z`.
pub fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Reads a stored stamp (any RFC 3339 time).
pub fn parse_stamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text.trim()).ok().map(|t| t.with_timezone(&Utc))
}

fn local(naive: NaiveDateTime) -> Result<DateTime<Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) => Ok(t.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        LocalResult::None => Err(Error::Invalid(format!("{naive} does not exist in the local time zone (clock change)"))),
    }
}

/// Reads a time typed by a person or sent by a program, relative to `now` for `HH:MM`:
/// RFC 3339 (`2026-10-13T09:30:00+02:00`), `2026-10-13 09:30`, `2026-10-13T09:30`,
/// `2026-10-13` (midnight), `13/10/2026 09:30`, `13/10/2026`, `09:30` (today), `now`/`adesso`.
pub fn parse_input(text: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let t = crate::clip(text.trim(), 64);
    if t.is_empty() || t.eq_ignore_ascii_case("now") || t.eq_ignore_ascii_case("adesso") || t.eq_ignore_ascii_case("ora") {
        return Ok(now);
    }
    if let Some(v) = parse_stamp(t) {
        return Ok(v);
    }
    for f in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%d/%m/%Y %H:%M:%S", "%d/%m/%Y %H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(t, f) {
            return local(n);
        }
    }
    for f in ["%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, f) {
            return local(d.and_time(NaiveTime::MIN));
        }
    }
    for f in ["%H:%M", "%H.%M", "%H:%M:%S"] {
        if let Ok(time) = NaiveTime::parse_from_str(t, f) {
            let today = now.with_timezone(&Local).date_naive();
            return local(today.and_time(time));
        }
    }
    Err(Error::Invalid(format!("`{t}` is not a time: use 2026-10-13 09:30, 13/10/2026 09:30, 09:30 or RFC 3339")))
}

/// [`parse_input`] as a stamp.
pub fn input_stamp(text: &str, now: DateTime<Utc>) -> Result<String> {
    parse_input(text, now).map(stamp)
}

/// Whole minutes from `a` to `b` (rounded; at least 1 when `b` is after `a`), or `None` when
/// `b` is before `a` or either stamp is unreadable.
pub fn minutes_between(a: &str, b: &str) -> Option<u32> {
    let (a, b) = (parse_stamp(a)?, parse_stamp(b)?);
    let secs = b.signed_duration_since(a).num_seconds();
    if secs < 0 {
        return None;
    }
    let minutes = (secs + 30) / 60;
    let minutes = if secs > 0 { minutes.max(1) } else { 0 };
    u32::try_from(minutes).ok()
}

/// A stamp shown in local time: `13/10/2026 09:30` (empty when unreadable).
pub fn local_text(stored: &str) -> String {
    parse_stamp(stored).map(|t| t.with_timezone(&Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
}

/// `YYYY-MM` of a stamp in local time (empty when unreadable), for monthly reports.
pub fn month(stored: &str) -> String {
    parse_stamp(stored).map(|t| t.with_timezone(&Local).format("%Y-%m").to_string()).unwrap_or_default()
}

/// The stamp `months` months after `stored` (the day clamped to the month's end).
pub fn add_months(stored: &str, months: u32) -> Option<String> {
    let t = parse_stamp(stored)?;
    let total = i64::from(t.year()) * 12 + i64::from(t.month0()) + i64::from(months);
    let year = i32::try_from(total.div_euclid(12)).ok()?;
    let month0 = u32::try_from(total.rem_euclid(12)).ok()?;
    let mut day = t.day();
    loop {
        if let Some(d) = NaiveDate::from_ymd_opt(year, month0 + 1, day) {
            return Some(stamp(Utc.from_utc_datetime(&d.and_time(t.time()))));
        }
        if day <= 28 {
            return None;
        }
        day -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        parse_stamp(s).unwrap()
    }

    #[test]
    fn stamps_round_trip_and_sort() {
        let t = at("2026-10-13T09:30:00+02:00");
        assert_eq!(stamp(t), "2026-10-13T07:30:00Z");
        assert!(stamp(at("2026-01-02T00:00:00Z")) < stamp(at("2026-10-01T00:00:00Z")));
    }

    #[test]
    fn inputs() {
        let now = at("2026-10-13T12:00:00Z");
        assert_eq!(input_stamp("now", now).unwrap(), "2026-10-13T12:00:00Z");
        assert_eq!(input_stamp("", now).unwrap(), "2026-10-13T12:00:00Z");
        assert_eq!(input_stamp("2026-10-13T09:30:00Z", now).unwrap(), "2026-10-13T09:30:00Z");
        for t in ["2026-10-13 09:30", "13/10/2026 09:30", "2026-10-13T09:30", "2026-10-13", "13/10/2026", "09:30", "9.30"] {
            assert!(parse_input(t, now).is_ok(), "{t}");
        }
        for t in ["ieri", "32/13/2026", "25:00", "2026-02-30", "\u{0}", &"9".repeat(10_000)] {
            assert!(parse_input(t, now).is_err(), "{t}");
        }
    }

    #[test]
    fn minutes() {
        assert_eq!(minutes_between("2026-10-13T09:00:00Z", "2026-10-13T09:45:00Z"), Some(45));
        assert_eq!(minutes_between("2026-10-13T09:00:00Z", "2026-10-13T09:00:10Z"), Some(1));
        assert_eq!(minutes_between("2026-10-13T09:00:00Z", "2026-10-13T09:00:00Z"), Some(0));
        assert_eq!(minutes_between("2026-10-13T10:00:00Z", "2026-10-13T09:00:00Z"), None);
        assert_eq!(minutes_between("x", "2026-10-13T09:00:00Z"), None);
    }

    #[test]
    fn months() {
        assert_eq!(add_months("2026-01-31T10:00:00Z", 1).unwrap(), "2026-02-28T10:00:00Z");
        assert_eq!(add_months("2026-10-13T10:00:00Z", 24).unwrap(), "2028-10-13T10:00:00Z");
        assert!(add_months("2026-10-13T10:00:00Z", u32::MAX).is_none());
        assert!(add_months("bad", 1).is_none());
    }
}
