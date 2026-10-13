//! Finding records from what people and agents type, and the JSON the commands answer with.

use remotecraft_core::chrono::{DateTime, Utc};
use remotecraft_core::privacy::{self, PURPOSES, PurposeState};
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
}
