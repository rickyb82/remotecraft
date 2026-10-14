//! The Privacy screen — tabs **Review · Notices · Audit log** — and the GDPR forms.
//!
//! - **Review**: what the record still needs, from `app.derived.findings` (the engine's
//!   `views::review` run on the mirror, no command), grouped by customer, each finding with the
//!   action that settles it.
//! - **Notices**: the versions of the privacy notice, newest first; the right pane shows the
//!   text of the one clicked as a selectable label (never an editor: versions never change).
//! - **Audit log**: `audit.list` through a [`Cached`] slot, filtered by customer, technician and
//!   command, its targets resolved to names.
//!
//! The four forms — notice delivery, consent, withdrawal, new notice version — live in
//! `app.privacy.form` and are drawn by [`form_ui`] in the right pane of whatever screen opened
//! them (the customer card, the device card, this screen). [`strip`] is the privacy section at
//! the top of a customer card. Every time is sent as the stamp the typed text stands for
//! (`format::stamp_from_input`, the same reading as `forms::time_field`).

use std::collections::{HashMap, HashSet};

use egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use remotecraft_engine::core::chrono::{DateTime, Duration, Utc};
use remotecraft_engine::core::privacy::{self, Method, Notice, PrivacyEvent, PrivacyKind, PurposeState};
use remotecraft_engine::core::{CustomerKind, Data, fold, limits, time};
use remotecraft_engine::views;
use serde_json::{Value, json};
use web_time::Instant;

use crate::derived::{self, Cached, Finding, NoticeState, PurposeRow};
use crate::forms::{self, FormAction, Picker};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, DeskApp, Screen, chrome, format};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    // Tabs and header.
    ("Review", "Revisione"),
    ("Notices", "Informative"),
    ("Audit log", "Registro di audit"),
    ("Last check {time}", "Ultimo controllo {time}"),
    // Review.
    ("Nothing to do: the GDPR record is complete", "Niente da fare: il registro GDPR è completo"),
    ("No privacy notice in the desk", "Nessuna informativa nel gestionale"),
    ("Notice not delivered", "Informativa non consegnata"),
    ("Notice outdated", "Informativa non aggiornata"),
    ("Unattended access consent missing", "Consenso all'accesso non presidiato mancante"),
    ("Unattended access consent expired", "Consenso all'accesso non presidiato scaduto"),
    ("Unattended access consent withdrawn", "Consenso all'accesso non presidiato revocato"),
    ("Past retention", "Oltre la conservazione"),
    ("Timer left running", "Timer rimasto acceso"),
    ("Current: {version}", "Attuale: {version}"),
    ("Has notice {old}, current is {new}", "Ha l'informativa {old}, l'attuale è la {new}"),
    ("Since {date}", "Dal {date}"),
    ("{technician}, since {time}", "{technician}, da {time}"),
    ("Add the notice", "Aggiungi l'informativa"),
    ("Record delivery", "Registra la consegna"),
    ("Record consent", "Registra consenso"),
    ("Erase personal data…", "Cancella i dati personali…"),
    ("Open intervention", "Apri l'intervento"),
    // The privacy strip of a customer card.
    ("Notice {version} delivered on {date} ({method})", "Informativa {version} consegnata il {date} ({method})"),
    ("Received notice {old}, current is {new}", "Ha ricevuto l'informativa {old}, l'attuale è la {new}"),
    ("History ({n})", "Storico ({n})"),
    ("Recorded by {who} on {date}", "Registrato da {who} il {date}"),
    ("Notice delivered", "Informativa consegnata"),
    ("Consent given", "Consenso dato"),
    ("Consent withdrawn", "Consenso revocato"),
    ("Withdraw", "Revoca"),
    ("Record…", "Registra…"),
    ("Expires {date}", "Scade il {date}"),
    ("Evidence", "Prova"),
    // Notices.
    ("New notice version", "Nuova versione dell'informativa"),
    ("Version", "Versione"),
    ("2026-01, v3…", "2026-01, v3…"),
    ("Title", "Titolo"),
    ("Privacy notice — technical support", "Informativa privacy — assistenza tecnica"),
    ("Text", "Testo"),
    ("Insert outline", "Inserisci la traccia"),
    (
        "Replace every line with your own text: controller, data processed, purposes and legal basis, retention, recipients, rights (articles 15–22).",
        "Sostituisci ogni riga con il tuo testo: titolare, dati trattati, finalità e base giuridica, conservazione, destinatari, diritti (articoli 15–22).",
    ),
    ("[replace with your text]", "[sostituisci con il tuo testo]"),
    ("1. Data controller", "1. Titolare del trattamento"),
    ("2. Data processed", "2. Dati trattati"),
    ("3. Purposes and legal basis", "3. Finalità e base giuridica"),
    ("4. Retention", "4. Conservazione"),
    ("5. Recipients", "5. Destinatari"),
    ("6. Your rights (articles 15–22)", "6. Diritti dell'interessato (articoli 15–22)"),
    ("Effective from", "In vigore dal"),
    ("A future date schedules the version", "Una data futura programma la versione"),
    ("Effective from {date}", "In vigore dal {date}"),
    ("Created by {who} on {date}", "Creata da {who} il {date}"),
    ("Current", "Attuale"),
    ("Scheduled", "Programmata"),
    ("{n} customers received it", "Ricevuta da {n} clienti"),
    ("Versions never change: a new text is a new version.", "Le versioni non cambiano mai: un testo nuovo è una nuova versione."),
    ("This version exists", "Questa versione esiste già"),
    ("Notice version added", "Versione dell'informativa aggiunta"),
    ("No privacy notice yet", "Ancora nessuna informativa"),
    (
        "Add the text you give customers (art. 13): deliveries and consents refer to its version.",
        "Aggiungi il testo che dai ai clienti (art. 13): consegne e consensi fanno riferimento alla sua versione.",
    ),
    ("Copy text", "Copia il testo"),
    ("Record delivery to a customer…", "Registra la consegna a un cliente…"),
    // Forms.
    ("Record notice delivery", "Registra la consegna dell'informativa"),
    ("Add the privacy notice first", "Prima aggiungi l'informativa"),
    ("Notice version", "Versione dell'informativa"),
    ("{version} — {title}", "{version} — {title}"),
    ("Method", "Modalità"),
    ("Verbal is weak evidence: note who heard it", "Il verbale è una prova debole: annota chi l'ha sentito"),
    ("When", "Quando"),
    ("Signer", "Firmatario"),
    (
        "Where the signed form or the mail is: archive number, subject, form id",
        "Dov'è il modulo firmato o la mail: numero d'archivio, oggetto, id del modulo",
    ),
    ("Note", "Nota"),
    ("Expires on", "Scade il"),
    ("The expiry is before the date", "La scadenza è prima della data"),
    ("Other purpose", "Altra finalità"),
    ("Letters, digits and _ only", "Solo lettere, cifre e _"),
    ("Pick at least one purpose", "Scegli almeno una finalità"),
    ("Also record the notice delivery (one signed form)", "Registra anche la consegna dell'informativa (un solo modulo firmato)"),
    ("Records the delivery of notice {version} together with the consent", "Registra la consegna dell'informativa {version} insieme al consenso"),
    ("Notice delivery recorded", "Consegna dell'informativa registrata"),
    ("Consent recorded ({n} events)", "Consenso registrato ({n} eventi)"),
    ("No notice delivery recorded: consent must be informed", "Nessuna consegna dell'informativa registrata: il consenso deve essere informato"),
    ("This adds a withdrawal event; the history is kept.", "Si aggiunge un evento di revoca; lo storico resta."),
    ("Record withdrawal", "Registra la revoca"),
    ("Withdrawal recorded", "Revoca registrata"),
    // Audit log.
    ("Command", "Comando"),
    ("Any command", "Qualsiasi comando"),
    ("Command: {name}", "Comando: {name}"),
    ("Customer: {name}", "Cliente: {name}"),
    ("Any customer", "Qualsiasi cliente"),
    ("Technician", "Tecnico"),
    ("Technician: {name}", "Tecnico: {name}"),
    ("Every technician", "Tutti i tecnici"),
    ("Targets", "Elementi"),
    ("Detail", "Dettaglio"),
    ("{n} entries", "{n} voci"),
    ("No audit entries", "Nessuna voce di audit"),
    ("Show more", "Mostra altre"),
    ("Only identifiers are logged, never personal data.", "Nel registro finiscono solo identificativi, mai dati personali."),
];

/// Row height of the Review and Audit lists.
const ROW_H: f32 = 40.0;
/// Row height of the Notices list (two lines).
const NOTICE_ROW_H: f32 = 56.0;
/// Below this window width the right pane replaces the list.
const NARROW: f32 = 980.0;
/// Width of the label column of the forms.
const LABEL_W: f32 = 110.0;
/// How many audit entries the log asks for first, and how many more "Show more" adds.
const AUDIT_LIMIT: usize = 100;
const AUDIT_STEP: usize = 200;
/// How many events the history of a customer lists.
const HISTORY_MAX: usize = 200;
/// The command families the audit log filters by (`audit.list {command}` matches a prefix).
const COMMAND_PREFIXES: &[&str] = &["customer", "device", "intervention", "privacy", "notice", "gdpr", "prefs", "rustdesk", "app"];
/// The headings **Insert outline** pastes into a new notice, each followed by [`PLACEHOLDER`].
const OUTLINE: &[&str] =
    &["1. Data controller", "2. Data processed", "3. Purposes and legal basis", "4. Retention", "5. Recipients", "6. Your rights (articles 15–22)"];
const PLACEHOLDER: &str = "[replace with your text]";
/// The message of a time that cannot be read (a shared label).
const NOT_A_TIME: &str = "Not a time: use 2026-10-13 09:30, 13/10/2026 09:30 or 09:30";

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `privacy.notice` for this customer.
    Delivered(u64),
    /// `privacy.consent` for this customer.
    Consented(u64),
    /// `privacy.withdraw` for this customer.
    Withdrawn(u64),
    /// `notice.add`.
    NoticeAdded,
    /// `audit.list` for the Audit log tab.
    Audit,
}

/// The tabs of the Privacy screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Review,
    Notices,
    Audit,
}

/// The notice-delivery form: the customer received (and acknowledged) a version of the notice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliveryForm {
    pub customer: u64,
    /// The version delivered (default: the one in effect).
    pub version: String,
    pub method: Method,
    /// When, as typed (`13/10/2026 09:30`); sent as a stamp.
    pub at: String,
    pub signer: String,
    pub evidence: String,
    pub note: String,
}

/// The consent form: one signed form, one or more purposes (and the notice delivery with it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConsentForm {
    pub customer: u64,
    /// Every known purpose with whether it is ticked.
    pub purposes: Vec<(String, bool)>,
    /// A purpose not in the list, as typed (slugged when sent).
    pub other: String,
    /// Also record the delivery of the notice (`with_notice`).
    pub with_notice: bool,
    pub method: Method,
    pub at: String,
    pub signer: String,
    pub evidence: String,
    pub note: String,
    /// An optional expiry, as typed.
    pub expires: String,
    /// The notice version the consent refers to (default: the one in effect).
    pub version: String,
}

/// The withdrawal form: the purposes currently given (or expired) to withdraw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WithdrawForm {
    pub customer: u64,
    pub purposes: Vec<(String, bool)>,
    pub method: Method,
    pub at: String,
    pub signer: String,
    pub note: String,
}

/// The new-notice-version form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoticeForm {
    pub version: String,
    pub title: String,
    pub text: String,
    /// From when it applies, as typed (a future date schedules it).
    pub effective: String,
}

/// The privacy form open in a right pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    Delivery(DeliveryForm),
    Consent(ConsentForm),
    Withdraw(WithdrawForm),
    Notice(NoticeForm),
}

impl Form {
    /// The customer the form records something for (none for a new notice version).
    pub fn customer(&self) -> Option<u64> {
        match self {
            Self::Delivery(f) => Some(f.customer),
            Self::Consent(f) => Some(f.customer),
            Self::Withdraw(f) => Some(f.customer),
            Self::Notice(_) => None,
        }
    }
}

/// The filters of the Audit log (`audit.list` params).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditFilter {
    pub customer: Option<u64>,
    pub device: Option<u64>,
    pub intervention: Option<u64>,
    /// A technician, empty = everyone.
    pub actor: String,
    /// A command family (`privacy`, `customer`…), empty = every command.
    pub command: String,
    /// How many entries to ask for (newest first).
    pub limit: usize,
}

impl Default for AuditFilter {
    fn default() -> Self {
        Self { customer: None, device: None, intervention: None, actor: String::new(), command: String::new(), limit: AUDIT_LIMIT }
    }
}

impl AuditFilter {
    /// The `audit.list` params of these filters.
    pub fn params(&self) -> Value {
        let mut p = serde_json::Map::new();
        p.insert("limit".into(), json!(self.limit.clamp(1, limits::MAX_LIST)));
        if let Some(c) = self.customer {
            p.insert("customer".into(), json!(c));
        }
        if let Some(d) = self.device {
            p.insert("device".into(), json!(d));
        }
        if let Some(i) = self.intervention {
            p.insert("intervention".into(), json!(i));
        }
        if !self.actor.is_empty() {
            p.insert("actor".into(), json!(self.actor));
        }
        if !self.command.is_empty() {
            p.insert("command".into(), json!(self.command));
        }
        Value::Object(p)
    }
}

/// This screen's own state: the open form, the selected notice, the audit slot and filters.
#[derive(Default)]
pub struct State {
    pub tab: Tab,
    pub form: Option<Form>,
    /// The job of the form's command in flight.
    pub busy: Option<u64>,
    /// The engine's refusal of the last submission (translated), shown above the buttons.
    pub error: String,
    /// The notice whose text the right pane shows (Notices tab).
    pub selected_notice: Option<u64>,
    /// The customer picker of "Record delivery to a customer…" in the notice pane.
    pub deliver_to: Option<Picker>,
    /// `audit.list`'s answer.
    pub audit: Cached,
    pub audit_filter: AuditFilter,
    /// The customer whose privacy history is expanded in the strip.
    pub history_open: Option<u64>,
    /// The warnings of the consent just recorded, for that customer's strip.
    pub consent_warnings: Option<(u64, Vec<String>)>,
    /// The notice form was cancelled on an empty Notices tab: do not open it again by itself.
    notice_dismissed: bool,
    /// The Review rows, keyed by the derived build and the search.
    review: Vec<ReviewRow>,
    /// The detail of each finding (aligned with `derived.findings`), built with the rows.
    review_details: Vec<String>,
    review_key: Option<(u64, Instant, String, crate::Language)>,
    /// How many customers received each notice version, keyed by the data's revision.
    delivered: HashMap<String, usize>,
    delivered_key: Option<u64>,
    /// The audit entries of the last answer, targets resolved.
    audit_rows: Vec<AuditRow>,
    /// The text typed in the audit log's customer filter.
    audit_customer: String,
}

/// What a form's buttons asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormAct {
    None,
    Save,
    Cancel,
    AddNotice,
}

/// A command a form runs: its id, params and what the answer is for.
type Request = (&'static str, Value, Intent);

// ---------------------------------------------------------------------------------------------
// Reading the data

/// The label of a review finding, from its issue code (to translate with `app.tr`).
pub fn finding_label(issue: &str) -> &'static str {
    match issue {
        "no_notice" => "No privacy notice in the desk",
        "notice_missing" => "Notice not delivered",
        "notice_outdated" => "Notice outdated",
        "remote_assistance_missing" => "Remote assistance consent missing",
        "remote_assistance_expired" => "Remote assistance consent expired",
        "remote_assistance_withdrawn" => "Remote assistance consent withdrawn",
        "unattended_access_missing" => "Unattended access consent missing",
        "unattended_access_expired" => "Unattended access consent expired",
        "unattended_access_withdrawn" => "Unattended access consent withdrawn",
        "retention_due" => "Past retention",
        "timer_running" => "Timer left running",
        _ => "Privacy",
    }
}

/// The tone of a review finding's chip.
fn finding_tone(issue: &str) -> Tone {
    match issue {
        "notice_missing" | "notice_outdated" | "timer_running" => Tone::Warn,
        "retention_due" => Tone::Neutral,
        _ => Tone::Bad,
    }
}

/// A notice by id.
fn notice_of(data: &Data, id: u64) -> Option<&Notice> {
    data.notices.iter().find(|n| n.id == id)
}

/// A privacy event by id (events are appended with growing ids; a hand-edited file falls back
/// to a scan).
fn event_of(data: &Data, id: u64) -> Option<&PrivacyEvent> {
    match data.privacy.binary_search_by_key(&id, |e| e.id) {
        Ok(i) => data.privacy.get(i),
        Err(_) => data.privacy.iter().find(|e| e.id == id),
    }
}

/// The version of the notice in effect at `now` (empty when none).
fn current_version(data: &Data, now: DateTime<Utc>) -> String {
    privacy::current_notice(&data.notices, now).map(|n| n.version.clone()).unwrap_or_default()
}

/// The notices newest first (by `effective_from`, then id).
fn notices_newest_first(data: &Data) -> Vec<&Notice> {
    let mut list: Vec<&Notice> = data.notices.iter().collect();
    list.sort_by(|a, b| b.effective_from.cmp(&a.effective_from).then(b.id.cmp(&a.id)));
    list
}

/// A purpose's description in the interface language (the id for a custom one).
fn purpose_full(id: &str, italian: bool) -> String {
    let text = views::purpose_text(id, italian);
    if text.is_empty() { id.to_string() } else { text }
}

/// A purpose's short name: its description up to the colon.
fn purpose_title(id: &str, italian: bool) -> String {
    let full = purpose_full(id, italian);
    match full.split_once(':') {
        Some((head, _)) if !head.trim().is_empty() => head.trim().to_string(),
        _ => full,
    }
}

/// Whether privacy events can be recorded for a customer: it exists and was not erased.
fn recordable(data: &Data, customer: u64) -> bool {
    data.customer(customer).is_some_and(|c| c.erased_at.is_none())
}

/// Now, as form text.
fn now_text(app: &DeskApp) -> String {
    format::input(&time::stamp(app.now()))
}

/// Who signs by default: the first contact, else the person themself.
fn default_signer(data: &Data, customer: u64) -> String {
    let Some(c) = data.customer(customer) else { return String::new() };
    match c.contacts.iter().map(|k| k.name.trim()).find(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None if c.kind == CustomerKind::Person => c.name.clone(),
        None => String::new(),
    }
}

/// A typed time as the stamp to send: `Ok(None)` when empty and optional; the error is a label.
fn stamp_of(text: &str, now: DateTime<Utc>, required: bool) -> Result<Option<String>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return if required { Err("Required") } else { Ok(None) };
    }
    format::stamp_from_input(text, now).map(Some).ok_or(NOT_A_TIME)
}

/// The ids of the PCs a withdrawal's warnings name (`device 7 \`PC\` has unattended access…`).
fn warned_devices(v: &Value) -> Vec<u64> {
    let mut out: Vec<u64> = v["warnings"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter_map(|w| w.strip_prefix("device ")?.split_whitespace().next()?.parse::<u64>().ok())
                .take(1000)
                .collect()
        })
        .unwrap_or_default();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------------------------
// The forms' requests

impl DeliveryForm {
    /// The `privacy.notice` call, or what is missing (a label).
    fn request(&self, data: &Data, now: DateTime<Utc>) -> Result<Request, &'static str> {
        if data.notices.is_empty() {
            return Err("Add the privacy notice first");
        }
        if self.version.trim().is_empty() {
            return Err("Required");
        }
        let at = stamp_of(&self.at, now, true)?.unwrap_or_default();
        if self.method == Method::Verbal && self.note.trim().is_empty() {
            return Err("Required");
        }
        let params = json!({
            "customer": self.customer,
            "version": self.version.trim(),
            "method": forms::method_id(self.method),
            "at": at,
            "signer": self.signer.trim(),
            "evidence": self.evidence.trim(),
            "note": self.note.trim(),
        });
        Ok(("privacy.notice", params, Intent::Delivered(self.customer)))
    }
}

impl ConsentForm {
    /// The purposes to send: the ticked ones and the other one, slugged.
    fn chosen(&self) -> Result<Vec<String>, &'static str> {
        let mut out: Vec<String> = self.purposes.iter().filter(|(_, on)| *on).map(|(p, _)| p.clone()).collect();
        if !self.other.trim().is_empty() {
            let p = privacy::purpose(&self.other).map_err(|_| "Letters, digits and _ only")?;
            if !out.contains(&p) {
                out.push(p);
            }
        }
        if out.is_empty() {
            return Err("Pick at least one purpose");
        }
        Ok(out)
    }

    /// The `privacy.consent` call, or what is missing (a label).
    fn request(&self, data: &Data, now: DateTime<Utc>) -> Result<Request, &'static str> {
        let purposes = self.chosen()?;
        let verbal = self.method == Method::Verbal;
        if self.signer.trim().is_empty() || (!verbal && self.evidence.trim().is_empty()) || (verbal && self.note.trim().is_empty()) {
            return Err("Required");
        }
        let at = stamp_of(&self.at, now, true)?.unwrap_or_default();
        let expires = stamp_of(&self.expires, now, false)?;
        if expires.as_ref().is_some_and(|e| *e < at) {
            return Err("The expiry is before the date");
        }
        if self.with_notice && (data.notices.is_empty() || (self.version.trim().is_empty() && current_version(data, now).is_empty())) {
            return Err("Add the privacy notice first");
        }
        let mut params = json!({
            "customer": self.customer,
            "purposes": purposes,
            "method": forms::method_id(self.method),
            "at": at,
            "signer": self.signer.trim(),
            "evidence": self.evidence.trim(),
            "note": self.note.trim(),
            "notice_version": self.version.trim(),
            "with_notice": self.with_notice,
        });
        if let (Some(e), Some(map)) = (expires, params.as_object_mut()) {
            map.insert("expires_at".into(), json!(e));
        }
        Ok(("privacy.consent", params, Intent::Consented(self.customer)))
    }
}

impl WithdrawForm {
    /// The `privacy.withdraw` call, or what is missing (a label).
    fn request(&self, now: DateTime<Utc>) -> Result<Request, &'static str> {
        let purposes: Vec<&String> = self.purposes.iter().filter(|(_, on)| *on).map(|(p, _)| p).collect();
        if purposes.is_empty() {
            return Err("Pick at least one purpose");
        }
        let at = stamp_of(&self.at, now, true)?.unwrap_or_default();
        let params = json!({
            "customer": self.customer,
            "purposes": purposes,
            "method": forms::method_id(self.method),
            "at": at,
            "signer": self.signer.trim(),
            "note": self.note.trim(),
        });
        Ok(("privacy.withdraw", params, Intent::Withdrawn(self.customer)))
    }
}

impl NoticeForm {
    /// Whether a notice with this version is already in the desk.
    fn exists(&self, data: &Data) -> bool {
        let v = self.version.trim();
        data.notices.iter().any(|n| n.version == v)
    }

    /// The `notice.add` call, or what is missing (a label).
    fn request(&self, data: &Data, now: DateTime<Utc>) -> Result<Request, &'static str> {
        if self.version.trim().is_empty() {
            return Err("Required");
        }
        if self.exists(data) {
            return Err("This version exists");
        }
        let effective = stamp_of(&self.effective, now, true)?.unwrap_or_default();
        let params = json!({
            "version": self.version.trim(),
            "title": self.title.trim(),
            "text": self.text,
            "effective_from": effective,
        });
        Ok(("notice.add", params, Intent::NoticeAdded))
    }
}

impl Form {
    fn request(&self, data: &Data, now: DateTime<Utc>) -> Result<Request, &'static str> {
        match self {
            Self::Delivery(f) => f.request(data, now),
            Self::Consent(f) => f.request(data, now),
            Self::Withdraw(f) => f.request(now),
            Self::Notice(f) => f.request(data, now),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Opening and closing the forms

/// Opens `form` for `customer`: selects the customer and, from a screen that does not draw
/// privacy forms, goes to the customer's card.
fn open_form(app: &mut DeskApp, customer: u64, form: Form) {
    app.privacy.form = Some(form);
    app.privacy.busy = None;
    app.privacy.error.clear();
    if app.selected_customer != Some(customer) {
        app.selected_customer = Some(customer);
    }
    if !matches!(app.screen, Screen::Customers | Screen::Devices | Screen::Privacy) {
        app.go(Screen::Customers);
    }
}

fn close_form(app: &mut DeskApp) {
    app.privacy.form = None;
    app.privacy.busy = None;
    app.privacy.error.clear();
}

/// Opens the consent form for a customer with `purposes` ticked (and the unattended-access
/// consent too when one of the customer's PCs is unattended without it). Nothing opens for an
/// unknown or erased customer.
pub fn open_consent(app: &mut DeskApp, customer: u64, purposes: &[&str]) {
    let data = app.data();
    if !recordable(data, customer) {
        return;
    }
    let now = app.now();
    let validity = data.preferences.consent_validity_months;
    let wanted: Vec<String> = purposes.iter().take(20).filter_map(|p| privacy::purpose(p).ok()).collect();
    let unattended = data.devices.iter().any(|d| d.customer_id == customer && d.unattended && !d.retired)
        && privacy::purpose_state(&data.privacy, customer, "unattended_access", now, validity).0 != PurposeState::Given;
    let list: Vec<(String, bool)> = privacy::known_purposes(&data.privacy)
        .into_iter()
        .map(|p| {
            let on = wanted.contains(&p) || (unattended && p == "unattended_access");
            (p, on)
        })
        .collect();
    let other = wanted.iter().find(|w| !list.iter().any(|(p, _)| p == *w)).cloned().unwrap_or_default();
    let version = current_version(data, now);
    let delivered = privacy::last_delivery(&data.privacy, customer).map(|e| e.notice_version.clone());
    let with_notice = !version.is_empty() && delivered.as_deref() != Some(version.as_str());
    let form = ConsentForm {
        customer,
        purposes: list,
        other,
        with_notice,
        method: Method::Paper,
        at: now_text(app),
        signer: default_signer(data, customer),
        version,
        ..ConsentForm::default()
    };
    open_form(app, customer, Form::Consent(form));
}

/// Opens the notice-delivery form for a customer: `version` when it exists, else the notice in
/// effect (else the newest one).
pub fn open_delivery(app: &mut DeskApp, customer: u64, version: Option<String>) {
    let data = app.data();
    if !recordable(data, customer) {
        return;
    }
    let now = app.now();
    let version = match version.filter(|v| data.notices.iter().any(|n| n.version == *v)) {
        Some(v) => v,
        None => {
            let current = current_version(data, now);
            if current.is_empty() { notices_newest_first(data).first().map(|n| n.version.clone()).unwrap_or_default() } else { current }
        }
    };
    let form = DeliveryForm {
        customer,
        version,
        method: Method::Paper,
        at: now_text(app),
        signer: default_signer(data, customer),
        ..DeliveryForm::default()
    };
    open_form(app, customer, Form::Delivery(form));
}

/// Opens the withdrawal form for a customer: the purposes currently given or expired, with
/// `purpose` ticked.
pub fn open_withdraw(app: &mut DeskApp, customer: u64, purpose: &str) {
    let data = app.data();
    if !recordable(data, customer) {
        return;
    }
    let now = app.now();
    let validity = data.preferences.consent_validity_months;
    let clicked = privacy::purpose(purpose).unwrap_or_default();
    let mut list: Vec<(String, bool)> = privacy::known_purposes(&data.privacy)
        .into_iter()
        .filter(|p| {
            *p == clicked
                || matches!(privacy::purpose_state(&data.privacy, customer, p, now, validity).0, PurposeState::Given | PurposeState::Expired)
        })
        .map(|p| {
            let on = p == clicked;
            (p, on)
        })
        .collect();
    if !clicked.is_empty() && !list.iter().any(|(p, _)| *p == clicked) {
        list.push((clicked, true));
    }
    let form = WithdrawForm {
        customer,
        purposes: list,
        method: Method::Paper,
        at: now_text(app),
        signer: default_signer(data, customer),
        note: String::new(),
    };
    open_form(app, customer, Form::Withdraw(form));
}

/// Opens the new-notice-version form on Privacy › Notices.
pub fn open_notice(app: &mut DeskApp) {
    let today = now_text(app);
    let form = NoticeForm {
        version: today.get(..7).unwrap_or_default().to_string(),
        title: app.tr("Privacy notice — technical support").to_string(),
        text: String::new(),
        effective: today,
    };
    app.privacy.form = Some(Form::Notice(form));
    app.privacy.busy = None;
    app.privacy.error.clear();
    app.privacy.notice_dismissed = false;
    app.privacy.tab = Tab::Notices;
    if app.screen != Screen::Privacy {
        app.go(Screen::Privacy);
    }
}

/// Opens the Audit log filtered to a record (the "Audit entries" links of the cards).
pub fn open_audit(app: &mut DeskApp, customer: Option<u64>, device: Option<u64>, intervention: Option<u64>) {
    app.privacy.audit_filter = AuditFilter { customer, device, intervention, ..AuditFilter::default() };
    app.privacy.tab = Tab::Audit;
    app.go(Screen::Privacy);
}

/// The customer whose privacy form is open, if any.
pub fn form_customer(app: &DeskApp) -> Option<u64> {
    app.privacy.form.as_ref().and_then(Form::customer)
}

/// Closes an open form (else, on this screen, the notice pane); answers whether one was open.
pub fn close(app: &mut DeskApp) -> bool {
    if app.privacy.form.is_some() {
        if matches!(app.privacy.form, Some(Form::Notice(_))) {
            app.privacy.notice_dismissed = true;
        }
        close_form(app);
        return true;
    }
    if app.screen == Screen::Privacy {
        if app.privacy.deliver_to.take().is_some() {
            return true;
        }
        if app.privacy.selected_notice.take().is_some() {
            return true;
        }
    }
    false
}

/// Opens the new-notice-version form (`n` on this screen).
pub fn new(app: &mut DeskApp) {
    open_notice(app);
}

/// Sends the open form's command (validated again: Cmd+Enter and tests come here too).
fn submit(app: &mut DeskApp) {
    if app.privacy.busy.is_some() {
        return;
    }
    let now = app.now();
    let request = match app.privacy.form.as_ref() {
        Some(form) => form.request(app.session.data(), now),
        None => return,
    };
    match request {
        Ok((command, params, intent)) => {
            app.privacy.error.clear();
            let job = app.run_as(command, params, crate::Intent::Privacy(intent));
            app.privacy.busy = Some(job);
        }
        Err(problem) => app.privacy.error = app.tr(problem).to_string(),
    }
}

/// Applies a finished command of this screen.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    if intent == Intent::Audit {
        if app.privacy.audit.finish(c) {
            let rows = app.privacy.audit.value.as_ref().map(|v| audit_rows(app.session.data(), v)).unwrap_or_default();
            app.privacy.audit_rows = rows;
        }
        return;
    }
    let mine = app.privacy.busy == Some(c.job);
    if mine {
        app.privacy.busy = None;
    }
    let v = match &c.result {
        Ok(v) => v,
        Err(e) => {
            if mine {
                app.privacy.error = app.language.tr_message(&e.to_string());
            }
            return;
        }
    };
    if mine {
        app.privacy.form = None;
        app.privacy.error.clear();
    }
    match intent {
        Intent::Delivered(_) => app.toast(app.tr("Notice delivery recorded").to_string()),
        Intent::Consented(customer) => {
            let n = v["events"].as_array().map_or(0, Vec::len);
            app.toast(app.language.fill("Consent recorded ({n} events)", &[("n", &n.to_string())]));
            let warnings = forms::warnings(app, v);
            if !warnings.is_empty() {
                app.privacy.consent_warnings = Some((customer, warnings));
            } else if app.privacy.consent_warnings.as_ref().is_some_and(|(c, _)| *c == customer) {
                app.privacy.consent_warnings = None;
            }
        }
        Intent::Withdrawn(customer) => {
            app.toast(app.tr("Withdrawal recorded").to_string());
            let devices = warned_devices(v);
            if !devices.is_empty() {
                app.customers.unattended_banner = Some((customer, devices));
            }
        }
        Intent::NoticeAdded => {
            app.toast(app.tr("Notice version added").to_string());
            if let Some(id) = forms::new_id(v, "notice") {
                app.privacy.selected_notice = Some(id);
                app.privacy.tab = Tab::Notices;
            }
        }
        Intent::Audit => {}
    }
}

// ---------------------------------------------------------------------------------------------
// The forms

/// Draws the open privacy form (in whatever pane calls it); answers whether one is open.
pub fn form_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let Some(mut form) = app.privacy.form.clone() else { return false };
    if form.customer().is_some_and(|c| !recordable(app.data(), c)) {
        close_form(app);
        return false;
    }
    let busy = app.privacy.busy.is_some();
    let view: &DeskApp = app;
    let act = match &mut form {
        Form::Delivery(f) => delivery_ui(view, ui, t, f, busy),
        Form::Consent(f) => consent_ui(view, ui, t, f, busy),
        Form::Withdraw(f) => withdraw_ui(view, ui, t, f, busy),
        Form::Notice(f) => notice_ui(view, ui, t, f, busy),
    };
    let notice = matches!(form, Form::Notice(_));
    match act {
        FormAct::None => app.privacy.form = Some(form),
        FormAct::Save => {
            app.privacy.form = Some(form);
            submit(app);
        }
        FormAct::Cancel => {
            close_form(app);
            if notice {
                app.privacy.notice_dismissed = true;
            }
        }
        FormAct::AddNotice => open_notice(app),
    }
    true
}

/// The Save/Cancel row's answer over what the header asked.
fn pick(act: FormAct, row: FormAction) -> FormAct {
    match row {
        FormAction::Save => FormAct::Save,
        FormAction::Cancel => FormAct::Cancel,
        FormAction::None => act,
    }
}

/// A form's title with a close button, and the customer it is for; answers whether the close
/// button was clicked.
fn form_header(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, title: &str, customer: Option<u64>) -> bool {
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(20.0).strong().color(t.ui_text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            cancel = widgets::icon_button(ui, t, Icon::Close, app.tr("Cancel"), true, false).clicked();
        });
    });
    if let Some(id) = customer {
        let name = crate::customer_name(app.data(), id);
        ui.horizontal(|ui| {
            chrome::avatar(ui, t, &name, chrome::color_for(&name), 20.0);
            ui.label(RichText::new(name).color(t.muted));
        });
    }
    ui.add_space(8.0);
    cancel
}

/// A form row: the label (`*` when required), a one-line field and, when required and empty,
/// "Required" under it. Ends the row.
fn text_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, required: bool) {
    let shown = if required { format!("{label} *") } else { label.to_string() };
    ui.label(RichText::new(shown).color(t.muted));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(f32::INFINITY));
        if required && value.trim().is_empty() {
            ui.label(RichText::new(app.tr("Required")).small().color(widgets::danger(t)));
        }
    });
    ui.end_row();
}

/// A form row with a multi-line field of `rows` lines (see [`text_row`]).
fn note_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, rows: usize, required: bool) {
    let shown = if required { format!("{label} *") } else { label.to_string() };
    top_label(ui, t, &shown);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(egui::TextEdit::multiline(value).desired_width(f32::INFINITY).desired_rows(rows));
        if required && value.trim().is_empty() {
            ui.label(RichText::new(app.tr("Required")).small().color(widgets::danger(t)));
        }
    });
    ui.end_row();
}

/// A grid label at the top of its cell (next to a tall field), not in the middle.
fn top_label(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).color(t.muted));
    });
}

/// A note under the field above it, muted or in a tone's colour. Ends the row.
fn hint_row(ui: &mut egui::Ui, t: &Tokens, text: &str, tone: Option<Tone>) {
    ui.label("");
    let color = tone.map_or(t.muted, |x| widgets::tone_colors(t, x).1);
    ui.add(egui::Label::new(RichText::new(text).small().color(color)).wrap());
    ui.end_row();
}

/// A line in a tone across a card: the tone's fill, a bar in its colour, an icon, wrapped text
/// and an optional small button at the right; answers whether the button was clicked. (Like
/// `widgets::banner`, with the bar painted on the line's own rect.)
fn tone_line(ui: &mut egui::Ui, t: &Tokens, tone: Tone, icon: Icon, text: &str, action: Option<&str>) -> bool {
    let (fill, color) = widgets::tone_colors(t, tone);
    let mut clicked = false;
    let frame = egui::Frame::NONE.fill(fill).corner_radius(8).inner_margin(egui::Margin { left: 12, right: 10, top: 7, bottom: 7 });
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), r, icon, color);
            let action_w = action.map_or(0.0, |a| ui.painter().layout_no_wrap(a.to_string(), FontId::proportional(14.0), color).size().x + 28.0);
            let text_w = (ui.available_width() - action_w).max(40.0);
            ui.allocate_ui_with_layout(vec2(text_w, 20.0), Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
            });
            if let Some(a) = action {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clicked = ui.small_button(a).clicked();
                });
            }
        });
    });
    let rect = shown.response.rect;
    let bar = Rect::from_min_max(pos2(rect.left() + 3.0, rect.top() + 6.0), pos2(rect.left() + 6.0, rect.bottom() - 6.0));
    ui.painter().rect_filled(bar, 1.5, color);
    ui.add_space(6.0);
    clicked
}

/// A small wrapped note in a colour, outside a grid.
fn note(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text).small().color(color)).wrap());
}

/// The notice versions for a combo: `(version, "{version} — {title}")`, newest first.
fn notice_options(app: &DeskApp, data: &Data) -> Vec<(String, String)> {
    notices_newest_first(data)
        .into_iter()
        .map(|n| {
            let label = if n.title.trim().is_empty() {
                n.version.clone()
            } else {
                app.language.fill("{version} — {title}", &[("version", &n.version), ("title", &format::clipped(&n.title, 48))])
            };
            (n.version.clone(), label)
        })
        .collect()
}

/// A form row with the notice-version combo and the *Current* chip. Ends the row.
fn version_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, salt: &str, value: &mut String, options: &[(String, String)], current: &str) {
    ui.label(RichText::new(app.tr("Notice version")).color(t.muted));
    ui.horizontal(|ui| {
        let shown = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_else(|| value.clone());
        let width = (ui.available_width() - 80.0).clamp(120.0, 250.0);
        egui::ComboBox::from_id_salt(salt).selected_text(format::clipped(&shown, 34)).width(width).show_ui(ui, |ui| {
            for (v, l) in options {
                ui.selectable_value(value, v.clone(), l);
            }
        });
        if !value.is_empty() && value.as_str() == current {
            widgets::tone_chip(ui, t, app.tr("Current"), Tone::Accent);
        }
    });
    ui.end_row();
}

/// The body of a form that needs a notice when the desk has none.
fn no_notice_body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, act: &mut FormAct) {
    tone_line(ui, t, Tone::Warn, Icon::Notice, app.tr("Add the privacy notice first"), None);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, t, Some(Icon::Plus), app.tr("Add the notice")).clicked() {
            *act = FormAct::AddNotice;
        }
        if ui.button(app.tr("Cancel")).clicked() {
            *act = FormAct::Cancel;
        }
    });
}

fn delivery_ui(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, f: &mut DeliveryForm, busy: bool) -> FormAct {
    let mut act = FormAct::None;
    if form_header(app, ui, t, app.tr("Record notice delivery"), Some(f.customer)) {
        act = FormAct::Cancel;
    }
    let data = app.data();
    let now = app.now();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if data.notices.is_empty() {
            no_notice_body(app, ui, t, &mut act);
            return;
        }
        let current = current_version(data, now);
        let options = notice_options(app, data);
        egui::Grid::new("privacy-delivery").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            version_row(app, ui, t, "privacy-delivery-version", &mut f.version, &options, &current);
            forms::method_combo(app, ui, t, app.tr("Method"), &mut f.method);
            forms::time_field(app, ui, t, &format!("{} *", app.tr("When")), &mut f.at, now, true);
            text_row(app, ui, t, app.tr("Signer"), &mut f.signer, "", false);
            let hint = app.tr("Where the signed form or the mail is: archive number, subject, form id");
            text_row(app, ui, t, app.tr("Evidence"), &mut f.evidence, hint, false);
            let verbal = f.method == Method::Verbal;
            if verbal {
                hint_row(ui, t, app.tr("Verbal is weak evidence: note who heard it"), Some(Tone::Warn));
            }
            note_row(app, ui, t, app.tr("Note"), &mut f.note, 3, verbal);
        });
        ui.add_space(4.0);
        widgets::error_line(ui, t, &app.privacy.error);
        let ready = f.request(data, now).is_ok();
        act = pick(act, forms::submit_row(app, ui, t, app.tr("Save"), busy, ready));
    });
    act
}

/// The purposes as checkboxes with their descriptions, and the note when none is ticked.
fn purpose_checks(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, purposes: &mut [(String, bool)], none_ok: bool) {
    let italian = app.italian();
    ui.label(RichText::new(format!("{} *", app.tr("Purposes"))).color(t.muted));
    ui.add_space(2.0);
    for (id, on) in purposes.iter_mut() {
        ui.checkbox(on, purpose_full(id, italian));
    }
    if !none_ok && !purposes.iter().any(|(_, on)| *on) {
        note(ui, app.tr("Pick at least one purpose"), widgets::danger(t));
    }
}

fn consent_ui(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, f: &mut ConsentForm, busy: bool) -> FormAct {
    let mut act = FormAct::None;
    if form_header(app, ui, t, app.tr("Record consent"), Some(f.customer)) {
        act = FormAct::Cancel;
    }
    let data = app.data();
    let now = app.now();
    let current = current_version(data, now);
    let delivered = privacy::last_delivery(&data.privacy, f.customer).is_some();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let other_ok = !f.other.trim().is_empty() && privacy::purpose(&f.other).is_ok();
        purpose_checks(app, ui, t, &mut f.purposes, other_ok);
        ui.add_space(8.0);
        let version = if f.version.trim().is_empty() { current.clone() } else { f.version.trim().to_string() };
        if !version.is_empty() {
            ui.checkbox(&mut f.with_notice, app.tr("Also record the notice delivery (one signed form)"));
            if f.with_notice {
                note(ui, &app.language.fill("Records the delivery of notice {version} together with the consent", &[("version", &version)]), t.muted);
            }
        } else {
            f.with_notice = false;
        }
        if !f.with_notice && !delivered {
            note(ui, app.tr("No notice delivery recorded: consent must be informed"), widgets::tone_colors(t, Tone::Warn).1);
        }
        ui.add_space(8.0);
        let verbal = f.method == Method::Verbal;
        egui::Grid::new("privacy-consent").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            ui.label(RichText::new(app.tr("Other purpose")).color(t.muted));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::TextEdit::singleline(&mut f.other).desired_width(f32::INFINITY));
                if !f.other.trim().is_empty() {
                    match privacy::purpose(&f.other) {
                        Ok(id) => ui.label(RichText::new(format!("→ {id}")).small().color(t.muted)),
                        Err(_) => ui.label(RichText::new(app.tr("Letters, digits and _ only")).small().color(widgets::danger(t))),
                    };
                }
            });
            ui.end_row();
            forms::method_combo(app, ui, t, app.tr("Method"), &mut f.method);
            let at = forms::time_field(app, ui, t, &format!("{} *", app.tr("When")), &mut f.at, now, true);
            text_row(app, ui, t, app.tr("Signer"), &mut f.signer, "", true);
            let hint = app.tr("Where the signed form or the mail is: archive number, subject, form id");
            text_row(app, ui, t, app.tr("Evidence"), &mut f.evidence, hint, !verbal);
            if verbal {
                hint_row(ui, t, app.tr("Verbal is weak evidence: note who heard it"), Some(Tone::Warn));
            }
            note_row(app, ui, t, app.tr("Note"), &mut f.note, 3, verbal);
            let expires = forms::time_field(app, ui, t, app.tr("Expires on"), &mut f.expires, now, false);
            if let (Some(at), Some(e)) = (at, expires)
                && !e.is_empty()
                && e < at
            {
                hint_row(ui, t, app.tr("The expiry is before the date"), Some(Tone::Bad));
            }
            if data.notices.len() > 1 {
                let options = notice_options(app, data);
                version_row(app, ui, t, "privacy-consent-version", &mut f.version, &options, &current);
            }
        });
        ui.add_space(4.0);
        widgets::error_line(ui, t, &app.privacy.error);
        let ready = f.request(data, now).is_ok();
        act = pick(act, forms::submit_row(app, ui, t, app.tr("Save"), busy, ready));
    });
    act
}

fn withdraw_ui(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, f: &mut WithdrawForm, busy: bool) -> FormAct {
    let mut act = FormAct::None;
    if form_header(app, ui, t, app.tr("Record withdrawal"), Some(f.customer)) {
        act = FormAct::Cancel;
    }
    let now = app.now();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        purpose_checks(app, ui, t, &mut f.purposes, false);
        ui.add_space(8.0);
        egui::Grid::new("privacy-withdraw").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            forms::method_combo(app, ui, t, app.tr("Method"), &mut f.method);
            forms::time_field(app, ui, t, &format!("{} *", app.tr("When")), &mut f.at, now, true);
            text_row(app, ui, t, app.tr("Signer"), &mut f.signer, "", false);
            note_row(app, ui, t, app.tr("Note"), &mut f.note, 3, false);
        });
        ui.add_space(6.0);
        note(ui, app.tr("This adds a withdrawal event; the history is kept."), t.muted);
        ui.add_space(4.0);
        widgets::error_line(ui, t, &app.privacy.error);
        let ready = f.request(now).is_ok();
        act = pick(act, forms::submit_row(app, ui, t, app.tr("Save"), busy, ready));
    });
    act
}

/// The outline **Insert outline** pastes: each heading and a placeholder line.
fn outline(app: &DeskApp) -> String {
    let placeholder = app.tr(PLACEHOLDER);
    OUTLINE.iter().map(|h| format!("{}\n{placeholder}", app.tr(h))).collect::<Vec<_>>().join("\n\n")
}

fn notice_ui(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, f: &mut NoticeForm, busy: bool) -> FormAct {
    let mut act = FormAct::None;
    if form_header(app, ui, t, app.tr("New notice version"), None) {
        act = FormAct::Cancel;
    }
    let data = app.data();
    let now = app.now();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("privacy-notice").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            ui.label(RichText::new(format!("{} *", app.tr("Version"))).color(t.muted));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::TextEdit::singleline(&mut f.version).hint_text(app.tr("2026-01, v3…")).desired_width(f32::INFINITY));
                if f.version.trim().is_empty() {
                    ui.label(RichText::new(app.tr("Required")).small().color(widgets::danger(t)));
                } else if f.exists(data) {
                    ui.label(RichText::new(app.tr("This version exists")).small().color(widgets::danger(t)));
                }
            });
            ui.end_row();
            text_row(app, ui, t, app.tr("Title"), &mut f.title, "", false);
            top_label(ui, t, app.tr("Text"));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if ui.small_button(app.tr("Insert outline")).clicked() {
                    let outline = outline(app);
                    if f.text.trim().is_empty() {
                        f.text = outline;
                    } else {
                        f.text.push_str("\n\n");
                        f.text.push_str(&outline);
                    }
                }
                ui.add(egui::TextEdit::multiline(&mut f.text).desired_width(f32::INFINITY).desired_rows(18));
                note(ui, app.tr("Replace every line with your own text: controller, data processed, purposes and legal basis, retention, recipients, rights (articles 15–22)."), t.muted);
            });
            ui.end_row();
            forms::time_field(app, ui, t, &format!("{} *", app.tr("Effective from")), &mut f.effective, now, true);
            hint_row(ui, t, app.tr("A future date schedules the version"), None);
        });
        ui.add_space(6.0);
        note(ui, app.tr("Versions never change: a new text is a new version."), t.muted);
        ui.add_space(4.0);
        widgets::error_line(ui, t, &app.privacy.error);
        let ready = f.request(data, now).is_ok();
        act = pick(act, forms::submit_row(app, ui, t, app.tr("Save"), busy, ready));
    });
    act
}

// ---------------------------------------------------------------------------------------------
// The privacy strip of a customer card

/// What a click in the strip asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StripAct {
    Delivery,
    AddNotice,
    /// The consent form with this purpose ticked (the remote-assistance one when `None`).
    Consent(Option<String>),
    Withdraw(String),
    History,
}

/// The privacy strip at the top of a customer card: the notice line, one line per purpose with
/// its state and the event behind it, **Record consent** and the expandable history.
pub fn strip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, customer: u64) {
    let Some(live) = app.data().customer(customer).map(|c| c.erased_at.is_none()) else { return };
    let Some(info) = app.derived.customer(customer).cloned() else { return };
    if let Some((who, list)) = app.privacy.consent_warnings.clone()
        && who == customer
        && tone_line(ui, t, Tone::Warn, Icon::Warning, &list.join("\n"), Some(app.tr("Close")))
    {
        app.privacy.consent_warnings = None;
    }
    let mut act: Option<StripAct> = None;
    let view: &DeskApp = app;
    widgets::section(ui, t, view.tr("Privacy"));
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (tone, icon, text, action) = match &info.notice {
            NoticeState::UpToDate { version, at, method } => {
                let method = view.tr(forms::method_label(Method::parse(method).unwrap_or(Method::Other)));
                let text = view.language.fill(
                    "Notice {version} delivered on {date} ({method})",
                    &[("version", version), ("date", &format::date(at, view.italian())), ("method", method)],
                );
                (Tone::Ok, Icon::ShieldCheck, text, None)
            }
            NoticeState::Outdated { old, new } => {
                let text = view.language.fill("Received notice {old}, current is {new}", &[("old", old), ("new", new)]);
                (Tone::Warn, Icon::Warning, text, Some(("Record delivery", StripAct::Delivery)))
            }
            NoticeState::NotDelivered { .. } => {
                (Tone::Bad, Icon::ShieldOff, view.tr("Notice not delivered").to_string(), Some(("Record delivery", StripAct::Delivery)))
            }
            NoticeState::NoNotice => {
                (Tone::Neutral, Icon::Notice, view.tr("No privacy notice in the desk").to_string(), Some(("Add the notice", StripAct::AddNotice)))
            }
        };
        let label = action.as_ref().filter(|_| live).map(|(l, _)| view.tr(l));
        if tone_line(ui, t, tone, icon, &text, label) {
            act = action.map(|(_, a)| a);
        }
        let unattended = !info.unattended_devices.is_empty();
        for row in &info.purposes {
            if let Some(a) = purpose_line(view, ui, t, row, live, unattended) {
                act = Some(a);
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if live && widgets::icon_text_button(ui, t, Icon::Signature, view.tr("Record consent"), true).clicked() {
                act = Some(StripAct::Consent(None));
            }
            if info.privacy_events > 0 {
                let text = view.language.fill("History ({n})", &[("n", &info.privacy_events.to_string())]);
                let open = view.privacy.history_open == Some(customer);
                let icon = if open { Icon::Chevron } else { Icon::History };
                if widgets::icon_text_button(ui, t, icon, &text, true).clicked() {
                    act = Some(StripAct::History);
                }
            }
        });
        if view.privacy.history_open == Some(customer) {
            history(view, ui, t, customer);
        }
    });
    ui.add_space(4.0);
    match act {
        Some(StripAct::Delivery) => open_delivery(app, customer, None),
        Some(StripAct::AddNotice) => open_notice(app),
        Some(StripAct::Consent(None)) => open_consent(app, customer, &["remote_assistance"]),
        Some(StripAct::Consent(Some(p))) => open_consent(app, customer, &[p.as_str()]),
        Some(StripAct::Withdraw(p)) => open_withdraw(app, customer, &p),
        Some(StripAct::History) => {
            app.privacy.history_open = if app.privacy.history_open == Some(customer) { None } else { Some(customer) };
        }
        None => {}
    }
}

/// One purpose of the strip: its name, the state chip, the event's details and the button
/// that changes it; answers what was clicked.
fn purpose_line(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, row: &PurposeRow, live: bool, unattended: bool) -> Option<StripAct> {
    let italian = app.italian();
    let mut act = None;
    ui.add_space(2.0);
    // The button first (right), then the name (truncated) and the chip in what is left.
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            if row.id == "unattended_access" && unattended && row.state == PurposeState::Missing {
                widgets::tone_chip(ui, t, app.tr("Missing"), Tone::Bad);
            } else {
                forms::state_chip(app, ui, t, row.state);
            }
            ui.add(egui::Label::new(RichText::new(purpose_title(&row.id, italian)).color(t.ui_text)).truncate())
                .on_hover_text(purpose_full(&row.id, italian));
        },
        |ui| {
            if !live {
                return;
            }
            let (label, next) = match row.state {
                PurposeState::Given | PurposeState::Expired => ("Withdraw", StripAct::Withdraw(row.id.clone())),
                PurposeState::Missing | PurposeState::Withdrawn => ("Record…", StripAct::Consent(Some(row.id.clone()))),
            };
            if ui.small_button(app.tr(label)).clicked() {
                act = Some(next);
            }
        },
    );
    if let Some(e) = row.event.and_then(|id| event_of(app.data(), id)) {
        let mut parts = vec![app.tr(forms::method_label(e.method)).to_string(), format::date(&e.at, italian)];
        if !e.signer.is_empty() {
            parts.push(e.signer.clone());
        }
        if let Some(x) = e.expires_at.as_deref().filter(|x| !x.is_empty()) {
            parts.push(app.language.fill("Expires {date}", &[("date", &format::date(x, italian))]));
        }
        if !e.evidence.is_empty() {
            parts.push(e.evidence.clone());
        }
        note(ui, &parts.join(" · "), t.muted);
    }
    act
}

/// The customer's privacy history, newest first (append-only: nothing here is editable).
fn history(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, customer: u64) {
    let data = app.data();
    let italian = app.italian();
    let mut events: Vec<&PrivacyEvent> = data.privacy.iter().filter(|e| e.customer_id == customer).collect();
    events.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
    for e in events.iter().take(HISTORY_MAX) {
        ui.add(egui::Separator::default().spacing(8.0));
        let (icon, label, tone) = match e.kind {
            PrivacyKind::NoticeDelivered => (Icon::Notice, "Notice delivered", Tone::Accent),
            PrivacyKind::ConsentGiven => (Icon::Signature, "Consent given", Tone::Ok),
            PrivacyKind::ConsentWithdrawn => (Icon::ShieldOff, "Consent withdrawn", Tone::Bad),
        };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, icon, widgets::tone_colors(t, tone).1);
            ui.label(RichText::new(app.tr(label)).strong().color(t.ui_text));
            let what = if e.kind == PrivacyKind::NoticeDelivered { e.notice_version.clone() } else { purpose_title(&e.purpose, italian) };
            if !what.is_empty() {
                ui.label(RichText::new(what).color(t.ui_text));
            }
        });
        let mut parts = vec![app.tr(forms::method_label(e.method)).to_string(), format::long(&e.at, italian)];
        for x in [&e.signer, &e.evidence] {
            if !x.is_empty() {
                parts.push(x.clone());
            }
        }
        if let Some(x) = e.expires_at.as_deref().filter(|x| !x.is_empty()) {
            parts.push(app.language.fill("Expires {date}", &[("date", &format::date(x, italian))]));
        }
        note(ui, &parts.join(" · "), t.muted);
        if !e.note.is_empty() {
            note(ui, &e.note, t.ui_text);
        }
        let recorded = app.language.fill("Recorded by {who} on {date}", &[("who", &e.recorded_by), ("date", &format::long(&e.recorded_at, italian))]);
        note(ui, &recorded, t.muted.gamma_multiply(0.8));
    }
    if events.len() > HISTORY_MAX {
        ui.label(RichText::new(format!("+{}", events.len().saturating_sub(HISTORY_MAX))).small().color(t.muted));
    }
}

// ---------------------------------------------------------------------------------------------
// The screen

/// What the right pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    None,
    Form,
    Notice(u64),
}

/// Draws the screen: the right pane (a form or a notice's text) when one is open, then the tab.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if app.privacy.tab == Tab::Audit {
        let params = app.privacy.audit_filter.params();
        derived::refresh(app, |a| &mut a.privacy.audit, "audit.list", params, crate::Intent::Privacy(Intent::Audit));
    }
    if app.privacy.tab == Tab::Notices && app.data().notices.is_empty() && app.privacy.form.is_none() && !app.privacy.notice_dismissed {
        open_notice(app);
    }
    if app.privacy.selected_notice.is_some_and(|id| notice_of(app.data(), id).is_none()) {
        app.privacy.selected_notice = None;
    }
    let pane = match (&app.privacy.form, app.privacy.tab, app.privacy.selected_notice) {
        (Some(_), _, _) => Pane::Form,
        (None, Tab::Notices, Some(id)) => Pane::Notice(id),
        _ => Pane::None,
    };
    if pane != Pane::None {
        if ui.ctx().content_rect().width() < NARROW {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("privacy-pane").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 14)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        pane_ui(app, ui, t, pane);
                    });
                });
            });
            return;
        }
        crate::detail_panel(ui, t, |ui| pane_ui(app, ui, t, pane));
    }
    crate::central_panel(ui, t, |ui| {
        header(app, ui, t);
        match app.privacy.tab {
            Tab::Review => review_list(app, ui, t),
            Tab::Notices => notices_list(app, ui, t),
            Tab::Audit => audit_list(app, ui, t),
        }
    });
}

fn pane_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, pane: Pane) {
    match pane {
        Pane::Form => {
            form_ui(app, ui, t);
        }
        Pane::Notice(id) => notice_pane(app, ui, t, id),
        Pane::None => {}
    }
}

/// A pill in the header: an icon, a text, accent when `on`; `closable` adds a `×` that answers
/// `true` in the second value when clicked.
fn pill(ui: &mut egui::Ui, t: &Tokens, icon: Option<Icon>, text: &str, on: bool, closable: bool) -> (egui::Response, bool) {
    let color = if on { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), color);
    let lead = if icon.is_some() { 22.0 } else { 0.0 };
    let tail = if closable { 18.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0 + lead + tail, 28.0), Sense::click());
    let p = ui.painter();
    let fill = if on {
        t.active
    } else if resp.hovered() {
        t.hover
    } else {
        t.panel
    };
    p.rect_filled(rect, 14.0, fill);
    if !on {
        p.rect_stroke(rect, 14.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    let mut x = rect.left() + 11.0;
    if let Some(i) = icon {
        icons::paint(p, Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(15.0, 15.0)), i, color);
        x += lead;
    }
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    let mut cleared = false;
    if closable {
        let x_rect = Rect::from_center_size(pos2(rect.right() - 15.0, rect.center().y), vec2(18.0, 18.0));
        let xr = ui.interact(x_rect, ui.id().with(("privacy-pill-x", text)), Sense::click());
        if xr.hovered() {
            ui.painter().circle_filled(x_rect.center(), 9.0, t.hover);
        }
        icons::paint(ui.painter(), x_rect.shrink(4.0), Icon::Close, color);
        cleared = xr.clicked();
    }
    (resp, cleared)
}

/// The header strip: the title, the tabs and the tab's own controls.
fn header(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let mut tab_pick = None;
    let mut new_notice = false;
    egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let q = app.search.trim();
            let title = if q.is_empty() { app.tr("Privacy").to_string() } else { format!("{} · \"{}\"", app.tr("Privacy"), format::clipped(q, 24)) };
            ui.label(RichText::new(title).strong().color(t.ui_text));
            widgets::divider(ui, t);
            let findings = app.derived.findings.len();
            for (tab, icon, label, count) in [
                (Tab::Review, Icon::ShieldCheck, "Review", findings),
                (Tab::Notices, Icon::Notice, "Notices", app.data().notices.len()),
                (Tab::Audit, Icon::Audit, "Audit log", 0),
            ] {
                let text = if count > 0 { format!("{} {count}", app.tr(label)) } else { app.tr(label).to_string() };
                if pill(ui, t, Some(icon), &text, app.privacy.tab == tab, false).0.clicked() {
                    tab_pick = Some(tab);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match app.privacy.tab {
                Tab::Review => {
                    let age = Duration::from_std(app.derived.built.elapsed()).unwrap_or(Duration::zero());
                    if let Some(at) = app.now().checked_sub_signed(age) {
                        let text = app.language.fill("Last check {time}", &[("time", &format::clock(&time::stamp(at)))]);
                        let wide = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(12.0), t.muted).size().x + 12.0;
                        if ui.available_width() >= wide {
                            ui.label(RichText::new(text).small().color(t.muted));
                        }
                    }
                }
                Tab::Notices => {
                    // The accent button when it fits, else its icon (the pane narrows the list).
                    let label = app.tr("New notice version");
                    let wide = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(14.0), t.on_accent).size().x + 60.0;
                    new_notice = if ui.available_width() >= wide {
                        widgets::primary_button(ui, t, Some(Icon::Plus), label).on_hover_text(app.tip("notice.add")).clicked()
                    } else {
                        widgets::icon_button(ui, t, Icon::Plus, label, true, false).clicked()
                    };
                }
                Tab::Audit => {
                    if app.privacy.audit.loading() {
                        ui.add(egui::Spinner::new().size(16.0).color(t.accent));
                    }
                    let n = app.privacy.audit_rows.len();
                    ui.label(RichText::new(app.language.fill("{n} entries", &[("n", &n.to_string())])).small().color(t.muted));
                }
            });
        });
        if app.privacy.tab == Tab::Audit {
            ui.add_space(2.0);
            ui.horizontal(|ui| audit_filters(app, ui, t));
        }
    });
    if let Some(tab) = tab_pick {
        app.privacy.tab = tab;
        app.privacy.deliver_to = None;
    }
    if new_notice {
        open_notice(app);
    }
}

/// A muted line with an icon under the header (a rule of the tab).
fn hint_bar(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(14, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Info, t.muted);
            ui.label(RichText::new(text).small().color(t.muted));
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Review

/// A row of the Review list: a customer's heading or a finding (indented under its customer).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReviewRow {
    Header(u64, String),
    Finding { index: usize, grouped: bool },
}

/// The Review rows: the findings without a customer first (`no_notice` on top), then each
/// customer's findings under their name. `fq` (folded) keeps the customers whose name has it.
fn review_rows(data: &Data, findings: &[Finding], fq: &str) -> Vec<ReviewRow> {
    let owner = |f: &Finding| f.customer.or_else(|| f.intervention.and_then(|i| data.intervention(i)).map(|i| i.customer_id));
    let wanted: HashSet<u64> = findings.iter().filter_map(owner).collect();
    let names: HashMap<u64, String> = data.customers.iter().filter(|c| wanted.contains(&c.id)).map(|c| (c.id, c.name.clone())).collect();
    let mut loose: Vec<usize> = Vec::new();
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    let mut at: HashMap<u64, usize> = HashMap::new();
    for (i, f) in findings.iter().enumerate() {
        match owner(f) {
            None => loose.push(i),
            Some(c) => {
                let name = names.get(&c).map(String::as_str).unwrap_or_default();
                if !fq.is_empty() && !fold(name).contains(fq) {
                    continue;
                }
                let g = *at.entry(c).or_insert_with(|| {
                    groups.push((c, Vec::new()));
                    groups.len().saturating_sub(1)
                });
                if let Some((_, list)) = groups.get_mut(g) {
                    list.push(i);
                }
            }
        }
    }
    loose.sort_by_key(|i| findings.get(*i).is_none_or(|f| f.issue != "no_notice"));
    let mut out: Vec<ReviewRow> = loose.into_iter().map(|index| ReviewRow::Finding { index, grouped: false }).collect();
    for (c, list) in groups {
        out.push(ReviewRow::Header(c, names.get(&c).cloned().unwrap_or_else(|| format!("#{c}"))));
        out.extend(list.into_iter().map(|index| ReviewRow::Finding { index, grouped: true }));
    }
    out
}

/// Rebuilds the Review rows and details when the findings, the search or the language changed.
fn refresh_review(app: &mut DeskApp) {
    let key = (app.derived.revision, app.derived.built, app.query(), app.language);
    if app.privacy.review_key.as_ref() == Some(&key) {
        return;
    }
    app.privacy.review = review_rows(app.session.data(), &app.derived.findings, &key.2);
    app.privacy.review_details = review_details(app);
    app.privacy.review_key = Some(key);
}

/// What the findings' details need from the big collections, gathered in one pass each (the
/// desk may hold 200k PCs and 1M interventions; the details are not built per frame).
struct DetailIndex<'a> {
    current: String,
    unattended: HashMap<u64, Vec<&'a str>>,
    remote: HashMap<u64, Vec<&'a str>>,
    /// Intervention → (technician, started at).
    running: HashMap<u64, (&'a str, &'a str)>,
}

fn detail_index<'a>(data: &'a Data, findings: &[Finding], now: DateTime<Utc>) -> DetailIndex<'a> {
    let customers: HashSet<u64> = findings.iter().filter_map(|f| f.customer).collect();
    let interventions: HashSet<u64> = findings.iter().filter_map(|f| f.intervention).collect();
    let mut unattended: HashMap<u64, Vec<&str>> = HashMap::new();
    let mut remote: HashMap<u64, Vec<&str>> = HashMap::new();
    for d in data.devices.iter().filter(|d| !d.retired && customers.contains(&d.customer_id)) {
        for (on, map) in [(d.unattended, &mut unattended), (!d.rustdesk_id.is_empty(), &mut remote)] {
            if !on {
                continue;
            }
            let names = map.entry(d.customer_id).or_default();
            if names.len() < 6 {
                names.push(d.name.as_str());
            }
        }
    }
    let running =
        data.interventions.iter().filter(|i| interventions.contains(&i.id)).map(|i| (i.id, (i.technician.as_str(), i.started_at.as_str()))).collect();
    DetailIndex { current: current_version(data, now), unattended, remote, running }
}

/// The details of every finding, in order.
fn review_details(app: &DeskApp) -> Vec<String> {
    let index = detail_index(app.data(), &app.derived.findings, app.now());
    app.derived.findings.iter().map(|f| finding_detail(app, f, &index)).collect()
}

/// What settles a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FindingAct {
    Delivery(u64),
    Consent(u64, &'static str),
    Erase(u64),
    Intervention(u64),
    AddNotice,
}

/// The action of a finding and its label.
fn finding_act(f: &Finding) -> Option<(FindingAct, &'static str)> {
    let issue = f.issue.as_str();
    match (issue, f.customer, f.intervention) {
        ("no_notice", _, _) => Some((FindingAct::AddNotice, "Add the notice")),
        ("notice_missing" | "notice_outdated", Some(c), _) => Some((FindingAct::Delivery(c), "Record delivery")),
        ("retention_due", Some(c), _) => Some((FindingAct::Erase(c), "Erase personal data…")),
        ("timer_running", _, Some(i)) => Some((FindingAct::Intervention(i), "Open intervention")),
        (i, Some(c), _) if i.starts_with("remote_assistance_") => Some((FindingAct::Consent(c, "remote_assistance"), "Record consent")),
        (i, Some(c), _) if i.starts_with("unattended_access_") => Some((FindingAct::Consent(c, "unattended_access"), "Record consent")),
        _ => None,
    }
}

fn run_finding(app: &mut DeskApp, act: FindingAct) {
    match act {
        FindingAct::Delivery(c) => open_delivery(app, c, None),
        FindingAct::Consent(c, p) => open_consent(app, c, &[p]),
        FindingAct::Erase(c) => {
            close_form(app);
            crate::erase::open(app, c);
        }
        FindingAct::Intervention(i) => app.open_intervention(i),
        FindingAct::AddNotice => open_notice(app),
    }
}

/// The muted detail of a finding, from the mirror (the engine's sentence is English).
fn finding_detail(app: &DeskApp, f: &Finding, index: &DetailIndex) -> String {
    let data = app.data();
    let now = app.now();
    let italian = app.italian();
    let info = f.customer.and_then(|c| app.derived.customer(c));
    let since = |date: &str| app.language.fill("Since {date}", &[("date", &format::date(date, italian))]);
    let devices = |map: &HashMap<u64, Vec<&str>>| f.customer.and_then(|c| map.get(&c)).map(|names| names.join(", ")).unwrap_or_default();
    match f.issue.as_str() {
        "no_notice" => app.tr("Add the text you give customers (art. 13): deliveries and consents refer to its version.").to_string(),
        "notice_missing" if index.current.is_empty() => String::new(),
        "notice_missing" => app.language.fill("Current: {version}", &[("version", &index.current)]),
        "notice_outdated" => match info.map(|i| &i.notice) {
            Some(NoticeState::Outdated { old, new }) => app.language.fill("Has notice {old}, current is {new}", &[("old", old), ("new", new)]),
            _ => String::new(),
        },
        "retention_due" => info.and_then(|i| i.retention_due.as_deref()).map(since).unwrap_or_default(),
        "timer_running" => match f.intervention.and_then(|i| index.running.get(&i)) {
            Some((technician, started)) => {
                let who = if f.technician.is_empty() { technician } else { f.technician.as_str() };
                app.language.fill("{technician}, since {time}", &[("technician", who), ("time", &format::short(started, now, italian))])
            }
            None => f.technician.clone(),
        },
        issue => {
            let purpose = if issue.starts_with("unattended_access_") { "unattended_access" } else { "remote_assistance" };
            let row = info.and_then(|i| i.purposes.iter().find(|p| p.id == purpose));
            let event = row.and_then(|r| r.event).and_then(|id| event_of(data, id));
            match (row.map(|r| r.state), event) {
                (Some(PurposeState::Withdrawn), Some(e)) => since(&e.at),
                (Some(PurposeState::Expired), Some(e)) => {
                    let validity = data.preferences.consent_validity_months;
                    let by_age = if validity > 0 { time::add_months(&e.at, validity) } else { None };
                    e.expires_at.clone().filter(|x| !x.is_empty()).or(by_age).map(|x| since(&x)).unwrap_or_default()
                }
                _ if purpose == "unattended_access" => devices(&index.unattended),
                _ => devices(&index.remote),
            }
        }
    }
}

fn review_list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    refresh_review(app);
    if app.privacy.review.is_empty() {
        if app.derived.findings.is_empty() {
            widgets::empty_state(ui, t, Icon::ShieldCheck, app.tr("Nothing to do: the GDPR record is complete"), "");
        } else {
            widgets::empty_state(ui, t, Icon::Search, app.tr("Nothing here yet"), "");
        }
        return;
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let rows = std::mem::take(&mut app.privacy.review);
    let mut act: Option<FindingAct> = None;
    let mut open: Option<Screen> = None;
    let mut target: Option<u64> = None;
    egui::ScrollArea::vertical().id_salt("privacy-review").auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, range| {
        for row in rows.get(range).unwrap_or_default() {
            match row {
                ReviewRow::Header(c, name) => {
                    if group_row(ui, t, *c, name).clicked() {
                        open = Some(Screen::Customers);
                        target = Some(*c);
                    }
                }
                ReviewRow::Finding { index, grouped } => match finding_row(app, ui, t, *index, *grouped) {
                    RowClick::Act(a) => act = Some(a),
                    RowClick::Open(screen, id) => {
                        open = Some(screen);
                        target = id;
                    }
                    RowClick::None => {}
                },
            }
        }
    });
    app.privacy.review = rows;
    if let Some(a) = act {
        run_finding(app, a);
    } else {
        match (open, target) {
            (Some(Screen::Customers), Some(c)) => app.open_customer(c),
            (Some(Screen::Interventions), Some(i)) => app.open_intervention(i),
            (Some(Screen::Privacy), _) => open_notice(app),
            _ => {}
        }
    }
}

/// A customer's heading in the Review list (click → the customer's card).
fn group_row(ui: &mut egui::Ui, t: &Tokens, customer: u64, name: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, if resp.hovered() { t.hover } else { t.chrome });
    p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.border));
    let cy = rect.center().y;
    let av = pos2(rect.left() + 26.0, cy);
    p.circle_filled(av, 12.0, chrome::color_for(name));
    p.text(av, Align2::CENTER_CENTER, format::initials(name), FontId::proportional(10.0), t.on_accent);
    let g = widgets::truncate(p, name, FontId::proportional(14.0), t.ui_text, (rect.width() - 100.0).max(20.0));
    let y = cy - g.size().y / 2.0;
    p.galley(pos2(rect.left() + 46.0, y), g.clone(), t.ui_text);
    p.galley(pos2(rect.left() + 46.5, y), g, t.ui_text);
    p.text(pos2(rect.right() - 40.0, cy), Align2::RIGHT_CENTER, format!("#{customer}"), FontId::proportional(12.0), t.muted);
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 20.0, cy), vec2(14.0, 14.0)), Icon::ChevronRight, t.muted);
    resp
}

/// What a click on a finding row asked for.
enum RowClick {
    None,
    Act(FindingAct),
    Open(Screen, Option<u64>),
}

/// A finding: the tone chip with its label, the detail, the action on hover.
fn finding_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, index: usize, grouped: bool) -> RowClick {
    let (rect, resp) = widgets::row(ui, t, ROW_H, false);
    let Some(f) = app.derived.findings.get(index) else { return RowClick::None };
    let hot = ui.rect_contains_pointer(rect);
    if hot && !resp.hovered() {
        ui.painter().rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 1.0)), 0.0, t.hover);
    }
    let p = ui.painter().clone();
    let cy = rect.center().y;
    let x0 = rect.left() + if grouped { 46.0 } else { 16.0 };
    let (fill, color) = widgets::tone_colors(t, finding_tone(&f.issue));
    let g = p.layout_no_wrap(app.tr(finding_label(&f.issue)).to_string(), FontId::proportional(12.0), color);
    let chip = Rect::from_min_size(pos2(x0, cy - 11.0), vec2(g.size().x + 16.0, 22.0));
    p.rect_filled(chip, 11.0, fill);
    p.galley(pos2(chip.left() + 8.0, cy - g.size().y / 2.0), g, color);
    let right = rect.right() - 12.0;
    let mut edge = right;
    let act = finding_act(f);
    let mut clicked = None;
    if hot && let Some((a, label)) = act {
        let g = p.layout_no_wrap(app.tr(label).to_string(), FontId::proportional(13.0), t.accent);
        let r = Rect::from_min_max(pos2(right - g.size().x - 22.0, cy - 13.0), pos2(right, cy + 13.0));
        let hr = ui.interact(r, ui.id().with(("privacy-finding", index)), Sense::click());
        p.rect_filled(r, 13.0, if hr.hovered() { t.active } else { t.card });
        p.rect_stroke(r, 13.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.galley(pos2(r.left() + 11.0, cy - g.size().y / 2.0), g, t.accent);
        if hr.clicked() {
            clicked = Some(a);
        }
        edge = r.left() - 10.0;
    }
    let detail = app.privacy.review_details.get(index).map(String::as_str).unwrap_or_default();
    if !detail.is_empty() {
        let dx = chip.right() + 10.0;
        let dg = widgets::truncate(&p, detail, FontId::proportional(13.0), t.muted, (edge - dx).max(10.0));
        p.galley(pos2(dx, cy - dg.size().y / 2.0), dg, t.muted);
    }
    if let Some(a) = clicked {
        return RowClick::Act(a);
    }
    if resp.clicked() {
        return match (f.issue.as_str(), f.customer, f.intervention) {
            ("no_notice", _, _) => RowClick::Open(Screen::Privacy, None),
            (_, Some(c), _) => RowClick::Open(Screen::Customers, Some(c)),
            (_, None, Some(i)) => RowClick::Open(Screen::Interventions, Some(i)),
            _ => RowClick::None,
        };
    }
    RowClick::None
}

// ---------------------------------------------------------------------------------------------
// Notices

/// A row of the Notices list.
struct NoticeRow {
    id: u64,
    version: String,
    title: String,
    effective: String,
    created_by: String,
    created_at: String,
    current: bool,
    scheduled: bool,
    delivered: usize,
}

/// Counts, per notice version, the customers who received it (once per data revision).
fn refresh_delivered(app: &mut DeskApp) {
    let revision = app.session.revision();
    if app.privacy.delivered_key == Some(revision) {
        return;
    }
    let counts = {
        let mut seen: HashSet<(&str, u64)> = HashSet::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in app.session.data().privacy.iter().filter(|e| e.kind == PrivacyKind::NoticeDelivered) {
            if seen.insert((e.notice_version.as_str(), e.customer_id)) {
                let n = counts.entry(e.notice_version.clone()).or_insert(0);
                *n = n.saturating_add(1);
            }
        }
        counts
    };
    app.privacy.delivered = counts;
    app.privacy.delivered_key = Some(revision);
}

fn notices_list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    refresh_delivered(app);
    let data = app.data();
    if data.notices.is_empty() {
        widgets::empty_state(
            ui,
            t,
            Icon::Notice,
            app.tr("No privacy notice yet"),
            app.tr("Add the text you give customers (art. 13): deliveries and consents refer to its version."),
        );
        return;
    }
    let now = app.now();
    let now_stamp = time::stamp(now);
    let current = privacy::current_notice(&data.notices, now).map(|n| n.id);
    let fq = app.query();
    let rows: Vec<NoticeRow> = notices_newest_first(data)
        .into_iter()
        .filter(|n| fq.is_empty() || fold(&n.version).contains(&fq) || fold(&n.title).contains(&fq))
        .map(|n| NoticeRow {
            id: n.id,
            version: n.version.clone(),
            title: n.title.clone(),
            effective: n.effective_from.clone(),
            created_by: n.created_by.clone(),
            created_at: n.created_at.clone(),
            current: current == Some(n.id),
            scheduled: n.effective_from > now_stamp,
            delivered: app.privacy.delivered.get(&n.version).copied().unwrap_or(0),
        })
        .collect();
    if rows.is_empty() {
        widgets::empty_state(ui, t, Icon::Search, app.tr("Nothing here yet"), "");
        return;
    }
    let mut picked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let height = (ui.available_height() - 30.0).max(NOTICE_ROW_H);
    egui::ScrollArea::vertical().id_salt("privacy-notices").max_height(height).auto_shrink([false, false]).show_rows(
        ui,
        NOTICE_ROW_H,
        rows.len(),
        |ui, range| {
            for row in rows.get(range).unwrap_or_default() {
                if notice_row(app, ui, t, row).clicked() {
                    picked = Some(row.id);
                }
            }
        },
    );
    hint_bar(ui, t, app.tr("Versions never change: a new text is a new version."));
    if let Some(id) = picked {
        app.privacy.selected_notice = Some(id);
        app.privacy.deliver_to = None;
    }
}

/// A notice version: the version, its chips and title; when it applies and how many received
/// it; who created it.
fn notice_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, n: &NoticeRow) -> egui::Response {
    let selected = app.privacy.selected_notice == Some(n.id);
    let (rect, resp) = widgets::row(ui, t, NOTICE_ROW_H, selected);
    let p = ui.painter().clone();
    let italian = app.italian();
    let cy = rect.center().y;
    let (y1, y2) = (cy - 9.0, cy + 10.0);
    icons::paint(&p, Rect::from_center_size(pos2(rect.left() + 24.0, cy), vec2(20.0, 20.0)), Icon::Notice, if selected { t.accent } else { t.muted });
    let x0 = rect.left() + 46.0;
    let right = rect.right() - 14.0;
    let created = app.language.fill("Created by {who} on {date}", &[("who", &n.created_by), ("date", &format::date(&n.created_at, italian))]);
    let cg = widgets::truncate(&p, &created, FontId::proportional(12.0), t.muted, (rect.width() * 0.32).max(60.0));
    let created_w = cg.size().x;
    p.galley(pos2(right - created_w, y1 - cg.size().y / 2.0), cg, t.muted);
    let text_right = right - created_w - 12.0;
    let vg = widgets::truncate(&p, &n.version, FontId::proportional(14.0), t.ui_text, ((text_right - x0) * 0.4).max(20.0));
    let vw = vg.size().x;
    p.galley(pos2(x0, y1 - vg.size().y / 2.0), vg.clone(), t.ui_text);
    p.galley(pos2(x0 + 0.5, y1 - vg.size().y / 2.0), vg, t.ui_text);
    let mut x = x0 + vw + 8.0;
    for (on, label, tone) in [(n.current, "Current", Tone::Accent), (n.scheduled, "Scheduled", Tone::Warn)] {
        if !on {
            continue;
        }
        let (fill, color) = widgets::tone_colors(t, tone);
        let g = p.layout_no_wrap(app.tr(label).to_string(), FontId::proportional(11.5), color);
        let r = Rect::from_min_size(pos2(x, y1 - 9.0), vec2(g.size().x + 14.0, 18.0));
        p.rect_filled(r, 9.0, fill);
        p.galley(pos2(r.left() + 7.0, y1 - g.size().y / 2.0), g, color);
        x = r.right() + 8.0;
    }
    if !n.title.is_empty() && x < text_right - 20.0 {
        let g = widgets::truncate(&p, &n.title, FontId::proportional(14.0), t.ui_text, text_right - x);
        p.galley(pos2(x, y1 - g.size().y / 2.0), g, t.ui_text);
    }
    let line = format!(
        "{} · {}",
        app.language.fill("Effective from {date}", &[("date", &format::date(&n.effective, italian))]),
        app.language.fill("{n} customers received it", &[("n", &n.delivered.to_string())]),
    );
    let g = widgets::truncate(&p, &line, FontId::proportional(12.5), t.muted, (right - x0).max(20.0));
    p.galley(pos2(x0, y2 - g.size().y / 2.0), g, t.muted);
    resp
}

/// The right pane of the Notices tab: the notice's facts, its text (selectable, never
/// editable), Copy text and Record delivery to a customer….
fn notice_pane(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64) {
    let now = app.now();
    let italian = app.italian();
    let Some(n) = notice_of(app.data(), id) else { return };
    let (version, title, effective, created_by, created_at) =
        (n.version.clone(), n.title.clone(), n.effective_from.clone(), n.created_by.clone(), n.created_at.clone());
    let current = privacy::current_notice(&app.data().notices, now).map(|c| c.id) == Some(id);
    let scheduled = effective > time::stamp(now);
    let delivered = app.privacy.delivered.get(&version).copied().unwrap_or(0);
    let (mut copy, mut deliver, mut close) = (false, false, false);
    ui.horizontal(|ui| {
        copy = widgets::icon_text_button(ui, t, Icon::Copy, app.tr("Copy text"), true).clicked();
        deliver = widgets::icon_text_button(ui, t, Icon::Signature, app.tr("Record delivery to a customer…"), true).clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::icon_button(ui, t, Icon::Close, app.tr("Close"), true, false).clicked();
        });
    });
    let mut picked = None;
    if let Some(mut picker) = app.privacy.deliver_to.take() {
        let mut keep = true;
        ui.add_space(4.0);
        widgets::card(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("privacy-deliver-to").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
                forms::customer_picker(app, ui, t, app.tr("Customer"), &mut picker, false);
            });
            if ui.button(app.tr("Cancel")).clicked() {
                keep = false;
            }
        });
        if let Some(c) = picker.id {
            picked = Some(c);
            keep = false;
        }
        if keep {
            app.privacy.deliver_to = Some(picker);
        }
    }
    ui.add_space(8.0);
    ui.add(egui::Label::new(RichText::new(&version).size(20.0).strong().color(t.ui_text)).wrap());
    if !title.is_empty() {
        ui.add(egui::Label::new(RichText::new(&title).color(t.muted)).wrap());
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if current {
            widgets::tone_chip(ui, t, app.tr("Current"), Tone::Accent);
        }
        if scheduled {
            widgets::tone_chip(ui, t, app.tr("Scheduled"), Tone::Warn);
        }
        ui.label(RichText::new(format!("#{id}")).small().color(t.muted));
    });
    ui.add_space(6.0);
    widgets::key_value(ui, t, app.tr("Effective from"), &format::long(&effective, italian));
    note(ui, &app.language.fill("Created by {who} on {date}", &[("who", &created_by), ("date", &format::long(&created_at, italian))]), t.muted);
    note(ui, &app.language.fill("{n} customers received it", &[("n", &delivered.to_string())]), t.muted);
    widgets::section(ui, t, app.tr("Text"));
    let mut text_copy = None;
    {
        let text = notice_of(app.session.data(), id).map(|n| n.text.as_str()).unwrap_or_default();
        if copy {
            text_copy = Some(text.to_string());
        }
        widgets::card(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt(("notice-text", id)).max_height(520.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).color(t.ui_text)).selectable(true).wrap());
            });
        });
    }
    if let Some(text) = text_copy {
        ui.ctx().copy_text(text);
        app.toast(app.tr("Copied").to_string());
    }
    if deliver {
        app.privacy.deliver_to = Some(Picker::default());
    }
    if close {
        app.privacy.selected_notice = None;
        app.privacy.deliver_to = None;
    }
    if let Some(c) = picked {
        open_delivery(app, c, Some(version));
    }
}

// ---------------------------------------------------------------------------------------------
// Audit log

/// A record an audit target opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Customer(u64),
    Device(u64),
    Intervention(u64),
    Notice(u64),
}

/// An audit target (`customer:3`) with the name it stands for, when the record is known.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    raw: String,
    label: String,
    open: Option<Open>,
}

/// An audit entry as the list shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuditRow {
    at: String,
    actor: String,
    command: String,
    targets: Vec<Target>,
    detail: String,
    /// The folded text the search matches: actor, command, target names, detail.
    search: String,
}

/// Splits `kind:id`.
fn target_parts(raw: &str) -> Option<(&str, u64)> {
    let (kind, id) = raw.split_once(':')?;
    Some((kind, id.trim().parse().ok()?))
}

/// The audit entries of an `audit.list` answer, their targets resolved to names in one pass
/// over each collection (the desk may hold 100k customers and 1M interventions).
fn audit_rows(data: &Data, v: &Value) -> Vec<AuditRow> {
    let Some(list) = v["entries"].as_array() else { return Vec::new() };
    let entries: Vec<&Value> = list.iter().take(limits::MAX_LIST).collect();
    let raw_targets = |e: &Value| -> Vec<String> {
        e["targets"].as_array().map(|ts| ts.iter().filter_map(Value::as_str).take(50).map(|s| format::clipped(s, 80)).collect()).unwrap_or_default()
    };
    let mut wanted: HashMap<&str, HashSet<u64>> = HashMap::new();
    let all: Vec<Vec<String>> = entries.iter().map(|e| raw_targets(e)).collect();
    for raw in all.iter().flatten() {
        if let Some((kind, id)) = target_parts(raw) {
            let kind = match kind {
                "customer" => "customer",
                "device" => "device",
                "intervention" => "intervention",
                "notice" => "notice",
                _ => continue,
            };
            wanted.entry(kind).or_default().insert(id);
        }
    }
    let empty = HashSet::new();
    let want = |kind: &str| wanted.get(kind).unwrap_or(&empty);
    let mut names: HashMap<(&str, u64), (String, Open)> = HashMap::new();
    for c in data.customers.iter().filter(|c| want("customer").contains(&c.id)) {
        names.insert(("customer", c.id), (c.name.clone(), Open::Customer(c.id)));
    }
    for d in data.devices.iter().filter(|d| want("device").contains(&d.id)) {
        names.insert(("device", d.id), (d.name.clone(), Open::Device(d.id)));
    }
    for i in data.interventions.iter().filter(|i| want("intervention").contains(&i.id)) {
        let label = if i.summary.is_empty() { format!("#{}", i.id) } else { format!("#{} {}", i.id, format::clipped(&i.summary, 40)) };
        names.insert(("intervention", i.id), (label, Open::Intervention(i.id)));
    }
    for n in data.notices.iter().filter(|n| want("notice").contains(&n.id)) {
        names.insert(("notice", n.id), (n.version.clone(), Open::Notice(n.id)));
    }
    entries
        .iter()
        .zip(all)
        .map(|(e, raws)| {
            let text = |k: &str| format::clipped(e[k].as_str().unwrap_or_default(), 400);
            let mut targets: Vec<Target> = raws
                .into_iter()
                .map(|raw| match target_parts(&raw).and_then(|(kind, id)| names.get(&(kind, id))) {
                    Some((label, open)) if !label.trim().is_empty() => Target { label: label.clone(), open: Some(*open), raw },
                    _ => Target { label: raw.clone(), open: None, raw },
                })
                .collect();
            // The records with a name first, the bare ids (privacy events…) after.
            targets.sort_by_key(|t| t.open.is_none());
            let (actor, command, detail) = (text("actor"), text("command"), text("detail"));
            let names = targets.iter().map(|t| t.label.as_str()).collect::<Vec<_>>().join(" ");
            let search = fold(&format!("{actor} {command} {names} {detail}"));
            AuditRow { at: text("at"), actor, command, targets, detail, search }
        })
        .collect()
}

/// The next `limit` of "Show more".
fn more_limit(limit: usize) -> usize {
    limit.saturating_add(AUDIT_STEP).min(limits::MAX_LIST)
}

/// The filter chips of the Audit log: customer, technician, command, and the record a card's
/// link set.
fn audit_filters(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let mut f = app.privacy.audit_filter.clone();
    // Customer: a picker in a popup.
    let name = f.customer.map(|c| crate::customer_name(app.data(), c));
    let (resp, cleared) = match &name {
        Some(n) => pill(ui, t, Some(Icon::Customer), &app.language.fill("Customer: {name}", &[("name", &format::clipped(n, 28))]), true, true),
        None => pill(ui, t, Some(Icon::Customer), app.tr("Customer"), false, false),
    };
    if cleared {
        f.customer = None;
    }
    let mut typed = std::mem::take(&mut app.privacy.audit_customer);
    {
        let view: &DeskApp = app;
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(260.0);
            if ui.selectable_label(f.customer.is_none(), view.tr("Any customer")).clicked() {
                f.customer = None;
                ui.close();
            }
            let r = ui.add(egui::TextEdit::singleline(&mut typed).hint_text(view.tr("Type a name…")).desired_width(240.0));
            if ui.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            let fq = fold(typed.trim());
            if fq.chars().count() >= 2 {
                let rid = derived::rustdesk_query(&typed);
                let data = view.data();
                let hits: Vec<(u64, String)> = data
                    .customers
                    .iter()
                    .filter(|c| derived::customer_filter(data, c, &fq, rid.as_deref()))
                    .take(8)
                    .map(|c| (c.id, c.name.clone()))
                    .collect();
                for (id, name) in hits {
                    if ui.selectable_label(f.customer == Some(id), name).clicked() {
                        f.customer = Some(id);
                        ui.close();
                    }
                }
            }
        });
    }
    app.privacy.audit_customer = typed;
    // Technician.
    let (resp, cleared) = if f.actor.is_empty() {
        pill(ui, t, Some(Icon::Account), app.tr("Technician"), false, false)
    } else {
        pill(ui, t, Some(Icon::Account), &app.language.fill("Technician: {name}", &[("name", &f.actor)]), true, true)
    };
    if cleared {
        f.actor.clear();
    }
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(180.0);
        if ui.selectable_label(f.actor.is_empty(), app.tr("Every technician")).clicked() {
            f.actor.clear();
            ui.close();
        }
        for name in &app.derived.technicians {
            if ui.selectable_label(f.actor == *name, name).clicked() {
                f.actor = name.clone();
                ui.close();
            }
        }
    });
    // Command family.
    let (resp, cleared) = if f.command.is_empty() {
        pill(ui, t, Some(Icon::Keyboard), app.tr("Command"), false, false)
    } else {
        pill(ui, t, Some(Icon::Keyboard), &app.language.fill("Command: {name}", &[("name", &f.command)]), true, true)
    };
    if cleared {
        f.command.clear();
    }
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(180.0);
        if ui.selectable_label(f.command.is_empty(), app.tr("Any command")).clicked() {
            f.command.clear();
            ui.close();
        }
        for prefix in COMMAND_PREFIXES {
            if ui.selectable_label(f.command == *prefix, RichText::new(*prefix).font(FontId::monospace(13.0))).clicked() {
                f.command = (*prefix).to_string();
                ui.close();
            }
        }
    });
    // The record a card's "Audit entries" link set.
    if let Some(d) = f.device {
        let name = crate::device_name(app.data(), Some(d));
        let shown = if name.is_empty() { format!("device:{d}") } else { format::clipped(&name, 28) };
        if pill(ui, t, Some(Icon::Device), &shown, true, true).1 {
            f.device = None;
        }
    }
    if let Some(i) = f.intervention
        && pill(ui, t, Some(Icon::Interventions), &format!("#{i}"), true, true).1
    {
        f.intervention = None;
    }
    if f != app.privacy.audit_filter {
        f.limit = AUDIT_LIMIT;
        app.privacy.audit_filter = f;
    }
}

/// The x of each column of the audit list: when, technician, command, targets, detail.
fn audit_columns(rect: Rect, has_detail: bool) -> (f32, f32, f32, f32, f32) {
    let when = rect.left() + 14.0;
    let actor = when + 138.0;
    let command = actor + 118.0;
    let targets = command + 168.0;
    let right = rect.right() - 12.0;
    let detail = if has_detail { right - ((right - targets) * 0.38).max(90.0) } else { right };
    (when, actor, command, targets, detail)
}

fn audit_list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    hint_bar(ui, t, app.tr("Only identifiers are logged, never personal data."));
    if !app.privacy.audit.error.is_empty() {
        let e = app.language.tr_message(&app.privacy.audit.error);
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(14, 2)).show(ui, |ui| widgets::error_line(ui, t, &e));
    }
    let fq = app.query();
    let shown: Vec<usize> =
        app.privacy.audit_rows.iter().enumerate().filter(|(_, r)| fq.is_empty() || r.search.contains(&fq)).map(|(i, _)| i).collect();
    if shown.is_empty() {
        if app.privacy.audit.loading() && app.privacy.audit.value.is_none() {
            widgets::empty_state(ui, t, Icon::Sync, app.tr("Loading…"), "");
        } else {
            widgets::empty_state(ui, t, Icon::Audit, app.tr("No audit entries"), "");
        }
        return;
    }
    // Column headings.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let (c_when, c_actor, c_command, c_targets, c_detail) = audit_columns(rect, true);
    let p = ui.painter();
    p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.border));
    for (x, label) in [(c_when, "When"), (c_actor, "Technician"), (c_command, "Command"), (c_targets, "Targets"), (c_detail, "Detail")] {
        p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, app.tr(label), FontId::proportional(11.5), t.muted);
    }
    let limit = app.privacy.audit_filter.limit;
    let more = app.privacy.audit_rows.len() >= limit && limit < limits::MAX_LIST;
    let n = shown.len().saturating_add(usize::from(more));
    ui.spacing_mut().item_spacing.y = 0.0;
    let rows = std::mem::take(&mut app.privacy.audit_rows);
    let mut open = None;
    let mut show_more = false;
    egui::ScrollArea::vertical().id_salt("privacy-audit").auto_shrink([false, false]).show_rows(ui, ROW_H, n, |ui, range| {
        for i in range {
            match shown.get(i).and_then(|k| rows.get(*k)) {
                Some(row) => {
                    if let Some(o) = audit_row(app, ui, t, row, i) {
                        open = Some(o);
                    }
                }
                None => {
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width(), ROW_H),
                        Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            if ui.button(app.tr("Show more")).clicked() {
                                show_more = true;
                            }
                        },
                    );
                }
            }
        }
    });
    app.privacy.audit_rows = rows;
    if show_more {
        app.privacy.audit_filter.limit = more_limit(limit);
    }
    match open {
        Some(Open::Customer(c)) => app.open_customer(c),
        Some(Open::Device(d)) => app.open_device(d),
        Some(Open::Intervention(i)) => app.open_intervention(i),
        Some(Open::Notice(n)) => {
            app.privacy.tab = Tab::Notices;
            app.privacy.selected_notice = Some(n);
        }
        None => {}
    }
}

/// An audit entry: when, who (avatar and name), the command in monospace, the targets as chips
/// (click opens the record), the detail; answers the record clicked.
fn audit_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, row: &AuditRow, i: usize) -> Option<Open> {
    let (rect, _) = widgets::row(ui, t, ROW_H, false);
    let (c_when, c_actor, c_command, c_targets, c_detail) = audit_columns(rect, !row.detail.is_empty());
    let p = ui.painter().clone();
    let cy = rect.center().y;
    let g = widgets::truncate(&p, &format::long(&row.at, app.italian()), FontId::proportional(12.5), t.muted, c_actor - c_when - 10.0);
    p.galley(pos2(c_when, cy - g.size().y / 2.0), g, t.muted);
    let who = if row.actor.is_empty() { "?" } else { row.actor.as_str() };
    let av = pos2(c_actor + 10.0, cy);
    p.circle_filled(av, 10.0, chrome::color_for(who));
    p.text(av, Align2::CENTER_CENTER, format::initials(who), FontId::proportional(9.0), t.on_accent);
    let g = widgets::truncate(&p, who, FontId::proportional(13.0), t.ui_text, c_command - c_actor - 36.0);
    p.galley(pos2(c_actor + 26.0, cy - g.size().y / 2.0), g, t.ui_text);
    let g = widgets::truncate(&p, &row.command, FontId::monospace(13.0), t.ui_text, c_targets - c_command - 10.0);
    p.galley(pos2(c_command, cy - g.size().y / 2.0), g, t.ui_text);
    let (fill, color) = widgets::tone_colors(t, Tone::Neutral);
    let end = c_detail - 10.0;
    let mut x = c_targets;
    let mut open = None;
    let mut hidden = 0usize;
    for (k, target) in row.targets.iter().enumerate() {
        let g =
            p.layout_no_wrap(format::clipped(&target.label, 28), FontId::proportional(11.5), if target.open.is_some() { t.ui_text } else { color });
        let w = g.size().x + 14.0;
        if x + w > end - 24.0 && k + 1 < row.targets.len() || x + w > end {
            hidden = row.targets.len().saturating_sub(k);
            break;
        }
        let chip = Rect::from_min_size(pos2(x, cy - 9.0), vec2(w, 18.0));
        let sense = if target.open.is_some() { Sense::click() } else { Sense::hover() };
        let hr = ui.interact(chip, ui.id().with(("privacy-audit-target", i, k)), sense);
        p.rect_filled(chip, 9.0, if hr.hovered() && target.open.is_some() { t.active } else { fill });
        p.rect_stroke(chip, 9.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.galley(pos2(chip.left() + 7.0, cy - g.size().y / 2.0), g, color);
        if hr.on_hover_text(&target.raw).clicked() {
            open = target.open;
        }
        x = chip.right() + 4.0;
    }
    if hidden > 0 {
        p.text(pos2(x + 2.0, cy), Align2::LEFT_CENTER, format!("+{hidden}"), FontId::proportional(11.5), t.muted);
    }
    if !row.detail.is_empty() {
        let g = widgets::truncate(&p, &row.detail, FontId::proportional(12.5), t.muted, rect.right() - 12.0 - c_detail);
        let r = Rect::from_min_size(pos2(c_detail, cy - g.size().y / 2.0), g.size());
        p.galley(r.min, g, t.muted);
        ui.interact(r, ui.id().with(("privacy-audit-detail", i)), Sense::hover()).on_hover_text(&row.detail);
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    /// The demo app in English (the demo desk's language is Italian).
    fn demo() -> DeskApp {
        let mut app = DeskApp::demo();
        app.language = Language::En;
        app
    }

    /// Renders one frame of the app at `w`×900 (no window, no GPU).
    fn frame_at(app: &mut DeskApp, w: f32) {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(w, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    fn frame(app: &mut DeskApp) {
        frame_at(app, 1440.0);
    }

    fn toast(app: &DeskApp) -> String {
        app.toast.as_ref().map(|(t, _)| t.clone()).unwrap_or_default()
    }

    #[test]
    fn every_tab_form_and_strip_renders_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = demo();
                app.language = lang;
                app.dark = dark;
                app.go(Screen::Privacy);
                for tab in [Tab::Review, Tab::Notices, Tab::Audit] {
                    app.privacy.tab = tab;
                    frame(&mut app);
                    frame(&mut app);
                }
                assert!(app.privacy.audit.value.is_some(), "the audit log loaded");
                app.privacy.tab = Tab::Notices;
                app.privacy.selected_notice = Some(1);
                frame(&mut app);
                app.privacy.deliver_to = Some(Picker { text: "corvo".into(), id: None });
                frame(&mut app);
                open_delivery(&mut app, 3, None);
                frame(&mut app);
                assert!(matches!(app.privacy.form, Some(Form::Delivery(_))));
                open_consent(&mut app, 1, &["remote_assistance"]);
                frame(&mut app);
                open_withdraw(&mut app, 1, "remote_assistance");
                frame(&mut app);
                open_notice(&mut app);
                frame(&mut app);
                frame_at(&mut app, 900.0);
                // In the customer card, with the history open and the consent form after it.
                assert!(close(&mut app));
                app.open_customer(1);
                app.privacy.history_open = Some(1);
                app.privacy.consent_warnings = Some((1, vec!["a warning".into()]));
                frame(&mut app);
                open_consent(&mut app, 1, &[]);
                frame(&mut app);
                assert_eq!(form_customer(&app), Some(1));
                assert_eq!(app.screen, Screen::Customers);
                // From the Devices screen the form stays there.
                app.open_device(4);
                open_consent(&mut app, 2, &["remote_assistance"]);
                frame(&mut app);
                assert_eq!(app.screen, Screen::Devices);
                // From Reports the customer's card opens.
                app.go(Screen::Reports);
                open_delivery(&mut app, 2, None);
                assert_eq!(app.screen, Screen::Customers);
                assert_eq!(app.selected_customer, Some(2));
                frame(&mut app);
            }
        }
    }

    #[test]
    fn a_consent_is_recorded_and_the_form_closes() {
        let mut app = demo();
        open_consent(&mut app, 3, &["remote_assistance"]);
        frame(&mut app);
        let Some(Form::Consent(f)) = app.privacy.form.as_mut() else { panic!("the consent form is open") };
        assert!(f.purposes.iter().any(|(p, on)| p == "remote_assistance" && *on));
        assert!(f.purposes.iter().any(|(p, on)| p == "unattended_access" && !*on), "Elio has no unattended PC");
        assert!(!f.with_notice, "Elio received the notice in effect");
        assert_eq!(f.signer, "Elio Brina", "a person signs for themself");
        assert_eq!(f.version, "2026-01");
        f.evidence = "Modulo firmato 2026/12".into();
        submit(&mut app);
        assert!(app.privacy.busy.is_some());
        frame(&mut app);
        assert!(app.privacy.form.is_none(), "{}", app.privacy.error);
        assert!(app.privacy.busy.is_none());
        assert_eq!(toast(&app), "Consent recorded (1 events)");
        assert_eq!(app.derived.gate(3).0, PurposeState::Given);
        assert!(app.derived.findings.iter().all(|f| f.customer != Some(3)), "the review settled");
    }

    #[test]
    fn a_consent_with_the_notice_and_a_warning() {
        let mut app = demo();
        app.run("customer.add", json!({"name": "Nuovo Studio", "contacts": [{"name": "Anna Neri"}]}));
        frame(&mut app);
        let id = app.data().customers.iter().find(|c| c.name == "Nuovo Studio").map(|c| c.id).unwrap_or_default();
        open_consent(&mut app, id, &["remote_assistance", "Ricerche di mercato"]);
        let Some(Form::Consent(f)) = app.privacy.form.as_mut() else { panic!("the consent form is open") };
        assert!(f.with_notice, "never delivered: the notice goes with the consent");
        assert_eq!(f.signer, "Anna Neri", "the first contact signs");
        assert_eq!(f.other, "ricerche_di_mercato", "a purpose not known yet goes to Other");
        f.method = Method::Verbal;
        assert_eq!(f.request(app.session.data(), app.session.now()).err(), Some("Required"), "verbal needs a note");
        f.note = "detto al telefono".into();
        f.expires = "2020-01-01 10:00".into();
        assert_eq!(f.request(app.session.data(), app.session.now()).err(), Some("The expiry is before the date"));
        f.expires.clear();
        submit(&mut app);
        frame(&mut app);
        assert!(app.privacy.form.is_none(), "{}", app.privacy.error);
        assert_eq!(toast(&app), "Consent recorded (3 events)", "the delivery and two purposes");
        assert!(app.privacy.consent_warnings.is_none());
        assert!(matches!(app.derived.customer(id).map(|c| &c.notice), Some(NoticeState::UpToDate { .. })));
        assert!(app.derived.customer(id).is_some_and(|c| c.purposes.iter().any(|p| p.id == "ricerche_di_mercato" && p.state == PurposeState::Given)));
        // Without the notice the engine warns: the warning reaches the strip.
        open_consent(&mut app, 2, &["marketing"]);
        frame(&mut app);
        if let Some(Form::Consent(f)) = app.privacy.form.as_mut() {
            f.evidence = "mail".into();
        }
        let mut data_less = demo();
        data_less.run("customer.add", json!({"name": "Senza Informativa"}));
        frame(&mut data_less);
        let other = data_less.data().customers.iter().find(|c| c.name == "Senza Informativa").map(|c| c.id).unwrap_or_default();
        open_consent(&mut data_less, other, &["marketing"]);
        if let Some(Form::Consent(f)) = data_less.privacy.form.as_mut() {
            f.with_notice = false;
            f.signer = "Ugo".into();
            f.evidence = "modulo".into();
        }
        submit(&mut data_less);
        frame(&mut data_less);
        assert!(data_less.privacy.consent_warnings.as_ref().is_some_and(|(c, w)| *c == other && w.len() == 1));
        data_less.open_customer(other);
        frame(&mut data_less);
    }

    #[test]
    fn a_refused_notice_keeps_the_form_open_with_the_error() {
        let mut app = demo();
        app.go(Screen::Privacy);
        open_notice(&mut app);
        assert_eq!(app.privacy.tab, Tab::Notices);
        let Some(Form::Notice(f)) = app.privacy.form.as_mut() else { panic!("the notice form is open") };
        assert_eq!(f.version.len(), 7, "YYYY-MM of today");
        assert_eq!(f.title, "Privacy notice — technical support");
        f.version = "2026-01".into();
        submit(&mut app);
        assert_eq!(app.privacy.error, "This version exists");
        assert!(app.privacy.busy.is_none());
        // The engine refuses it too (past the local check).
        let job = app.run_as("notice.add", json!({"version": "2026-01"}), crate::Intent::Privacy(Intent::NoticeAdded));
        app.privacy.busy = Some(job);
        frame(&mut app);
        assert!(app.privacy.form.is_some(), "the form stays open");
        assert!(app.privacy.error.contains("2026-01"), "{}", app.privacy.error);
        // A new version is added and selected.
        if let Some(Form::Notice(f)) = app.privacy.form.as_mut() {
            f.version = "2026-10".into();
            f.text = "Testo".into();
        }
        submit(&mut app);
        frame(&mut app);
        assert!(app.privacy.form.is_none(), "{}", app.privacy.error);
        assert_eq!(toast(&app), "Notice version added");
        assert_eq!(app.privacy.selected_notice, Some(2));
        frame(&mut app);
        // Esc closes the pane, then nothing.
        assert!(close(&mut app));
        assert!(!close(&mut app));
    }

    #[test]
    fn a_withdrawal_names_the_unattended_pcs() {
        let mut app = demo();
        open_withdraw(&mut app, 1, "unattended_access");
        let Some(Form::Withdraw(f)) = app.privacy.form.as_ref() else { panic!("the withdrawal form is open") };
        assert_eq!(f.purposes, vec![("remote_assistance".to_string(), false), ("unattended_access".to_string(), true)], "only what is given");
        submit(&mut app);
        frame(&mut app);
        assert!(app.privacy.form.is_none(), "{}", app.privacy.error);
        assert_eq!(toast(&app), "Withdrawal recorded");
        assert_eq!(app.customers.unattended_banner, Some((1, vec![1, 2])));
        assert_eq!(app.derived.purpose(1, "unattended_access"), PurposeState::Withdrawn);
        // A purpose never given can still be withdrawn when named.
        open_withdraw(&mut app, 2, "marketing");
        assert!(matches!(&app.privacy.form, Some(Form::Withdraw(f)) if f.purposes.contains(&("marketing".to_string(), true))));
    }

    #[test]
    fn a_delivery_needs_a_notice_and_a_note_when_verbal() {
        let now = time::now();
        let f = DeliveryForm { customer: 1, version: "x".into(), at: "09:30".into(), ..DeliveryForm::default() };
        assert_eq!(f.request(&Data::new(), now).err(), Some("Add the privacy notice first"));
        let mut app = demo();
        open_delivery(&mut app, 2, Some("nope".into()));
        let Some(Form::Delivery(f)) = app.privacy.form.as_mut() else { panic!("the delivery form is open") };
        assert_eq!(f.version, "2026-01", "an unknown version falls back to the current one");
        assert!(f.signer.is_empty(), "a company without contacts has no default signer");
        f.method = Method::Verbal;
        assert_eq!(f.request(app.session.data(), now).err(), Some("Required"));
        f.note = "al telefono, sentito da Riccardo".into();
        f.at = "ieri alle tre".into();
        assert_eq!(f.request(app.session.data(), now).err(), Some(NOT_A_TIME));
        f.at = "13/10/2026 09:30".into();
        let (command, params, _) = f.request(app.session.data(), now).unwrap_or(("", Value::Null, Intent::Audit));
        assert_eq!(command, "privacy.notice");
        assert!(params["at"].as_str().is_some_and(|s| s.ends_with('Z')), "a stamp is sent, not the text");
        submit(&mut app);
        frame(&mut app);
        assert_eq!(toast(&app), "Notice delivery recorded");
        // On an empty desk the form only offers to add the notice.
        let mut empty = DeskApp::new(Box::new(crate::Session::new(remotecraft_engine::Desk::in_memory(Data::new()))));
        empty.run("customer.add", json!({"name": "Primo"}));
        frame(&mut empty);
        open_delivery(&mut empty, 1, None);
        empty.go(Screen::Customers);
        frame(&mut empty);
        assert!(matches!(&empty.privacy.form, Some(Form::Delivery(f)) if f.version.is_empty()));
    }

    #[test]
    fn consent_validation() {
        let now = time::now();
        let data = remotecraft_engine::core::mock::demo(now);
        let mut f = ConsentForm { customer: 1, at: "09:30".into(), signer: "Marta".into(), evidence: "modulo".into(), ..ConsentForm::default() };
        assert_eq!(f.request(&data, now).err(), Some("Pick at least one purpose"));
        f.other = "!!!".into();
        assert_eq!(f.request(&data, now).err(), Some("Letters, digits and _ only"));
        f.other = "a".repeat(10_000);
        assert_eq!(f.request(&data, now).err(), Some("Letters, digits and _ only"));
        f.other = "Assistenza remota".into();
        assert_eq!(f.chosen(), Ok(vec!["remote_assistance".to_string()]));
        f.signer.clear();
        assert_eq!(f.request(&data, now).err(), Some("Required"));
        f.signer = "Marta".into();
        f.at.clear();
        assert_eq!(f.request(&data, now).err(), Some("Required"));
        f.at = "09:30".into();
        f.with_notice = true;
        assert_eq!(f.request(&Data::new(), now).err(), Some("Add the privacy notice first"));
        let (_, params, intent) = f.request(&data, now).unwrap_or(("", Value::Null, Intent::Audit));
        assert_eq!(intent, Intent::Consented(1));
        assert_eq!(params["with_notice"], true);
        assert!(params.get("expires_at").is_none());
        let w = WithdrawForm { customer: 1, at: "09:30".into(), ..WithdrawForm::default() };
        assert_eq!(w.request(now).err(), Some("Pick at least one purpose"));
        let n = NoticeForm { version: " ".into(), effective: "09:30".into(), ..NoticeForm::default() };
        assert_eq!(n.request(&data, now).err(), Some("Required"));
    }

    #[test]
    fn the_review_labels_and_settles_every_issue() {
        for issue in [
            "no_notice",
            "notice_missing",
            "notice_outdated",
            "remote_assistance_missing",
            "remote_assistance_expired",
            "remote_assistance_withdrawn",
            "unattended_access_missing",
            "unattended_access_expired",
            "unattended_access_withdrawn",
            "retention_due",
            "timer_running",
        ] {
            let label = finding_label(issue);
            assert_ne!(label, "Privacy", "{issue}");
            assert_ne!(Language::It.tr(label), label, "{issue} is translated");
            let f = Finding { issue: issue.into(), customer: Some(1), intervention: Some(1), technician: String::new(), detail: String::new() };
            assert!(finding_act(&f).is_some(), "{issue} has an action");
        }
        assert_eq!(finding_label("something new"), "Privacy");
        let mut app = demo();
        frame(&mut app);
        let rows = review_rows(app.session.data(), &app.derived.findings, "");
        assert_eq!(rows.first(), Some(&ReviewRow::Header(3, "Elio Brina".into())));
        assert!(matches!(rows.get(1), Some(ReviewRow::Finding { grouped: true, .. })));
        assert!(review_rows(app.session.data(), &app.derived.findings, "corvo").is_empty(), "the search keeps matching customers");
        let elio = app.derived.findings.iter().find(|f| f.customer == Some(3)).cloned();
        assert_eq!(elio.as_ref().and_then(finding_act).map(|a| a.0), Some(FindingAct::Consent(3, "remote_assistance")));
        let index = detail_index(app.session.data(), &app.derived.findings, app.now());
        assert_eq!(elio.map(|f| finding_detail(&app, &f, &index)).unwrap_or_default(), "Notebook");
        app.go(Screen::Privacy);
        frame(&mut app);
        assert_eq!(app.privacy.review_details.len(), app.derived.findings.len());
        // The hover action opens the consent form in the pane of this screen.
        app.go(Screen::Privacy);
        run_finding(&mut app, FindingAct::Consent(3, "remote_assistance"));
        assert_eq!(form_customer(&app), Some(3));
        assert_eq!(app.screen, Screen::Privacy);
        frame(&mut app);
        // An empty desk: the missing notice comes first and adds it.
        let empty = DeskApp::new(Box::new(crate::Session::new(remotecraft_engine::Desk::in_memory(Data::new()))));
        let mut empty = empty;
        frame(&mut empty);
        let rows = review_rows(empty.session.data(), &empty.derived.findings, "");
        assert!(matches!(rows.first(), Some(ReviewRow::Finding { grouped: false, .. })));
        run_finding(&mut empty, FindingAct::AddNotice);
        assert!(matches!(empty.privacy.form, Some(Form::Notice(_))));
        assert_eq!(empty.screen, Screen::Privacy);
        frame(&mut empty);
        // Cancelling the form on an empty Notices tab does not reopen it.
        assert!(close(&mut empty));
        frame(&mut empty);
        assert!(empty.privacy.form.is_none());
    }

    #[test]
    fn the_audit_log_loads_resolves_targets_and_grows() {
        let mut app = demo();
        app.go(Screen::Privacy);
        app.privacy.tab = Tab::Audit;
        app.run("privacy.notice", json!({"customer": 2}));
        frame(&mut app);
        frame(&mut app);
        frame(&mut app);
        let first = app.privacy.audit_rows.first().cloned().unwrap_or_default();
        assert_eq!(first.command, "privacy.notice");
        assert!(first.targets.iter().any(|t| t.label == "Ferramenta Corvo" && t.open == Some(Open::Customer(2))));
        assert!(first.targets.iter().any(|t| t.raw.starts_with("privacy:") && t.open.is_none()), "unknown kinds stay raw");
        assert_eq!(app.privacy.audit_filter.params()["limit"], 100);
        // A filter re-runs the command.
        app.privacy.audit_filter.command = "customer".into();
        frame(&mut app);
        frame(&mut app);
        assert!(app.privacy.audit_rows.is_empty());
        app.privacy.audit_filter = AuditFilter { customer: Some(2), actor: "riccardo".into(), ..AuditFilter::default() };
        let p = app.privacy.audit_filter.params();
        assert_eq!((p["customer"].as_u64(), p["actor"].as_str()), (Some(2), Some("riccardo")));
        frame(&mut app);
        frame(&mut app);
        assert_eq!(app.privacy.audit_rows.len(), 1);
        assert_eq!(more_limit(100), 300);
        assert_eq!(more_limit(900), limits::MAX_LIST);
        open_audit(&mut app, None, Some(4), Some(5));
        frame(&mut app);
        frame(&mut app);
        assert_eq!(app.privacy.audit_filter.device, Some(4));
    }

    #[test]
    fn forms_do_not_open_for_unknown_or_erased_customers() {
        let mut app = demo();
        open_consent(&mut app, 99, &["remote_assistance"]);
        open_delivery(&mut app, 99, None);
        open_withdraw(&mut app, 99, "remote_assistance");
        assert!(app.privacy.form.is_none());
        app.run("gdpr.erase", json!({"customer": 3, "confirm": true, "reason": "richiesta"}));
        frame(&mut app);
        open_delivery(&mut app, 3, None);
        assert!(app.privacy.form.is_none());
        // The erased customer's card shows a read-only strip.
        app.open_customer(3);
        app.privacy.history_open = Some(3);
        frame(&mut app);
        // A form whose customer disappears closes by itself.
        open_consent(&mut app, 2, &[]);
        app.privacy.form = Some(Form::Consent(ConsentForm { customer: 77, ..ConsentForm::default() }));
        frame(&mut app);
        assert!(app.privacy.form.is_none());
    }

    #[test]
    fn hostile_answers_and_texts_do_not_crash() {
        let data = remotecraft_engine::core::mock::demo(time::now());
        let rows = audit_rows(
            &data,
            &json!({"entries": [1, "x", null, {"targets": [5, null, "customer:abc", "device:99999", ":", "notice:1", "customer:1"], "at": 7}]}),
        );
        assert_eq!(rows.len(), 4);
        let last = rows.last().cloned().unwrap_or_default();
        assert_eq!(last.targets.len(), 5);
        assert!(last.targets.iter().any(|t| t.label == "2026-01" && t.open == Some(Open::Notice(1))));
        assert!(last.targets.iter().any(|t| t.label == "device:99999" && t.open.is_none()));
        assert!(audit_rows(&data, &json!({"entries": "nope"})).is_empty());
        assert!(audit_rows(&data, &Value::Null).is_empty());
        assert_eq!(warned_devices(&json!({"warnings": ["device 7 `PC` has unattended access", "device x", 3, "other"]})), vec![7]);
        assert!(warned_devices(&json!({"warnings": "x"})).is_empty());
        assert_eq!(target_parts("customer:12"), Some(("customer", 12)));
        assert_eq!(target_parts("customer:"), None);
        assert_eq!(purpose_title("remote_assistance", true), "Assistenza remota");
        assert_eq!(purpose_title("custom_one", false), "custom_one");
        assert!(event_of(&data, 4).is_some_and(|e| e.customer_id == 2));
        assert!(event_of(&data, 999).is_none());
        // A huge notice text renders and copies.
        let mut app = demo();
        app.go(Screen::Privacy);
        open_notice(&mut app);
        if let Some(Form::Notice(f)) = app.privacy.form.as_mut() {
            f.version = "x".repeat(5_000);
            f.text = "riga\n".repeat(20_000);
        }
        frame(&mut app);
        submit(&mut app);
        frame(&mut app);
        frame(&mut app);
        assert!(app.privacy.selected_notice.is_some(), "{}", app.privacy.error);
    }

    #[test]
    fn labels_are_translated_once_and_the_same_everywhere() {
        let mut seen: Vec<&str> = Vec::new();
        for (en, it) in LABELS_IT {
            assert!(!en.is_empty() && !it.is_empty(), "{en}");
            assert!(!seen.contains(en), "duplicate {en}");
            seen.push(en);
            assert!(!crate::i18n::LABELS.contains(en), "{en} is already shared");
            assert_eq!(Language::It.tr(en), *it, "{en} is translated elsewhere differently");
        }
        let app = demo();
        assert!(outline(&app).contains("[replace with your text]"));
        let mut it = demo();
        it.language = Language::It;
        assert!(outline(&it).starts_with("1. Titolare del trattamento"));
        for label in ["Purposes", "Customer", "Copied", "Close", "Remote assistance consent missing"] {
            assert_ne!(Language::It.tr(label), label, "{label} has a shared translation");
        }
    }
}
