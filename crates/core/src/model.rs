//! The desk's records: customers, their devices, the interventions and the preferences. Every
//! record is plain data (serde) with `#[serde(default)]`, so a newer file still loads.

use serde::{Deserialize, Serialize};

use crate::privacy::{Notice, PrivacyEvent};

/// A private person or a company.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerKind {
    #[default]
    Company,
    Person,
}

impl CustomerKind {
    pub fn parse(text: &str) -> Option<Self> {
        match crate::fold(text.trim()).as_str() {
            "company" | "azienda" | "societa" | "ditta" | "business" => Some(Self::Company),
            "person" | "persona" | "privato" | "private" | "individual" => Some(Self::Person),
            _ => None,
        }
    }
}

/// A person to talk to at a customer (a company's office manager, the IT contact…).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Contact {
    pub name: String,
    pub role: String,
    pub email: String,
    pub phone: String,
}

/// A customer of the support desk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Customer {
    pub id: u64,
    pub kind: CustomerKind,
    pub name: String,
    /// VAT number (partita IVA) or tax code (codice fiscale).
    pub tax_id: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub contacts: Vec<Contact>,
    pub tags: Vec<String>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    /// Hidden from lists (no longer a customer), data kept.
    pub archived: bool,
    /// Set when the personal data was erased on request (GDPR art. 17).
    pub erased_at: Option<String>,
    /// The customer's id in the operator's intranet, when imported from there (`customer.import`).
    pub intranet_id: Option<u64>,
}

/// A computer (or server, phone…) the desk looks after.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub id: u64,
    pub customer_id: u64,
    /// What people call it: `PC reception`, `Notebook Anna`.
    pub name: String,
    /// The RustDesk ID (canonical form, see [`crate::rustdesk::normalize`]); empty = none.
    pub rustdesk_id: String,
    pub hostname: String,
    pub os: String,
    pub serial: String,
    pub location: String,
    /// Who uses it.
    pub assigned_to: String,
    pub notes: String,
    /// A permanent RustDesk password is set on the client (unattended access). The password
    /// itself is never stored by RemoteCraft.
    pub unattended: bool,
    /// No longer in service: hidden from lists, history kept.
    pub retired: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// How an intervention was carried out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionKind {
    /// Remote control with RustDesk.
    #[default]
    Remote,
    /// At the customer's site.
    Onsite,
    /// Phone or chat support without remote control.
    Phone,
    /// In the lab (the device was brought in).
    Lab,
    Other,
}

impl InterventionKind {
    pub fn parse(text: &str) -> Option<Self> {
        match crate::fold(text.trim()).as_str() {
            "remote" | "remoto" | "remota" | "rustdesk" | "teleassistenza" => Some(Self::Remote),
            "onsite" | "on-site" | "on_site" | "sede" | "in sede" | "presso il cliente" | "trasferta" => Some(Self::Onsite),
            "phone" | "telefono" | "telefonica" | "chat" | "call" => Some(Self::Phone),
            "lab" | "laboratorio" => Some(Self::Lab),
            "other" | "altro" => Some(Self::Other),
            _ => None,
        }
    }

    /// Remote control of the customer's device (needs the remote-assistance consent).
    pub fn is_remote(self) -> bool {
        self == Self::Remote
    }
}

/// One piece of work for a customer: who, when, how long, what was done.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Intervention {
    pub id: u64,
    pub customer_id: u64,
    pub device_id: Option<u64>,
    pub kind: InterventionKind,
    pub technician: String,
    /// One line: what the request was.
    pub summary: String,
    /// What was done, found, left to do.
    pub notes: String,
    /// A ticket or job number in another system.
    pub reference: String,
    pub started_at: String,
    /// `None` while the timer runs.
    pub ended_at: Option<String>,
    /// Minutes worked: the span between start and end, or what the technician entered (pauses).
    pub minutes: u32,
    pub billable: bool,
    /// Why a remote intervention went ahead without a recorded consent.
    pub consent_override: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Intervention {
    pub fn running(&self) -> bool {
        self.ended_at.is_none()
    }
}

/// The desk's preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// `it` or `en`: the language of texts made for people (MCP summaries, reports).
    pub language: String,
    /// `light`, `dark` or `system`: the interface's theme.
    pub theme: String,
    /// The data controller (titolare del trattamento) named in exports.
    pub company_name: String,
    /// Billable time is rounded up to this many minutes per intervention (0 or 1 = exact).
    pub billing_unit_minutes: u32,
    /// Shortest billable intervention, in minutes (0 = none).
    pub minimum_minutes: u32,
    /// Remote interventions need the customer's `remote_assistance` consent (or a reason).
    pub require_consent: bool,
    /// Consents older than this many months count as expired (0 = never).
    pub consent_validity_months: u32,
    /// Customers without interventions for this many years are proposed for erasure (0 = never).
    pub retention_years: u32,
    /// The RustDesk server the clients use (shown in setup instructions).
    pub rustdesk_host: String,
    /// The server's public key (`id_ed25519.pub`).
    pub rustdesk_key: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: "it".into(),
            theme: "system".into(),
            company_name: String::new(),
            billing_unit_minutes: 15,
            minimum_minutes: 0,
            require_consent: true,
            consent_validity_months: 0,
            retention_years: 10,
            rustdesk_host: String::new(),
            rustdesk_key: String::new(),
        }
    }
}

impl Preferences {
    /// Billable minutes of one intervention: nothing while it runs or when not billable; else
    /// the minimum, then rounded up to the billing unit.
    pub fn billable_minutes(&self, i: &Intervention) -> u64 {
        if !i.billable || i.running() {
            return 0;
        }
        let m = u64::from(i.minutes.max(self.minimum_minutes));
        let unit = u64::from(self.billing_unit_minutes);
        if unit <= 1 { m } else { m.div_ceil(unit).saturating_mul(unit) }
    }
}

/// The last id given to each kind of record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counters {
    pub customer: u64,
    pub device: u64,
    pub intervention: u64,
    pub notice: u64,
    pub privacy: u64,
}

/// Everything the desk stores (`desk.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    /// Format version of the file.
    pub version: u32,
    pub counters: Counters,
    pub customers: Vec<Customer>,
    pub devices: Vec<Device>,
    pub interventions: Vec<Intervention>,
    pub notices: Vec<Notice>,
    pub privacy: Vec<PrivacyEvent>,
    pub preferences: Preferences,
}

/// The file format written by this version.
pub const FORMAT_VERSION: u32 = 1;

impl Data {
    pub fn new() -> Self {
        Self { version: FORMAT_VERSION, ..Self::default() }
    }

    pub fn customer(&self, id: u64) -> Option<&Customer> {
        self.customers.iter().find(|c| c.id == id)
    }

    pub fn customer_mut(&mut self, id: u64) -> Option<&mut Customer> {
        self.customers.iter_mut().find(|c| c.id == id)
    }

    pub fn device(&self, id: u64) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn device_mut(&mut self, id: u64) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn intervention(&self, id: u64) -> Option<&Intervention> {
        self.interventions.iter().find(|i| i.id == id)
    }

    pub fn intervention_mut(&mut self, id: u64) -> Option<&mut Intervention> {
        self.interventions.iter_mut().find(|i| i.id == id)
    }

    /// Checks that ids are unique and references point somewhere, and moves the counters past
    /// the highest ids (a file edited by hand must not make the next record reuse an id). A
    /// RustDesk ID that is not one (`123456&calc.exe`: it ends up in links and command lines)
    /// is cleared and reported; a valid one is kept in its canonical form.
    pub fn repair(&mut self) -> Vec<String> {
        let mut problems = Vec::new();
        let max = |ids: &mut dyn Iterator<Item = u64>| ids.max().unwrap_or(0);
        self.counters.customer = self.counters.customer.max(max(&mut self.customers.iter().map(|c| c.id)));
        self.counters.device = self.counters.device.max(max(&mut self.devices.iter().map(|d| d.id)));
        self.counters.intervention = self.counters.intervention.max(max(&mut self.interventions.iter().map(|i| i.id)));
        self.counters.notice = self.counters.notice.max(max(&mut self.notices.iter().map(|n| n.id)));
        self.counters.privacy = self.counters.privacy.max(max(&mut self.privacy.iter().map(|p| p.id)));
        let mut seen = std::collections::HashSet::new();
        self.customers.retain(|c| {
            let fresh = c.id != 0 && seen.insert(c.id);
            if !fresh {
                problems.push(format!("customer {} dropped: duplicate or zero id", c.id));
            }
            fresh
        });
        for d in &mut self.devices {
            match crate::rustdesk::normalize(&d.rustdesk_id) {
                Ok(id) => d.rustdesk_id = id,
                Err(_) => {
                    d.rustdesk_id.clear();
                    problems.push(format!("device {}: invalid RustDesk ID cleared", d.id));
                }
            }
        }
        let customers: std::collections::HashSet<u64> = self.customers.iter().map(|c| c.id).collect();
        for d in &self.devices {
            if !customers.contains(&d.customer_id) {
                problems.push(format!("device {} points to missing customer {}", d.id, d.customer_id));
            }
        }
        for i in &self.interventions {
            if !customers.contains(&i.customer_id) {
                problems.push(format!("intervention {} points to missing customer {}", i.id, i.customer_id));
            }
        }
        if self.version == 0 {
            self.version = FORMAT_VERSION;
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_parse_in_english_and_italian() {
        assert_eq!(CustomerKind::parse("Privato"), Some(CustomerKind::Person));
        assert_eq!(CustomerKind::parse("società"), Some(CustomerKind::Company));
        assert_eq!(InterventionKind::parse("in sede"), Some(InterventionKind::Onsite));
        assert_eq!(InterventionKind::parse("Laboratorio"), Some(InterventionKind::Lab));
        assert_eq!(InterventionKind::parse("??"), None);
    }

    #[test]
    fn old_or_partial_files_load() {
        let d: Data = serde_json::from_str(r#"{"customers":[{"id":3,"name":"X","unknown":1}],"devices":[{"id":1,"customer_id":9}]}"#).unwrap();
        let mut d = d;
        let problems = d.repair();
        assert_eq!(d.counters.customer, 3);
        assert_eq!(d.counters.device, 1);
        assert_eq!(d.version, FORMAT_VERSION);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn repair_drops_duplicate_ids() {
        let mut d = Data::new();
        d.customers.push(Customer { id: 1, ..Customer::default() });
        d.customers.push(Customer { id: 1, ..Customer::default() });
        d.customers.push(Customer { id: 0, ..Customer::default() });
        d.repair();
        assert_eq!(d.customers.len(), 1);
    }

    #[test]
    fn repair_clears_rustdesk_ids_that_are_not_ids() {
        // Edited by hand (a synced folder, a restored backup): the ID ends up in a
        // `rustdesk://` link the desktop app opens and in `rustdesk --connect`.
        let text = r#"{"customers":[{"id":1,"name":"X"}],"devices":[
            {"id":1,"customer_id":1,"rustdesk_id":"123456&calc.exe&rem"},
            {"id":2,"customer_id":1,"rustdesk_id":"123 456 789"},
            {"id":3,"customer_id":1,"rustdesk_id":"\"x\" & y"},
            {"id":4,"customer_id":1,"rustdesk_id":""}]}"#;
        let mut d: Data = serde_json::from_str(text).unwrap();
        let problems = d.repair();
        assert_eq!(problems, vec!["device 1: invalid RustDesk ID cleared", "device 3: invalid RustDesk ID cleared"]);
        let ids: Vec<&str> = d.devices.iter().map(|d| d.rustdesk_id.as_str()).collect();
        assert_eq!(ids, ["", "123456789", "", ""]);
        // Repaired data has nothing left to report.
        assert!(d.repair().is_empty());
    }

    #[test]
    fn billable_rounding() {
        let prefs = Preferences { billing_unit_minutes: 15, minimum_minutes: 10, ..Preferences::default() };
        let mut i = Intervention { minutes: 3, billable: true, ended_at: Some("x".into()), ..Intervention::default() };
        assert_eq!(prefs.billable_minutes(&i), 15);
        i.minutes = 16;
        assert_eq!(prefs.billable_minutes(&i), 30);
        i.billable = false;
        assert_eq!(prefs.billable_minutes(&i), 0);
        i.billable = true;
        i.ended_at = None;
        assert_eq!(prefs.billable_minutes(&i), 0);
        let exact = Preferences { billing_unit_minutes: 0, ..Preferences::default() };
        i.ended_at = Some("x".into());
        assert_eq!(exact.billable_minutes(&i), 16);
    }
}
