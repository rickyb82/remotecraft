//! Text for people: dates and durations from the desk's stamps, in the language of the
//! interface. Stamps are UTC RFC 3339 (`core::time`); they are shown in the local time zone
//! of the machine (the browser's on the web).

use remotecraft_engine::core::chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, Utc};
use remotecraft_engine::core::time;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_IT: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
const MONTHS_LONG: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_LONG_IT: [&str; 12] =
    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];

fn local(stamp: &str) -> Option<DateTime<Local>> {
    time::parse_stamp(stamp).map(|t| t.with_timezone(&Local))
}

fn month_name(m: u32, italian: bool) -> &'static str {
    let table = if italian { MONTHS_IT } else { MONTHS };
    table.get((m as usize).clamp(1, 12) - 1).copied().unwrap_or("")
}

/// A short date for a list row: the time today, `12 Oct` this year, else `12/10/2025`.
pub fn short(stamp: &str, now: DateTime<Utc>, italian: bool) -> String {
    let Some(t) = local(stamp) else { return String::new() };
    let n = now.with_timezone(&Local);
    if t.date_naive() == n.date_naive() {
        return t.format("%H:%M").to_string();
    }
    if t.year() == n.year() {
        return format!("{} {}", t.day(), month_name(t.month(), italian));
    }
    if italian { t.format("%d/%m/%Y").to_string() } else { t.format("%Y-%m-%d").to_string() }
}

/// A full date and time: `12 Oct 2026, 09:30`.
pub fn long(stamp: &str, italian: bool) -> String {
    let Some(t) = local(stamp) else { return String::new() };
    format!("{} {} {}, {}", t.day(), month_name(t.month(), italian), t.year(), t.format("%H:%M"))
}

/// The date only: `12 Oct 2026`.
pub fn date(stamp: &str, italian: bool) -> String {
    let Some(t) = local(stamp) else { return String::new() };
    format!("{} {} {}", t.day(), month_name(t.month(), italian), t.year())
}

/// `09:30`.
pub fn clock(stamp: &str) -> String {
    local(stamp).map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// `October 2026` / `ottobre 2026` for a `YYYY-MM` key.
pub fn month_title(key: &str, italian: bool) -> String {
    let Some((y, m)) = key.split_once('-') else { return key.to_string() };
    let Ok(m) = m.parse::<usize>() else { return key.to_string() };
    let table = if italian { MONTHS_LONG_IT } else { MONTHS_LONG };
    match table.get(m.clamp(1, 12) - 1) {
        Some(name) => format!("{name} {y}"),
        None => key.to_string(),
    }
}

/// Minutes as `1:30` (hours:minutes); under an hour, `45 min`.
pub fn duration(minutes: u64) -> String {
    if minutes < 60 { format!("{minutes} min") } else { format!("{}:{:02}", minutes / 60, minutes % 60) }
}

/// Minutes as `1 h 30`, `45 min`, or `2 h` — for sentences.
pub fn duration_words(minutes: u64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// A time typed by a person for a form, as the engine accepts it, from a stamp:
/// `2026-10-13 09:30` in local time (empty for an unreadable stamp).
pub fn input(stamp: &str) -> String {
    local(stamp).map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// A time typed in a form (`13/10/2026 09:30`, `09:30`, `now`…) as the stamp the engine stores,
/// read in this machine's time zone (the browser's on the web), so what the technician types is
/// what is saved whatever zone the server runs in. `None` when unreadable.
pub fn stamp_from_input(text: &str, now: DateTime<Utc>) -> Option<String> {
    // Past year 9999 a stamp is written `+10000-…`: no longer RFC 3339, nor in the stamps' order.
    time::input_stamp(text, now).ok().filter(|s| time::parse_stamp(s).is_some())
}

/// The last moment a stamp can hold (stamps are four-digit years).
const LAST_STAMP: &str = "9999-12-31T23:59:59Z";

/// The end of a period typed in a form (`To`, included) as the stamp the engine compares with,
/// which is excluded: a date alone (`2026-10-31`, `31/10/2026`) counts the whole day, so it
/// sends the start of the next day; a time (`2026-10-31 18:00`) is that moment. `None` when
/// unreadable.
pub fn end_stamp_from_input(text: &str, now: DateTime<Utc>) -> Option<String> {
    let t = text.trim();
    // The date-only forms `core::time::parse_input` reads (at local midnight).
    for f in ["%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(day) = NaiveDate::parse_from_str(t, f) {
            if day.year() > 9999 {
                return None;
            }
            let next = day.succ_opt().filter(|d| d.year() <= 9999).and_then(|d| stamp_from_input(&d.format("%Y-%m-%d").to_string(), now));
            // The last day stamps can hold (`9999-12-31`, a common "no end"): up to its last
            // second, or to the last stamp when that second is already past it in UTC.
            return next.or_else(|| {
                (day.year() == 9999)
                    .then(|| stamp_from_input(&format!("{} 23:59:59", day.format("%Y-%m-%d")), now).unwrap_or_else(|| LAST_STAMP.to_string()))
            });
        }
    }
    stamp_from_input(t, now)
}

/// The end an excluded stamp stands for, for people: a local midnight is the end of the day
/// before (`2026-11-01T00:00` local → `31 Oct 2026`, what `2026-10-31` was typed as); any other
/// time is shown with its date and time. Empty when unreadable.
pub fn end_text(stamp: &str, italian: bool) -> String {
    let Some(t) = local(stamp) else { return String::new() };
    if t.time() != NaiveTime::MIN {
        return long(stamp, italian);
    }
    let day = t.date_naive().pred_opt().unwrap_or_else(|| t.date_naive());
    format!("{} {} {}", day.day(), month_name(day.month(), italian), day.year())
}

/// The days of month `month` (1–12) of `year` (31 for a month out of range).
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The first and the last day of the month `months_ago` months before `now`'s, as form inputs
/// (the end included, as the `To` fields read it).
pub fn month_bounds(now: DateTime<Utc>, months_ago: u32) -> (String, String) {
    let n = now.with_timezone(&Local);
    let total = i64::from(n.year()) * 12 + i64::from(n.month0()) - i64::from(months_ago);
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12));
    let m = u32::try_from(m0 + 1).unwrap_or(1);
    (format!("{y:04}-{m:02}-01"), format!("{y:04}-{m:02}-{:02}", days_in_month(y, m)))
}

/// `h:mm:ss` from a stamp to `now` (`0:00:00` when unreadable or in the future).
pub fn elapsed(started_at: &str, now: DateTime<Utc>) -> String {
    let secs = time::parse_stamp(started_at).map(|s| now.signed_duration_since(s).num_seconds().max(0)).unwrap_or(0);
    format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// `h:mm` from a stamp to `now`, for rows.
pub fn elapsed_short(started_at: &str, now: DateTime<Utc>) -> String {
    let secs = time::parse_stamp(started_at).map(|s| now.signed_duration_since(s).num_seconds().max(0)).unwrap_or(0);
    format!("{}:{:02}", secs / 3600, (secs / 60) % 60)
}

/// The bounds of the month of a stamp (local), as form inputs: `(first day, last day)`.
pub fn month_of(stamp: &str) -> Option<(String, String)> {
    let t = local(stamp)?;
    Some(month_bounds(t.with_timezone(&Utc), 0))
}

/// Joins CSV fields with `;`, quoting what needs it.
pub fn csv_line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| if f.contains([';', '"', '\n', '\r']) { format!("\"{}\"", f.replace('"', "\"\"")) } else { f.clone() })
        .collect::<Vec<_>>()
        .join(";")
}

/// Clips a name for a chip.
pub fn clipped(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Initials for an avatar: `Studio Tecnico Lunaria` → `SL`.
pub fn initials(name: &str) -> String {
    let words: Vec<char> = name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).collect();
    match words.as_slice() {
        [] => "?".into(),
        [a] => a.to_uppercase().to_string(),
        [a, .., z] => format!("{}{}", a.to_uppercase(), z.to_uppercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_durations() {
        let now = time::parse_stamp("2026-10-13T12:00:00Z").unwrap();
        assert_eq!(short("", now, true), "");
        assert_eq!(short("garbage", now, true), "");
        assert!(short("2026-10-13T11:00:00Z", now, true).contains(':'));
        let this_year = short("2026-03-02T11:00:00Z", now, true);
        assert!(this_year.ends_with("mar"), "{this_year}");
        // Another year shows the local date (the 2nd or the 3rd, depending on the zone).
        let other_year = short("2025-03-02T11:00:00Z", now, false);
        assert!(other_year == "2025-03-02" || other_year == "2025-03-03", "{other_year}");
        let day = long("2026-03-02T11:00:00Z", false);
        assert!(day.starts_with("2 Mar 2026, ") || day.starts_with("3 Mar 2026, "), "{day}");
        assert_eq!(duration(45), "45 min");
        assert_eq!(duration(90), "1:30");
        assert_eq!(duration(600), "10:00");
        assert_eq!(duration_words(120), "2 h");
        assert_eq!(duration_words(125), "2 h 05");
        assert_eq!(month_title("2026-10", true), "ottobre 2026");
        assert_eq!(month_title("2026-10", false), "October 2026");
        assert_eq!(month_title("x", false), "x");
        assert_eq!(month_title("2026-13", false), "December 2026");
        let (from, to) = month_bounds(now, 0);
        assert_eq!((from.as_str(), to.as_str()), ("2026-10-01", "2026-10-31"));
        assert_eq!(month_bounds(now, 1), ("2026-09-01".to_string(), "2026-09-30".to_string()));
        assert_eq!(month_bounds(now, 8), ("2026-02-01".to_string(), "2026-02-28".to_string()));
        assert_eq!(month_bounds(now, 10), ("2025-12-01".to_string(), "2025-12-31".to_string()));
        assert_eq!(initials("Studio Tecnico Lunaria"), "SL");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("élio"), "É");
        assert_eq!(clipped("abcdef", 4), "abc…");
        assert_eq!(clipped("abc", 4), "abc");
        assert_eq!(input("bad"), "");
        assert!(stamp_from_input("09:30", now).is_some_and(|s| s.ends_with('Z')));
        assert_eq!(elapsed("2026-10-13T10:58:30Z", now), "1:01:30");
        assert_eq!(elapsed("2026-10-13T13:00:00Z", now), "0:00:00");
        assert_eq!(elapsed("bad", now), "0:00:00");
        assert_eq!(elapsed_short("2026-10-13T10:58:30Z", now), "1:01");
        assert!(month_of("2026-10-13T12:00:00Z").is_some_and(|(a, b)| a == "2026-10-01" && b == "2026-10-31"));
        assert!(month_of("x").is_none());
        assert_eq!(csv_line(&["a".into(), "b;c".into()]), "a;\"b;c\"");
        assert!(stamp_from_input("ieri", now).is_none());
    }

    #[test]
    fn a_period_end_typed_as_a_date_counts_the_whole_day() {
        let now = time::parse_stamp("2026-10-13T12:00:00Z").unwrap();
        let next_day = stamp_from_input("2026-11-01", now);
        assert!(next_day.is_some());
        assert_eq!(end_stamp_from_input("2026-10-31", now), next_day);
        assert_eq!(end_stamp_from_input(" 31/10/2026 ", now), next_day);
        assert_eq!(end_stamp_from_input("2026-12-31", now), stamp_from_input("2027-01-01", now));
        assert_eq!(end_stamp_from_input("2028-02-28", now), stamp_from_input("2028-02-29", now));
        // A time is the moment itself.
        assert_eq!(end_stamp_from_input("2026-10-31 18:00", now), stamp_from_input("2026-10-31 18:00", now));
        assert_eq!(end_stamp_from_input("ieri", now), None);
        assert_eq!(end_stamp_from_input("9999-99-99", now), None);
        // `9999-12-31`, a common "no end": up to its last second, a stamp that still reads and
        // sorts as one (never `+10000-01-01…`), in either form.
        for to in ["9999-12-31", "31/12/9999"] {
            let end = end_stamp_from_input(to, now).unwrap_or_default();
            assert!(end.starts_with("9999-12-31T") && time::parse_stamp(&end).is_some(), "{to}: {end}");
            assert!("2026-10-13T12:00:00Z" < end.as_str(), "{end}");
            assert!(!end_text(&end, false).is_empty(), "{end}");
        }
        assert!(end_stamp_from_input("9999-12-30", now).is_some_and(|e| e.starts_with("9999-12-3")));
        // Past year 9999: a stamp that reads as one (east of UTC, the first hours of 10000 are
        // still 9999 in UTC), or nothing; a whole day past 9999 is nothing.
        for typed in ["10000-01-01", "+10000-01-01", "10000-01-01 10:00", "+10000-06-01T10:00:00Z", "99999-01-01"] {
            for got in [end_stamp_from_input(typed, now), stamp_from_input(typed, now)] {
                assert!(got.as_deref().is_none_or(|s| time::parse_stamp(s).is_some()), "{typed}: {got:?}");
            }
        }
        assert_eq!(end_stamp_from_input("10000-01-01", now), None);
        assert_eq!(end_stamp_from_input("01/01/10000", now), None);
        assert_eq!(end_stamp_from_input("", now), Some(time::stamp(now)), "as stamp_from_input: callers skip an empty bound");
        // Shown back as the day it was typed as.
        let shown = end_text(&next_day.unwrap_or_default(), false);
        assert_eq!(shown, "31 Oct 2026");
        assert_eq!(end_text(&end_stamp_from_input("2026-10-01", now).unwrap_or_default(), true), "1 ott 2026");
        let evening = stamp_from_input("2026-10-31 18:00", now).unwrap_or_default();
        assert_eq!(end_text(&evening, false), long(&evening, false));
        assert_eq!(end_text("bad", false), "");
        assert_eq!((days_in_month(2026, 2), days_in_month(2028, 2), days_in_month(1900, 2), days_in_month(2000, 2)), (28, 29, 28, 29));
        assert_eq!((days_in_month(2026, 4), days_in_month(2026, 12), days_in_month(2026, 13)), (30, 31, 31));
    }
}
