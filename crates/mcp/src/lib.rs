//! RemoteCraft's MCP server (layer L6): the Model Context Protocol over stdio (newline-delimited
//! JSON-RPC 2.0), so an AI assistant can look up customers and PCs, time and log
//! interventions, and keep the GDPR record — in plain language.
//!
//! Every tool is an engine command run through a [`Runner`]: the local [`Desk`], or a
//! RemoteCraft server over HTTP (`remotecraft-cli mcp --server URL --token T`). What the API
//! does, an agent can do, and nothing else.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};

use remotecraft_engine::{COMMANDS, Desk};
use serde_json::{Value, json};

/// MCP protocol revision this server prefers.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Revisions it also speaks; a client asking for another gets [`PROTOCOL_VERSION`].
pub const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// Running interventions (`resources/read`).
pub const RUNNING_URI: &str = "remotecraft://running";
/// The privacy review (`resources/read`).
pub const REVIEW_URI: &str = "remotecraft://review";
/// Longest request line read (bytes).
const MAX_LINE: usize = 8 * 1024 * 1024;

const INSTRUCTIONS: &str = "RemoteCraft is a support desk: customers, their PCs with the RustDesk ID, interventions with times and notes, and the GDPR record \
(privacy notice delivered, consents given or withdrawn). Find people and PCs with find_customers / find_device (a RustDesk ID like 123 456 789 works everywhere a device is asked). \
Time work with start_intervention (it returns the rustdesk:// link) and stop_intervention, or record past work with log_intervention. \
Remote interventions need the customer's remote_assistance consent: when start_intervention refuses, tell the person and either record_consent (with the evidence) \
or, only if the person says the customer agreed another way, retry with consent_override and the reason. \
Never invent consents, signers or evidence: ask. Before gdpr_erase, delete_intervention or archive_customer, show what will happen and wait for a yes. \
gdpr_export returns personal data: give it only to the person it is about or to who asked on the desk's behalf. Answer in the person's language.";

/// Runs engine commands (locally or on a server).
pub trait Runner {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String>;
}

impl Runner for Desk {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        self.execute(command, &params).map_err(|e| e.to_string())
    }
}

/// The server: one runner, as the technician it was started for.
pub struct Server {
    runner: Box<dyn Runner>,
}

/// A tool: its name, the command it runs, its description, its input properties and the
/// required ones.
struct Tool {
    name: &'static str,
    command: &'static str,
    description: &'static str,
    properties: Value,
    required: &'static [&'static str],
}

fn tools() -> Vec<Tool> {
    let customer = json!({"type": ["string", "integer"], "description": "Customer id or name (also e-mail, tax id or a contact's name)"});
    let device = json!({"type": ["string", "integer"], "description": "Device id, RustDesk ID (123 456 789) or name / host name"});
    let id = json!({"type": "integer", "minimum": 1, "description": "Intervention id"});
    let when = |what: &str| json!({"type": "string", "description": format!("{what}: 2026-10-13 09:30, 13/10/2026 09:30, 09:30 (today) or RFC 3339; local time")});
    let kind = json!({"type": "string", "enum": ["remote", "onsite", "phone", "lab", "other"], "description": "remote = RustDesk (default with a PC that has a RustDesk ID)"});
    let text = json!({"type": "string"});
    let flag = json!({"type": "boolean"});
    let limit = json!({"type": "integer", "minimum": 1, "maximum": 1000});
    let purposes = json!({"type": "array", "items": {"type": "string"}, "description": "remote_assistance, unattended_access, marketing, newsletter or a custom purpose"});
    let method = json!({"type": "string", "enum": ["paper", "email", "online", "verbal", "other"], "description": "How it happened (default paper)"});
    let contacts = json!({"type": "array", "items": {"type": "object", "properties": {"name": text, "role": text, "email": text, "phone": text}}});
    let tags = json!({"type": "array", "items": {"type": "string"}});
    let customer_fields = json!({
        "name": text, "kind": {"type": "string", "enum": ["company", "person"]}, "tax_id": {"type": "string", "description": "VAT number or tax code"},
        "email": text, "phone": text, "address": text, "contacts": contacts, "tags": tags, "notes": text,
        "allow_duplicate": {"type": "boolean", "description": "Keep a second customer with the same name or tax id"},
    });
    let device_fields = json!({
        "name": {"type": "string", "description": "PC reception, Notebook Anna…"}, "rustdesk_id": {"type": "string", "description": "As shown in the RustDesk window"},
        "hostname": text, "os": text, "serial": text, "location": text, "assigned_to": {"type": "string", "description": "Who uses it"}, "notes": text,
        "unattended": {"type": "boolean", "description": "A permanent RustDesk password is set on the client (the password itself is never stored)"},
    });
    let with = |base: &Value, extra: Value| {
        let mut v = base.clone();
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        v
    };
    let event = json!({"customer": customer, "method": method, "at": when("When it happened (default now)"), "signer": {"type": "string", "description": "Who signed or agreed"}, "evidence": {"type": "string", "description": "Where the proof is: archive number, mail, form id"}, "note": text});
    vec![
        Tool {
            name: "find_customers",
            command: "customer.list",
            description: "Search customers by name, tax id, e-mail, phone, contact, tag, RustDesk ID or host name of one of their PCs. Empty query = everyone.",
            properties: json!({"query": text, "tag": text, "include_archived": flag, "limit": limit}),
            required: &[],
        },
        Tool {
            name: "get_customer",
            command: "customer.get",
            description: "A customer's card: details, contacts, PCs with RustDesk IDs, the last 20 interventions, totals and the privacy status.",
            properties: json!({"customer": customer}),
            required: &["customer"],
        },
        Tool {
            name: "add_customer",
            command: "customer.add",
            description: "Create a customer. Then add their PCs (add_device) and the GDPR record (record_notice_delivery, record_consent).",
            properties: customer_fields.clone(),
            required: &["name"],
        },
        Tool {
            name: "update_customer",
            command: "customer.update",
            description: "Change a customer's details: only the fields given change (contacts and tags are replaced as a whole).",
            properties: with(&customer_fields, json!({"customer": customer})),
            required: &["customer"],
        },
        Tool {
            name: "archive_customer",
            command: "customer.archive",
            description: "Hide a customer who is no longer a customer (data kept); archived=false brings them back. Ask first.",
            properties: json!({"customer": customer, "archived": flag}),
            required: &["customer"],
        },
        Tool {
            name: "add_device",
            command: "device.add",
            description: "Add a PC (or server, phone…) to a customer, with its RustDesk ID. Never store RustDesk passwords.",
            properties: with(&device_fields, json!({"customer": customer})),
            required: &["customer"],
        },
        Tool {
            name: "update_device",
            command: "device.update",
            description: "Change a device: RustDesk ID, name, OS, who uses it, unattended access; `customer` moves it to another customer.",
            properties: with(&device_fields, json!({"device": device, "customer": customer})),
            required: &["device"],
        },
        Tool {
            name: "find_device",
            command: "device.find",
            description: "Who owns this RustDesk ID (or host name / serial)? Returns the device and its customer.",
            properties: json!({"rustdesk_id": text, "query": text}),
            required: &[],
        },
        Tool {
            name: "list_devices",
            command: "device.list",
            description: "Devices, of one customer or all, filtered by a query.",
            properties: json!({"customer": customer, "query": text, "include_retired": flag, "limit": limit}),
            required: &[],
        },
        Tool {
            name: "connect_device",
            command: "device.connect",
            description: "The rustdesk:// link and command line to connect to a PC, with the consent state and warnings. Opening the link is up to the person.",
            properties: json!({"device": device}),
            required: &["device"],
        },
        Tool {
            name: "start_intervention",
            command: "intervention.start",
            description: "Start the timer on an intervention for a customer or a device. For remote work it checks the remote_assistance consent and returns the RustDesk link.",
            properties: json!({"customer": customer, "device": device, "kind": kind, "summary": {"type": "string", "description": "The request, in one line"}, "notes": text, "reference": {"type": "string", "description": "Ticket number elsewhere"}, "started_at": when("Start (default now)"), "billable": flag, "consent_override": {"type": "string", "description": "Only when the person says the customer agreed another way: the reason, kept and audited"}}),
            required: &[],
        },
        Tool {
            name: "stop_intervention",
            command: "intervention.stop",
            description: "Stop the timer (by default the technician's running intervention) and add what was done to the notes.",
            properties: json!({"id": id, "ended_at": when("End (default now)"), "notes": {"type": "string", "description": "What was done, found, left to do (appended)"}, "summary": text, "minutes": {"type": "integer", "minimum": 0, "description": "Minutes worked when not the whole span (pauses)"}, "billable": flag}),
            required: &[],
        },
        Tool {
            name: "log_intervention",
            command: "intervention.add",
            description: "Record work already done: start and end (or minutes), what was done.",
            properties: json!({"customer": customer, "device": device, "kind": kind, "summary": text, "notes": text, "started_at": when("Start"), "ended_at": when("End"), "minutes": {"type": "integer", "minimum": 0}, "technician": text, "reference": text, "billable": flag, "consent_override": text}),
            required: &["summary", "started_at"],
        },
        Tool {
            name: "update_intervention",
            command: "intervention.update",
            description: "Correct an intervention: times, minutes, notes (replaced), kind, device, billable. Making it remote work, or moving remote work to another PC, needs the customer's remote_assistance consent like log_intervention: when it refuses, ask the person; only with a reason from them retry with consent_override.",
            properties: json!({"id": id, "kind": kind, "summary": text, "notes": text, "reference": text, "started_at": when("Start"), "ended_at": when("End"), "minutes": {"type": "integer", "minimum": 0}, "billable": flag, "technician": text, "device": device, "consent_override": {"type": "string", "description": "Only when it becomes remote work (or moves to another PC) and the person says the customer agreed another way: the reason, kept and audited"}}),
            required: &["id"],
        },
        Tool {
            name: "delete_intervention",
            command: "intervention.remove",
            description: "Delete an intervention for good. Show it and ask first.",
            properties: json!({"id": id}),
            required: &["id"],
        },
        Tool {
            name: "list_interventions",
            command: "intervention.list",
            description: "Interventions newest first, filtered by customer, device, technician, kind, period, text; with total and billable minutes.",
            properties: json!({"customer": customer, "device": device, "technician": text, "kind": kind, "from": when("From (included)"), "to": when("To (excluded)"), "running": flag, "billable": flag, "query": text, "limit": limit}),
            required: &[],
        },
        Tool {
            name: "running_interventions",
            command: "intervention.running",
            description: "Timers running now, with elapsed minutes.",
            properties: json!({"technician": text}),
            required: &[],
        },
        Tool {
            name: "time_report",
            command: "report.time",
            description: "Hours worked and billable hours (rounded per intervention to the desk's unit) per customer, technician, month, device or kind, over a period.",
            properties: json!({"from": when("From (included)"), "to": when("To (excluded)"), "customer": customer, "technician": text, "group_by": {"type": "string", "enum": ["customer", "technician", "month", "device", "kind"]}}),
            required: &[],
        },
        Tool {
            name: "privacy_status",
            command: "privacy.status",
            description: "A customer's GDPR state: the privacy notice delivered vs the current one, and each purpose (given, withdrawn, expired, missing) with method, signer and evidence.",
            properties: json!({"customer": customer}),
            required: &["customer"],
        },
        Tool {
            name: "privacy_history",
            command: "privacy.history",
            description: "Every notice delivery, consent and withdrawal of a customer, newest first.",
            properties: json!({"customer": customer}),
            required: &["customer"],
        },
        Tool {
            name: "record_notice_delivery",
            command: "privacy.notice",
            description: "Record that the customer received the privacy notice (informativa); version defaults to the current one.",
            properties: with(&event, json!({"version": text})),
            required: &["customer"],
        },
        Tool {
            name: "record_consent",
            command: "privacy.consent",
            description: "Record consent for one or more purposes, with how, who signed and where the proof is. with_notice=true also records the notice delivery (one signed form). Never invent signer or evidence.",
            properties: with(
                &event,
                json!({"purposes": purposes, "expires_at": when("Expiry, if the consent is time-limited"), "notice_version": text, "with_notice": flag}),
            ),
            required: &["customer", "purposes"],
        },
        Tool {
            name: "withdraw_consent",
            command: "privacy.withdraw",
            description: "Record that the customer withdrew consent for purposes. Warns about PCs with unattended access to clean up.",
            properties: with(&event, json!({"purposes": purposes})),
            required: &["customer", "purposes"],
        },
        Tool {
            name: "list_notices",
            command: "notice.list",
            description: "The versions of the privacy notice, the current one marked, with how many customers received each.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "get_notice",
            command: "notice.get",
            description: "The text of a privacy notice version (default: the current one).",
            properties: json!({"version": text}),
            required: &[],
        },
        Tool {
            name: "add_notice",
            command: "notice.add",
            description: "Add a new version of the privacy notice (versions never change once added).",
            properties: json!({"version": text, "title": text, "text": text, "effective_from": when("In effect from (default now)")}),
            required: &["version"],
        },
        Tool {
            name: "gdpr_export",
            command: "gdpr.export",
            description: "Everything the desk holds about a customer (GDPR art. 15/20): details, devices, interventions, privacy record, access log. Personal data: hand it only to whom it is due.",
            properties: json!({"customer": customer}),
            required: &["customer"],
        },
        Tool {
            name: "gdpr_erase",
            command: "gdpr.erase",
            description: "Erase a customer's personal data (art. 17): anonymises the customer, devices and intervention texts; hours and the consent record stay without names. Cannot be undone: offer gdpr_export first, show what will be erased and ask for an explicit yes.",
            properties: json!({"customer": customer, "confirm": {"type": "boolean", "description": "Must be true"}, "reason": {"type": "string", "description": "Why (request received on…, retention expired…)"}}),
            required: &["customer", "confirm", "reason"],
        },
        Tool {
            name: "gdpr_review",
            command: "gdpr.review",
            description: "What needs attention: customers without the current notice, remote access without consent, expired consents, data past retention, timers left running.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "audit_log",
            command: "audit.list",
            description: "Who changed or exported what, newest first (ids only, no personal data).",
            properties: json!({"customer": customer, "device": device, "intervention": id, "actor": text, "command": text, "limit": limit}),
            required: &[],
        },
        Tool {
            name: "get_preferences",
            command: "prefs.get",
            description: "Desk preferences: language, company name, billing unit and minimum, consent rule and validity, retention years, RustDesk server.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "set_preferences",
            command: "prefs.set",
            description: "Change preferences (only the fields given).",
            properties: json!({"language": {"type": "string", "enum": ["it", "en"]}, "theme": {"type": "string", "enum": ["light", "dark", "system"]}, "company_name": text, "billing_unit_minutes": {"type": "integer", "minimum": 0, "maximum": 480}, "minimum_minutes": {"type": "integer", "minimum": 0, "maximum": 480}, "require_consent": flag, "consent_validity_months": {"type": "integer", "minimum": 0, "maximum": 240}, "retention_years": {"type": "integer", "minimum": 0, "maximum": 100}, "rustdesk_host": text, "rustdesk_key": text}),
            required: &[],
        },
        Tool {
            name: "rustdesk_setup",
            command: "rustdesk.setup",
            description: "How to point a RustDesk client at the desk's own server: ID server, relay, key, the config string for Settings › Network › Import server config, and the name that makes the Windows installer configure itself.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "check_rustdesk_server",
            command: "rustdesk.check",
            description: "Check that the desk's RustDesk server answers (hbbs on 21115/21116, hbbr relay on 21117) and that its key is known.",
            properties: json!({"host": {"type": "string", "description": "Default: the server next to the desk"}}),
            required: &[],
        },
        Tool {
            name: "status",
            command: "app.status",
            description: "Counts, running timers, where the data is.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "list_commands",
            command: "app.commands",
            description: "Every engine command with its parameters.",
            properties: json!({}),
            required: &[],
        },
        Tool {
            name: "run_command",
            command: "",
            description: "Run any engine command by id (see list_commands) with its params.",
            properties: json!({"command": text, "params": {"type": "object"}}),
            required: &["command"],
        },
    ]
}

fn tools_json() -> Value {
    Value::Array(
        tools()
            .into_iter()
            .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": {"type": "object", "properties": t.properties, "required": t.required}}))
            .collect(),
    )
}

fn prompts() -> Value {
    json!([
        {"name": "new_customer", "title": "New customer", "description": "Create a customer from a description: details, PCs with RustDesk IDs, privacy notice and consents.",
         "arguments": [{"name": "description", "description": "E.g. 'Studio Rossi, P.IVA …, due PC: reception ID 123 456 789…; firmato informativa e consenso assistenza remota, modulo 2026/12'", "required": true}]},
        {"name": "log_work", "title": "Log work", "description": "Record an intervention from a plain-language note (who, which PC, when, how long, what was done).",
         "arguments": [{"name": "note", "description": "E.g. 'ieri 15-16:30 da Corvo, PC cassa in remoto, aggiornato Windows'", "required": true}]},
        {"name": "monthly_report", "title": "Monthly report", "description": "Hours per customer for a month, with the interventions behind them, ready to invoice.",
         "arguments": [{"name": "month", "description": "YYYY-MM (default: last month)", "required": false}, {"name": "customer", "description": "One customer only", "required": false}]},
        {"name": "privacy_check", "title": "Privacy check", "description": "Review the GDPR record and propose what to do: notices to deliver, consents to collect, data to erase.",
         "arguments": []},
    ])
}

fn prompt_text(name: &str, args: &Value) -> Option<(&'static str, String)> {
    let a = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().trim().chars().take(4_000).collect::<String>();
    match name {
        "new_customer" => {
            let d = a("description");
            if d.is_empty() {
                return None;
            }
            Some((
                "Create a customer with PCs and GDPR record",
                format!(
                    "Create this customer in RemoteCraft (answer in the language of the description):\n\n{d}\n\n\
1. find_customers with the name and the tax id: if they already exist, show it and ask whether to update instead.\n\
2. add_customer with what the description gives (company or person, tax id, e-mail, phone, address, contacts, tags). Do not invent missing details.\n\
3. add_device for each PC, with its RustDesk ID exactly as given (unattended=true only if a permanent password is said to be set). Never store passwords.\n\
4. If the description says the privacy notice was delivered and/or consent was given: list_notices to know the current version, then record_consent \
(with_notice=true when it is one signed form) with method, signer and evidence as stated. If signer or evidence are not stated, ask before recording.\n\
5. End with privacy_status and a short summary: customer id, PCs and RustDesk IDs, what is still missing for the GDPR record."
                ),
            ))
        }
        "log_work" => {
            let n = a("note");
            if n.is_empty() {
                return None;
            }
            Some((
                "Record an intervention from a note",
                format!(
                    "Record this work in RemoteCraft:\n\n{n}\n\n\
1. Find the customer (find_customers) and, if a PC or RustDesk ID is mentioned, the device (find_device or list_devices with the customer). Ask when ambiguous.\n\
2. Work out start and end (or minutes) in local time, the kind (remote, onsite, phone, lab) and a one-line summary; put the details in notes.\n\
3. Call log_intervention. If it refuses for missing remote_assistance consent, explain and ask how the customer agreed; only with a reason from the person retry with consent_override.\n\
4. Show what was recorded: id, customer, PC, times, minutes, warnings."
                ),
            ))
        }
        "monthly_report" => {
            let month = a("month");
            let customer = a("customer");
            Some((
                "Hours per customer for a month",
                format!(
                    "Prepare the RemoteCraft hours report for {}{}.\n\
1. time_report with from = the first day of the month and to = the first day of the next (group_by customer{}).\n\
2. For each customer with hours, list_interventions for the same period: date, PC, kind, summary, minutes, billable.\n\
3. Present a table per customer with the total hours and the billable hours (rounded as the desk's billing unit says), and flag interventions without a summary or still running.",
                    if month.is_empty() { "last month".to_string() } else { month },
                    if customer.is_empty() { String::new() } else { format!(", customer {customer}") },
                    if customer.is_empty() { "" } else { ", customer filter" },
                ),
            ))
        }
        "privacy_check" => Some((
            "Review the GDPR record",
            "Review the GDPR record of RemoteCraft.\n\
1. gdpr_review. Group the findings: privacy notice to deliver or update, remote_assistance or unattended_access consent missing/expired/withdrawn, data past retention, timers left running.\n\
2. For each group propose the concrete action (the form to have signed, the PC whose permanent RustDesk password should be removed, the customer to export and erase) without doing it.\n\
3. Do not record consents or erase data until the person confirms with the details (signer, evidence, reason)."
                .to_string(),
        )),
        _ => None,
    }
}

impl Server {
    pub fn new(runner: impl Runner + 'static) -> Self {
        Self { runner: Box::new(runner) }
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        if name == "run_command" {
            let command =
                args.get("command").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty()).ok_or("`command` is required")?.to_string();
            let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
            return self.runner.run(&command, params);
        }
        let tool =
            tools().into_iter().find(|t| t.name == name).ok_or_else(|| format!("unknown tool {:?}", name.chars().take(40).collect::<String>()))?;
        let args = if args.is_object() { args.clone() } else { json!({}) };
        // Pass only the declared arguments: an agent cannot slip in parameters the tool does not show.
        let mut params = json!({});
        if let (Some(props), Some(out)) = (tool.properties.as_object(), params.as_object_mut()) {
            for k in props.keys() {
                if let Some(v) = args.get(k) {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        self.runner.run(tool.command, params)
    }

    fn running_text(&mut self) -> String {
        match self.runner.run("intervention.running", json!({})) {
            Ok(v) => {
                let rows: Vec<String> = v["running"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .map(|i| {
                                format!(
                                    "#{} {} — {} — {} — {} min — {}",
                                    i["id"],
                                    i["technician"].as_str().unwrap_or(""),
                                    i["customer_name"].as_str().unwrap_or(""),
                                    i["device_name"].as_str().unwrap_or(""),
                                    i["elapsed_minutes"],
                                    i["summary"].as_str().unwrap_or("")
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if rows.is_empty() { "No running interventions.".into() } else { rows.join("\n") }
            }
            Err(e) => format!("error: {e}"),
        }
    }

    fn review_text(&mut self) -> String {
        match self.runner.run("gdpr.review", json!({})) {
            Ok(v) => {
                let rows: Vec<String> = v["findings"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .map(|f| {
                                format!(
                                    "{} {} — {}",
                                    f["issue"].as_str().unwrap_or(""),
                                    f["name"].as_str().unwrap_or(""),
                                    f["text"].as_str().unwrap_or("")
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if rows.is_empty() { "Nothing to review.".into() } else { rows.join("\n") }
            }
            Err(e) => format!("error: {e}"),
        }
    }

    /// Answers one JSON-RPC message (`None` for notifications).
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned()?;
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).filter(|v| SUPPORTED_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false, "subscribe": false}, "prompts": {"listChanged": false}},
                "serverInfo": {"name": "remotecraft", "title": "RemoteCraft", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools_json()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                Ok(match self.call(name, &args) {
                    Ok(v) => {
                        let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                        let structured = if v.is_object() { v } else { json!({"result": v}) };
                        json!({"content": [{"type": "text", "text": text}], "structuredContent": structured, "isError": false})
                    }
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            "prompts/list" => Ok(json!({"prompts": prompts()})),
            "prompts/get" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match prompt_text(name, &args) {
                    Some((description, text)) => {
                        Ok(json!({"description": description, "messages": [{"role": "user", "content": {"type": "text", "text": text}}]}))
                    }
                    None => Err((-32602, format!("unknown prompt or missing arguments: {:?}", name.chars().take(40).collect::<String>()))),
                }
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": RUNNING_URI, "name": "running", "title": "Running interventions", "description": "Timers running now", "mimeType": "text/plain"},
                {"uri": REVIEW_URI, "name": "review", "title": "Privacy review", "description": "What the GDPR record needs", "mimeType": "text/plain"},
            ]})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => match params.get("uri").and_then(Value::as_str) {
                Some(RUNNING_URI) => Ok(json!({"contents": [{"uri": RUNNING_URI, "mimeType": "text/plain", "text": self.running_text()}]})),
                Some(REVIEW_URI) => Ok(json!({"contents": [{"uri": REVIEW_URI, "mimeType": "text/plain", "text": self.review_text()}]})),
                _ => Err((-32002, "resource not found".to_string())),
            },
            _ => Err((-32601, format!("method not found: {}", method.chars().take(64).collect::<String>()))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        })
    }

    /// Serves newline-delimited JSON-RPC until the input ends.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = if line.len() > MAX_LINE {
                Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "request too large"}}))
            } else {
                match serde_json::from_str::<Value>(&line) {
                    Ok(Value::Array(batch)) => {
                        let replies: Vec<Value> = batch.iter().take(100).filter_map(|m| self.handle(m)).collect();
                        (!replies.is_empty()).then_some(Value::Array(replies))
                    }
                    Ok(msg) => self.handle(&msg),
                    Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})),
                }
            };
            if let Some(r) = reply {
                writeln!(output, "{r}")?;
                output.flush()?;
            }
        }
        Ok(())
    }
}

/// Every tool's command exists in the engine (checked by the tests).
pub fn tool_commands() -> Vec<&'static str> {
    tools().into_iter().map(|t| t.command).filter(|c| !c.is_empty()).collect()
}

/// The engine catalogue, for `list_commands` callers that want it without a runner.
pub fn commands() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.id).collect()
}

#[cfg(test)]
mod tests;
