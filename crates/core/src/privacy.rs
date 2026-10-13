//! The GDPR record: the versions of the privacy notice (informativa, art. 13), and for each
//! customer an append-only list of events — notice delivered, consent given, consent
//! withdrawn — with when, how, who signed and where the evidence is. The state of a purpose
//! is the latest event for it; nothing is ever overwritten, so the history is the proof
//! (accountability, art. 5(2) and 7(1)).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, time};

/// A version of the privacy notice the desk gives its customers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notice {
    pub id: u64,
    /// `2026-01`, `v3`…: what consents and deliveries refer to.
    pub version: String,
    pub title: String,
    pub text: String,
    /// From when it applies; the current notice is the latest one already in effect.
    pub effective_from: String,
    pub created_at: String,
    pub created_by: String,
}

/// What happened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyKind {
    /// The customer received (and acknowledged) the privacy notice.
    #[default]
    NoticeDelivered,
    ConsentGiven,
    ConsentWithdrawn,
}

/// How it happened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Signed on paper (keep the original or a scan: `evidence`).
    #[default]
    Paper,
    Email,
    /// A web form or the framework's own flow.
    Online,
    /// Said aloud (phone, at the start of a remote session): weak evidence, note who heard it.
    Verbal,
    Other,
}

impl Method {
    pub fn parse(text: &str) -> Option<Self> {
        match crate::fold(text.trim()).as_str() {
            "paper" | "carta" | "cartaceo" | "firma" | "signed" | "firmato" => Some(Self::Paper),
            "email" | "e-mail" | "mail" | "pec" => Some(Self::Email),
            "online" | "web" | "form" | "modulo online" => Some(Self::Online),
            "verbal" | "verbale" | "orale" | "telefono" | "phone" => Some(Self::Verbal),
            "other" | "altro" => Some(Self::Other),
            _ => None,
        }
    }
}

/// One event of a customer's privacy history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyEvent {
    pub id: u64,
    pub customer_id: u64,
    pub kind: PrivacyKind,
    /// The purpose of a consent (see [`PURPOSES`]); empty for a notice delivery.
    pub purpose: String,
    /// The notice version the person was given when this happened.
    pub notice_version: String,
    pub method: Method,
    /// When it happened (may be before it was recorded).
    pub at: String,
    /// A consent given for a limited time.
    pub expires_at: Option<String>,
    /// Who signed or agreed (the customer, or the person acting for a company).
    pub signer: String,
    /// Where the proof is: `scansione 2026/118`, a mail's subject, a form id.
    pub evidence: String,
    pub note: String,
    /// The technician who recorded it, and when.
    pub recorded_by: String,
    pub recorded_at: String,
}

/// The purposes the desk asks consent for: id, English and Italian description.
pub const PURPOSES: &[(&str, &str, &str)] = &[
    (
        "remote_assistance",
        "Remote assistance: a technician controls the PC with RustDesk, with the person present, and may see data on the screen",
        "Assistenza remota: un tecnico controlla il PC con RustDesk, con la persona presente, e può vedere i dati sullo schermo",
    ),
    (
        "unattended_access",
        "Unattended access: the technician may connect with a permanent password without someone at the PC (maintenance, updates)",
        "Accesso non presidiato: il tecnico può collegarsi con una password permanente senza nessuno al PC (manutenzione, aggiornamenti)",
    ),
    ("marketing", "Commercial communications about the desk's services", "Comunicazioni commerciali sui servizi offerti"),
    ("newsletter", "Newsletter", "Newsletter"),
];

/// A purpose id: one of [`PURPOSES`] or a custom one (`[a-z0-9_]`, up to 40 characters).
pub fn purpose(text: &str) -> Result<String> {
    let p: String = crate::slug(text).replace('-', "_");
    let p = match p.as_str() {
        "assistenza_remota" | "remote" | "assistenza" => "remote_assistance".to_string(),
        "accesso_non_presidiato" | "unattended" | "accesso_permanente" => "unattended_access".to_string(),
        _ => p,
    };
    if p.is_empty() || p.len() > 40 {
        return Err(Error::Invalid(format!("`{}` is not a purpose: use remote_assistance, unattended_access, marketing…", crate::clip(text, 40))));
    }
    Ok(p)
}

/// Where a purpose stands for a customer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PurposeState {
    Given,
    Withdrawn,
    /// Given, but past its `expires_at` or older than the validity the desk set.
    Expired,
    Missing,
}

/// The latest consent or withdrawal for `purpose`, and what it means at `now`.
pub fn purpose_state<'a>(
    events: &'a [PrivacyEvent],
    customer: u64,
    purpose: &str,
    now: DateTime<Utc>,
    validity_months: u32,
) -> (PurposeState, Option<&'a PrivacyEvent>) {
    let latest = events
        .iter()
        .filter(|e| e.customer_id == customer && e.purpose == purpose && e.kind != PrivacyKind::NoticeDelivered)
        .max_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
    let Some(e) = latest else { return (PurposeState::Missing, None) };
    if e.kind == PrivacyKind::ConsentWithdrawn {
        return (PurposeState::Withdrawn, Some(e));
    }
    let now = time::stamp(now);
    let expired_by_date = e.expires_at.as_deref().is_some_and(|x| x <= now.as_str());
    let expired_by_age = validity_months > 0 && time::add_months(&e.at, validity_months).is_some_and(|x| x <= now);
    if expired_by_date || expired_by_age { (PurposeState::Expired, Some(e)) } else { (PurposeState::Given, Some(e)) }
}

/// The notice in effect at `now`: the latest `effective_from` not after it.
pub fn current_notice(notices: &[Notice], now: DateTime<Utc>) -> Option<&Notice> {
    let now = time::stamp(now);
    notices.iter().filter(|n| n.effective_from <= now).max_by(|a, b| a.effective_from.cmp(&b.effective_from).then(a.id.cmp(&b.id)))
}

/// The last notice delivery recorded for a customer.
pub fn last_delivery(events: &[PrivacyEvent], customer: u64) -> Option<&PrivacyEvent> {
    events.iter().filter(|e| e.customer_id == customer && e.kind == PrivacyKind::NoticeDelivered).max_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)))
}

/// Every purpose with a known description, plus the custom ones used in `events`.
pub fn known_purposes(events: &[PrivacyEvent]) -> Vec<String> {
    let mut out: Vec<String> = PURPOSES.iter().map(|p| p.0.to_string()).collect();
    for e in events {
        if !e.purpose.is_empty() && !out.contains(&e.purpose) {
            out.push(e.purpose.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: u64, kind: PrivacyKind, at: &str) -> PrivacyEvent {
        PrivacyEvent { id, customer_id: 1, kind, purpose: "remote_assistance".into(), at: at.into(), ..PrivacyEvent::default() }
    }

    #[test]
    fn latest_event_wins() {
        let now = time::parse_stamp("2026-10-13T00:00:00Z").unwrap();
        let mut events = vec![ev(1, PrivacyKind::ConsentGiven, "2026-01-01T00:00:00Z")];
        assert_eq!(purpose_state(&events, 1, "remote_assistance", now, 0).0, PurposeState::Given);
        assert_eq!(purpose_state(&events, 2, "remote_assistance", now, 0).0, PurposeState::Missing);
        assert_eq!(purpose_state(&events, 1, "marketing", now, 0).0, PurposeState::Missing);
        assert_eq!(purpose_state(&events, 1, "remote_assistance", now, 6).0, PurposeState::Expired);
        events.push(ev(2, PrivacyKind::ConsentWithdrawn, "2026-05-01T00:00:00Z"));
        assert_eq!(purpose_state(&events, 1, "remote_assistance", now, 0).0, PurposeState::Withdrawn);
        events.push(ev(3, PrivacyKind::ConsentGiven, "2026-06-01T00:00:00Z"));
        assert_eq!(purpose_state(&events, 1, "remote_assistance", now, 0).0, PurposeState::Given);
        events[2].expires_at = Some("2026-09-01T00:00:00Z".into());
        assert_eq!(purpose_state(&events, 1, "remote_assistance", now, 0).0, PurposeState::Expired);
    }

    #[test]
    fn current_notice_is_the_latest_in_effect() {
        let now = time::parse_stamp("2026-10-13T00:00:00Z").unwrap();
        let n = |id, v: &str, from: &str| Notice { id, version: v.into(), effective_from: from.into(), ..Notice::default() };
        let notices = vec![n(1, "v1", "2025-01-01T00:00:00Z"), n(2, "v2", "2026-03-01T00:00:00Z"), n(3, "v3", "2027-01-01T00:00:00Z")];
        assert_eq!(current_notice(&notices, now).map(|n| n.version.as_str()), Some("v2"));
        assert!(current_notice(&[], now).is_none());
    }

    #[test]
    fn purposes_are_slugged() {
        assert_eq!(purpose("Assistenza remota").unwrap(), "remote_assistance");
        assert_eq!(purpose("Unattended").unwrap(), "unattended_access");
        assert_eq!(purpose("Ricerche di mercato").unwrap(), "ricerche_di_mercato");
        assert!(purpose("!!!").is_err());
        assert!(purpose(&"a".repeat(41)).is_err());
    }
}
