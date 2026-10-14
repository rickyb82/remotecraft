//! Finding records from what people and agents type, and the JSON the commands answer with.
//! Public so an interface can match and summarise records exactly as the engine does.

use std::collections::HashMap;

use remotecraft_core::chrono::{DateTime, Utc};
use remotecraft_core::privacy::{self, PURPOSES, PrivacyEvent, PrivacyKind, PurposeState};
use remotecraft_core::{Customer, Data, Device, Error, Intervention, Result, fold, rustdesk, time};
use serde_json::{Value, json};

/// How many candidates an ambiguous name lists.
const CANDIDATES: usize = 6;

fn as_id(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().trim_start_matches('#').parse().ok(),
        _ => None,
    }
}

fn ambiguous(what: &str, query: &str, names: Vec<String>) -> Error {
    Error::Invalid(format!("`{query}` matches several {what}: {} — use the id", names.into_iter().take(CANDIDATES).collect::<Vec<_>>().join("; ")))
}

/// A customer from an id (`12`, `"#12"`) or a name, e-mail, tax id or contact (unique match;
/// an exact name wins over partial matches). Erased customers are found by id only.
pub fn customer_ref(data: &Data, v: &Value) -> Result<u64> {
    if let Some(id) = as_id(v) {
        return data.customer(id).map(|c| c.id).ok_or_else(|| Error::NotFound(format!("no customer {id}")));
    }
    let Some(q) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(Error::Invalid("name the customer: an id or a name".into()));
    };
    let q = remotecraft_core::clip(q, 200);
    let fq = fold(q);
    let live = || data.customers.iter().filter(|c| c.erased_at.is_none());
    let exact: Vec<&Customer> = live()
        .filter(|c| fold(&c.name) == fq || (!c.email.is_empty() && fold(&c.email) == fq) || (!c.tax_id.is_empty() && fold(&c.tax_id) == fq))
        .collect();
    let pick = |list: Vec<&Customer>| -> Result<u64> {
        match list.as_slice() {
            [one] => Ok(one.id),
            [] => Err(Error::NotFound(format!("no customer matches `{q}`"))),
            many => Err(ambiguous("customers", q, many.iter().map(|c| format!("{} {}", c.id, c.name)).collect())),
        }
    };
    if !exact.is_empty() {
        return pick(exact);
    }
    pick(live().filter(|c| customer_matches(c, &fq)).collect())
}

/// Whether a customer's own fields contain the folded query.
pub fn customer_matches(c: &Customer, fq: &str) -> bool {
    [&c.name, &c.tax_id, &c.email, &c.phone, &c.address].iter().any(|f| fold(f).contains(fq))
        || c.contacts.iter().any(|k| fold(&k.name).contains(fq) || fold(&k.email).contains(fq) || fold(&k.phone).contains(fq))
        || c.tags.iter().any(|t| fold(t) == fq)
}

/// A device from an id, a RustDesk ID (`123 456 789`), a name or a host name (within
/// `customer` when given).
pub fn device_ref(data: &Data, v: &Value, customer: Option<u64>) -> Result<u64> {
    if let Value::Number(_) = v
        && let Some(id) = as_id(v)
    {
        return scoped_device(data, id, customer);
    }
    let Some(q) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(Error::Invalid("name the device: an id, a RustDesk ID or a name".into()));
    };
    let q = remotecraft_core::clip(q, 200);
    let scoped = || data.devices.iter().filter(|d| customer.is_none_or(|c| d.customer_id == c));
    if let Ok(rid) = rustdesk::normalize(q)
        && !rid.is_empty()
    {
        let by_rustdesk: Vec<&Device> = scoped().filter(|d| d.rustdesk_id == rid).collect();
        if let Some(d) = by_rustdesk.iter().find(|d| !d.retired).or(by_rustdesk.first()) {
            return Ok(d.id);
        }
    }
    // A short number is an id (`"7"`, `"#7"`), not a RustDesk ID.
    if let Some(id) = as_id(v)
        && data.device(id).is_some()
    {
        return scoped_device(data, id, customer);
    }
    let fq = fold(q);
    let exact: Vec<&Device> = scoped().filter(|d| fold(&d.name) == fq || (!d.hostname.is_empty() && fold(&d.hostname) == fq)).collect();
    let list = if exact.is_empty() {
        scoped().filter(|d| fold(&d.name).contains(&fq) || fold(&d.hostname).contains(&fq) || fold(&d.assigned_to).contains(&fq)).collect()
    } else {
        exact
    };
    let active: Vec<&Device> = list.iter().copied().filter(|d| !d.retired).collect();
    let list = if active.is_empty() { list } else { active };
    match list.as_slice() {
        [one] => Ok(one.id),
        [] => Err(Error::NotFound(format!("no device matches `{q}`"))),
        many => Err(ambiguous("devices", q, many.iter().map(|d| format!("{} {} ({})", d.id, d.name, customer_name(data, d.customer_id))).collect())),
    }
}

/// Device `id`, which must belong to `customer` when one is named.
fn scoped_device(data: &Data, id: u64, customer: Option<u64>) -> Result<u64> {
    let d = data.device(id).ok_or_else(|| Error::NotFound(format!("no device {id}")))?;
    match customer {
        Some(c) if d.customer_id != c => {
            Err(Error::NotFound(format!("no device {id} for customer {c} (it belongs to {})", customer_name(data, d.customer_id))))
        }
        _ => Ok(d.id),
    }
}

pub fn customer_name(data: &Data, id: u64) -> String {
    data.customer(id).map(|c| c.name.clone()).unwrap_or_default()
}

pub fn device_name(data: &Data, id: Option<u64>) -> String {
    id.and_then(|id| data.device(id)).map(|d| d.name.clone()).unwrap_or_default()
}

pub fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub fn customer_json(c: &Customer) -> Value {
    to_value(c)
}

/// A customer in a list: the essentials, the number of devices and the last intervention.
pub fn customer_row(data: &Data, c: &Customer) -> Value {
    let devices = data.devices.iter().filter(|d| d.customer_id == c.id && !d.retired).count();
    let last = data.interventions.iter().filter(|i| i.customer_id == c.id).map(|i| i.started_at.as_str()).max().unwrap_or_default();
    json!({
        "id": c.id, "kind": c.kind, "name": c.name, "tax_id": c.tax_id, "email": c.email, "phone": c.phone, "tags": c.tags,
        "devices": devices, "last_intervention_at": last, "archived": c.archived, "erased": c.erased_at.is_some(),
    })
}

pub fn device_json(data: &Data, d: &Device) -> Value {
    let mut v = to_value(d);
    v["customer_name"] = json!(customer_name(data, d.customer_id));
    v["rustdesk_display"] = json!(rustdesk::display(&d.rustdesk_id));
    if !d.rustdesk_id.is_empty() {
        v["connect_uri"] = json!(rustdesk::connect_uri(&d.rustdesk_id));
    }
    v
}

pub fn intervention_json(data: &Data, i: &Intervention, now: DateTime<Utc>) -> Value {
    let mut v = to_value(i);
    v["customer_name"] = json!(customer_name(data, i.customer_id));
    v["device_name"] = json!(device_name(data, i.device_id));
    v["running"] = json!(i.running());
    v["billable_minutes"] = json!(data.preferences.billable_minutes(i));
    v["started_local"] = json!(time::local_text(&i.started_at));
    if i.running() {
        v["elapsed_minutes"] = json!(time::minutes_between(&i.started_at, &time::stamp(now)).unwrap_or(0));
    }
    v
}

/// The description of a purpose in the desk's language.
pub fn purpose_text(id: &str, italian: bool) -> String {
    PURPOSES.iter().find(|p| p.0 == id).map(|p| if italian { p.2 } else { p.1 }).unwrap_or_default().to_string()
}

/// Where a customer stands: the notice delivered against the current one, and every purpose.
pub fn privacy_status(data: &Data, customer: u64, now: DateTime<Utc>) -> Value {
    let prefs = &data.preferences;
    let italian = prefs.language != "en";
    let current = privacy::current_notice(&data.notices, now);
    let delivered = privacy::last_delivery(&data.privacy, customer);
    let up_to_date = match (current, delivered) {
        (Some(c), Some(d)) => d.notice_version == c.version,
        (None, Some(_)) => true,
        (_, None) => false,
    };
    let purposes: Vec<Value> = privacy::known_purposes(&data.privacy)
        .into_iter()
        .map(|p| {
            let (state, event) = privacy::purpose_state(&data.privacy, customer, &p, now, prefs.consent_validity_months);
            json!({
                "purpose": p, "description": purpose_text(&p, italian), "state": state,
                "at": event.map(|e| e.at.clone()), "method": event.map(|e| e.method), "signer": event.map(|e| e.signer.clone()),
                "notice_version": event.map(|e| e.notice_version.clone()), "expires_at": event.and_then(|e| e.expires_at.clone()),
                "evidence": event.map(|e| e.evidence.clone()),
            })
        })
        .collect();
    let remote = privacy::purpose_state(&data.privacy, customer, "remote_assistance", now, prefs.consent_validity_months).0;
    json!({
        "customer_id": customer,
        "notice": {
            "current_version": current.map(|n| n.version.clone()),
            "delivered_version": delivered.map(|d| d.notice_version.clone()),
            "delivered_at": delivered.map(|d| d.at.clone()),
            "up_to_date": up_to_date,
        },
        "purposes": purposes,
        "remote_assistance": remote,
        "remote_ready": remote == PurposeState::Given,
    })
}

/// A purpose state as a word.
pub fn state_name(s: PurposeState) -> &'static str {
    match s {
        PurposeState::Given => "given",
        PurposeState::Withdrawn => "withdrawn",
        PurposeState::Expired => "expired",
        PurposeState::Missing => "missing",
    }
}

/// Whether a device's own fields contain the folded query (or its RustDesk ID equals it).
pub fn device_matches(d: &Device, fq: &str) -> bool {
    let rid = rustdesk::normalize(fq).ok().filter(|r| !r.is_empty());
    rid.is_some_and(|r| d.rustdesk_id == r)
        || [&d.name, &d.hostname, &d.os, &d.serial, &d.location, &d.assigned_to].iter().any(|f| fold(f).contains(fq))
}

/// Whether an intervention's texts (or its customer's or device's name) contain the folded query.
/// The customer and the device are looked up only when the intervention's own texts do not
/// match; [`intervention_matches_with`] answers the same from names folded in advance.
pub fn intervention_matches(data: &Data, i: &Intervention, fq: &str) -> bool {
    own_texts_match(i, fq) || {
        let device = i.device_id.and_then(|d| data.device(d)).map(|d| (fold(&d.name), d.rustdesk_id.as_str()));
        related_match(fq, &fold(&customer_name(data, i.customer_id)), device.as_ref().map(|(name, rid)| (name.as_str(), *rid)))
    }
}

/// [`intervention_matches`] with the lookups done by the caller, for an interface that matches
/// many interventions at once (every keystroke of a search): `customer` is the folded name of
/// the intervention's customer (empty when it has none), `device` the folded name and the
/// RustDesk ID of its device (`None` without one).
pub fn intervention_matches_with(i: &Intervention, fq: &str, customer: &str, device: Option<(&str, &str)>) -> bool {
    own_texts_match(i, fq) || related_match(fq, customer, device)
}

/// Whether the intervention's summary, notes, reference or technician contain the folded query.
fn own_texts_match(i: &Intervention, fq: &str) -> bool {
    fold(&i.summary).contains(fq) || fold(&i.notes).contains(fq) || fold(&i.reference).contains(fq) || fold(&i.technician).contains(fq)
}

/// Whether the folded customer name or device name contains the folded query (or the RustDesk
/// ID equals it).
fn related_match(fq: &str, customer: &str, device: Option<(&str, &str)>) -> bool {
    customer.contains(fq) || device.is_some_and(|(name, rid)| name.contains(fq) || rid == fq)
}

/// A desk's records grouped by customer in one pass, each group in the data's order: what a
/// view over every customer (the privacy review, an interface's per-customer summary) reads
/// instead of rescanning every device, intervention and privacy event for each customer, which
/// costs customers × records (seconds on a desk of a few thousand customers).
#[derive(Clone, Debug)]
pub struct ByCustomer<'a> {
    data: &'a Data,
    devices: HashMap<u64, Vec<&'a Device>>,
    interventions: HashMap<u64, Vec<&'a Intervention>>,
    privacy: HashMap<u64, Vec<&'a PrivacyEvent>>,
}

impl<'a> ByCustomer<'a> {
    pub fn new(data: &'a Data) -> Self {
        let mut devices: HashMap<u64, Vec<&Device>> = HashMap::new();
        for d in &data.devices {
            devices.entry(d.customer_id).or_default().push(d);
        }
        let mut interventions: HashMap<u64, Vec<&Intervention>> = HashMap::new();
        for i in &data.interventions {
            interventions.entry(i.customer_id).or_default().push(i);
        }
        let mut privacy: HashMap<u64, Vec<&PrivacyEvent>> = HashMap::new();
        for e in &data.privacy {
            privacy.entry(e.customer_id).or_default().push(e);
        }
        Self { data, devices, interventions, privacy }
    }

    /// The data the groups were made from.
    pub fn data(&self) -> &'a Data {
        self.data
    }

    /// A customer's devices, retired ones included.
    pub fn devices(&self, customer: u64) -> &[&'a Device] {
        self.devices.get(&customer).map_or(&[], Vec::as_slice)
    }

    /// A customer's interventions.
    pub fn interventions(&self, customer: u64) -> &[&'a Intervention] {
        self.interventions.get(&customer).map_or(&[], Vec::as_slice)
    }

    /// A customer's privacy events.
    pub fn privacy(&self, customer: u64) -> &[&'a PrivacyEvent] {
        self.privacy.get(&customer).map_or(&[], Vec::as_slice)
    }

    /// [`privacy::last_delivery`] over the customer's own events, picked as that function picks
    /// (the latest `at`, then the highest id; the group keeps the data's order, so ties end
    /// alike). `review_tests` checks that both answer the same.
    pub fn last_delivery(&self, customer: u64) -> Option<&'a PrivacyEvent> {
        self.privacy(customer).iter().copied().filter(|e| e.kind == PrivacyKind::NoticeDelivered).max_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)))
    }

    /// [`privacy::purpose_state`] over the customer's own events: the latest consent or
    /// withdrawal is picked here as that function picks it, and that function says what it
    /// means at `now`.
    pub fn purpose_state(&self, customer: u64, purpose: &str, now: DateTime<Utc>, validity_months: u32) -> (PurposeState, Option<&'a PrivacyEvent>) {
        let latest = self
            .privacy(customer)
            .iter()
            .copied()
            .filter(|e| e.purpose == purpose && e.kind != PrivacyKind::NoticeDelivered)
            .max_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
        match latest {
            Some(e) => privacy::purpose_state(std::slice::from_ref(e), customer, purpose, now, validity_months),
            None => (PurposeState::Missing, None),
        }
    }
}

/// What the GDPR record needs: notices to deliver, consents to collect, data past retention,
/// timers left running. The same list as `gdpr.review`, for interfaces that hold the data.
pub fn review(data: &Data, now: DateTime<Utc>) -> Value {
    review_grouped(&ByCustomer::new(data), now)
}

/// [`review`] from records already grouped by customer, for an interface that groups them once
/// for its own per-customer summaries too.
pub fn review_grouped(groups: &ByCustomer<'_>, now: DateTime<Utc>) -> Value {
    let data = groups.data();
    let prefs = &data.preferences;
    let current = privacy::current_notice(&data.notices, now);
    let mut findings = Vec::new();
    if current.is_none() {
        findings.push(json!({"issue": "no_notice", "text": "no privacy notice in the desk: add the text you give customers (notice.add)"}));
    }
    let now_stamp = time::stamp(now);
    for c in data.customers.iter().filter(|c| c.erased_at.is_none()) {
        let devices = groups.devices(c.id);
        let work = groups.interventions(c.id);
        let mut add = |issue: &str, text: String| findings.push(json!({"customer": c.id, "name": c.name, "issue": issue, "text": text}));
        if !c.archived
            && let Some(cur) = current
        {
            match groups.last_delivery(c.id) {
                None => add("notice_missing", format!("privacy notice {} not delivered", cur.version)),
                Some(e) if e.notice_version != cur.version => {
                    add("notice_outdated", format!("has notice {}, current is {}", e.notice_version, cur.version))
                }
                Some(_) => {}
            }
        }
        let remote_devices = devices.iter().any(|d| !d.retired && !d.rustdesk_id.is_empty());
        let remote_work = work.iter().any(|i| i.kind.is_remote());
        let unattended_devices = devices.iter().any(|d| d.unattended && !d.retired);
        for purpose in ["remote_assistance", "unattended_access"] {
            let (state, _) = groups.purpose_state(c.id, purpose, now, prefs.consent_validity_months);
            let needed = match purpose {
                "remote_assistance" => !c.archived && (remote_devices || remote_work),
                _ => unattended_devices,
            };
            if needed && state != PurposeState::Given {
                add(
                    &format!("{purpose}_{}", state_name(state)),
                    format!(
                        "{purpose} consent is {} but the customer has {}",
                        state_name(state),
                        if purpose == "remote_assistance" { "PCs with RustDesk or remote interventions" } else { "devices with unattended access" }
                    ),
                );
            }
        }
        if prefs.retention_years > 0 {
            let last = work.iter().map(|i| i.started_at.as_str()).max().unwrap_or(c.created_at.as_str());
            if let Some(due) = time::add_months(last, prefs.retention_years.saturating_mul(12))
                && due <= now_stamp
            {
                add(
                    "retention_due",
                    format!("no activity since {}: past the {}-year retention, consider gdpr.erase", time::local_text(last), prefs.retention_years),
                );
            }
        }
    }
    for i in data.interventions.iter().filter(|i| i.running()) {
        if time::minutes_between(&i.started_at, &now_stamp).unwrap_or(0) > 12 * 60 {
            findings.push(json!({"intervention": i.id, "technician": i.technician, "issue": "timer_running", "text": format!("running since {}: stop it or fix the times", time::local_text(&i.started_at))}));
        }
    }
    json!({"findings": findings, "count": findings.len()})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Data {
        remotecraft_core::mock::demo(time::now())
    }

    #[test]
    fn customers_by_id_name_and_fields() {
        let d = demo();
        assert_eq!(customer_ref(&d, &json!(2)).unwrap(), 2);
        assert_eq!(customer_ref(&d, &json!("#2")).unwrap(), 2);
        assert_eq!(customer_ref(&d, &json!("corvo")).unwrap(), 2);
        assert_eq!(customer_ref(&d, &json!("studio tecnico lunaria")).unwrap(), 1);
        assert_eq!(customer_ref(&d, &json!("Marta")).unwrap(), 1);
        assert_eq!(customer_ref(&d, &json!("IT09876543210")).unwrap(), 2);
        assert!(customer_ref(&d, &json!("example")).is_err()); // ambiguous
        assert!(customer_ref(&d, &json!("nessuno")).is_err());
        assert!(customer_ref(&d, &json!(99)).is_err());
        assert!(customer_ref(&d, &json!([1])).is_err());
        assert!(customer_ref(&d, &json!("")).is_err());
    }

    #[test]
    fn devices_by_rustdesk_id_and_name() {
        let d = demo();
        assert_eq!(device_ref(&d, &json!("100 200 300"), None).unwrap(), 1);
        assert_eq!(device_ref(&d, &json!(4), None).unwrap(), 4);
        assert_eq!(device_ref(&d, &json!("4"), None).unwrap(), 4);
        assert_eq!(device_ref(&d, &json!("pc cassa"), None).unwrap(), 4);
        assert_eq!(device_ref(&d, &json!("LUN-CAD01"), None).unwrap(), 2);
        assert!(device_ref(&d, &json!("pc"), None).is_err());
        assert_eq!(device_ref(&d, &json!("notebook"), Some(3)).unwrap(), 5);
        assert!(device_ref(&d, &json!("pc cassa"), Some(1)).is_err());
    }

    #[test]
    fn matching_and_review_from_the_data() {
        let d = demo();
        assert!(device_matches(d.device(1).unwrap(), "100 200 300"));
        assert!(device_matches(d.device(1).unwrap(), "lun-seg"));
        assert!(!device_matches(d.device(1).unwrap(), "cassa"));
        assert!(intervention_matches(&d, d.intervention(1).unwrap(), "stampante"));
        assert!(intervention_matches(&d, d.intervention(1).unwrap(), "lunaria"));
        assert!(!intervention_matches(&d, d.intervention(1).unwrap(), "nulla"));
        let r = review(&d, time::now());
        assert!(r["count"].as_u64().unwrap() >= 1);
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use remotecraft_core::chrono::TimeDelta;
    use remotecraft_core::{InterventionKind, Method, Notice};

    /// The review as it was before the records were grouped by customer (every customer
    /// rescans every record): the grouped one must answer exactly the same.
    fn review_by_scanning(data: &Data, now: DateTime<Utc>) -> Value {
        let prefs = &data.preferences;
        let current = privacy::current_notice(&data.notices, now);
        let mut findings = Vec::new();
        if current.is_none() {
            findings.push(json!({"issue": "no_notice", "text": "no privacy notice in the desk: add the text you give customers (notice.add)"}));
        }
        let now_stamp = time::stamp(now);
        for c in data.customers.iter().filter(|c| c.erased_at.is_none()) {
            let mut add = |issue: &str, text: String| findings.push(json!({"customer": c.id, "name": c.name, "issue": issue, "text": text}));
            if !c.archived
                && let Some(cur) = current
            {
                match privacy::last_delivery(&data.privacy, c.id) {
                    None => add("notice_missing", format!("privacy notice {} not delivered", cur.version)),
                    Some(e) if e.notice_version != cur.version => {
                        add("notice_outdated", format!("has notice {}, current is {}", e.notice_version, cur.version))
                    }
                    Some(_) => {}
                }
            }
            let remote_devices = data.devices.iter().any(|d| d.customer_id == c.id && !d.retired && !d.rustdesk_id.is_empty());
            let remote_work = data.interventions.iter().any(|i| i.customer_id == c.id && i.kind.is_remote());
            for purpose in ["remote_assistance", "unattended_access"] {
                let (state, _) = privacy::purpose_state(&data.privacy, c.id, purpose, now, prefs.consent_validity_months);
                let needed = match purpose {
                    "remote_assistance" => !c.archived && (remote_devices || remote_work),
                    _ => data.devices.iter().any(|d| d.customer_id == c.id && d.unattended && !d.retired),
                };
                if needed && state != PurposeState::Given {
                    add(
                        &format!("{purpose}_{}", state_name(state)),
                        format!(
                            "{purpose} consent is {} but the customer has {}",
                            state_name(state),
                            if purpose == "remote_assistance" {
                                "PCs with RustDesk or remote interventions"
                            } else {
                                "devices with unattended access"
                            }
                        ),
                    );
                }
            }
            if prefs.retention_years > 0 {
                let last =
                    data.interventions.iter().filter(|i| i.customer_id == c.id).map(|i| i.started_at.as_str()).max().unwrap_or(c.created_at.as_str());
                if let Some(due) = time::add_months(last, prefs.retention_years.saturating_mul(12))
                    && due <= now_stamp
                {
                    add(
                        "retention_due",
                        format!(
                            "no activity since {}: past the {}-year retention, consider gdpr.erase",
                            time::local_text(last),
                            prefs.retention_years
                        ),
                    );
                }
            }
        }
        for i in data.interventions.iter().filter(|i| i.running()) {
            if time::minutes_between(&i.started_at, &now_stamp).unwrap_or(0) > 12 * 60 {
                findings.push(json!({"intervention": i.id, "technician": i.technician, "issue": "timer_running", "text": format!("running since {}: stop it or fix the times", time::local_text(&i.started_at))}));
            }
        }
        json!({"findings": findings, "count": findings.len()})
    }

    /// A deterministic desk of `n` customers with every case the review looks at: archived and
    /// erased customers, a repeated customer id, PCs with and without RustDesk, unattended,
    /// retired or of no known customer, remote and other work, timers running for minutes or
    /// days, deliveries of old and current notices, consents given, withdrawn, expired by date
    /// or by age and of custom purposes, events interleaved across customers and tied on time
    /// (and id), old customers past retention.
    fn synthetic(n: u64, now: DateTime<Utc>) -> Data {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut rnd = move |m: u64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % m.max(1)
        };
        let ago = |minutes: u64| time::stamp(now - TimeDelta::minutes(i64::try_from(minutes).unwrap_or(0)));
        let mut data = Data::new();
        data.preferences.consent_validity_months = 24;
        data.preferences.retention_years = 10;
        for (id, version, days) in [(1, "v1", 4000), (2, "v2", 30)] {
            data.notices.push(Notice { id, version: version.into(), effective_from: ago(days * 1440), ..Notice::default() });
        }
        data.notices.push(Notice { id: 3, version: "v3".into(), effective_from: time::stamp(now + TimeDelta::days(10)), ..Notice::default() });
        let kinds = [InterventionKind::Remote, InterventionKind::Onsite, InterventionKind::Phone, InterventionKind::Lab, InterventionKind::Other];
        let (mut device_id, mut work_id) = (0, 0);
        for i in 0..n {
            let id = if i > 0 && i % 97 == 0 { i } else { i + 1 };
            data.customers.push(Customer {
                id,
                name: format!("Cliente {i}"),
                archived: rnd(7) == 0,
                erased_at: (rnd(11) == 0).then(|| ago(10)),
                created_at: ago(rnd(6000) * 1440),
                ..Customer::default()
            });
            for _ in 0..rnd(4) {
                device_id += 1;
                let customer_id = if rnd(50) == 0 { n + 10 } else { id };
                data.devices.push(Device {
                    id: device_id,
                    customer_id,
                    rustdesk_id: if rnd(3) == 0 { String::new() } else { format!("{}", 100_000_000 + device_id) },
                    unattended: rnd(4) == 0,
                    retired: rnd(6) == 0,
                    ..Device::default()
                });
            }
            for _ in 0..rnd(40) {
                work_id += 1;
                let started = rnd(5000 * 1440);
                let running = rnd(30) == 0;
                data.interventions.push(Intervention {
                    id: work_id,
                    customer_id: id,
                    kind: kinds.get(usize::try_from(rnd(5)).unwrap_or(0)).copied().unwrap_or_default(),
                    technician: format!("tech{}", rnd(3)),
                    started_at: ago(if running && rnd(2) == 0 { rnd(600) } else { started }),
                    ended_at: (!running).then(|| ago(started.saturating_sub(60))),
                    minutes: u32::try_from(rnd(240)).unwrap_or(0),
                    ..Intervention::default()
                });
            }
        }
        let purposes = ["remote_assistance", "unattended_access", "marketing", "custom_one", "custom_two"];
        let privacy_kinds = [PrivacyKind::NoticeDelivered, PrivacyKind::ConsentGiven, PrivacyKind::ConsentWithdrawn, PrivacyKind::ConsentGiven];
        // A few instants only, so the latest event is often a tie broken by id (or not at all).
        let instants: Vec<String> = [20, 400, 900, 1200].iter().map(|d| ago(d * 1440)).collect();
        for k in 0..n.saturating_mul(4) {
            let kind = privacy_kinds.get(usize::try_from(rnd(4)).unwrap_or(0)).copied().unwrap_or_default();
            let purpose =
                if kind == PrivacyKind::NoticeDelivered { "" } else { purposes.get(usize::try_from(rnd(5)).unwrap_or(0)).copied().unwrap_or("") };
            data.privacy.push(PrivacyEvent {
                id: if rnd(20) == 0 { k } else { k + 1 },
                customer_id: rnd(n + 2),
                kind,
                purpose: purpose.into(),
                notice_version: ["v1", "v2", "v3"].get(usize::try_from(rnd(3)).unwrap_or(0)).copied().unwrap_or("v2").into(),
                method: Method::Paper,
                at: instants.get(usize::try_from(rnd(4)).unwrap_or(0)).cloned().unwrap_or_default(),
                expires_at: match rnd(5) {
                    0 => Some(ago(1440)),
                    1 => Some(time::stamp(now + TimeDelta::days(30))),
                    _ => None,
                },
                ..PrivacyEvent::default()
            });
        }
        // Recorded out of order (an event entered late), so ties are broken by id, not position.
        for k in (1..data.privacy.len()).rev() {
            data.privacy.swap(k, usize::try_from(rnd(k as u64 + 1)).unwrap_or(0));
        }
        data
    }

    #[test]
    fn the_grouped_review_answers_as_scanning_every_record() {
        let now = time::parse_stamp("2026-10-13T08:00:00Z").unwrap();
        let demo = remotecraft_core::mock::demo(now);
        assert_eq!(review(&demo, now), review_by_scanning(&demo, now));
        assert_eq!(review(&Data::new(), now), review_by_scanning(&Data::new(), now));
        for n in [1, 7, 120, 600] {
            let mut data = synthetic(n, now);
            let grouped = review(&data, now);
            assert_eq!(grouped, review_by_scanning(&data, now), "{n} customers");
            let issues: Vec<&str> = grouped["findings"].as_array().unwrap().iter().filter_map(|f| f["issue"].as_str()).collect();
            if n >= 120 {
                for issue in [
                    "notice_missing",
                    "notice_outdated",
                    "remote_assistance_missing",
                    "remote_assistance_withdrawn",
                    "remote_assistance_expired",
                    "unattended_access_missing",
                    "unattended_access_expired",
                    "retention_due",
                    "timer_running",
                ] {
                    assert!(issues.contains(&issue), "{n} customers: no {issue}");
                }
            }
            // Without a notice, without retention and without a validity.
            data.notices.clear();
            data.preferences.retention_years = 0;
            data.preferences.consent_validity_months = 0;
            assert_eq!(review(&data, now), review_by_scanning(&data, now), "{n} customers, other preferences");
        }
        // The groups keep each customer's records in the data's order.
        let data = synthetic(50, now);
        let groups = ByCustomer::new(&data);
        let mine: Vec<u64> = data.privacy.iter().filter(|e| e.customer_id == 3).map(|e| e.id).collect();
        assert_eq!(groups.privacy(3).iter().map(|e| e.id).collect::<Vec<_>>(), mine);
        assert!(groups.devices(u64::MAX).is_empty() && groups.interventions(u64::MAX).is_empty() && groups.privacy(u64::MAX).is_empty());
        assert_eq!(groups.last_delivery(3), privacy::last_delivery(&data.privacy, 3));
    }
}
