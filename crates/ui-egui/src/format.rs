//! Text for people: dates and durations from the desk's stamps, in the language of the
//! interface. Stamps are UTC RFC 3339 (`core::time`); they are shown in the local time zone
//! of the machine (the browser's on the web).

use remotecraft_engine::core::chrono::{DateTime, Datelike, Local, Utc};
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

/// The first and the day after the last of the month of `now`, as form inputs.
pub fn month_bounds(now: DateTime<Utc>, months_ago: u32) -> (String, String) {
    let n = now.with_timezone(&Local);
    let total = i64::from(n.year()) * 12 + i64::from(n.month0()) - i64::from(months_ago);
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12));
    let next = total + 1;
    let (ny, nm0) = (next.div_euclid(12), next.rem_euclid(12));
    (format!("{y:04}-{:02}-01", m0 + 1), format!("{ny:04}-{:02}-01", nm0 + 1))
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
        assert_eq!(short("2025-03-02T11:00:00Z", now, false), "2025-03-02");
        assert!(long("2026-03-02T11:00:00Z", false).starts_with("2 Mar 2026, "));
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
        assert!(from.ends_with("-01") && to.ends_with("-01") && from < to, "{from} {to}");
        let (from, _) = month_bounds(now, 1);
        assert!(from.starts_with("2026-09"));
        assert_eq!(initials("Studio Tecnico Lunaria"), "SL");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("élio"), "É");
        assert_eq!(clipped("abcdef", 4), "abc…");
        assert_eq!(clipped("abc", 4), "abc");
        assert_eq!(input("bad"), "");
    }
}
