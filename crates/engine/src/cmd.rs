//! The command table: every user-visible action, with its id, label, menu path and parameters,
//! and the dispatcher that runs it. The CLI, the HTTP API and the MCP server call the same
//! commands.

use remotecraft_core::privacy::{self, Method, Notice, PrivacyEvent, PrivacyKind, PurposeState};
use remotecraft_core::{
    Contact, Customer, CustomerKind, Data, Device, Error, Intervention, InterventionKind, Result as CoreResult, fold, limits, rustdesk, time,
};
use serde_json::{Value, json};

use crate::params::{boolean, bounded, first, list, notes, number, page, required, text};
use crate::views::{self, customer_ref, device_ref, intervention_json, privacy_status, state_name, to_value};
use crate::{Ctx, Desk, Result};

/// A command's description (the catalogue the CLI, the MCP server and an interface show).
#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: &'static str,
    pub params: &'static str,
    /// Changes the desk (saved, audited, rolled back on failure).
    pub writes: bool,
    /// Recorded in the audit log even though it changes nothing (access to personal data).
    pub audited: bool,
}

const fn c(id: &'static str, label: &'static str, menu: &'static str, params: &'static str, writes: bool, audited: bool) -> Command {
    Command { id, label, menu, params, writes, audited }
}

const W: bool = true;
const R: bool = false;

pub const COMMANDS: &[Command] = &[
    c(
        "customer.add",
        "New Customer",
        "Customers › New",
        "{name, kind?: company|person, tax_id?, email?, phone?, address?, contacts?: [{name, role?, email?, phone?}], tags?: [..] | \"a, b\", notes?, allow_duplicate?: bool} → customer",
        W,
        W,
    ),
    c(
        "customer.update",
        "Edit Customer",
        "Customers › Edit",
        "{customer: id|name, ...fields of customer.add (only those given change; contacts and tags are replaced)}",
        W,
        W,
    ),
    c("customer.get", "Customer", "Customers › Open", "{customer: id|name} → customer, devices, recent interventions, privacy status, totals", R, R),
    c(
        "customer.list",
        "Customers",
        "Customers",
        "{query? (name, tax id, e-mail, phone, contact, tag, RustDesk ID, host name), tag?, include_archived?: bool, limit? (100), offset?} → customers",
        R,
        R,
    ),
    c("customer.archive", "Archive Customer", "Customers › Archive", "{customer, archived?: bool (default true)}", W, W),
    c(
        "customer.remove",
        "Delete Customer",
        "Customers › Delete",
        "{customer} (only a customer without interventions or privacy records; otherwise customer.archive or gdpr.erase)",
        W,
        W,
    ),
    c(
        "device.add",
        "New Device",
        "Devices › New",
        "{customer, name?, rustdesk_id?, hostname?, os?, serial?, location?, assigned_to?, notes?, unattended?: bool} → device (RustDesk passwords are never stored)",
        W,
        W,
    ),
    c("device.update", "Edit Device", "Devices › Edit", "{device: id|RustDesk ID|name, customer? (moves it), ...fields of device.add}", W, W),
    c("device.get", "Device", "Devices › Open", "{device} → device, customer, recent interventions, consent for remote access", R, R),
    c("device.list", "Devices", "Devices", "{customer?, query?, include_retired?: bool, limit? (200), offset?} → devices", R, R),
    c("device.find", "Find by RustDesk ID", "Devices › Find", "{rustdesk_id | query} → the device(s) and their customer", R, R),
    c("device.retire", "Retire Device", "Devices › Retire", "{device, retired?: bool (default true)}", W, W),
    c("device.remove", "Delete Device", "Devices › Delete", "{device} (only a device without interventions; otherwise device.retire)", W, W),
    c(
        "device.connect",
        "Connect",
        "Devices › Connect with RustDesk",
        "{device} → rustdesk:// link, command line, consent state and warnings (audited)",
        R,
        W,
    ),
    c(
        "intervention.start",
        "Start Intervention",
        "Interventions › Start",
        "{customer? , device? (one of the two), kind?: remote|onsite|phone|lab|other, summary?, notes?, reference?, started_at? (now), billable?: bool, consent_override?: reason} → the running intervention, the RustDesk link, warnings",
        W,
        W,
    ),
    c(
        "intervention.stop",
        "Stop Intervention",
        "Interventions › Stop",
        "{id? (default: the technician's running one), ended_at? (now), notes? (appended), summary?, minutes? (instead of the span, e.g. pauses), billable?} → intervention",
        W,
        W,
    ),
    c(
        "intervention.add",
        "Log Intervention",
        "Interventions › Log",
        "{customer?, device?, kind?, summary, notes?, started_at, ended_at? | minutes, technician? (default: you), reference?, billable?, consent_override?} → intervention",
        W,
        W,
    ),
    c(
        "intervention.update",
        "Edit Intervention",
        "Interventions › Edit",
        "{id, kind?, summary?, notes?, reference?, started_at?, ended_at?, minutes?, billable?, technician?, device? (null = none)}",
        W,
        W,
    ),
    c("intervention.remove", "Delete Intervention", "Interventions › Delete", "{id}", W, W),
    c("intervention.get", "Intervention", "Interventions › Open", "{id} → intervention", R, R),
    c(
        "intervention.list",
        "Interventions",
        "Interventions",
        "{customer?, device?, technician?, kind?, from?, to?, running?: bool, billable?: bool, query?, limit? (50), offset?} → newest first, with totals",
        R,
        R,
    ),
    c(
        "intervention.running",
        "Running",
        "Interventions › Running",
        "{technician? (default: everyone)} → running interventions with elapsed minutes",
        R,
        R,
    ),
    c(
        "report.time",
        "Time Report",
        "Reports › Time",
        "{from?, to?, customer?, technician?, group_by?: customer|technician|month|device|kind (customer)} → minutes and billable minutes (rounded per intervention) per group",
        R,
        R,
    ),
    c("notice.add", "New Privacy Notice", "Privacy › Notices › New", "{version, title?, text?, effective_from? (now)} → notice", W, W),
    c("notice.list", "Privacy Notices", "Privacy › Notices", "{} → versions (without text), the current one marked", R, R),
    c("notice.get", "Privacy Notice", "Privacy › Notices › Open", "{version? (default: current)} → the notice with its text", R, R),
    c("privacy.purposes", "Purposes", "Privacy › Purposes", "{} → purposes consent is asked for, with descriptions", R, R),
    c("privacy.status", "Privacy Status", "Privacy › Status", "{customer} → notice delivered vs current, state of each purpose", R, R),
    c("privacy.history", "Privacy History", "Privacy › History", "{customer} → every notice delivery, consent and withdrawal", R, R),
    c(
        "privacy.notice",
        "Record Notice Delivery",
        "Privacy › Notice Delivered",
        "{customer, version? (current), method?: paper|email|online|verbal|other, at? (now), signer?, evidence?, note?}",
        W,
        W,
    ),
    c(
        "privacy.consent",
        "Record Consent",
        "Privacy › Consent",
        "{customer, purposes: [remote_assistance, unattended_access, marketing, …] | purpose, method?, at? (now), signer?, evidence?, note?, expires_at?, notice_version? (current), with_notice?: bool (also record the notice delivery)}",
        W,
        W,
    ),
    c("privacy.withdraw", "Record Withdrawal", "Privacy › Withdraw", "{customer, purposes | purpose, method?, at? (now), signer?, note?}", W, W),
    c(
        "gdpr.export",
        "Export Personal Data",
        "Privacy › Export",
        "{customer} → everything the desk holds about the customer (art. 15 and 20), audited",
        R,
        W,
    ),
    c(
        "gdpr.erase",
        "Erase Personal Data",
        "Privacy › Erase",
        "{customer, confirm: true, reason} → anonymises the customer, their devices and intervention notes; times and the privacy record stay (art. 17)",
        W,
        W,
    ),
    c(
        "gdpr.review",
        "Privacy Review",
        "Privacy › Review",
        "{} → customers without the current notice, remote access without consent, expired consents, data past retention, timers left running",
        R,
        R,
    ),
    c("audit.list", "Audit Log", "Privacy › Audit Log", "{customer?, device?, intervention?, actor?, command?, limit? (100)} → newest first", R, R),
    c("prefs.get", "Preferences", "Settings › Preferences", "{} → every preference", R, R),
    c(
        "prefs.set",
        "Set Preferences",
        "Settings › Preferences",
        "{key, value} or {language?: it|en, theme?: light|dark|system, company_name?, billing_unit_minutes?, minimum_minutes?, require_consent?, consent_validity_months?, retention_years?, rustdesk_host?, rustdesk_key?}",
        W,
        W,
    ),
    c(
        "rustdesk.setup",
        "RustDesk Client Setup",
        "Help › RustDesk",
        "{} → what to enter in each RustDesk client (ID server, relay, key), the config string to import and the self-configuring Windows installer name",
        R,
        R,
    ),
    c(
        "rustdesk.check",
        "Check RustDesk Server",
        "Help › RustDesk",
        "{host? (default: the server next to the desk)} → whether hbbs and hbbr answer on 21115-21117, and the key",
        R,
        R,
    ),
    c("app.status", "Status", "Help › Status", "{} → counts, running interventions, where the data is, load problems", R, R),
    c("app.commands", "Commands", "Help › Commands", "{} → every command and its parameters", R, R),
];

/// Runs a command (the caller has found it in [`COMMANDS`] and takes care of saving).
pub(crate) fn dispatch(desk: &mut Desk, ctx: &mut Ctx, id: &str, p: &Value) -> Result<Value> {
    Ok(match id {
        "customer.add" => customer_add(desk.data_mut(), ctx, p)?,
        "customer.update" => customer_update(desk.data_mut(), ctx, p)?,
        "customer.get" => customer_get(desk.data(), ctx, p)?,
        "customer.list" => customer_list(desk.data(), p)?,
        "customer.archive" => customer_archive(desk.data_mut(), ctx, p)?,
        "customer.remove" => customer_remove(desk.data_mut(), ctx, p)?,
        "device.add" => device_add(desk.data_mut(), ctx, p)?,
        "device.update" => device_update(desk.data_mut(), ctx, p)?,
        "device.get" => device_get(desk.data(), ctx, p)?,
        "device.list" => device_list(desk.data(), p)?,
        "device.find" => device_find(desk.data(), p)?,
        "device.retire" => device_retire(desk.data_mut(), ctx, p)?,
        "device.remove" => device_remove(desk.data_mut(), ctx, p)?,
        "device.connect" => device_connect(desk.data(), ctx, p)?,
        "intervention.start" => intervention_start(desk.data_mut(), ctx, p)?,
        "intervention.stop" => intervention_stop(desk.data_mut(), ctx, p)?,
        "intervention.add" => intervention_add(desk.data_mut(), ctx, p)?,
        "intervention.update" => intervention_update(desk.data_mut(), ctx, p)?,
        "intervention.remove" => intervention_remove(desk.data_mut(), ctx, p)?,
        "intervention.get" => {
            let i = intervention_of(desk.data(), p)?;
            json!({"intervention": intervention_json(desk.data(), i, ctx.now)})
        }
        "intervention.list" => intervention_list(desk.data(), ctx, p)?,
        "intervention.running" => intervention_running(desk.data(), ctx, p)?,
        "report.time" => report_time(desk.data(), ctx, p)?,
        "notice.add" => notice_add(desk.data_mut(), ctx, p)?,
        "notice.list" => notice_list(desk.data(), ctx),
        "notice.get" => notice_get(desk.data(), ctx, p)?,
        "privacy.purposes" => purposes(desk.data()),
        "privacy.status" => {
            let id = customer_of(desk.data(), p)?;
            json!({"customer": desk.data().customer(id).map(|c| c.name.clone()), "status": privacy_status(desk.data(), id, ctx.now)})
        }
        "privacy.history" => privacy_history(desk.data(), p)?,
        "privacy.notice" => privacy_notice(desk.data_mut(), ctx, p)?,
        "privacy.consent" => privacy_consent(desk.data_mut(), ctx, p)?,
        "privacy.withdraw" => privacy_withdraw(desk.data_mut(), ctx, p)?,
        "gdpr.export" => gdpr_export(desk, ctx, p)?,
        "gdpr.erase" => gdpr_erase(desk.data_mut(), ctx, p)?,
        "gdpr.review" => gdpr_review(desk.data(), ctx),
        "audit.list" => audit_list(desk, p)?,
        "prefs.get" => json!({"preferences": to_value(&desk.data().preferences)}),
        "prefs.set" => prefs_set(desk.data_mut(), p)?,
        "rustdesk.setup" => rustdesk_setup(desk),
        "rustdesk.check" => rustdesk_check(desk, p)?,
        "app.status" => app_status(desk, ctx),
        "app.commands" => commands_json(),
        _ => return Err(crate::Error::UnknownCommand(id.to_string())),
    })
}

/// The catalogue as JSON.
pub fn commands_json() -> Value {
    json!({"commands": COMMANDS.iter().map(|c| json!({"id": c.id, "label": c.label, "menu": c.menu, "params": c.params, "writes": c.writes})).collect::<Vec<_>>()})
}

// ---------------------------------------------------------------------------------------------
// Helpers

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

fn customer_of(data: &Data, p: &Value) -> CoreResult<u64> {
    let v = first(p, &["customer", "customer_id", "id"]).ok_or_else(|| invalid("`customer` is required (an id or a name)"))?;
    customer_ref(data, v)
}

fn device_of(data: &Data, p: &Value, customer: Option<u64>) -> CoreResult<u64> {
    let v = first(p, &["device", "device_id", "id"]).ok_or_else(|| invalid("`device` is required (an id, a RustDesk ID or a name)"))?;
    device_ref(data, v, customer)
}

fn intervention_of<'a>(data: &'a Data, p: &Value) -> CoreResult<&'a Intervention> {
    let id = number(p, "id")?.or(number(p, "intervention")?).ok_or_else(|| invalid("`id` (the intervention) is required"))?;
    data.intervention(id).ok_or_else(|| Error::NotFound(format!("no intervention {id}")))
}

fn next(counter: &mut u64) -> CoreResult<u64> {
    *counter = counter.checked_add(1).ok_or_else(|| invalid("id counter exhausted"))?;
    Ok(*counter)
}

fn italian(data: &Data) -> bool {
    data.preferences.language != "en"
}

/// The time parameter `key` as a stamp, `now` when absent.
fn when(p: &Value, key: &str, ctx: &Ctx) -> CoreResult<String> {
    match text(p, key)? {
        None => Ok(time::stamp(ctx.now)),
        Some(t) => time::input_stamp(&t, ctx.now),
    }
}

fn optional_when(p: &Value, key: &str, ctx: &Ctx) -> CoreResult<Option<String>> {
    match text(p, key)? {
        None => Ok(None),
        Some(t) if t.is_empty() => Ok(None),
        Some(t) => time::input_stamp(&t, ctx.now).map(Some),
    }
}

fn contacts(p: &Value) -> CoreResult<Option<Vec<Contact>>> {
    let Some(v) = p.get("contacts").filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(items) = v.as_array() else { return Err(invalid("`contacts` must be a list of {name, role, email, phone}")) };
    if items.len() > limits::MAX_CONTACTS {
        return Err(invalid(format!("at most {} contacts", limits::MAX_CONTACTS)));
    }
    let mut out = Vec::new();
    for item in items {
        if !item.is_object() {
            return Err(invalid("`contacts` must be a list of {name, role, email, phone}"));
        }
        let k = Contact {
            name: text(item, "name")?.unwrap_or_default(),
            role: text(item, "role")?.unwrap_or_default(),
            email: text(item, "email")?.unwrap_or_default(),
            phone: text(item, "phone")?.unwrap_or_default(),
        };
        if !remotecraft_core::valid_email(&k.email) {
            return Err(invalid(format!("`{}` is not an e-mail address", k.email)));
        }
        if !(k.name.is_empty() && k.email.is_empty() && k.phone.is_empty()) {
            out.push(k);
        }
    }
    Ok(Some(out))
}

/// Applies the customer fields present in `p`.
fn apply_customer(c: &mut Customer, p: &Value) -> CoreResult<()> {
    if let Some(name) = text(p, "name")? {
        if name.is_empty() {
            return Err(invalid("the name cannot be empty"));
        }
        c.name = name;
    }
    if let Some(k) = text(p, "kind")? {
        c.kind = CustomerKind::parse(&k).ok_or_else(|| invalid(format!("`{k}` is not a kind: company or person")))?;
    }
    if let Some(v) = text(p, "tax_id")? {
        c.tax_id = v.to_uppercase().replace(' ', "");
    }
    if let Some(v) = text(p, "email")? {
        if !remotecraft_core::valid_email(&v) {
            return Err(invalid(format!("`{v}` is not an e-mail address")));
        }
        c.email = v;
    }
    if let Some(v) = text(p, "phone")? {
        c.phone = v;
    }
    if let Some(v) = text(p, "address")? {
        c.address = v;
    }
    if let Some(v) = contacts(p)? {
        c.contacts = v;
    }
    if let Some(v) = list(p, "tags", limits::MAX_TAGS)? {
        c.tags = v.into_iter().map(|t| remotecraft_core::clip(&t, 60).to_lowercase()).collect();
    }
    if let Some(v) = notes(p, "notes", limits::MAX_NOTES)? {
        c.notes = v;
    }
    Ok(())
}

/// Refuses a second customer with the same tax id (or, without tax ids, the same name),
/// looking only at what changed (`by_name`, `by_tax`).
fn check_duplicate(data: &Data, c: &Customer, p: &Value, by_name: bool, by_tax: bool) -> CoreResult<()> {
    if boolean(p, "allow_duplicate")?.unwrap_or(false) {
        return Ok(());
    }
    let clash = data.customers.iter().find(|o| {
        o.id != c.id
            && o.erased_at.is_none()
            && ((by_tax && !c.tax_id.is_empty() && o.tax_id == c.tax_id)
                || (by_name && o.tax_id.is_empty() && c.tax_id.is_empty() && fold(&o.name) == fold(&c.name)))
    });
    match clash {
        Some(o) => {
            Err(Error::Refused(format!("customer {} `{}` already has this name or tax id (allow_duplicate: true to keep both)", o.id, o.name)))
        }
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// Customers

fn customer_add(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    required(p, "name")?;
    if data.customers.len() >= limits::MAX_CUSTOMERS {
        return Err(Error::Refused("the desk is full".into()));
    }
    let mut c = Customer { created_at: time::stamp(ctx.now), updated_at: time::stamp(ctx.now), ..Customer::default() };
    apply_customer(&mut c, p)?;
    check_duplicate(data, &c, p, true, true)?;
    c.id = next(&mut data.counters.customer)?;
    ctx.touch("customer", c.id);
    let out =
        json!({"customer": views::customer_json(&c), "next": "device.add for their PCs, privacy.notice and privacy.consent for the GDPR record"});
    data.customers.push(c);
    Ok(out)
}

fn customer_update(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    let mut c = data.customer(id).cloned().ok_or_else(|| Error::NotFound(format!("no customer {id}")))?;
    if c.erased_at.is_some() {
        return Err(Error::Refused("this customer's data was erased".into()));
    }
    // `customer` names who to edit: a new name comes in `name`.
    let (old_name, old_tax) = (c.name.clone(), c.tax_id.clone());
    apply_customer(&mut c, p)?;
    // Duplicates already in the desk (allowed when added) do not block other edits.
    check_duplicate(data, &c, p, fold(&c.name) != fold(&old_name), c.tax_id != old_tax)?;
    c.updated_at = time::stamp(ctx.now);
    ctx.touch("customer", id);
    let out = json!({"customer": views::customer_json(&c)});
    if let Some(slot) = data.customer_mut(id) {
        *slot = c;
    }
    Ok(out)
}

fn customer_get(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    let c = data.customer(id).ok_or_else(|| Error::NotFound(format!("no customer {id}")))?;
    let devices: Vec<Value> = data.devices.iter().filter(|d| d.customer_id == id).map(|d| views::device_json(data, d)).collect();
    let mut work: Vec<&Intervention> = data.interventions.iter().filter(|i| i.customer_id == id).collect();
    work.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    let minutes: u64 = work.iter().map(|i| u64::from(i.minutes)).sum();
    let recent: Vec<Value> = work.iter().take(20).map(|i| intervention_json(data, i, ctx.now)).collect();
    Ok(json!({
        "customer": views::customer_json(c),
        "devices": devices,
        "interventions": recent,
        "totals": {"interventions": work.len(), "minutes": minutes, "running": work.iter().filter(|i| i.running()).count()},
        "privacy": privacy_status(data, id, ctx.now),
    }))
}

fn customer_list(data: &Data, p: &Value) -> CoreResult<Value> {
    let q = text(p, "query")?.map(|q| fold(&q)).filter(|q| !q.is_empty());
    let tag = text(p, "tag")?.map(|t| t.to_lowercase()).filter(|t| !t.is_empty());
    let archived = boolean(p, "include_archived")?.unwrap_or(false);
    let (limit, offset) = page(p, 100)?;
    let rid = q.as_deref().and_then(|q| rustdesk::normalize(q).ok()).filter(|r| !r.is_empty());
    let mut rows: Vec<&Customer> = data
        .customers
        .iter()
        .filter(|c| archived || (!c.archived && c.erased_at.is_none()))
        .filter(|c| tag.as_ref().is_none_or(|t| c.tags.contains(t)))
        .filter(|c| {
            let Some(q) = &q else { return true };
            views::customer_matches(c, q)
                || data
                    .devices
                    .iter()
                    .filter(|d| d.customer_id == c.id)
                    .any(|d| rid.as_ref().is_some_and(|r| &d.rustdesk_id == r) || fold(&d.hostname).contains(q.as_str()) || fold(&d.name) == *q)
        })
        .collect();
    rows.sort_by_key(|c| fold(&c.name));
    let total = rows.len();
    let list: Vec<Value> = rows.into_iter().skip(offset).take(limit).map(|c| views::customer_row(data, c)).collect();
    Ok(json!({"customers": list, "total": total}))
}

fn customer_archive(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    let archived = boolean(p, "archived")?.unwrap_or(true);
    let now = time::stamp(ctx.now);
    let c = data.customer_mut(id).ok_or_else(|| Error::NotFound(format!("no customer {id}")))?;
    c.archived = archived;
    c.updated_at = now;
    ctx.touch("customer", id);
    Ok(json!({"customer": id, "archived": archived}))
}

fn customer_remove(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    let work = data.interventions.iter().filter(|i| i.customer_id == id).count();
    let records = data.privacy.iter().filter(|e| e.customer_id == id).count();
    if work > 0 || records > 0 {
        return Err(Error::Refused(format!(
            "customer {id} has {work} interventions and {records} privacy records: archive it (customer.archive) or erase the personal data (gdpr.erase)"
        )));
    }
    let devices: Vec<u64> = data.devices.iter().filter(|d| d.customer_id == id).map(|d| d.id).collect();
    data.devices.retain(|d| d.customer_id != id);
    data.customers.retain(|c| c.id != id);
    ctx.touch("customer", id);
    for d in &devices {
        ctx.touch("device", *d);
    }
    Ok(json!({"removed": id, "devices_removed": devices}))
}

// ---------------------------------------------------------------------------------------------
// Devices

fn apply_device(data: &Data, d: &mut Device, p: &Value) -> CoreResult<()> {
    if let Some(v) = text(p, "name")? {
        d.name = v;
    }
    if let Some(v) = text(p, "rustdesk_id")? {
        let rid = rustdesk::normalize(&v)?;
        if !rid.is_empty()
            && let Some(o) = data.devices.iter().find(|o| o.id != d.id && !o.retired && o.rustdesk_id == rid)
        {
            return Err(Error::Refused(format!(
                "RustDesk ID {} is already on device {} `{}` of {}: update or retire that one first",
                rustdesk::display(&rid),
                o.id,
                o.name,
                views::customer_name(data, o.customer_id)
            )));
        }
        d.rustdesk_id = rid;
    }
    if let Some(v) = text(p, "hostname")? {
        d.hostname = v;
    }
    if let Some(v) = text(p, "os")? {
        d.os = v;
    }
    if let Some(v) = text(p, "serial")? {
        d.serial = v;
    }
    if let Some(v) = text(p, "location")? {
        d.location = v;
    }
    if let Some(v) = text(p, "assigned_to")? {
        d.assigned_to = v;
    }
    if let Some(v) = notes(p, "notes", limits::MAX_NOTES)? {
        d.notes = v;
    }
    if let Some(v) = boolean(p, "unattended")? {
        d.unattended = v;
    }
    if d.name.is_empty() {
        d.name = if d.hostname.is_empty() { "PC".into() } else { d.hostname.clone() };
    }
    Ok(())
}

/// Warnings about consent for a device with unattended access.
fn unattended_warning(data: &Data, d: &Device, ctx: &Ctx) -> Option<String> {
    if !d.unattended {
        return None;
    }
    let (state, _) = privacy::purpose_state(&data.privacy, d.customer_id, "unattended_access", ctx.now, data.preferences.consent_validity_months);
    (state != PurposeState::Given).then(|| {
        format!("device {} has unattended access but the customer's unattended_access consent is {}: record it (privacy.consent) or remove the permanent password from the RustDesk client", d.id, state_name(state))
    })
}

fn device_add(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let customer = customer_of_strict(data, p)?;
    if data.devices.len() >= limits::MAX_DEVICES {
        return Err(Error::Refused("the desk is full".into()));
    }
    let mut d = Device { customer_id: customer, created_at: time::stamp(ctx.now), updated_at: time::stamp(ctx.now), ..Device::default() };
    apply_device(data, &mut d, p)?;
    d.id = next(&mut data.counters.device)?;
    ctx.touch("customer", customer);
    ctx.touch("device", d.id);
    let warnings: Vec<String> = unattended_warning(data, &d, ctx).into_iter().collect();
    let out = json!({"device": views::device_json(data, &d), "warnings": warnings});
    data.devices.push(d);
    Ok(out)
}

/// `customer` only (not `id`, which names other records in some commands).
fn customer_of_strict(data: &Data, p: &Value) -> CoreResult<u64> {
    let v = first(p, &["customer", "customer_id"]).ok_or_else(|| invalid("`customer` is required (an id or a name)"))?;
    let id = customer_ref(data, v)?;
    if data.customer(id).is_some_and(|c| c.erased_at.is_some()) {
        return Err(Error::Refused("this customer's data was erased".into()));
    }
    Ok(id)
}

fn device_update(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = device_of(data, p, None)?;
    let mut d = data.device(id).cloned().ok_or_else(|| Error::NotFound(format!("no device {id}")))?;
    if first(p, &["customer", "customer_id"]).is_some() {
        d.customer_id = customer_of_strict(data, p)?;
    }
    apply_device(data, &mut d, p)?;
    d.updated_at = time::stamp(ctx.now);
    ctx.touch("device", id);
    ctx.touch("customer", d.customer_id);
    let warnings: Vec<String> = unattended_warning(data, &d, ctx).into_iter().collect();
    let out = json!({"device": views::device_json(data, &d), "warnings": warnings});
    if let Some(slot) = data.device_mut(id) {
        *slot = d;
    }
    Ok(out)
}

fn device_get(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let id = device_of(data, p, None)?;
    let d = data.device(id).ok_or_else(|| Error::NotFound(format!("no device {id}")))?;
    let mut work: Vec<&Intervention> = data.interventions.iter().filter(|i| i.device_id == Some(id)).collect();
    work.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    let customer = data.customer(d.customer_id);
    Ok(json!({
        "device": views::device_json(data, d),
        "customer": customer.map(|c| views::customer_row(data, c)),
        "interventions": work.iter().take(20).map(|i| intervention_json(data, i, ctx.now)).collect::<Vec<_>>(),
        "privacy": privacy_status(data, d.customer_id, ctx.now),
    }))
}

fn device_list(data: &Data, p: &Value) -> CoreResult<Value> {
    let customer = match first(p, &["customer", "customer_id"]) {
        Some(v) => Some(customer_ref(data, v)?),
        None => None,
    };
    let q = text(p, "query")?.map(|q| fold(&q)).filter(|q| !q.is_empty());
    let rid = q.as_deref().and_then(|q| rustdesk::normalize(q).ok()).filter(|r| !r.is_empty());
    let retired = boolean(p, "include_retired")?.unwrap_or(false);
    let (limit, offset) = page(p, 200)?;
    let rows: Vec<&Device> = data
        .devices
        .iter()
        .filter(|d| customer.is_none_or(|c| d.customer_id == c))
        .filter(|d| retired || !d.retired)
        .filter(|d| {
            let Some(q) = &q else { return true };
            rid.as_ref().is_some_and(|r| &d.rustdesk_id == r)
                || [&d.name, &d.hostname, &d.os, &d.serial, &d.location, &d.assigned_to].iter().any(|f| fold(f).contains(q.as_str()))
        })
        .collect();
    let total = rows.len();
    Ok(json!({"devices": rows.into_iter().skip(offset).take(limit).map(|d| views::device_json(data, d)).collect::<Vec<_>>(), "total": total}))
}

fn device_find(data: &Data, p: &Value) -> CoreResult<Value> {
    let q = text(p, "rustdesk_id")?.or(text(p, "query")?).filter(|q| !q.is_empty()).ok_or_else(|| invalid("`rustdesk_id` or `query` is required"))?;
    let rid = rustdesk::normalize(&q).ok().filter(|r| !r.is_empty());
    let fq = fold(&q);
    let found: Vec<Value> = data
        .devices
        .iter()
        .filter(|d| rid.as_ref().is_some_and(|r| &d.rustdesk_id == r) || (!fq.is_empty() && (fold(&d.hostname) == fq || fold(&d.serial) == fq)))
        .take(limits::MAX_LIST)
        .map(|d| {
            let mut v = views::device_json(data, d);
            v["customer"] = data.customer(d.customer_id).map(|c| views::customer_row(data, c)).unwrap_or(Value::Null);
            v
        })
        .collect();
    Ok(json!({"devices": found, "rustdesk_id": rid}))
}

fn device_retire(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = device_of(data, p, None)?;
    let retired = boolean(p, "retired")?.unwrap_or(true);
    let now = time::stamp(ctx.now);
    let d = data.device_mut(id).ok_or_else(|| Error::NotFound(format!("no device {id}")))?;
    d.retired = retired;
    d.updated_at = now;
    ctx.touch("device", id);
    Ok(json!({"device": id, "retired": retired}))
}

fn device_remove(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = device_of(data, p, None)?;
    let work = data.interventions.iter().filter(|i| i.device_id == Some(id)).count();
    if work > 0 {
        return Err(Error::Refused(format!("device {id} has {work} interventions: retire it instead (device.retire)")));
    }
    data.devices.retain(|d| d.id != id);
    ctx.touch("device", id);
    Ok(json!({"removed": id}))
}

/// The consent checks for remote work on a customer (and device): warnings, and an error when
/// the desk requires consent and no reason was given.
fn remote_checks(data: &Data, ctx: &mut Ctx, customer: u64, device: Option<&Device>, override_reason: &str) -> CoreResult<Vec<String>> {
    let prefs = &data.preferences;
    let mut warnings = Vec::new();
    let (state, _) = privacy::purpose_state(&data.privacy, customer, "remote_assistance", ctx.now, prefs.consent_validity_months);
    if state != PurposeState::Given {
        if prefs.require_consent && override_reason.is_empty() {
            return Err(Error::Refused(format!(
                "the customer's remote_assistance consent is {}: record it first (privacy.consent), or give consent_override with the reason (e.g. the person agreed on the phone)",
                state_name(state)
            )));
        }
        warnings.push(format!("remote_assistance consent is {}", state_name(state)));
        if !override_reason.is_empty() {
            ctx.detail = format!("consent override: {override_reason}");
        }
    }
    if let Some(d) = device
        && let Some(w) = unattended_warning(data, d, ctx)
    {
        warnings.push(w);
    }
    if let Some(current) = privacy::current_notice(&data.notices, ctx.now) {
        match privacy::last_delivery(&data.privacy, customer) {
            None => warnings.push(format!("the customer has not received the privacy notice (current: {})", current.version)),
            Some(e) if e.notice_version != current.version => {
                warnings.push(format!("the customer received notice {} but the current one is {}", e.notice_version, current.version));
            }
            Some(_) => {}
        }
    }
    Ok(warnings)
}

fn connect_info(d: &Device) -> Value {
    if d.rustdesk_id.is_empty() {
        return Value::Null;
    }
    json!({"rustdesk_id": d.rustdesk_id, "display": rustdesk::display(&d.rustdesk_id), "uri": rustdesk::connect_uri(&d.rustdesk_id), "command": rustdesk::connect_command(&d.rustdesk_id), "unattended": d.unattended})
}

fn device_connect(data: &Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = device_of(data, p, None)?;
    let d = data.device(id).ok_or_else(|| Error::NotFound(format!("no device {id}")))?;
    if d.rustdesk_id.is_empty() {
        return Err(Error::Refused(format!("device {id} `{}` has no RustDesk ID (device.update with rustdesk_id)", d.name)));
    }
    ctx.touch("device", id);
    ctx.touch("customer", d.customer_id);
    // Never refuses: connecting is the technician's call; intervention.start enforces consent.
    let (warnings, allowed) = {
        let mut probe = Ctx { actor: ctx.actor.clone(), now: ctx.now, targets: Vec::new(), detail: String::new() };
        match remote_checks(data, &mut probe, d.customer_id, Some(d), "") {
            Ok(w) => (w, true),
            Err(e) => (vec![e.to_string()], false),
        }
    };
    Ok(json!({
        "device": views::device_json(data, d),
        "customer": views::customer_name(data, d.customer_id),
        "connect": connect_info(d),
        "consent_ok": allowed,
        "warnings": warnings,
    }))
}

// ---------------------------------------------------------------------------------------------
// Interventions

/// The customer and device of a new intervention: from `device` (its customer), `customer`, or
/// both (they must agree).
fn work_target(data: &Data, p: &Value) -> CoreResult<(u64, Option<u64>)> {
    let customer = match first(p, &["customer", "customer_id"]) {
        Some(v) => Some(customer_ref(data, v)?),
        None => None,
    };
    let device = match first(p, &["device", "device_id"]) {
        Some(v) => Some(device_ref(data, v, customer)?),
        None => None,
    };
    let customer = match (customer, device) {
        (Some(c), _) => c,
        (None, Some(d)) => data.device(d).map(|d| d.customer_id).ok_or_else(|| Error::NotFound(format!("no device {d}")))?,
        (None, None) => return Err(invalid("name the `customer` or the `device`")),
    };
    let c = data.customer(customer).ok_or_else(|| Error::NotFound(format!("no customer {customer}")))?;
    if c.erased_at.is_some() {
        return Err(Error::Refused("this customer's data was erased".into()));
    }
    Ok((customer, device))
}

fn kind_param(p: &Value, default: InterventionKind) -> CoreResult<InterventionKind> {
    match text(p, "kind")? {
        None => Ok(default),
        Some(k) if k.is_empty() => Ok(default),
        Some(k) => InterventionKind::parse(&k).ok_or_else(|| invalid(format!("`{k}` is not a kind: remote, onsite, phone, lab or other"))),
    }
}

fn default_kind(data: &Data, device: Option<u64>) -> InterventionKind {
    match device.and_then(|d| data.device(d)) {
        Some(d) if !d.rustdesk_id.is_empty() => InterventionKind::Remote,
        Some(_) => InterventionKind::Onsite,
        None => InterventionKind::Phone,
    }
}

fn intervention_start(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let (customer, device) = work_target(data, p)?;
    let kind = kind_param(p, default_kind(data, device))?;
    let override_reason = text(p, "consent_override")?.unwrap_or_default();
    let dev = device.and_then(|d| data.device(d)).cloned();
    let mut warnings = if kind.is_remote() { remote_checks(data, ctx, customer, dev.as_ref(), &override_reason)? } else { Vec::new() };
    if data.customer(customer).is_some_and(|c| c.archived) {
        warnings.push("the customer is archived".into());
    }
    let mine: Vec<u64> = data.interventions.iter().filter(|i| i.running() && i.technician == ctx.actor).map(|i| i.id).collect();
    if !mine.is_empty() {
        warnings.push(format!("you have other running interventions: {mine:?}"));
    }
    if data.interventions.len() >= limits::MAX_INTERVENTIONS {
        return Err(Error::Refused("the desk is full".into()));
    }
    let started = when(p, "started_at", ctx)?;
    let mut i = Intervention {
        customer_id: customer,
        device_id: device,
        kind,
        technician: ctx.actor.clone(),
        summary: text(p, "summary")?.unwrap_or_default(),
        notes: notes(p, "notes", limits::MAX_NOTES)?.unwrap_or_default(),
        reference: text(p, "reference")?.unwrap_or_default(),
        started_at: started,
        ended_at: None,
        minutes: 0,
        billable: boolean(p, "billable")?.unwrap_or(true),
        consent_override: if kind.is_remote() && !ctx.detail.is_empty() { override_reason } else { String::new() },
        created_at: time::stamp(ctx.now),
        updated_at: time::stamp(ctx.now),
        id: 0,
    };
    i.id = next(&mut data.counters.intervention)?;
    ctx.touch("intervention", i.id);
    ctx.touch("customer", customer);
    if let Some(d) = device {
        ctx.touch("device", d);
    }
    let connect = match (&dev, kind.is_remote()) {
        (Some(d), true) => connect_info(d),
        _ => Value::Null,
    };
    let out = json!({"intervention": intervention_json(data, &i, ctx.now), "connect": connect, "warnings": warnings});
    data.interventions.push(i);
    Ok(out)
}

fn append(old: &str, new: &str) -> String {
    match (old.is_empty(), new.is_empty()) {
        (_, true) => old.to_string(),
        (true, false) => new.to_string(),
        (false, false) => remotecraft_core::clip(&format!("{old}\n{new}"), limits::MAX_NOTES).to_string(),
    }
}

/// Minutes between two stamps, refused beyond [`limits::MAX_MINUTES`].
fn span(start: &str, end: &str) -> CoreResult<u32> {
    let m = time::minutes_between(start, end).ok_or_else(|| invalid("the end is before the start"))?;
    if m > limits::MAX_MINUTES {
        return Err(invalid(format!("{m} minutes is longer than a week: check the times, or give `minutes`")));
    }
    Ok(m)
}

fn intervention_stop(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = match number(p, "id")?.or(number(p, "intervention")?) {
        Some(id) => id,
        None => {
            let mine: Vec<&Intervention> = data.interventions.iter().filter(|i| i.running() && i.technician == ctx.actor).collect();
            match mine.as_slice() {
                [one] => one.id,
                [] => return Err(Error::NotFound(format!("{} has no running intervention", ctx.actor))),
                many => {
                    return Err(invalid(format!(
                        "several running interventions: {} — give the `id`",
                        many.iter().map(|i| format!("{} {}", i.id, i.summary)).collect::<Vec<_>>().join("; ")
                    )));
                }
            }
        }
    };
    let mut i = data.intervention(id).cloned().ok_or_else(|| Error::NotFound(format!("no intervention {id}")))?;
    if !i.running() {
        return Err(Error::Refused(format!("intervention {id} is already closed (intervention.update to change it)")));
    }
    let ended = when(p, "ended_at", ctx)?;
    let computed = span(&i.started_at, &ended)?;
    i.minutes = bounded(p, "minutes", u64::from(limits::MAX_MINUTES))?.unwrap_or(computed);
    i.ended_at = Some(ended);
    if let Some(n) = notes(p, "notes", limits::MAX_NOTES)? {
        i.notes = append(&i.notes, &n);
    }
    if let Some(s) = text(p, "summary")?.filter(|s| !s.is_empty()) {
        i.summary = s;
    }
    if let Some(b) = boolean(p, "billable")? {
        i.billable = b;
    }
    i.updated_at = time::stamp(ctx.now);
    ctx.touch("intervention", id);
    ctx.touch("customer", i.customer_id);
    let out = json!({"intervention": intervention_json(data, &i, ctx.now)});
    if let Some(slot) = data.intervention_mut(id) {
        *slot = i;
    }
    Ok(out)
}

fn intervention_add(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let (customer, device) = work_target(data, p)?;
    let kind = kind_param(p, default_kind(data, device))?;
    let summary = required(p, "summary")?;
    let start_text = text(p, "started_at")?.filter(|s| !s.is_empty()).ok_or_else(|| invalid("`started_at` is required (e.g. 2026-10-13 09:30)"))?;
    let started = time::input_stamp(&start_text, ctx.now)?;
    let minutes = bounded(p, "minutes", u64::from(limits::MAX_MINUTES))?;
    let (ended, minutes) = match (optional_when(p, "ended_at", ctx)?, minutes) {
        (Some(end), m) => {
            let s = span(&started, &end)?;
            (end, m.unwrap_or(s))
        }
        (None, Some(m)) => {
            let start = time::parse_stamp(&started).ok_or_else(|| invalid("bad start"))?;
            (time::stamp(start + remotecraft_core::chrono::Duration::minutes(i64::from(m))), m)
        }
        (None, None) => return Err(invalid("give `ended_at` or `minutes`")),
    };
    let override_reason = text(p, "consent_override")?.unwrap_or_default();
    let dev = device.and_then(|d| data.device(d)).cloned();
    let warnings = if kind.is_remote() { remote_checks(data, ctx, customer, dev.as_ref(), &override_reason)? } else { Vec::new() };
    if data.interventions.len() >= limits::MAX_INTERVENTIONS {
        return Err(Error::Refused("the desk is full".into()));
    }
    let technician = text(p, "technician")?.filter(|t| !t.is_empty()).unwrap_or_else(|| ctx.actor.clone());
    let mut i = Intervention {
        id: 0,
        customer_id: customer,
        device_id: device,
        kind,
        technician,
        summary,
        notes: notes(p, "notes", limits::MAX_NOTES)?.unwrap_or_default(),
        reference: text(p, "reference")?.unwrap_or_default(),
        started_at: started,
        ended_at: Some(ended),
        minutes,
        billable: boolean(p, "billable")?.unwrap_or(true),
        consent_override: if kind.is_remote() && !ctx.detail.is_empty() { override_reason } else { String::new() },
        created_at: time::stamp(ctx.now),
        updated_at: time::stamp(ctx.now),
    };
    i.id = next(&mut data.counters.intervention)?;
    ctx.touch("intervention", i.id);
    ctx.touch("customer", customer);
    if let Some(d) = device {
        ctx.touch("device", d);
    }
    let out = json!({"intervention": intervention_json(data, &i, ctx.now), "warnings": warnings});
    data.interventions.push(i);
    Ok(out)
}

fn intervention_update(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let mut i = intervention_of(data, p)?.clone();
    if data.customer(i.customer_id).is_some_and(|c| c.erased_at.is_some()) {
        return Err(Error::Refused("this customer's data was erased".into()));
    }
    if let Some(k) = text(p, "kind")?.filter(|k| !k.is_empty()) {
        i.kind = InterventionKind::parse(&k).ok_or_else(|| invalid(format!("`{k}` is not a kind: remote, onsite, phone, lab or other")))?;
    }
    if let Some(v) = text(p, "summary")? {
        i.summary = v;
    }
    if let Some(v) = notes(p, "notes", limits::MAX_NOTES)? {
        i.notes = v;
    }
    if let Some(v) = text(p, "reference")? {
        i.reference = v;
    }
    if let Some(v) = text(p, "technician")?.filter(|t| !t.is_empty()) {
        i.technician = v;
    }
    if let Some(v) = boolean(p, "billable")? {
        i.billable = v;
    }
    match p.get("device") {
        Some(Value::Null) => i.device_id = None,
        Some(v) => {
            let d = device_ref(data, v, Some(i.customer_id))?;
            i.device_id = Some(d);
        }
        None => {}
    }
    let mut times_changed = false;
    if let Some(s) = optional_when(p, "started_at", ctx)? {
        i.started_at = s;
        times_changed = true;
    }
    if let Some(e) = optional_when(p, "ended_at", ctx)? {
        i.ended_at = Some(e);
        times_changed = true;
    }
    if let Some(m) = bounded(p, "minutes", u64::from(limits::MAX_MINUTES))? {
        i.minutes = m;
    } else if times_changed && let Some(end) = &i.ended_at {
        i.minutes = span(&i.started_at, end)?;
    }
    if let Some(end) = &i.ended_at
        && end < &i.started_at
    {
        return Err(invalid("the end is before the start"));
    }
    i.updated_at = time::stamp(ctx.now);
    ctx.touch("intervention", i.id);
    ctx.touch("customer", i.customer_id);
    let out = json!({"intervention": intervention_json(data, &i, ctx.now)});
    if let Some(slot) = data.intervention_mut(i.id) {
        *slot = i;
    }
    Ok(out)
}

fn intervention_remove(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let i = intervention_of(data, p)?;
    let (id, customer) = (i.id, i.customer_id);
    data.interventions.retain(|x| x.id != id);
    ctx.touch("intervention", id);
    ctx.touch("customer", customer);
    Ok(json!({"removed": id}))
}

/// The filters shared by the intervention list and the time report.
struct WorkFilter {
    customer: Option<u64>,
    device: Option<u64>,
    technician: Option<String>,
    kind: Option<InterventionKind>,
    from: Option<String>,
    to: Option<String>,
    running: Option<bool>,
    billable: Option<bool>,
    query: Option<String>,
}

impl WorkFilter {
    fn read(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Self> {
        let customer = match first(p, &["customer", "customer_id"]) {
            Some(v) => Some(customer_ref(data, v)?),
            None => None,
        };
        let device = match first(p, &["device", "device_id"]) {
            Some(v) => Some(device_ref(data, v, customer)?),
            None => None,
        };
        let kind = match text(p, "kind")?.filter(|k| !k.is_empty()) {
            Some(k) => Some(InterventionKind::parse(&k).ok_or_else(|| invalid(format!("`{k}` is not a kind")))?),
            None => None,
        };
        Ok(Self {
            customer,
            device,
            technician: text(p, "technician")?.filter(|t| !t.is_empty()),
            kind,
            from: optional_when(p, "from", ctx)?,
            to: optional_when(p, "to", ctx)?,
            running: boolean(p, "running")?,
            billable: boolean(p, "billable")?,
            query: text(p, "query")?.map(|q| fold(&q)).filter(|q| !q.is_empty()),
        })
    }

    fn keeps(&self, i: &Intervention) -> bool {
        self.customer.is_none_or(|c| i.customer_id == c)
            && self.device.is_none_or(|d| i.device_id == Some(d))
            && self.technician.as_ref().is_none_or(|t| fold(&i.technician) == fold(t))
            && self.kind.is_none_or(|k| i.kind == k)
            && self.from.as_ref().is_none_or(|f| i.started_at >= *f)
            && self.to.as_ref().is_none_or(|t| i.started_at < *t)
            && self.running.is_none_or(|r| i.running() == r)
            && self.billable.is_none_or(|b| i.billable == b)
            && self.query.as_ref().is_none_or(|q| {
                fold(&i.summary).contains(q.as_str()) || fold(&i.notes).contains(q.as_str()) || fold(&i.reference).contains(q.as_str())
            })
    }
}

fn billable_minutes(data: &Data, i: &Intervention) -> u64 {
    data.preferences.billable_minutes(i)
}

fn intervention_list(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let f = WorkFilter::read(data, ctx, p)?;
    let (limit, offset) = page(p, 50)?;
    let mut rows: Vec<&Intervention> = data.interventions.iter().filter(|i| f.keeps(i)).collect();
    rows.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
    let minutes: u64 = rows.iter().map(|i| u64::from(i.minutes)).sum();
    let billable: u64 = rows.iter().map(|i| billable_minutes(data, i)).sum();
    let total = rows.len();
    Ok(json!({
        "interventions": rows.into_iter().skip(offset).take(limit).map(|i| intervention_json(data, i, ctx.now)).collect::<Vec<_>>(),
        "total": total,
        "minutes": minutes,
        "billable_minutes": billable,
    }))
}

fn intervention_running(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let tech = text(p, "technician")?.filter(|t| !t.is_empty());
    let rows: Vec<Value> = data
        .interventions
        .iter()
        .filter(|i| i.running() && tech.as_ref().is_none_or(|t| fold(&i.technician) == fold(t)))
        .map(|i| intervention_json(data, i, ctx.now))
        .collect();
    Ok(json!({"running": rows}))
}

fn hours(minutes: u64) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn report_time(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let f = WorkFilter::read(data, ctx, p)?;
    let group = text(p, "group_by")?.filter(|g| !g.is_empty()).unwrap_or_else(|| "customer".into());
    if !["customer", "technician", "month", "device", "kind"].contains(&group.as_str()) {
        return Err(invalid("`group_by` must be customer, technician, month, device or kind"));
    }
    let mut groups: std::collections::BTreeMap<String, (String, u64, u64, u64)> = std::collections::BTreeMap::new();
    let mut running = 0usize;
    for i in data.interventions.iter().filter(|i| f.keeps(i)) {
        if i.running() {
            running += 1;
            continue;
        }
        let (key, label) = match group.as_str() {
            "technician" => (i.technician.clone(), i.technician.clone()),
            "month" => (time::month(&i.started_at), time::month(&i.started_at)),
            "device" => {
                (format!("{:012}", i.device_id.unwrap_or(0)), if i.device_id.is_some() { views::device_name(data, i.device_id) } else { "—".into() })
            }
            "kind" => {
                let k = to_value(&i.kind).as_str().unwrap_or_default().to_string();
                (k.clone(), k)
            }
            _ => (format!("{:012}", i.customer_id), views::customer_name(data, i.customer_id)),
        };
        let g = groups.entry(key).or_insert((label, 0, 0, 0));
        g.1 += 1;
        g.2 += u64::from(i.minutes);
        g.3 += billable_minutes(data, i);
    }
    let rows: Vec<Value> = groups
        .into_iter()
        .map(|(key, (label, count, minutes, billable))| {
            json!({"key": key.trim_start_matches('0'), "label": label, "interventions": count, "minutes": minutes, "hours": hours(minutes), "billable_minutes": billable, "billable_hours": hours(billable)})
        })
        .collect();
    let minutes: u64 = rows.iter().filter_map(|r| r["minutes"].as_u64()).sum();
    let billable: u64 = rows.iter().filter_map(|r| r["billable_minutes"].as_u64()).sum();
    Ok(json!({
        "group_by": group,
        "from": f.from, "to": f.to,
        "rows": rows,
        "totals": {"minutes": minutes, "hours": hours(minutes), "billable_minutes": billable, "billable_hours": hours(billable)},
        "billing_unit_minutes": data.preferences.billing_unit_minutes,
        "minimum_minutes": data.preferences.minimum_minutes,
        "running_excluded": running,
    }))
}

// ---------------------------------------------------------------------------------------------
// Privacy notices and the GDPR record

fn notice_add(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let version = required(p, "version")?;
    if data.notices.iter().any(|n| n.version == version) {
        return Err(Error::Refused(format!("notice version `{version}` exists: versions never change, add a new one")));
    }
    if data.notices.len() >= limits::MAX_NOTICES {
        return Err(Error::Refused("too many notice versions".into()));
    }
    let mut n = Notice {
        id: 0,
        version,
        title: text(p, "title")?.unwrap_or_default(),
        text: notes(p, "text", limits::MAX_NOTICE)?.unwrap_or_default(),
        effective_from: when(p, "effective_from", ctx)?,
        created_at: time::stamp(ctx.now),
        created_by: ctx.actor.clone(),
    };
    n.id = next(&mut data.counters.notice)?;
    ctx.touch("notice", n.id);
    let out = json!({"notice": to_value(&n)});
    data.notices.push(n);
    Ok(out)
}

fn notice_list(data: &Data, ctx: &Ctx) -> Value {
    let current = privacy::current_notice(&data.notices, ctx.now).map(|n| n.id);
    let mut list: Vec<&Notice> = data.notices.iter().collect();
    list.sort_by(|a, b| b.effective_from.cmp(&a.effective_from));
    json!({"notices": list.iter().map(|n| json!({
        "id": n.id, "version": n.version, "title": n.title, "effective_from": n.effective_from, "text_length": n.text.len(),
        "current": Some(n.id) == current, "delivered_to": data.privacy.iter().filter(|e| e.kind == PrivacyKind::NoticeDelivered && e.notice_version == n.version).count(),
    })).collect::<Vec<_>>(), "current": current})
}

fn notice_get(data: &Data, ctx: &Ctx, p: &Value) -> CoreResult<Value> {
    let n = match text(p, "version")?.filter(|v| !v.is_empty()) {
        Some(v) => data.notices.iter().find(|n| n.version == v).ok_or_else(|| Error::NotFound(format!("no notice version `{v}`")))?,
        None => {
            privacy::current_notice(&data.notices, ctx.now).ok_or_else(|| Error::NotFound("no notice in effect: add one with notice.add".into()))?
        }
    };
    Ok(json!({"notice": to_value(n)}))
}

fn purposes(data: &Data) -> Value {
    let it = italian(data);
    json!({"purposes": privacy::known_purposes(&data.privacy).into_iter().map(|p| json!({"purpose": p, "description": views::purpose_text(&p, it)})).collect::<Vec<_>>()})
}

fn privacy_history(data: &Data, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    let mut events: Vec<&PrivacyEvent> = data.privacy.iter().filter(|e| e.customer_id == id).collect();
    events.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
    Ok(json!({"customer": id, "events": events.iter().map(|e| to_value(*e)).collect::<Vec<_>>()}))
}

fn method_param(p: &Value) -> CoreResult<Method> {
    match text(p, "method")?.filter(|m| !m.is_empty()) {
        None => Ok(Method::Paper),
        Some(m) => Method::parse(&m).ok_or_else(|| invalid(format!("`{m}` is not a method: paper, email, online, verbal or other"))),
    }
}

/// The notice version an event refers to: the one given (it must exist) or the current one.
fn notice_version(data: &Data, ctx: &Ctx, p: &Value, key: &str) -> CoreResult<String> {
    match text(p, key)?.filter(|v| !v.is_empty()) {
        Some(v) if data.notices.iter().any(|n| n.version == v) => Ok(v),
        Some(v) => Err(Error::NotFound(format!("no notice version `{v}`: add it with notice.add"))),
        None => Ok(privacy::current_notice(&data.notices, ctx.now).map(|n| n.version.clone()).unwrap_or_default()),
    }
}

fn base_event(ctx: &Ctx, p: &Value, customer: u64, kind: PrivacyKind, version: String) -> CoreResult<PrivacyEvent> {
    let at = when(p, "at", ctx)?;
    if at > time::stamp(ctx.now + remotecraft_core::chrono::Duration::days(1)) {
        return Err(invalid("`at` is in the future"));
    }
    Ok(PrivacyEvent {
        id: 0,
        customer_id: customer,
        kind,
        purpose: String::new(),
        notice_version: version,
        method: method_param(p)?,
        at,
        expires_at: None,
        signer: text(p, "signer")?.unwrap_or_default(),
        evidence: text(p, "evidence")?.unwrap_or_default(),
        note: notes(p, "note", 4_000)?.unwrap_or_default(),
        recorded_by: ctx.actor.clone(),
        recorded_at: time::stamp(ctx.now),
    })
}

fn push_event(data: &mut Data, ctx: &mut Ctx, mut e: PrivacyEvent) -> CoreResult<Value> {
    if data.privacy.len() >= limits::MAX_PRIVACY_EVENTS {
        return Err(Error::Refused("the privacy record is full".into()));
    }
    e.id = next(&mut data.counters.privacy)?;
    ctx.touch("privacy", e.id);
    ctx.touch("customer", e.customer_id);
    let v = to_value(&e);
    data.privacy.push(e);
    Ok(v)
}

fn privacy_notice(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let customer = customer_of_strict(data, p)?;
    let version = notice_version(data, ctx, p, "version")?;
    if version.is_empty() {
        return Err(Error::Refused("there is no privacy notice in the desk: add it first (notice.add with version, title and text)".into()));
    }
    let e = base_event(ctx, p, customer, PrivacyKind::NoticeDelivered, version)?;
    let event = push_event(data, ctx, e)?;
    Ok(json!({"event": event, "status": privacy_status(data, customer, ctx.now)}))
}

fn purposes_param(p: &Value) -> CoreResult<Vec<String>> {
    let mut out = Vec::new();
    for raw in list(p, "purposes", 20)?.unwrap_or_default().into_iter().chain(text(p, "purpose")?) {
        if raw.is_empty() {
            continue;
        }
        let id = privacy::purpose(&raw)?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    if out.is_empty() {
        return Err(invalid("give `purposes` (e.g. [\"remote_assistance\"]) or `purpose`"));
    }
    Ok(out)
}

fn privacy_consent(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let customer = customer_of_strict(data, p)?;
    let purposes = purposes_param(p)?;
    let version = notice_version(data, ctx, p, "notice_version")?;
    let expires = optional_when(p, "expires_at", ctx)?;
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    if boolean(p, "with_notice")?.unwrap_or(false) {
        if version.is_empty() {
            return Err(Error::Refused("with_notice: there is no privacy notice in the desk (notice.add)".into()));
        }
        let e = base_event(ctx, p, customer, PrivacyKind::NoticeDelivered, version.clone())?;
        events.push(push_event(data, ctx, e)?);
    } else if privacy::last_delivery(&data.privacy, customer).is_none() {
        warnings.push(
            "no privacy notice delivery is recorded for this customer: consent must be informed (privacy.notice, or with_notice: true)".to_string(),
        );
    }
    for purpose in purposes {
        let mut e = base_event(ctx, p, customer, PrivacyKind::ConsentGiven, version.clone())?;
        e.purpose = purpose;
        e.expires_at = expires.clone();
        events.push(push_event(data, ctx, e)?);
    }
    Ok(json!({"events": events, "warnings": warnings, "status": privacy_status(data, customer, ctx.now)}))
}

fn privacy_withdraw(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let customer = customer_of_strict(data, p)?;
    let purposes = purposes_param(p)?;
    let version = privacy::current_notice(&data.notices, ctx.now).map(|n| n.version.clone()).unwrap_or_default();
    let mut events = Vec::new();
    for purpose in &purposes {
        let mut e = base_event(ctx, p, customer, PrivacyKind::ConsentWithdrawn, version.clone())?;
        e.purpose = purpose.clone();
        events.push(push_event(data, ctx, e)?);
    }
    let mut warnings = Vec::new();
    if purposes.iter().any(|p| p == "unattended_access" || p == "remote_assistance") {
        for d in data.devices.iter().filter(|d| d.customer_id == customer && d.unattended && !d.retired) {
            warnings.push(format!(
                "device {} `{}` has unattended access: remove the permanent password from its RustDesk client, then device.update unattended=false",
                d.id, d.name
            ));
        }
    }
    Ok(json!({"events": events, "warnings": warnings, "status": privacy_status(data, customer, ctx.now)}))
}

fn gdpr_export(desk: &Desk, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let data = desk.data();
    let id = customer_of(data, p)?;
    let c = data.customer(id).ok_or_else(|| Error::NotFound(format!("no customer {id}")))?;
    ctx.touch("customer", id);
    let devices: Vec<&Device> = data.devices.iter().filter(|d| d.customer_id == id).collect();
    let device_ids: Vec<u64> = devices.iter().map(|d| d.id).collect();
    let work: Vec<&Intervention> = data.interventions.iter().filter(|i| i.customer_id == id).collect();
    let events: Vec<&PrivacyEvent> = data.privacy.iter().filter(|e| e.customer_id == id).collect();
    let versions: Vec<&String> = events.iter().map(|e| &e.notice_version).collect();
    let notices: Vec<&Notice> = data.notices.iter().filter(|n| versions.contains(&&n.version)).collect();
    let mine = |t: &String| {
        t == &format!("customer:{id}")
            || device_ids.iter().any(|d| t == &format!("device:{d}"))
            || work.iter().any(|i| t == &format!("intervention:{}", i.id))
    };
    let audit: Vec<Value> = desk.audit_entries(100_000).into_iter().filter(|e| e.targets.iter().any(mine)).map(|e| to_value(&e)).collect();
    Ok(json!({
        "exported_at": time::stamp(ctx.now),
        "controller": data.preferences.company_name,
        "format": "remotecraft-export-1",
        "customer": to_value(c),
        "devices": devices.iter().map(|d| to_value(*d)).collect::<Vec<_>>(),
        "interventions": work.iter().map(|i| to_value(*i)).collect::<Vec<_>>(),
        "privacy_events": events.iter().map(|e| to_value(*e)).collect::<Vec<_>>(),
        "notices": notices.iter().map(|n| to_value(*n)).collect::<Vec<_>>(),
        "access_log": audit,
    }))
}

fn gdpr_erase(data: &mut Data, ctx: &mut Ctx, p: &Value) -> CoreResult<Value> {
    let id = customer_of(data, p)?;
    if boolean(p, "confirm")? != Some(true) {
        return Err(Error::Refused("erasure cannot be undone: pass confirm: true (after gdpr.export if the person wants a copy)".into()));
    }
    let reason = required(p, "reason")?;
    if data.interventions.iter().any(|i| i.customer_id == id && i.running()) {
        return Err(Error::Refused("stop the running interventions first".into()));
    }
    let now = time::stamp(ctx.now);
    let c = data.customer_mut(id).ok_or_else(|| Error::NotFound(format!("no customer {id}")))?;
    if c.erased_at.is_some() {
        return Err(Error::Refused("already erased".into()));
    }
    *c = Customer {
        id,
        kind: c.kind,
        name: format!("Cliente cancellato #{id}"),
        created_at: c.created_at.clone(),
        updated_at: now.clone(),
        archived: true,
        erased_at: Some(now.clone()),
        ..Customer::default()
    };
    ctx.touch("customer", id);
    let mut devices = 0;
    for d in data.devices.iter_mut().filter(|d| d.customer_id == id) {
        *d = Device {
            id: d.id,
            customer_id: id,
            name: format!("Dispositivo #{}", d.id),
            os: d.os.clone(),
            retired: true,
            created_at: d.created_at.clone(),
            updated_at: now.clone(),
            ..Device::default()
        };
        devices += 1;
    }
    let mut work = 0;
    for i in data.interventions.iter_mut().filter(|i| i.customer_id == id) {
        i.summary = String::new();
        i.notes = String::new();
        i.reference = String::new();
        i.consent_override = String::new();
        i.updated_at = now.clone();
        work += 1;
    }
    let mut events = 0;
    for e in data.privacy.iter_mut().filter(|e| e.customer_id == id) {
        e.signer = String::new();
        e.note = String::new();
        events += 1;
    }
    ctx.detail = format!("erasure: {reason}");
    Ok(json!({
        "erased": id,
        "devices_anonymised": devices,
        "interventions_kept_without_text": work,
        "privacy_events_kept": events,
        "note": "times and billing data stay for accounting; the privacy record stays as proof (without names and notes); the audit log keeps only ids",
    }))
}

fn gdpr_review(data: &Data, ctx: &Ctx) -> Value {
    views::review(data, ctx.now)
}

fn audit_list(desk: &Desk, p: &Value) -> CoreResult<Value> {
    let data = desk.data();
    let mut targets = Vec::new();
    if let Some(v) = first(p, &["customer", "customer_id"]) {
        targets.push(format!("customer:{}", customer_ref(data, v)?));
    }
    if let Some(v) = first(p, &["device", "device_id"]) {
        targets.push(format!("device:{}", device_ref(data, v, None)?));
    }
    if let Some(i) = number(p, "intervention")? {
        targets.push(format!("intervention:{i}"));
    }
    let actor = text(p, "actor")?.filter(|a| !a.is_empty()).map(|a| fold(&a));
    let command = text(p, "command")?.filter(|c| !c.is_empty());
    let (limit, _) = page(p, 100)?;
    let mut rows: Vec<Value> = desk
        .audit_entries(1_000_000)
        .into_iter()
        .rev()
        .filter(|e| targets.iter().all(|t| e.targets.contains(t)))
        .filter(|e| actor.as_ref().is_none_or(|a| fold(&e.actor) == *a))
        .filter(|e| command.as_ref().is_none_or(|c| e.command == *c || e.command.starts_with(&format!("{c}."))))
        .take(limit)
        .map(|e| to_value(&e))
        .collect();
    rows.shrink_to_fit();
    Ok(json!({"entries": rows}))
}

fn prefs_set(data: &mut Data, p: &Value) -> CoreResult<Value> {
    // `{key, value}` is the same as `{<key>: value}`.
    let mut p = p.clone();
    if let (Some(k), Some(v)) = (p.get("key").and_then(Value::as_str).map(str::to_string), p.get("value").cloned()) {
        p[k.as_str()] = v;
    }
    let prefs = &mut data.preferences;
    let mut changed = Vec::new();
    if let Some(l) = text(&p, "language")? {
        let l = l.to_lowercase();
        if l != "it" && l != "en" {
            return Err(invalid("`language` must be it or en"));
        }
        prefs.language = l;
        changed.push("language");
    }
    if let Some(v) = text(&p, "theme")? {
        let v = v.to_lowercase();
        if !matches!(v.as_str(), "light" | "dark" | "system") {
            return Err(invalid("`theme` must be light, dark or system"));
        }
        prefs.theme = v;
        changed.push("theme");
    }
    if let Some(v) = text(&p, "company_name")? {
        prefs.company_name = v;
        changed.push("company_name");
    }
    if let Some(v) = bounded(&p, "billing_unit_minutes", 480)? {
        prefs.billing_unit_minutes = v;
        changed.push("billing_unit_minutes");
    }
    if let Some(v) = bounded(&p, "minimum_minutes", 480)? {
        prefs.minimum_minutes = v;
        changed.push("minimum_minutes");
    }
    if let Some(v) = boolean(&p, "require_consent")? {
        prefs.require_consent = v;
        changed.push("require_consent");
    }
    if let Some(v) = bounded(&p, "consent_validity_months", 240)? {
        prefs.consent_validity_months = v;
        changed.push("consent_validity_months");
    }
    if let Some(v) = bounded(&p, "retention_years", 100)? {
        prefs.retention_years = v;
        changed.push("retention_years");
    }
    if let Some(v) = text(&p, "rustdesk_host")? {
        prefs.rustdesk_host = v;
        changed.push("rustdesk_host");
    }
    if let Some(v) = text(&p, "rustdesk_key")? {
        prefs.rustdesk_key = v;
        changed.push("rustdesk_key");
    }
    if changed.is_empty() {
        return Err(invalid("nothing to set: see prefs.get for the keys"));
    }
    Ok(json!({"changed": changed, "preferences": to_value(&data.preferences)}))
}

/// The server name and key clients need: the preferences, else what the host knows.
fn rustdesk_server(desk: &Desk) -> (String, String) {
    let prefs = &desk.data().preferences;
    let env = desk.rustdesk();
    let host = if prefs.rustdesk_host.is_empty() { env.host.clone() } else { prefs.rustdesk_host.clone() };
    let key = if prefs.rustdesk_key.is_empty() { env.key.clone() } else { prefs.rustdesk_key.clone() };
    (host, key)
}

fn rustdesk_setup(desk: &Desk) -> Value {
    let (host, key) = rustdesk_server(desk);
    let mut missing = Vec::new();
    if host.is_empty() {
        missing.push("rustdesk_host (prefs.set, or RUSTDESK_HOST in the container)");
    }
    if key.is_empty() {
        missing.push("rustdesk_key (prefs.set; in the container it is read from id_ed25519.pub)");
    }
    let ready = missing.is_empty();
    json!({
        "id_server": host, "relay_server": host, "api_server": "", "key": key,
        "where": "RustDesk › Settings › Network › ID/Relay server",
        "config_string": if ready { json!(rustdesk::config_string(&host, &host, "", &key)) } else { Value::Null },
        "config_string_where": "RustDesk › Settings › Network › Import server config (paste the string)",
        "windows_installer_name": if ready { json!(rustdesk::installer_name(&host, &key)) } else { Value::Null },
        "windows_installer_note": "rename the official Windows installer to this name: it installs already pointed at this server",
        "ports": rustdesk::PORTS.iter().map(|(p, what)| json!({"port": p, "service": what})).collect::<Vec<_>>(),
        "missing": missing,
    })
}

fn rustdesk_check(desk: &Desk, p: &Value) -> CoreResult<Value> {
    let env = desk.rustdesk();
    let Some(probe) = &env.probe else {
        return Err(Error::Refused("this RemoteCraft cannot reach the network (no probe): run the check from the CLI or the server".into()));
    };
    let (public_host, key) = rustdesk_server(desk);
    let host = text(p, "host")?
        .filter(|h| !h.is_empty())
        .or_else(|| Some(env.check_host.clone()).filter(|h| !h.is_empty()))
        .or_else(|| Some(public_host.clone()).filter(|h| !h.is_empty()))
        .ok_or_else(|| invalid("give `host`, or set rustdesk_host (prefs.set)"))?;
    if host.chars().any(|c| c.is_whitespace() || c == '/' || c == '@') || host.len() > 255 {
        return Err(invalid(format!("`{host}` is not a host name")));
    }
    let ports: Vec<Value> = rustdesk::PORTS
        .iter()
        .map(|(port, what)| match probe(&host, *port) {
            Ok(ms) => json!({"port": port, "service": what, "ok": true, "ms": ms}),
            Err(e) => json!({"port": port, "service": what, "ok": false, "error": e}),
        })
        .collect();
    let all = ports.iter().all(|p| p["ok"] == true);
    Ok(json!({
        "host": host,
        "public_host": public_host,
        "ports": ports,
        "key_known": !key.is_empty(),
        "ok": all && !key.is_empty(),
        "note": "UDP 21116 cannot be tested with a connection: if clients stay offline while TCP works, check UDP forwarding on the router",
    }))
}

fn app_status(desk: &Desk, ctx: &Ctx) -> Value {
    let d = desk.data();
    json!({
        "name": "remotecraft",
        "version": env!("CARGO_PKG_VERSION"),
        "actor": ctx.actor,
        "data_dir": desk.paths().map(|p| p.dir.display().to_string()),
        "customers": d.customers.iter().filter(|c| !c.archived && c.erased_at.is_none()).count(),
        "devices": d.devices.iter().filter(|x| !x.retired).count(),
        "interventions": d.interventions.len(),
        "running": d.interventions.iter().filter(|i| i.running()).map(|i| json!({"id": i.id, "technician": i.technician, "customer": views::customer_name(d, i.customer_id), "started_at": i.started_at})).collect::<Vec<_>>(),
        "notices": d.notices.len(),
        "problems": desk.problems(),
    })
}
