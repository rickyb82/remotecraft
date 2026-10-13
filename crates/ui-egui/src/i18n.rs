//! Interface languages. Labels are written in English in the code and looked up here; anything
//! without a translation shows in English. Messages with values use `{name}` placeholders.

mod it;

/// An interface language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    En,
    It,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::En, Self::It];

    pub const fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::It => "Italiano",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::It => "it",
        }
    }

    /// Reads a code or a locale tag (`it`, `it-IT`, `it_CH.UTF-8`).
    pub fn parse(tag: &str) -> Option<Self> {
        match tag.trim().split(['-', '_', '.']).next()?.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "it" => Some(Self::It),
            _ => None,
        }
    }

    /// The system's language when supported, else English.
    pub fn system() -> Self {
        sys_locale::get_locale().and_then(|tag| Self::parse(&tag)).unwrap_or_default()
    }

    /// Translates an interface label.
    pub fn tr(self, text: &str) -> &str {
        match self {
            Self::En => text,
            Self::It => it::translate(text).unwrap_or(text),
        }
    }

    /// Translates a message and fills its `{name}` placeholders.
    pub fn fill(self, text: &str, values: &[(&str, &str)]) -> String {
        let mut out = self.tr(text).to_string();
        for (k, v) in values {
            out = out.replace(&format!("{{{k}}}"), v);
        }
        out
    }

    /// Translates the engine's error messages by their leading phrase, keeping the details.
    pub fn tr_message(self, text: &str) -> String {
        if self == Self::En {
            return text.to_string();
        }
        for (en, itx) in it::MESSAGES {
            if let Some(rest) = text.strip_prefix(en) {
                return format!("{itx}{rest}");
            }
        }
        text.to_string()
    }
}

/// Every label the interface translates (kept in step with the `it` table by a test). The
/// view modules declare theirs in their own `LABELS` and the test walks them all.
pub const LABELS: &[&str] = &[
    "Customers",
    "Devices",
    "Interventions",
    "Privacy",
    "Reports",
    "Settings",
    "Search",
    "Search customers, PCs, RustDesk IDs…",
    "Light theme",
    "Dark theme",
    "Language",
    "System",
    "Light",
    "Dark",
    "Loading…",
    "Saved",
    "Error",
    "Cancel",
    "Save",
    "Close",
    "Done",
    "Back",
    "Yes",
    "No",
    "Delete",
    "Connecting…",
    "Server unreachable ({url}): {error}. Start it with: remotecraft-cli serve",
    "Working as {actor}",
    "{n} customers",
    "{n} devices",
    "{n} interventions",
    "Keyboard shortcuts",
    "Nothing here yet",
    "Start intervention",
    "Running",
    "No running interventions",
    "Exported",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_has_an_italian_translation() {
        let mut seen: Vec<&str> = Vec::new();
        for label in crate::all_labels() {
            assert!(!seen.contains(&label), "duplicate {label}");
            seen.push(label);
            assert!(it::translate(label).is_some_and(|t| !t.is_empty()), "missing Italian: {label}");
        }
        for c in remotecraft_engine::COMMANDS {
            assert!(it::translate(c.label).is_some_and(|t| !t.is_empty()), "missing Italian for command label: {}", c.label);
        }
        assert_eq!(Language::It.tr("Customers"), "Clienti");
        assert_eq!(Language::En.tr("Customers"), "Customers");
        assert_eq!(Language::It.fill("{n} customers", &[("n", "3")]), "3 clienti");
        assert_eq!(Language::It.tr_message("no customer 7"), "nessun cliente 7");
        assert_eq!(Language::parse("it-IT"), Some(Language::It));
        assert_eq!(Language::parse("de"), None);
    }
}
