use remotecraft_core::time::parse_stamp;
use serde_json::{Value, json};

use super::*;

fn now() -> DateTime<Utc> {
    parse_stamp("2026-10-13T08:00:00Z").unwrap()
}

/// The demo desk at a fixed time.
fn demo() -> Desk {
    let mut d = Desk::in_memory(remotecraft_core::mock::demo(now()));
    d.set_clock(now());
    d.set_actor("riccardo");
    d
}

fn empty() -> Desk {
    let mut d = Desk::in_memory(Data::new());
    d.set_clock(now());
    d.set_actor("anna");
    d
}

fn run(d: &mut Desk, cmd: &str, p: Value) -> Value {
    match d.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

fn fails(d: &mut Desk, cmd: &str, p: Value) -> String {
    match d.execute(cmd, &p) {
        Ok(v) => panic!("{cmd} {p} should fail, got {v}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn every_command_survives_empty_params() {
    for c in COMMANDS {
        for p in [json!({}), Value::Null] {
            let mut d = demo();
            let before = d.data().clone();
            let r = d.execute(c.id, &p);
            if r.is_err() {
                assert_eq!(d.data(), &before, "{} changed the desk on failure", c.id);
            }
        }
    }
}

#[test]
fn every_command_survives_hostile_params() {
    let junk = [
        json!([]),
        json!("x"),
        json!(42),
        json!({"customer": [], "device": {}, "id": -1, "name": 5, "rustdesk_id": "\u{0}".repeat(50), "started_at": "\u{202e}", "minutes": 1e300}),
        json!({"customer": "x".repeat(1_000_000), "query": "%%%", "purposes": [1, 2], "limit": -5, "offset": "a"}),
        json!({"customer": 1, "device": 1, "id": u64::MAX, "started_at": "9999-99-99", "ended_at": "0000-01-01", "minutes": u64::MAX, "confirm": "maybe"}),
        json!({"customer": 1, "id": 1, "contacts": [1, "x"], "tags": {"a": 1}, "key": "language", "value": "klingon"}),
        json!({"customer": 1, "id": 1, "group_by": "nothing", "from": "yesterday-ish", "version": "", "purposes": "", "at": "2999-01-01"}),
    ];
    for c in COMMANDS {
        for p in &junk {
            let mut d = demo();
            let before = d.data().clone();
            if d.execute(c.id, p).is_err() {
                assert_eq!(d.data(), &before, "{} changed the desk on failure with {p}", c.id);
            }
        }
    }
}

#[test]
fn unknown_commands_and_bad_params_are_errors() {
    let mut d = demo();
    assert!(matches!(d.execute("nope", &json!({})), Err(Error::UnknownCommand(_))));
    assert!(d.execute("customer.list", &json!([1])).is_err());
}

#[test]
fn customers_are_added_found_and_protected_from_duplicates() {
    let mut d = empty();
    let c = run(
        &mut d,
        "customer.add",
        json!({"name": "Bar Aurora", "kind": "azienda", "tax_id": "it 0011 2233 445", "email": "bar@aurora.example", "tags": "bar, centro", "contacts": [{"name": "Lia", "phone": "333"}]}),
    );
    assert_eq!(c["customer"]["id"], 1);
    assert_eq!(c["customer"]["tax_id"], "IT00112233445");
    assert_eq!(c["customer"]["tags"], json!(["bar", "centro"]));
    assert!(fails(&mut d, "customer.add", json!({"name": "Altro", "tax_id": "IT00112233445"})).contains("already"));
    run(&mut d, "customer.add", json!({"name": "Altro", "tax_id": "IT00112233445", "allow_duplicate": true}));
    assert!(fails(&mut d, "customer.add", json!({"name": "X", "email": "not-an-address"})).contains("e-mail"));
    assert!(fails(&mut d, "customer.add", json!({"name": "  "})).contains("required"));
    let u = run(&mut d, "customer.update", json!({"customer": "Bar Aurora", "phone": "+39 0471 1", "name": "Bar Aurora snc"}));
    assert_eq!(u["customer"]["name"], "Bar Aurora snc");
    assert_eq!(run(&mut d, "customer.list", json!({"query": "lia"}))["total"], 1);
    assert_eq!(run(&mut d, "customer.list", json!({"tag": "centro"}))["total"], 1);
    run(&mut d, "customer.archive", json!({"customer": 1}));
    assert_eq!(run(&mut d, "customer.list", json!({}))["total"], 1);
    assert_eq!(run(&mut d, "customer.list", json!({"include_archived": true}))["total"], 2);
}

#[test]
fn devices_keep_rustdesk_ids_unique() {
    let mut d = demo();
    let v = run(&mut d, "device.add", json!({"customer": "corvo", "name": "PC ufficio", "rustdesk_id": "555 666 777", "os": "Windows 11"}));
    assert_eq!(v["device"]["rustdesk_id"], "555666777");
    assert_eq!(v["device"]["rustdesk_display"], "555 666 777");
    assert_eq!(v["device"]["connect_uri"], "rustdesk://connection/new/555666777");
    let err = fails(&mut d, "device.add", json!({"customer": 3, "rustdesk_id": "555666777"}));
    assert!(err.contains("already on device"), "{err}");
    assert!(fails(&mut d, "device.add", json!({"customer": 3, "rustdesk_id": "12; rm -rf /"})).contains("not a RustDesk ID"));
    let found = run(&mut d, "device.find", json!({"rustdesk_id": "555-666-777"}));
    assert_eq!(found["devices"][0]["customer"]["name"], "Ferramenta Corvo");
    assert_eq!(run(&mut d, "customer.list", json!({"query": "555 666 777"}))["customers"][0]["id"], 2);
    // A retired device frees its RustDesk ID.
    run(&mut d, "device.retire", json!({"device": "555666777"}));
    run(&mut d, "device.add", json!({"customer": 3, "rustdesk_id": "555666777"}));
    // A device with history cannot be deleted, only retired.
    assert!(fails(&mut d, "device.remove", json!({"device": 1})).contains("retire"));
    // Moving a device to another customer.
    let moved = run(&mut d, "device.update", json!({"device": 4, "customer": 3}));
    assert_eq!(moved["device"]["customer_id"], 3);
}

#[test]
fn timer_start_and_stop() {
    let mut d = demo();
    let s = run(&mut d, "intervention.start", json!({"device": "100 200 300", "summary": "Outlook non si apre"}));
    assert_eq!(s["intervention"]["kind"], "remote");
    assert_eq!(s["intervention"]["customer_id"], 1);
    assert_eq!(s["intervention"]["running"], true);
    assert_eq!(s["connect"]["uri"], "rustdesk://connection/new/100200300");
    let id = s["intervention"]["id"].as_u64().unwrap();
    assert_eq!(run(&mut d, "intervention.running", json!({}))["running"].as_array().unwrap().len(), 1);
    d.set_clock(now() + remotecraft_core::chrono::Duration::minutes(37));
    let stopped = run(&mut d, "intervention.stop", json!({"notes": "Profilo ricreato."}));
    assert_eq!(stopped["intervention"]["id"], id);
    assert_eq!(stopped["intervention"]["minutes"], 37);
    assert_eq!(stopped["intervention"]["running"], false);
    assert!(fails(&mut d, "intervention.stop", json!({"id": id})).contains("already closed"));
    assert!(fails(&mut d, "intervention.stop", json!({})).contains("no running"));
    // Another technician's timer is not stopped by mistake.
    run(&mut d, "intervention.start", json!({"customer": 2, "kind": "phone"}));
    assert!(d.execute_as("anna", "intervention.stop", &json!({})).is_err());
}

#[test]
fn stop_rejects_an_end_before_the_start() {
    let mut d = demo();
    let s = run(&mut d, "intervention.start", json!({"customer": 2, "kind": "telefono", "started_at": "2026-10-13T08:00:00Z"}));
    let id = s["intervention"]["id"].clone();
    assert!(fails(&mut d, "intervention.stop", json!({"id": id, "ended_at": "2026-10-13T07:00:00Z"})).contains("before"));
    let ok = run(&mut d, "intervention.stop", json!({"id": id, "ended_at": "2026-10-13T09:00:00Z", "minutes": 45}));
    assert_eq!(ok["intervention"]["minutes"], 45);
}

#[test]
fn logging_past_work() {
    let mut d = demo();
    let v = run(
        &mut d,
        "intervention.add",
        json!({"customer": "Ferramenta Corvo", "kind": "in sede", "summary": "Cablaggio", "started_at": "2026-10-12T14:00:00Z", "ended_at": "2026-10-12T16:30:00Z"}),
    );
    assert_eq!(v["intervention"]["minutes"], 150);
    assert_eq!(v["intervention"]["kind"], "onsite");
    let v = run(
        &mut d,
        "intervention.add",
        json!({"customer": 2, "kind": "phone", "summary": "Telefonata", "started_at": "2026-10-12T10:00:00Z", "minutes": 7, "billable": false}),
    );
    assert_eq!(v["intervention"]["ended_at"], "2026-10-12T10:07:00Z");
    assert!(fails(&mut d, "intervention.add", json!({"customer": 2, "summary": "x", "started_at": "2026-10-12T10:00:00Z"})).contains("ended_at"));
    assert!(
        fails(
            &mut d,
            "intervention.add",
            json!({"customer": 2, "summary": "x", "started_at": "2026-10-01T10:00:00Z", "ended_at": "2026-10-12T10:00:00Z"})
        )
        .contains("week")
    );
    assert!(
        fails(&mut d, "intervention.add", json!({"device": 1, "customer": 2, "summary": "x", "started_at": "10:00", "minutes": 5}))
            .contains("no device")
    );
    let u = run(&mut d, "intervention.update", json!({"id": v["intervention"]["id"], "ended_at": "2026-10-12T10:20:00Z"}));
    assert_eq!(u["intervention"]["minutes"], 20);
}

#[test]
fn remote_work_needs_consent() {
    let mut d = demo();
    // Elio Brina (3) received the notice but gave no remote consent.
    let err = fails(&mut d, "intervention.start", json!({"device": "300400500"}));
    assert!(err.contains("remote_assistance consent is missing"), "{err}");
    // Not remote: allowed.
    run(&mut d, "intervention.start", json!({"customer": 3, "kind": "lab", "summary": "Ritiro"}));
    // With a reason it goes ahead, and the reason is kept and audited.
    let v = run(&mut d, "intervention.start", json!({"device": "300400500", "consent_override": "Consenso a voce al telefono, modulo da firmare"}));
    assert_eq!(v["intervention"]["consent_override"], "Consenso a voce al telefono, modulo da firmare");
    assert!(v["warnings"][0].as_str().unwrap().contains("missing"));
    let audit = d.audit_entries(10);
    assert!(audit.last().unwrap().detail.contains("consent override"));
    // Recording the consent lifts the block.
    let c = run(
        &mut d,
        "privacy.consent",
        json!({"customer": 3, "purposes": ["remote_assistance"], "method": "carta", "signer": "Elio Brina", "evidence": "Modulo 2026/40"}),
    );
    assert_eq!(c["status"]["remote_ready"], true);
    assert!(c["warnings"].as_array().unwrap().is_empty());
    run(&mut d, "intervention.start", json!({"device": "300400500"}));
    // The desk can turn the rule off.
    let mut e = demo();
    run(&mut e, "prefs.set", json!({"require_consent": false}));
    let v = run(&mut e, "intervention.start", json!({"device": "300400500"}));
    assert!(!v["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn consent_history_withdrawal_and_expiry() {
    let mut d = demo();
    let st = run(&mut d, "privacy.status", json!({"customer": 1}));
    assert_eq!(st["status"]["remote_ready"], true);
    assert_eq!(st["status"]["notice"]["up_to_date"], true);
    let w = run(&mut d, "privacy.withdraw", json!({"customer": 1, "purpose": "unattended_access", "note": "Non vogliono più accessi notturni"}));
    assert_eq!(w["warnings"].as_array().unwrap().len(), 2); // two devices with unattended access
    let connect = run(&mut d, "device.connect", json!({"device": 1}));
    assert_eq!(connect["consent_ok"], true);
    assert!(connect["warnings"][0].as_str().unwrap().contains("unattended"));
    let h = run(&mut d, "privacy.history", json!({"customer": 1}));
    assert_eq!(h["events"].as_array().unwrap().len(), 4);
    assert_eq!(h["events"][0]["kind"], "consent_withdrawn");
    // Consents older than the validity expire.
    run(&mut d, "prefs.set", json!({"key": "consent_validity_months", "value": 6}));
    let st = run(&mut d, "privacy.status", json!({"customer": 1}));
    assert_eq!(st["status"]["remote_assistance"], "expired");
    // Future dates are refused.
    assert!(fails(&mut d, "privacy.consent", json!({"customer": 1, "purpose": "marketing", "at": "2030-01-01"})).contains("future"));
}

#[test]
fn notices_have_versions_and_deliveries() {
    let mut d = demo();
    assert!(fails(&mut d, "notice.add", json!({"version": "2026-01"})).contains("exists"));
    run(&mut d, "notice.add", json!({"version": "2026-10", "title": "Informativa", "text": "Testo nuovo"}));
    let l = run(&mut d, "notice.list", json!({}));
    assert_eq!(l["notices"][0]["version"], "2026-10");
    assert_eq!(l["notices"][0]["current"], true);
    assert_eq!(run(&mut d, "notice.get", json!({}))["notice"]["text"], "Testo nuovo");
    let st = run(&mut d, "privacy.status", json!({"customer": 1}));
    assert_eq!(st["status"]["notice"]["up_to_date"], false);
    run(&mut d, "privacy.notice", json!({"customer": 1, "method": "email", "evidence": "Mail del 13/10"}));
    let st = run(&mut d, "privacy.status", json!({"customer": 1}));
    assert_eq!(st["status"]["notice"]["up_to_date"], true);
    assert!(fails(&mut d, "privacy.notice", json!({"customer": 1, "version": "v9"})).contains("no notice version"));
    let mut e = empty();
    run(&mut e, "customer.add", json!({"name": "A"}));
    assert!(fails(&mut e, "privacy.notice", json!({"customer": 1})).contains("notice.add"));
    let c = run(&mut e, "privacy.consent", json!({"customer": 1, "purpose": "remote_assistance"}));
    assert!(c["warnings"][0].as_str().unwrap().contains("informed"));
}

#[test]
fn time_report_rounds_billable_time() {
    let mut d = demo();
    let r = run(&mut d, "report.time", json!({"group_by": "customer"}));
    // Lunaria: 25+70+120+45+20+40 = 320 min; billable rounded to 15: 30+75+120+45+30+45 = 345.
    let row = r["rows"].as_array().unwrap().iter().find(|r| r["label"] == "Studio Tecnico Lunaria").unwrap().clone();
    assert_eq!(row["minutes"], 320);
    assert_eq!(row["billable_minutes"], 345);
    assert_eq!(row["hours"], "5:20");
    // Corvo: 15+10(not billable)+35 = 60; billable 15+0+45 = 60.
    let row = r["rows"].as_array().unwrap().iter().find(|r| r["label"] == "Ferramenta Corvo").unwrap().clone();
    assert_eq!(row["minutes"], 60);
    assert_eq!(row["billable_minutes"], 60);
    run(&mut d, "prefs.set", json!({"billing_unit_minutes": 0, "minimum_minutes": 30}));
    let r = run(&mut d, "report.time", json!({"customer": 2}));
    assert_eq!(r["totals"]["billable_minutes"], 30 + 35);
    let by_tech = run(&mut d, "report.time", json!({"group_by": "technician", "from": "2026-10-01", "to": "2026-10-14"}));
    assert_eq!(by_tech["rows"].as_array().unwrap().len(), 1);
    assert!(fails(&mut d, "report.time", json!({"group_by": "planet"})).contains("group_by"));
    let list = run(&mut d, "intervention.list", json!({"customer": 1, "kind": "remote"}));
    assert_eq!(list["total"], 4);
    assert_eq!(run(&mut d, "intervention.list", json!({"query": "plotter"}))["total"], 1);
}

#[test]
fn export_and_erasure() {
    let mut d = demo();
    let x = run(&mut d, "gdpr.export", json!({"customer": 1}));
    assert_eq!(x["customer"]["name"], "Studio Tecnico Lunaria");
    assert_eq!(x["devices"].as_array().unwrap().len(), 3);
    assert_eq!(x["interventions"].as_array().unwrap().len(), 6);
    assert_eq!(x["privacy_events"].as_array().unwrap().len(), 3);
    assert_eq!(x["notices"].as_array().unwrap().len(), 1);
    assert!(fails(&mut d, "gdpr.erase", json!({"customer": 1, "reason": "richiesta"})).contains("confirm"));
    assert!(fails(&mut d, "gdpr.erase", json!({"customer": 1, "confirm": true})).contains("reason"));
    let e = run(&mut d, "gdpr.erase", json!({"customer": 1, "confirm": true, "reason": "Richiesta via PEC del 12/10"}));
    assert_eq!(e["devices_anonymised"], 3);
    let c = d.data().customer(1).unwrap().clone();
    assert!(c.erased_at.is_some() && c.email.is_empty() && c.contacts.is_empty() && c.notes.is_empty());
    assert!(d.data().devices.iter().filter(|x| x.customer_id == 1).all(|x| x.rustdesk_id.is_empty() && x.retired));
    assert!(d.data().interventions.iter().filter(|i| i.customer_id == 1).all(|i| i.notes.is_empty() && i.summary.is_empty() && i.minutes > 0));
    assert!(d.data().privacy.iter().filter(|p| p.customer_id == 1).all(|p| p.signer.is_empty()));
    // The serialized desk no longer holds the name anywhere.
    assert!(!serde_json::to_string(d.data()).unwrap().contains("Lunaria"));
    // The time report still counts the hours.
    let r = run(&mut d, "report.time", json!({"customer": 1}));
    assert_eq!(r["totals"]["minutes"], 320);
    // Erased customers are not found by name and cannot be edited.
    assert!(d.execute("customer.get", &json!({"customer": "Lunaria"})).is_err());
    assert!(fails(&mut d, "customer.update", json!({"customer": 1, "phone": "1"})).contains("erased"));
    assert!(fails(&mut d, "intervention.start", json!({"customer": 1})).contains("erased"));
    assert!(d.audit_entries(5).iter().any(|a| a.command == "gdpr.erase" && a.detail.contains("PEC")));
}

#[test]
fn review_finds_gaps() {
    let mut d = demo();
    let r = run(&mut d, "gdpr.review", json!({}));
    let issues: Vec<String> =
        r["findings"].as_array().unwrap().iter().map(|f| format!("{}:{}", f["customer"], f["issue"].as_str().unwrap())).collect();
    assert!(issues.contains(&"3:remote_assistance_missing".to_string()), "{issues:?}");
    assert!(!issues.iter().any(|i| i.starts_with("1:")), "{issues:?}");
    let mut e = empty();
    let r = run(&mut e, "gdpr.review", json!({}));
    assert_eq!(r["findings"][0]["issue"], "no_notice");
}

#[test]
fn customer_remove_only_without_history() {
    let mut d = demo();
    assert!(fails(&mut d, "customer.remove", json!({"customer": 1})).contains("gdpr.erase"));
    let id = run(&mut d, "customer.add", json!({"name": "Prova"}))["customer"]["id"].clone();
    run(&mut d, "device.add", json!({"customer": id, "name": "PC"}));
    let r = run(&mut d, "customer.remove", json!({"customer": id}));
    assert_eq!(r["devices_removed"].as_array().unwrap().len(), 1);
}

#[test]
fn audit_records_who_did_what_without_personal_data() {
    let mut d = empty();
    d.execute_as("marco", "customer.add", &json!({"name": "Segreto Srl", "email": "a@b.it"})).unwrap();
    run(&mut d, "customer.get", json!({"customer": 1}));
    run(&mut d, "gdpr.export", json!({"customer": 1}));
    let a = run(&mut d, "audit.list", json!({"customer": 1}));
    let entries = a["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2); // add and export, not the plain read
    assert_eq!(entries[0]["command"], "gdpr.export");
    assert_eq!(entries[1]["actor"], "marco");
    assert!(!a.to_string().contains("Segreto"));
    assert_eq!(run(&mut d, "audit.list", json!({"actor": "MARCO"}))["entries"].as_array().unwrap().len(), 1);
}

#[test]
fn files_persist_reload_and_roll_back() {
    let dir = std::env::temp_dir().join(format!("remotecraft-engine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let paths = Paths::under(&dir);
    let mut d = Desk::open(paths.clone()).unwrap();
    d.execute("customer.add", &json!({"name": "Persistente"})).unwrap();
    // Another process (the CLI next to the server) changes the file: the desk sees it.
    let mut other = Desk::open(paths.clone()).unwrap();
    assert_eq!(other.data().customers.len(), 1);
    std::thread::sleep(std::time::Duration::from_millis(20));
    other.execute("customer.add", &json!({"name": "Dall'altro processo"})).unwrap();
    let list = d.execute("customer.list", &json!({})).unwrap();
    assert_eq!(list["total"], 2);
    assert!(std::fs::read_to_string(&paths.audit).unwrap().lines().count() >= 2);
    // A failed write leaves the desk as it was.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let before = d.data().clone();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let r = d.execute("customer.add", &json!({"name": "Non salvato"}));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root ignores directory permissions; only check when the write really failed.
        if r.is_err() {
            assert_eq!(d.data(), &before);
        }
    }
    // A corrupted file is an error, never an empty desk.
    std::fs::write(&paths.desk, "{broken").unwrap();
    assert!(Desk::open(paths).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefs_and_setup() {
    let mut d = empty();
    assert!(fails(&mut d, "prefs.set", json!({})).contains("nothing"));
    assert!(fails(&mut d, "prefs.set", json!({"language": "fr"})).contains("language"));
    assert!(fails(&mut d, "prefs.set", json!({"billing_unit_minutes": 100_000})).contains("at most"));
    let s = run(&mut d, "rustdesk.setup", json!({}));
    assert_eq!(s["missing"].as_array().unwrap().len(), 2);
    d.set_default_rustdesk_key("KEY=");
    run(&mut d, "prefs.set", json!({"rustdesk_host": "rustdesk.example.it"}));
    let s = run(&mut d, "rustdesk.setup", json!({}));
    assert_eq!(s["key"], "KEY=");
    assert_eq!(s["id_server"], "rustdesk.example.it");
    assert!(s["missing"].as_array().unwrap().is_empty());
    assert_eq!(run(&mut d, "app.commands", json!({}))["commands"].as_array().unwrap().len(), COMMANDS.len());
}

#[test]
fn rustdesk_setup_and_check() {
    let mut d = empty();
    assert!(fails(&mut d, "rustdesk.check", json!({})).contains("probe"));
    d.set_default_rustdesk_host("rd.example.it");
    d.set_default_rustdesk_key("abcKEY+1=");
    let s = run(&mut d, "rustdesk.setup", json!({}));
    assert_eq!(s["id_server"], "rd.example.it");
    assert!(s["config_string"].as_str().unwrap().ends_with("Jye"));
    assert_eq!(s["windows_installer_name"], "rustdesk-host=rd.example.it,key=abcKEY+1=,.exe");
    // The preferences win over what the container says.
    run(&mut d, "prefs.set", json!({"rustdesk_host": "assistenza.example.it"}));
    assert_eq!(run(&mut d, "rustdesk.setup", json!({}))["relay_server"], "assistenza.example.it");
    d.set_rustdesk_probe(
        "127.0.0.1",
        Box::new(|host: &str, port: u16| if host == "127.0.0.1" && port != 21115 { Ok(3) } else { Err("refused".into()) }),
    );
    let c = run(&mut d, "rustdesk.check", json!({}));
    assert_eq!(c["host"], "127.0.0.1");
    assert_eq!(c["ok"], false);
    assert_eq!(c["ports"][0]["ok"], false);
    assert_eq!(c["ports"][1]["ok"], true);
    assert!(fails(&mut d, "rustdesk.check", json!({"host": "a b"})).contains("host"));
    let c = run(&mut d, "rustdesk.check", json!({"host": "elsewhere"}));
    assert_eq!(c["ports"][2]["ok"], false);
}

#[test]
fn rustdesk_check_probes_the_ports_in_parallel() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Each probe waits (at most 5 s) until all three run at once: one after the other, a host
    // that drops the packets would cost three timeouts.
    let active = std::sync::Arc::new(AtomicUsize::new(0));
    let seen = active.clone();
    let mut d = empty();
    d.set_rustdesk_probe(
        "127.0.0.1",
        Box::new(move |_: &str, port: u16| {
            seen.fetch_add(1, Ordering::SeqCst);
            for _ in 0..500 {
                if seen.load(Ordering::SeqCst) >= 3 {
                    return Ok(u64::from(port));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err("alone".into())
        }),
    );
    let c = run(&mut d, "rustdesk.check", json!({}));
    let ports = c["ports"].as_array().unwrap();
    assert_eq!(ports.len(), 3);
    assert!(ports.iter().all(|p| p["ok"] == true), "{c}");
    assert_eq!(ports[0]["ms"], 21115);
    assert_eq!(ports[2]["ms"], 21117);
    // The prepared check answers the same as the command.
    let prepared = d.prepare_rustdesk_check(&json!({"host": "127.0.0.1"})).unwrap();
    assert_eq!(prepared.host(), "127.0.0.1");
    assert_eq!(prepared.run()["ports"].as_array().unwrap().len(), 3);
    assert!(d.prepare_rustdesk_check(&json!("x")).is_err());
    assert!(d.prepare_rustdesk_check(&json!({"host": "a/b"})).is_err());
    // A probe that panics is a failed port, not a crash.
    d.set_rustdesk_probe("127.0.0.1", Box::new(|_: &str, port: u16| if port == 21116 { panic!("probe bug") } else { Ok(1) }));
    let c = run(&mut d, "rustdesk.check", json!({}));
    assert_eq!(c["ports"][1]["ok"], false);
    assert_eq!(c["ports"][0]["ok"], true);
}

#[test]
fn putting_a_pc_back_in_service_keeps_rustdesk_ids_unique() {
    let mut d = demo();
    let old = run(&mut d, "device.add", json!({"customer": 3, "name": "Vecchio PC", "rustdesk_id": "111222333"}))["device"]["id"].clone();
    run(&mut d, "device.retire", json!({"device": old}));
    // The RustDesk ID went to the new PC.
    let new = run(&mut d, "device.add", json!({"customer": 3, "name": "Nuovo PC", "rustdesk_id": "111 222 333"}))["device"]["id"].clone();
    let before = d.data().clone();
    let err = fails(&mut d, "device.retire", json!({"device": old, "retired": false}));
    assert!(err.contains("111 222 333") && err.contains(&format!("device {new} `Nuovo PC`")) && err.contains("Elio Brina"), "{err}");
    assert_eq!(d.data(), &before);
    // Retiring again is always allowed; so is putting back once the other one is retired.
    run(&mut d, "device.retire", json!({"device": old}));
    run(&mut d, "device.retire", json!({"device": new}));
    run(&mut d, "device.retire", json!({"device": old, "retired": false}));
    assert!(!d.data().device(old.as_u64().unwrap()).unwrap().retired);
    // A PC without a RustDesk ID never clashes.
    let bare = run(&mut d, "device.add", json!({"customer": 3, "name": "Senza ID"}))["device"]["id"].clone();
    run(&mut d, "device.retire", json!({"device": bare}));
    run(&mut d, "device.retire", json!({"device": bare, "retired": false}));
}

#[test]
fn a_quick_connection_needs_only_the_rustdesk_id() {
    let mut d = demo();
    let before = d.data().clone();
    // A PC the desk does not know: the link only, tied to no customer, nothing changed.
    let v = run(&mut d, "device.connect", json!({"rustdesk_id": "987 654 321"}));
    assert_eq!(v["connect"]["uri"], "rustdesk://connection/new/987654321");
    assert_eq!(v["connect"]["display"], "987 654 321");
    assert!(v["device"].is_null() && v["consent_ok"].is_null(), "{v}");
    assert!(v["warnings"][0].as_str().unwrap().contains("not in the desk"), "{v}");
    assert_eq!(d.data(), &before);
    // Audited without the ID: no record to name, and an ID typed is not kept.
    let last = d.audit_entries(1).pop().unwrap();
    assert_eq!((last.command.as_str(), last.targets.len()), ("device.connect", 0));
    assert!(!serde_json::to_string(&last).unwrap().contains("987"));
    // A known PC is the same as naming it: its customer, its consent.
    let known = run(&mut d, "device.connect", json!({"rustdesk_id": "100-200-300"}));
    assert_eq!(known["device"]["id"], 1);
    assert_eq!(known["customer"], "Studio Tecnico Lunaria");
    // `id` names the PC as `device` does.
    assert_eq!(run(&mut d, "device.connect", json!({"id": 1, "rustdesk_id": "987654321"}))["device"]["id"], 1);
    // A name or a host name is not dialled as an ID.
    for name in ["LUN-SEG01", "PC segreteria", "Lunaria"] {
        assert!(fails(&mut d, "device.connect", json!({"rustdesk_id": name})).contains("no PC has the RustDesk ID"), "{name}");
    }
    // A retired PC connects as itself: its customer, its consent, both in the audit.
    run(&mut d, "privacy.withdraw", json!({"customer": 1, "purposes": ["remote_assistance"]}));
    run(&mut d, "device.retire", json!({"device": 1}));
    let retired = run(&mut d, "device.connect", json!({"rustdesk_id": "100 200 300"}));
    assert_eq!((retired["device"]["id"].as_u64(), retired["consent_ok"].as_bool()), (Some(1), Some(false)), "{retired}");
    let last = d.audit_entries(1).pop().unwrap();
    assert_eq!(last.targets, vec!["device:1", "customer:1"]);
    // Not an ID, or nothing at all: refused.
    assert!(fails(&mut d, "device.connect", json!({"rustdesk_id": "12; rm -rf /"})).contains("not a RustDesk ID"));
    assert!(!fails(&mut d, "device.connect", json!({"rustdesk_id": "  "})).is_empty());
}

#[test]
fn an_unchanged_rustdesk_id_is_not_a_clash() {
    // Two PCs in service with one ID (a desk written before put-backs were checked).
    let mut data = remotecraft_core::mock::demo(now());
    let rid = data.devices.first().map(|x| x.rustdesk_id.clone()).unwrap();
    let mut twin = data.devices.first().cloned().unwrap();
    twin.id = data.devices.iter().map(|x| x.id).max().unwrap() + 1;
    twin.name = "Gemello".into();
    data.devices.push(twin.clone());
    let mut d = Desk::in_memory(data);
    d.set_clock(now());
    // Saving either one with every field, the ID unchanged, goes through.
    let v = run(&mut d, "device.update", json!({"device": twin.id, "name": "Gemello 2", "rustdesk_id": remotecraft_core::rustdesk::display(&rid)}));
    assert_eq!(v["device"]["name"], "Gemello 2");
    run(&mut d, "device.update", json!({"device": 1, "rustdesk_id": rid}));
    // Taking an ID another PC in service holds is still refused.
    let other = run(&mut d, "device.add", json!({"customer": 3, "name": "Altro", "rustdesk_id": "999888777"}))["device"]["id"].clone();
    let err = fails(&mut d, "device.update", json!({"device": other, "rustdesk_id": rid}));
    assert!(err.contains("already on device"), "{err}");
}

#[test]
fn editing_work_into_remote_work_checks_consent() {
    let mut d = demo();
    // Elio Brina (3) gave no remote consent: on-site work is fine.
    let v = run(
        &mut d,
        "intervention.add",
        json!({"customer": 3, "kind": "onsite", "summary": "Visita", "started_at": "2026-10-12T10:00:00Z", "minutes": 30}),
    );
    let id = v["intervention"]["id"].as_u64().unwrap();
    // Turning it into remote work is refused like logging remote work.
    let before = d.data().clone();
    let err = fails(&mut d, "intervention.update", json!({"id": id, "kind": "remote"}));
    assert!(err.contains("remote_assistance consent is missing"), "{err}");
    assert_eq!(d.data(), &before);
    // With the reason it goes ahead: kept on the intervention and in the audit log.
    let u = run(&mut d, "intervention.update", json!({"id": id, "kind": "remote", "consent_override": "Consenso a voce al telefono"}));
    assert_eq!(u["intervention"]["kind"], "remote");
    assert_eq!(u["intervention"]["consent_override"], "Consenso a voce al telefono");
    assert!(u["warnings"][0].as_str().unwrap().contains("missing"), "{u}");
    assert!(d.audit_entries(1)[0].detail.contains("consent override"));
    // Other edits of remote work do not ask again, and keep the reason.
    let u = run(&mut d, "intervention.update", json!({"id": id, "summary": "Visita e controllo"}));
    assert_eq!(u["intervention"]["consent_override"], "Consenso a voce al telefono");
    assert_eq!(u["warnings"], json!([]));
    // Remote work moved to a PC asks again.
    assert!(fails(&mut d, "intervention.update", json!({"id": id, "device": "300400500"})).contains("consent"));
    let u = run(&mut d, "intervention.update", json!({"id": id, "device": "300400500", "consent_override": "Ha accettato al telefono"}));
    assert_eq!(u["intervention"]["device_id"], 5);
    assert_eq!(u["intervention"]["consent_override"], "Ha accettato al telefono");
    // Naming the same PC again, or taking the PC off, is no new remote access.
    run(&mut d, "intervention.update", json!({"id": id, "device": 5}));
    run(&mut d, "intervention.update", json!({"id": id, "device": null}));
    // With the consent recorded nothing is asked, and no reason is needed.
    let w = run(
        &mut d,
        "intervention.add",
        json!({"customer": 1, "kind": "phone", "summary": "Telefonata", "started_at": "2026-10-12T11:00:00Z", "minutes": 5}),
    );
    let u = run(&mut d, "intervention.update", json!({"id": w["intervention"]["id"], "kind": "remote"}));
    assert_eq!(u["intervention"]["consent_override"], "");
}

#[test]
fn every_desk_starts_at_its_own_revision() {
    let dir = std::env::temp_dir().join(format!("remotecraft-revision-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let paths = Paths::under(&dir);
    let mut a = Desk::open(paths.clone()).unwrap();
    a.execute("customer.add", &json!({"name": "Uno"})).unwrap();
    // The same file opened again (a server restarted) does not reuse the revisions: a client
    // still holding the old one gets the desk, not `unchanged`.
    let mut b = Desk::open(paths).unwrap();
    assert_ne!(a.revision(), b.revision());
    assert!(b.snapshot(a.revision(), "x").unwrap().is_some());
    let c = Desk::in_memory(Data::new());
    for r in [a.revision(), b.revision(), c.revision()] {
        assert!(r > 0 && r < 1 << 53, "{r}");
    }
    assert_ne!(Desk::in_memory(Data::new()).revision(), c.revision());
    let _ = std::fs::remove_dir_all(&dir);
}
