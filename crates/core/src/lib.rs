//! RemoteCraft's model (layer L0): customers and their contacts, their PCs with the RustDesk
//! ID, the interventions with times and notes, the privacy notices (informativa) and the GDPR
//! record of what each customer received and agreed to, the preferences, and the demo desk.
//!
//! Nothing here touches the network or the disk except [`storage`]; `remotecraft-engine`
//! drives the model with commands.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod mock;
pub mod model;
pub mod privacy;
pub mod rustdesk;
pub mod storage;
pub mod time;

pub use chrono;
pub use model::{Contact, Counters, Customer, CustomerKind, Data, Device, Intervention, InterventionKind, Preferences};
pub use privacy::{Method, Notice, PrivacyEvent, PrivacyKind, PurposeState};

/// Hard limits that keep hostile input (files, parameters, requests) from exhausting memory.
pub mod limits {
    /// Longest name, e-mail, phone, address, host name… kept.
    pub const MAX_FIELD: usize = 400;
    /// Longest free text (notes, summaries) kept.
    pub const MAX_NOTES: usize = 64 * 1024;
    /// Longest privacy notice text kept.
    pub const MAX_NOTICE: usize = 512 * 1024;
    /// Customers in a desk.
    pub const MAX_CUSTOMERS: usize = 100_000;
    /// Devices in a desk.
    pub const MAX_DEVICES: usize = 200_000;
    /// Interventions in a desk.
    pub const MAX_INTERVENTIONS: usize = 1_000_000;
    /// Privacy events in a desk.
    pub const MAX_PRIVACY_EVENTS: usize = 1_000_000;
    /// Notice versions in a desk.
    pub const MAX_NOTICES: usize = 1_000;
    /// Contacts and tags per customer.
    pub const MAX_CONTACTS: usize = 100;
    pub const MAX_TAGS: usize = 50;
    /// Longest single intervention (minutes): a week.
    pub const MAX_MINUTES: u32 = 7 * 24 * 60;
    /// Rows returned by one list command.
    pub const MAX_LIST: usize = 1_000;
    /// Largest desk file read from disk.
    pub const MAX_FILE: u64 = 1024 * 1024 * 1024;
}

/// Errors of the model.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum Error {
    /// A parameter is missing or invalid.
    #[error("{0}")]
    Invalid(String),
    /// A customer, device, intervention or notice was not found.
    #[error("{0}")]
    NotFound(String),
    /// The action is refused (a rule of the desk, e.g. consent).
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Truncates at a char boundary.
pub fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// Drops control characters (keeps newlines and tabs), trims and clips.
pub fn clean(text: &str, max: usize) -> String {
    let cleaned: String = text.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
    clip(cleaned.trim(), max).to_string()
}

/// One line of text: control characters and newlines become spaces, trimmed and clipped.
pub fn line(text: &str) -> String {
    let cleaned: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    clip(cleaned.trim(), limits::MAX_FIELD).to_string()
}

/// `Anna Rossi` → `anna-rossi`; at most 80 characters of ASCII letters, digits and dashes.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.trim().chars().take(80) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Lower case without accents, for searches (`Città` matches `citta`).
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// A plausible e-mail address (empty is fine: the field is optional).
pub fn valid_email(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let Some((local, domain)) = text.rsplit_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !text.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_char_boundaries() {
        assert_eq!(clip("città", 4), "citt");
        assert_eq!(clip("città", 5), "citt");
        assert_eq!(clip("", 3), "");
        assert_eq!(clip("€€€", 1), "");
    }

    #[test]
    fn clean_and_line_drop_controls() {
        assert_eq!(clean("  a\u{0}b\nc  ", 100), "ab\nc");
        assert_eq!(line("a\nb\u{7}c"), "a b c");
        assert_eq!(line(&"x".repeat(10_000)).len(), limits::MAX_FIELD);
    }

    #[test]
    fn slug_and_fold() {
        assert_eq!(slug("  Anna Rossi! "), "anna-rossi");
        assert_eq!(slug("///"), "");
        assert_eq!(fold("Città ÈLITE"), "citta elite");
    }

    #[test]
    fn emails() {
        assert!(valid_email(""));
        assert!(valid_email("anna@example.it"));
        assert!(!valid_email("anna@example"));
        assert!(!valid_email("anna example@x.it"));
        assert!(!valid_email("@x.it"));
    }
}
