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
            Self::It => it::translate(text).or_else(|| crate::module_label(text)).unwrap_or(text),
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
        let mut out =
            it::MESSAGES.iter().find_map(|(en, itx)| text.strip_prefix(en).map(|rest| format!("{itx}{rest}"))).unwrap_or_else(|| text.to_string());
        // Phrases after a name or a number (`x` is not a time, intervention 5 is already closed).
        for (en, itx) in it::INFIX {
            if out.contains(en) {
                out = out.replace(en, itx);
            }
        }
        out
    }
}

/// The shared labels the interface translates (kept in step with the `it` table by a test).
/// Each screen declares its own in its `LABELS_IT` table of `(English, Italian)` pairs.
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
    "Export saved (recorded in the audit log)",
    "Privacy review: {n} findings",
    "Opening RustDesk… If nothing happens, install the RustDesk client or run: {command}",
    "Privacy notice not delivered",
    "Privacy notice outdated",
    "Remote assistance consent missing",
    "Remote assistance consent withdrawn",
    "Remote assistance consent expired",
    "A PC has unattended access without consent",
    "Type a name…",
    "Name or RustDesk ID…",
    "No PC",
    "Now",
    "−1 h",
    "Yesterday",
    "Required",
    "Not a time: use 2026-10-13 09:30, 13/10/2026 09:30 or 09:30",
    "Saving…",
    "Remote",
    "On site",
    "Phone",
    "Lab",
    "Other",
    "Paper",
    "E-mail",
    "Online",
    "Verbal",
    "Given",
    "Expired",
    "Withdrawn",
    "Missing",
    "Copied",
    "Start intervention on the selection",
    "Stop my running intervention",
    "New record on this screen",
    "Log past work",
    "Edit the open record",
    "Connect to the open PC",
    "Record consent for the open customer",
    "Next / previous row",
    "Close the card, the form or the pane",
    "Customers, Devices, Interventions, Privacy, Reports",
    "Reload this screen's data",
    "Save the form, or start on the PC found by the search",
    "This help",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_has_an_italian_translation() {
        let mut seen: Vec<&str> = Vec::new();
        for label in LABELS {
            assert!(!seen.contains(label), "duplicate {label}");
            seen.push(label);
            assert!(it::translate(label).is_some_and(|t| !t.is_empty()), "missing Italian: {label}");
        }
        // A screen may repeat a label (shared or another screen's) only with the same Italian:
        // two translations of one English text would show differently from screen to screen.
        let mut italian: Vec<(&str, &str)> = LABELS.iter().filter_map(|l| it::translate(l).map(|t| (*l, t))).collect();
        for (en, itx) in crate::module_labels() {
            assert!(!en.is_empty() && !itx.is_empty(), "empty row {en:?} {itx:?}");
            if let Some((_, other)) = italian.iter().find(|(e, _)| *e == en) {
                assert_eq!(*other, itx, "label {en} translated two ways");
            } else {
                italian.push((en, itx));
            }
        }
        for c in remotecraft_engine::COMMANDS {
            assert!(it::translate(c.label).is_some_and(|t| !t.is_empty()), "missing Italian for command label: {}", c.label);
        }
        assert_eq!(Language::It.tr("Customers"), "Clienti");
        assert_eq!(Language::En.tr("Customers"), "Customers");
        assert_eq!(Language::It.fill("{n} customers", &[("n", "3")]), "3 clienti");
        assert_eq!(Language::It.tr_message("no customer 7"), "nessun cliente 7");
        assert_eq!(Language::It.tr_message("`ieri` is not a time: use 2026-10-13 09:30"), "`ieri` non è un orario: use 2026-10-13 09:30");
        assert!(
            Language::It.tr_message("intervention 5 is already closed (intervention.update to change it)").starts_with("intervento 5 è già chiuso (")
        );
        assert_eq!(
            Language::It.tr_message("the customer's remote_assistance consent is missing: record it first (privacy.consent), or give consent_override with the reason (e.g. the person agreed on the phone)"),
            "il consenso del cliente all'assistenza remota è mancante: registralo prima, oppure indica il motivo per procedere (e.g. the person agreed on the phone)"
        );
        assert_eq!(Language::En.tr_message("intervention 5 is already closed"), "intervention 5 is already closed");
        assert_eq!(Language::parse("it-IT"), Some(Language::It));
        assert_eq!(Language::parse("de"), None);
    }
}
