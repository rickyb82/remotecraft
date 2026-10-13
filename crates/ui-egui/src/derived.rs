//! What the screens read every frame but the desk does not store: each customer's privacy
//! state, counts and warnings, the running timers, the known technicians and tags, the
//! review findings. Rebuilt when the data's revision changes (or a minute passed, since expiry
//! and the "timer left running" finding depend on the clock), never per frame.
//!
//! Also [`Cached`]: the slot a screen keeps for a command whose answer is not in the data
//! (report, audit log, RustDesk setup and check, status).

use std::collections::HashMap;

use remotecraft_engine::core::chrono::{DateTime, Utc};
use remotecraft_engine::core::privacy::{self, PurposeState};
use remotecraft_engine::core::{Customer, Data, fold, rustdesk, time};
use remotecraft_engine::views;
use serde_json::Value;
use web_time::Instant;

use crate::icons::Icon;
use crate::widgets::Tone;
use crate::{Completed, DeskApp, Intent};

/// Where a customer stands with the privacy notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeState {
    UpToDate { version: String, at: String, method: String },
    Outdated { old: String, new: String },
    NotDelivered { current: String },
    NoNotice,
}

/// A purpose's state for a customer, with the event behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurposeRow {
    pub id: String,
    pub state: PurposeState,
    /// The latest consent or withdrawal event, if any.
    pub event: Option<u64>,
}

/// What the screens show about a customer at a glance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerDerived {
    pub devices: usize,
    /// Active PCs with a RustDesk ID.
    pub remote_devices: bool,
    pub unattended_devices: Vec<u64>,
    pub interventions: usize,
    pub minutes: u64,
    pub running: usize,
    /// Stamp of the latest intervention (empty when none).
    pub last_intervention: String,
    pub privacy_events: usize,
    pub notice: NoticeState,
    pub purposes: Vec<PurposeRow>,
    /// The privacy dot of the customer's row: nothing when nothing is needed.
    pub dot: Option<(Icon, Tone)>,
    /// Why the dot is what it is: labels of `i18n::LABELS`, to translate with `app.tr`.
    pub reasons: Vec<&'static str>,
    /// Stamp after which the retention period passed (the review proposes erasure).
    pub retention_due: Option<String>,
}

/// One finding of the privacy review (`engine::views::review`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// `notice_missing`, `notice_outdated`, `remote_assistance_missing`, `unattended_access_expired`,
    /// `retention_due`, `timer_running`, `no_notice`…
    pub issue: String,
    pub customer: Option<u64>,
    pub intervention: Option<u64>,
    pub technician: String,
    /// The engine's sentence (English).
    pub detail: String,
}

/// The derived state of the whole desk.
#[derive(Clone, Debug)]
pub struct Derived {
    pub revision: u64,
    pub built: Instant,
    pub customers: HashMap<u64, CustomerDerived>,
    /// Running interventions, newest first.
    pub running: Vec<u64>,
    /// Everyone who ever recorded an intervention, plus the actor; sorted.
    pub technicians: Vec<String>,
    /// Every tag in use; sorted.
    pub tags: Vec<String>,
    pub findings: Vec<Finding>,
    /// The RustDesk server's name or key is unknown (nowhere to point the clients at).
    pub rustdesk_missing: bool,
}

impl Default for Derived {
    fn default() -> Self {
        Self {
            revision: u64::MAX,
            built: Instant::now(),
            customers: HashMap::new(),
            running: Vec::new(),
            technicians: Vec::new(),
            tags: Vec::new(),
            findings: Vec::new(),
            rustdesk_missing: true,
        }
    }
}

/// How long a build stays fresh without a revision change (the clock moves on).
const MAX_AGE_SECS: u64 = 60;

impl Derived {
    /// Whether a rebuild is due.
    pub fn stale(&self, revision: u64) -> bool {
        self.revision != revision || self.built.elapsed().as_secs() >= MAX_AGE_SECS
    }

    pub fn customer(&self, id: u64) -> Option<&CustomerDerived> {
        self.customers.get(&id)
    }

    /// The remote-assistance gate of a customer: the state and the event behind it.
    pub fn gate(&self, customer: u64) -> (PurposeState, Option<&PurposeRow>) {
        match self.customers.get(&customer).and_then(|c| c.purposes.iter().find(|p| p.id == "remote_assistance")) {
            Some(row) => (row.state, Some(row)),
            None => (PurposeState::Missing, None),
        }
    }

    /// A purpose's state for a customer.
    pub fn purpose(&self, customer: u64, purpose: &str) -> PurposeState {
        self.customers.get(&customer).and_then(|c| c.purposes.iter().find(|p| p.id == purpose)).map(|p| p.state).unwrap_or(PurposeState::Missing)
    }
}

/// Builds the derived state from the data; `defaults` is what the host knows about the
/// RustDesk server (`DeskSession::rustdesk_defaults`), `actor` the technician at work.
pub fn build(data: &Data, now: DateTime<Utc>, revision: u64, defaults: &(String, String), actor: &str) -> Derived {
    let prefs = &data.preferences;
    let now_stamp = time::stamp(now);
    let current = privacy::current_notice(&data.notices, now);
    let known = privacy::known_purposes(&data.privacy);
    let mut customers = HashMap::with_capacity(data.customers.len());
    for c in &data.customers {
        let devices: Vec<_> = data.devices.iter().filter(|d| d.customer_id == c.id && !d.retired).collect();
        let remote_devices = devices.iter().any(|d| !d.rustdesk_id.is_empty());
        let unattended_devices: Vec<u64> = devices.iter().filter(|d| d.unattended).map(|d| d.id).collect();
        let work: Vec<_> = data.interventions.iter().filter(|i| i.customer_id == c.id).collect();
        let remote_work = work.iter().any(|i| i.kind.is_remote());
        let delivery = privacy::last_delivery(&data.privacy, c.id);
        let notice = match (current, delivery) {
            (None, _) => NoticeState::NoNotice,
            (Some(cur), None) => NoticeState::NotDelivered { current: cur.version.clone() },
            (Some(cur), Some(d)) if d.notice_version == cur.version => {
                NoticeState::UpToDate { version: cur.version.clone(), at: d.at.clone(), method: method_name(d.method) }
            }
            (Some(cur), Some(d)) => NoticeState::Outdated { old: d.notice_version.clone(), new: cur.version.clone() },
        };
        let purposes: Vec<PurposeRow> = known
            .iter()
            .map(|p| {
                let (state, event) = privacy::purpose_state(&data.privacy, c.id, p, now, prefs.consent_validity_months);
                PurposeRow { id: p.clone(), state, event: event.map(|e| e.id) }
            })
            .collect();
        let remote = purposes.iter().find(|p| p.id == "remote_assistance").map(|p| p.state).unwrap_or(PurposeState::Missing);
        let unattended = purposes.iter().find(|p| p.id == "unattended_access").map(|p| p.state).unwrap_or(PurposeState::Missing);
        let mut reasons = Vec::new();
        let needs_remote = remote_devices || remote_work;
        match &notice {
            NoticeState::NotDelivered { .. } => reasons.push("Privacy notice not delivered"),
            NoticeState::Outdated { .. } => reasons.push("Privacy notice outdated"),
            _ => {}
        }
        match remote {
            PurposeState::Missing if needs_remote => reasons.push("Remote assistance consent missing"),
            PurposeState::Withdrawn if needs_remote => reasons.push("Remote assistance consent withdrawn"),
            PurposeState::Expired => reasons.push("Remote assistance consent expired"),
            _ => {}
        }
        if !unattended_devices.is_empty() && unattended != PurposeState::Given {
            reasons.push("A PC has unattended access without consent");
        }
        let dot = if c.erased_at.is_some() {
            None
        } else if needs_remote && matches!(remote, PurposeState::Missing | PurposeState::Withdrawn) {
            Some((Icon::ShieldOff, Tone::Bad))
        } else if !reasons.is_empty() {
            Some((Icon::Warning, Tone::Warn))
        } else if matches!(notice, NoticeState::UpToDate { .. }) && remote == PurposeState::Given {
            Some((Icon::ShieldCheck, Tone::Ok))
        } else {
            None
        };
        let last = work.iter().map(|i| i.started_at.as_str()).max().unwrap_or_default().to_string();
        let retention_due = if prefs.retention_years > 0 && c.erased_at.is_none() {
            let since = if last.is_empty() { c.created_at.as_str() } else { last.as_str() };
            time::add_months(since, prefs.retention_years.saturating_mul(12)).filter(|due| *due <= now_stamp).map(|_| since.to_string())
        } else {
            None
        };
        customers.insert(
            c.id,
            CustomerDerived {
                devices: devices.len(),
                remote_devices,
                unattended_devices,
                interventions: work.len(),
                minutes: work.iter().map(|i| u64::from(i.minutes)).sum(),
                running: work.iter().filter(|i| i.running()).count(),
                last_intervention: last,
                privacy_events: data.privacy.iter().filter(|e| e.customer_id == c.id).count(),
                notice,
                purposes,
                dot,
                reasons,
                retention_due,
            },
        );
    }
    let mut running: Vec<&remotecraft_engine::core::Intervention> = data.interventions.iter().filter(|i| i.running()).collect();
    running.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    let mut technicians: Vec<String> = data.interventions.iter().map(|i| i.technician.clone()).filter(|t| !t.is_empty()).collect();
    if !actor.is_empty() {
        technicians.push(actor.to_string());
    }
    technicians.sort();
    technicians.dedup();
    let mut tags: Vec<String> = data.customers.iter().filter(|c| c.erased_at.is_none()).flat_map(|c| c.tags.iter().cloned()).collect();
    tags.sort();
    tags.dedup();
    let findings = views::review(data, now)["findings"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|f| Finding {
                    issue: f["issue"].as_str().unwrap_or_default().to_string(),
                    customer: f["customer"].as_u64(),
                    intervention: f["intervention"].as_u64(),
                    technician: f["technician"].as_str().unwrap_or_default().to_string(),
                    detail: f["text"].as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let host = if prefs.rustdesk_host.is_empty() { &defaults.0 } else { &prefs.rustdesk_host };
    let key = if prefs.rustdesk_key.is_empty() { &defaults.1 } else { &prefs.rustdesk_key };
    Derived {
        revision,
        built: Instant::now(),
        customers,
        running: running.iter().map(|i| i.id).collect(),
        technicians,
        tags,
        findings,
        rustdesk_missing: host.is_empty() || key.is_empty(),
    }
}

/// A method as the engine spells it (`paper`, `email`…), for sentences.
pub fn method_name(m: remotecraft_engine::core::privacy::Method) -> String {
    serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Whether a customer matches the folded query, as `customer.list` matches: its own fields, or
/// one of its devices by RustDesk ID (`rid`), host name or exact name.
pub fn customer_filter(data: &Data, c: &Customer, fq: &str, rid: Option<&str>) -> bool {
    if fq.is_empty() {
        return true;
    }
    views::customer_matches(c, fq)
        || data
            .devices
            .iter()
            .filter(|d| d.customer_id == c.id)
            .any(|d| rid.is_some_and(|r| d.rustdesk_id == r) || (!d.hostname.is_empty() && fold(&d.hostname).contains(fq)) || fold(&d.name) == fq)
}

/// The normalised RustDesk ID of a query, when it is one.
pub fn rustdesk_query(search: &str) -> Option<String> {
    rustdesk::normalize(search.trim()).ok().filter(|r| !r.is_empty())
}

/// The slot of a command whose answer a screen shows (a report, the audit log…): the key it
/// was run with, the job in flight, the last answer.
#[derive(Clone, Debug, Default)]
pub struct Cached {
    /// `(revision, refresh serial, params)` of the last run.
    pub key: Option<(u64, u64, Value)>,
    pub job: Option<u64>,
    pub value: Option<Value>,
    pub error: String,
}

impl Cached {
    /// Whether the command should run (again) for `params` now.
    pub fn needs(&self, app: &DeskApp, params: &Value) -> bool {
        self.needs_key(app.session.revision(), app.refresh_serial, params)
    }

    fn needs_key(&self, revision: u64, serial: u64, params: &Value) -> bool {
        self.job.is_none() && self.key.as_ref().is_none_or(|(r, s, p)| *r != revision || *s != serial || p != params)
    }

    pub fn loading(&self) -> bool {
        self.job.is_some()
    }

    /// Applies a completion when it is this slot's; answers whether it was.
    pub fn finish(&mut self, c: &Completed) -> bool {
        if self.job != Some(c.job) {
            return false;
        }
        self.job = None;
        match &c.result {
            Ok(v) => {
                self.value = Some(v.clone());
                self.error.clear();
            }
            Err(e) => self.error = e.to_string(),
        }
        true
    }
}

/// Runs `command` for the slot `pick` selects when its key changed (see [`Cached::needs`]).
pub fn refresh(app: &mut DeskApp, pick: fn(&mut DeskApp) -> &mut Cached, command: &str, params: Value, intent: Intent) {
    let (revision, serial) = (app.session.revision(), app.refresh_serial);
    if !pick(app).needs_key(revision, serial, &params) {
        return;
    }
    let key = (revision, serial, params.clone());
    let job = app.run_as(command, params, intent);
    let slot = pick(app);
    slot.key = Some(key);
    slot.job = Some(job);
}

#[cfg(test)]
mod tests {
    use super::*;
    use remotecraft_engine::core::mock;

    #[test]
    fn derived_state_of_the_demo_desk() {
        let now = time::parse_stamp("2026-10-13T08:00:00Z").unwrap();
        let data = mock::demo(now);
        let d = build(&data, now, 1, &("rd.example".into(), "KEY".into()), "riccardo");
        assert!(!d.rustdesk_missing);
        assert_eq!(d.technicians, vec!["riccardo"]);
        assert!(d.tags.contains(&"contratto".to_string()));
        assert!(d.running.is_empty());
        let lunaria = d.customer(1).unwrap();
        assert_eq!(lunaria.devices, 3);
        assert!(lunaria.remote_devices);
        assert_eq!(lunaria.unattended_devices.len(), 2);
        assert_eq!(lunaria.interventions, 6);
        assert_eq!(lunaria.minutes, 320);
        assert!(matches!(lunaria.notice, NoticeState::UpToDate { .. }));
        assert_eq!(lunaria.dot, Some((Icon::ShieldCheck, Tone::Ok)));
        assert!(lunaria.reasons.is_empty());
        let elio = d.customer(3).unwrap();
        assert_eq!(elio.dot, Some((Icon::ShieldOff, Tone::Bad)));
        assert!(elio.reasons.contains(&"Remote assistance consent missing"));
        assert_eq!(d.gate(3).0, PurposeState::Missing);
        assert_eq!(d.gate(1).0, PurposeState::Given);
        assert!(d.findings.iter().any(|f| f.issue == "remote_assistance_missing" && f.customer == Some(3)));
        assert!(d.stale(2));
        assert!(!d.stale(1));
        let none = build(&Data::new(), now, 1, &(String::new(), String::new()), "");
        assert!(none.rustdesk_missing);
        assert!(none.findings.iter().any(|f| f.issue == "no_notice"));
        assert!(customer_filter(&data, data.customer(1).unwrap(), "100200300", Some("100200300")));
        assert!(customer_filter(&data, data.customer(1).unwrap(), "lun-seg", None));
        assert!(!customer_filter(&data, data.customer(2).unwrap(), "lun-seg", None));
        assert_eq!(rustdesk_query(" 100 200 300 ").as_deref(), Some("100200300"));
        assert!(rustdesk_query("corvo").is_none());
    }

    #[test]
    fn cached_slots_follow_their_job() {
        let mut app = DeskApp::demo();
        let mut slot = Cached::default();
        assert!(slot.needs(&app, &serde_json::json!({"a": 1})));
        let key = (app.session.revision(), app.refresh_serial, serde_json::json!({}));
        let job = app.run_as("app.status", serde_json::json!({}), Intent::None);
        slot.key = Some(key);
        slot.job = Some(job);
        assert!(slot.loading());
        let done = app.session.pump();
        let c = done.into_iter().next().unwrap();
        assert!(slot.finish(&c));
        assert!(!slot.loading());
        assert_eq!(slot.value.as_ref().unwrap()["name"], "remotecraft");
        assert!(!slot.needs(&app, &serde_json::json!({})));
        app.refresh_serial += 1;
        assert!(slot.needs(&app, &serde_json::json!({})));
        assert!(!slot.finish(&Completed { job: 999, command: String::new(), result: Ok(Value::Null) }));
    }
}
