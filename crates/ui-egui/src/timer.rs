//! The Work card and the running-timer chip: MailCraft's compose card, for work instead of
//! mail. One card docked at the bottom right (dragged by its title bar, minimised to the bar)
//! starts a timer on a customer or a PC, follows the running intervention while the technician
//! types the summary and the notes, stops it with notes and times, and logs past work. A pill
//! in the top bar ticks while one of the technician's interventions runs. Everything goes
//! through `intervention.start`, `intervention.stop`, `intervention.add` and `privacy.consent`;
//! the card never touches the data itself and reads the elapsed time from the desk's clock.

use egui::{Align, FontId, Key, Layout, Modifiers, Rect, Response, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::core::chrono::{DateTime, Duration, Utc};
use crate::core::privacy::{self, PurposeState};
use crate::core::{self, Data, rustdesk, time};
use crate::engine::views;
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone, icon_button};
use crate::{Completed, DeskApp, Request, format};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("Start timer", "Avvia timer"),
    ("Log past work", "Registra lavoro svolto"),
    ("Minimize", "Riduci"),
    ("Choose a customer or a PC", "Scegli un cliente o un PC"),
    ("Type a name or a RustDesk ID", "Scrivi un nome o un ID RustDesk"),
    ("No match", "Nessun risultato"),
    ("Change", "Cambia"),
    // `Kind` and `Technician` are declared by the Reports screen (one owner per label).
    ("Remote", "Remoto"),
    ("On site", "In sede"),
    ("Phone", "Telefono"),
    ("Lab", "Laboratorio"),
    ("Other", "Altro"),
    ("Summary", "Oggetto"),
    ("What is the request?", "Qual è la richiesta?"),
    ("Summary is required", "L'oggetto è obbligatorio"),
    ("Notes", "Note"),
    ("What was done, found, left to do", "Cosa è stato fatto, trovato, lasciato da fare"),
    ("Reference", "Riferimento"),
    ("Ticket or job number", "Numero di ticket o commessa"),
    ("Billable", "Fatturabile"),
    ("Started at", "Inizio"),
    ("Ended at", "Fine"),
    ("Minutes", "Minuti"),
    ("Leave empty to use the elapsed time", "Lascia vuoto per usare il tempo trascorso"),
    ("Minutes must be a number", "I minuti devono essere un numero"),
    ("Give the end time or the minutes", "Indica la fine o i minuti"),
    ("The end is before the start", "La fine è prima dell'inizio"),
    ("Not a time: use 2026-10-13 09:30, 13/10/2026 09:30 or 09:30", "Non è un orario: usa 2026-10-13 09:30, 13/10/2026 09:30 o 09:30"),
    ("now", "adesso"),
    ("Now", "Adesso"),
    ("−15 min", "−15 min"),
    ("−30 min", "−30 min"),
    ("−1 h", "−1 h"),
    ("Start", "Avvia"),
    ("Stop", "Ferma"),
    ("Connect again", "Collegati di nuovo"),
    ("Timer started", "Timer avviato"),
    ("Stopped · {duration} · {customer}", "Chiuso · {duration} · {customer}"),
    ("Intervention saved", "Intervento salvato"),
    ("Remote-assistance consent is {state}", "Il consenso all'assistenza remota è {state}"),
    ("missing", "mancante"),
    ("expired", "scaduto"),
    ("withdrawn", "revocato"),
    ("Reason to proceed without consent", "Motivo per procedere senza consenso"),
    ("The person agreed on the phone", "La persona ha acconsentito al telefono"),
    ("Start anyway", "Avvia comunque"),
    ("Log anyway", "Registra comunque"),
    ("Record consent first", "Registra prima il consenso"),
    ("Who agreed", "Chi acconsente"),
    ("Evidence or note", "Prova o nota"),
    ("Record verbal consent and start", "Registra il consenso verbale e avvia"),
];

/// The card's width.
const CARD_W: f32 = 420.0;
/// The width of a field's label in the card.
const LABEL_W: f32 = 96.0;
/// How many PCs and how many customers the target picker lists.
const PICKER_ROWS: usize = 5;
/// The kinds of work: the engine's word, the label and the glyph.
const KINDS: [(&str, &str, Icon); 5] = [
    ("remote", "Remote", Icon::Remote),
    ("onsite", "On site", Icon::Onsite),
    ("phone", "Phone", Icon::Phone),
    ("lab", "Lab", Icon::Lab),
    ("other", "Other", Icon::More),
];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `intervention.start`, from the card or straight from a row; `opened` says the RustDesk
    /// link was already opened in the click's frame.
    Start { customer: Option<u64>, device: Option<u64>, opened: bool },
    /// `intervention.stop {id}`.
    Stop { id: u64 },
    /// `intervention.add` from the Log mode.
    Log,
    /// `privacy.consent` recorded from the refusal panel: the refused start is sent again.
    Consent,
}

/// What the card is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Pick the target and start the timer.
    #[default]
    Start,
    /// Follow a running intervention: summary and notes typed while working.
    Running,
    /// Stop it: notes, end time, minutes.
    Stop,
    /// Log work done earlier.
    Log,
}

/// Who the work is for: the customer, and the PC when known. `text` is what the picker holds
/// while nothing is picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub customer: Option<u64>,
    pub device: Option<u64>,
    pub text: String,
}

/// The running intervention the card follows, as the engine answered it: the mirror of a
/// remote desk may not hold it yet when the answer arrives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Running {
    pub id: u64,
    pub customer_id: u64,
    pub device_id: Option<u64>,
    pub started_at: String,
    pub customer_name: String,
    pub device_name: String,
}

/// The panel shown when a start was refused for lack of the remote-assistance consent: a
/// reason to go ahead anyway, or a verbal consent to record first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refusal {
    /// `missing`, `expired` or `withdrawn`, as the engine said.
    pub state: String,
    pub reason: String,
    /// The inline consent form is open.
    pub record: bool,
    pub signer: String,
    pub note: String,
    pub error: String,
}

/// The fields a text box of the card can be: each has a stable egui id, so the card knows
/// when one of its own boxes has the focus (Ctrl+Enter) and which one to focus next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Target,
    Summary,
    Notes,
    Reference,
    Started,
    Ended,
    Minutes,
    Technician,
    Reason,
    Signer,
    Note,
}

impl Field {
    const ALL: [Self; 11] = [
        Self::Target,
        Self::Summary,
        Self::Notes,
        Self::Reference,
        Self::Started,
        Self::Ended,
        Self::Minutes,
        Self::Technician,
        Self::Reason,
        Self::Signer,
        Self::Note,
    ];

    fn id(self) -> egui::Id {
        egui::Id::new(("work-card", self as u8))
    }
}

/// The card: one form shared by the modes, plus where it is and what it waits for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    pub mode: Mode,
    pub target: Target,
    /// `remote`, `onsite`, `phone`, `lab` or `other`.
    pub kind: String,
    /// The technician picked the kind: the default rule no longer applies.
    pub kind_touched: bool,
    pub summary: String,
    /// Notes to append when stopping (or the notes of past work).
    pub notes: String,
    pub reference: String,
    pub billable: bool,
    /// Form text; empty = now.
    pub started_at: String,
    /// Form text; empty = now (Stop) or not given (Log).
    pub ended_at: String,
    /// Form text; empty = the span between start and end.
    pub minutes: String,
    pub technician: String,
    /// The intervention followed in Running and Stop mode.
    pub running: Option<Running>,
    /// The engine's warnings of the last start, translated.
    pub warnings: Vec<String>,
    pub error: String,
    pub refusal: Option<Refusal>,
    /// A command of the card is in flight.
    pub busy: bool,
    pub minimized: bool,
    /// Where the technician dragged the card (from its bottom-right home).
    pub offset: egui::Vec2,
    /// The field to focus on the next quiet frame.
    pub refocus: Option<Field>,
}

impl Card {
    /// A card in `mode` for `target`, with the defaults of the desk.
    fn new(app: &DeskApp, mode: Mode, target: Target) -> Self {
        let now = app.now();
        Self {
            mode,
            kind: kind_default(app.data(), &target).to_string(),
            billable: true,
            started_at: if mode == Mode::Log { format::input(&time::stamp(now)) } else { String::new() },
            technician: app.session.actor().to_string(),
            refocus: Some(if target.customer.is_some() { Field::Summary } else { Field::Target }),
            target,
            ..Self::default()
        }
    }
}

/// This screen's own state: the card, when open.
#[derive(Default)]
pub struct State {
    pub card: Option<Card>,
}

/// What the card's buttons asked for this frame; applied once the card is written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// The main button of the mode (or Ctrl+Enter).
    Submit,
    /// Start (or log) with the reason given in the refusal panel.
    Anyway,
    /// Record the verbal consent from the refusal panel.
    Consent,
    /// Cancel the form (Start, Log) or go back to Running (Stop).
    Cancel,
    /// Close the card (the timer keeps running).
    Close,
    /// Go to Stop mode for this intervention.
    Stop(u64),
    /// Follow this intervention (several of mine running).
    Follow(u64),
    /// Open the RustDesk client on this PC again.
    Connect(u64),
}

// ---------------------------------------------------------------------------------------------
// Opening the card

/// Opens the card in Start mode for a customer and/or a device (the target picker when
/// neither is given). An open card is reused and un-minimised.
pub fn open_start(app: &mut DeskApp, customer: Option<u64>, device: Option<u64>) {
    open_form(app, Mode::Start, customer, device);
}

/// Opens the card in Log mode (work done earlier) for a customer and/or a device.
pub fn open_log(app: &mut DeskApp, customer: Option<u64>, device: Option<u64>) {
    open_form(app, Mode::Log, customer, device);
}

fn open_form(app: &mut DeskApp, mode: Mode, customer: Option<u64>, device: Option<u64>) {
    let customer = device.and_then(|d| app.data().device(d)).map(|d| d.customer_id).or(customer);
    let target = Target { customer, device, text: String::new() };
    let mut card = Card::new(app, mode, target);
    if let Some(old) = app.timer.card.take() {
        card.offset = old.offset;
    }
    app.timer.card = Some(card);
}

/// Opens the card on a running intervention of the actor: in Running mode, or straight in Stop
/// mode with the notes focused. Nothing happens when the mirror does not hold it.
pub fn open_running(app: &mut DeskApp, id: u64, stop: bool) {
    let d = app.data();
    let Some(i) = d.intervention(id) else { return };
    let running = Running {
        id,
        customer_id: i.customer_id,
        device_id: i.device_id,
        started_at: i.started_at.clone(),
        customer_name: crate::customer_name(d, i.customer_id),
        device_name: crate::device_name(d, i.device_id),
    };
    let (summary, billable) = (i.summary.clone(), i.billable);
    let target = Target { customer: Some(i.customer_id), device: i.device_id, text: String::new() };
    let same = app.timer.card.as_ref().is_some_and(|c| c.running.as_ref().is_some_and(|r| r.id == id));
    let fresh = Card::new(app, Mode::Running, target.clone());
    let mut card = app.timer.card.take().unwrap_or(fresh);
    if !same {
        card.summary = summary;
        card.notes.clear();
        card.billable = billable;
        card.warnings.clear();
    }
    card.running = Some(running);
    card.target = target;
    card.mode = if stop { Mode::Stop } else { Mode::Running };
    if stop {
        card.ended_at.clear();
        card.minutes.clear();
    }
    card.minimized = false;
    card.error.clear();
    card.refusal = None;
    card.refocus = Some(Field::Notes);
    app.timer.card = Some(card);
}

/// The fast path from a row: starts the timer on a PC at once (`intervention.start {device}`,
/// the summary is typed later in the card). When the mirror says the remote-assistance consent
/// is fine, the RustDesk link is opened in this very frame: a browser only opens it within the
/// click's gesture. The card opens when the engine answers.
pub fn start_now(app: &mut DeskApp, device: u64) {
    let Some(customer) = app.data().device(device).map(|d| d.customer_id) else { return };
    let opened = open_link_now(app, customer, device);
    app.run_as(
        "intervention.start",
        json!({"device": device}),
        crate::Intent::Timer(Intent::Start { customer: Some(customer), device: Some(device), opened }),
    );
}

/// Stops the actor's most recent running intervention right now, without notes (the `x`
/// shortcut): `intervention.stop {id}`. Nothing happens when none runs.
pub fn stop_mine(app: &mut DeskApp) {
    let Some(id) = mine_running(app.data(), app.session.actor()).first().map(|m| m.0) else { return };
    let ended = time::stamp(app.now());
    if let Some(c) = app.timer.card.as_mut()
        && c.running.as_ref().is_some_and(|r| r.id == id)
    {
        c.busy = true;
    }
    app.run_as("intervention.stop", json!({"id": id, "ended_at": ended}), crate::Intent::Timer(Intent::Stop { id }));
}

/// Closes an open card: minimises it when expanded, closes it when minimised (Escape, like
/// MailCraft's compose card). Answers whether a card was open.
pub fn close(app: &mut DeskApp) -> bool {
    match app.timer.card.as_mut() {
        None => false,
        Some(c) if !c.minimized => {
            c.minimized = true;
            true
        }
        Some(_) => {
            app.timer.card = None;
            true
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reading the mirror

/// The engine's default kind: remote when the PC has a RustDesk ID, on site for a PC without,
/// phone for a customer alone.
fn kind_default(data: &Data, target: &Target) -> &'static str {
    match target.device.and_then(|d| data.device(d)) {
        Some(d) if !d.rustdesk_id.is_empty() => "remote",
        Some(_) => "onsite",
        None => "phone",
    }
}

/// Whether the engine would let remote work start on this customer without a reason.
fn consent_ready(data: &Data, customer: u64, now: DateTime<Utc>) -> bool {
    let prefs = &data.preferences;
    !prefs.require_consent
        || privacy::purpose_state(&data.privacy, customer, "remote_assistance", now, prefs.consent_validity_months).0 == PurposeState::Given
}

/// Opens the RustDesk link on `device` now when it has an ID and the consent is fine; answers
/// whether it did.
fn open_link_now(app: &mut DeskApp, customer: u64, device: u64) -> bool {
    let now = app.now();
    let uri = {
        let d = app.data();
        match d.device(device) {
            Some(dev) if !dev.rustdesk_id.is_empty() && consent_ready(d, customer, now) => rustdesk::connect_uri(&dev.rustdesk_id),
            _ => return false,
        }
    };
    app.requests.push(Request::OpenUrl(uri));
    true
}

/// The actor's running interventions, newest first: `(id, customer, device, started_at)`.
fn mine_running(data: &Data, actor: &str) -> Vec<(u64, String, String, String)> {
    let mut mine: Vec<_> = data
        .interventions
        .iter()
        .filter(|i| i.running() && i.technician == actor)
        .map(|i| (i.id, crate::customer_name(data, i.customer_id), crate::device_name(data, i.device_id), i.started_at.clone()))
        .collect();
    mine.sort_by(|a, b| b.3.cmp(&a.3).then(b.0.cmp(&a.0)));
    mine
}

/// The followed intervention as the mirror has it now, else as the engine answered:
/// `(customer, device, started_at, rustdesk_id)`.
fn running_view(data: &Data, r: &Running) -> (String, String, String, String) {
    let rid = r.device_id.and_then(|d| data.device(d)).map(|d| d.rustdesk_id.clone()).unwrap_or_default();
    match data.intervention(r.id) {
        Some(i) => (crate::customer_name(data, i.customer_id), crate::device_name(data, i.device_id), i.started_at.clone(), rid),
        None => (r.customer_name.clone(), r.device_name.clone(), r.started_at.clone(), rid),
    }
}

/// A form text as the stamp to send (empty or `now` = now); the error to show otherwise.
fn stamp_of(app: &DeskApp, text: &str) -> Result<String, String> {
    format::stamp_from_input(text, app.now()).ok_or_else(|| app.tr("Not a time: use 2026-10-13 09:30, 13/10/2026 09:30 or 09:30").to_string())
}

/// A minutes field: `None` when empty; the error when not a number.
fn minutes_of(app: &DeskApp, text: &str) -> Result<Option<u32>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<u32>().map(Some).map_err(|_| app.tr("Minutes must be a number").to_string())
}

/// `now` moved by `minutes`, as form text.
fn shifted(now: DateTime<Utc>, minutes: i64) -> String {
    let t = Duration::try_minutes(minutes).and_then(|d| now.checked_add_signed(d)).unwrap_or(now);
    format::input(&time::stamp(t))
}

/// The state word in the engine's refusal (`… consent is missing: …`).
fn refused_state(message: &str) -> String {
    message.split_once("consent is ").map(|(_, rest)| rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Sending

fn target_params(c: &Card) -> Option<Value> {
    let customer = c.target.customer?;
    Some(match c.target.device {
        Some(d) => json!({"device": d}),
        None => json!({"customer": customer}),
    })
}

fn set_error(app: &mut DeskApp, text: String) {
    if let Some(c) = app.timer.card.as_mut() {
        c.error = text;
    }
}

/// Sends `intervention.start` from the card; `anyway` adds the refusal panel's reason.
fn submit_start(app: &mut DeskApp, anyway: bool) {
    let Some(c) = app.timer.card.clone() else { return };
    let Some(mut params) = target_params(&c) else {
        set_error(app, app.tr("Choose a customer or a PC").to_string());
        return;
    };
    let started = match stamp_of(app, &c.started_at) {
        Ok(s) => s,
        Err(e) => return set_error(app, e),
    };
    params["kind"] = json!(c.kind);
    params["summary"] = json!(c.summary.trim());
    params["reference"] = json!(c.reference.trim());
    params["billable"] = json!(c.billable);
    params["started_at"] = json!(started);
    let reason = c.refusal.as_ref().map(|r| r.reason.trim().to_string()).unwrap_or_default();
    if anyway && !reason.is_empty() {
        params["consent_override"] = json!(reason);
    }
    let customer = c.target.customer;
    let device = c.target.device;
    // The RustDesk link opens in the click's frame when the mirror says the consent is fine.
    let opened = c.kind == "remote" && customer.zip(device).is_some_and(|(cu, dev)| open_link_now(app, cu, dev));
    if let Some(card) = app.timer.card.as_mut() {
        card.busy = true;
        card.error.clear();
    }
    app.run_as("intervention.start", params, crate::Intent::Timer(Intent::Start { customer, device, opened }));
}

/// Sends `intervention.stop` for the followed intervention, always with its id.
fn submit_stop(app: &mut DeskApp) {
    let Some(c) = app.timer.card.clone() else { return };
    let Some(r) = c.running else { return };
    let ended = match stamp_of(app, &c.ended_at) {
        Ok(s) => s,
        Err(e) => return set_error(app, e),
    };
    let minutes = match minutes_of(app, &c.minutes) {
        Ok(m) => m,
        Err(e) => return set_error(app, e),
    };
    let mut params = json!({"id": r.id, "notes": c.notes.trim(), "billable": c.billable, "ended_at": ended});
    if !c.summary.trim().is_empty() {
        params["summary"] = json!(c.summary.trim());
    }
    if let Some(m) = minutes {
        params["minutes"] = json!(m);
    }
    if let Some(card) = app.timer.card.as_mut() {
        card.busy = true;
        card.error.clear();
    }
    app.run_as("intervention.stop", params, crate::Intent::Timer(Intent::Stop { id: r.id }));
}

/// Sends `intervention.add` from the Log mode; `anyway` adds the refusal panel's reason.
fn submit_log(app: &mut DeskApp, anyway: bool) {
    let Some(c) = app.timer.card.clone() else { return };
    let Some(mut params) = target_params(&c) else {
        set_error(app, app.tr("Choose a customer or a PC").to_string());
        return;
    };
    if c.summary.trim().is_empty() {
        return set_error(app, app.tr("Summary is required").to_string());
    }
    let started = match stamp_of(app, &c.started_at) {
        Ok(s) => s,
        Err(e) => return set_error(app, e),
    };
    let ended = if c.ended_at.trim().is_empty() {
        None
    } else {
        match stamp_of(app, &c.ended_at) {
            Ok(s) => Some(s),
            Err(e) => return set_error(app, e),
        }
    };
    let minutes = match minutes_of(app, &c.minutes) {
        Ok(m) => m,
        Err(e) => return set_error(app, e),
    };
    if ended.is_none() && minutes.is_none() {
        return set_error(app, app.tr("Give the end time or the minutes").to_string());
    }
    if ended.as_deref().is_some_and(|e| e < started.as_str()) {
        return set_error(app, app.tr("The end is before the start").to_string());
    }
    params["kind"] = json!(c.kind);
    params["summary"] = json!(c.summary.trim());
    params["started_at"] = json!(started);
    if let Some(e) = ended {
        params["ended_at"] = json!(e);
    }
    if let Some(m) = minutes {
        params["minutes"] = json!(m);
    }
    if !c.technician.trim().is_empty() {
        params["technician"] = json!(c.technician.trim());
    }
    params["reference"] = json!(c.reference.trim());
    params["billable"] = json!(c.billable);
    params["notes"] = json!(c.notes.trim());
    let reason = c.refusal.as_ref().map(|r| r.reason.trim().to_string()).unwrap_or_default();
    if anyway && !reason.is_empty() {
        params["consent_override"] = json!(reason);
    }
    if let Some(card) = app.timer.card.as_mut() {
        card.busy = true;
        card.error.clear();
    }
    app.run_as("intervention.add", params, crate::Intent::Timer(Intent::Log));
}

/// Records the verbal consent typed in the refusal panel (`privacy.consent`); the refused
/// start or log is sent again when it succeeds.
fn submit_consent(app: &mut DeskApp) {
    let Some(c) = app.timer.card.clone() else { return };
    let (Some(r), Some(customer)) = (c.refusal, c.target.customer) else { return };
    if r.signer.trim().is_empty() {
        return;
    }
    let params = json!({
        "customer": customer, "purposes": ["remote_assistance"], "method": "verbal",
        "signer": r.signer.trim(), "note": r.note.trim(),
    });
    if let Some(card) = app.timer.card.as_mut() {
        card.busy = true;
        if let Some(r) = card.refusal.as_mut() {
            r.error.clear();
        }
    }
    app.run_as("privacy.consent", params, crate::Intent::Timer(Intent::Consent));
}

// ---------------------------------------------------------------------------------------------
// Results

/// Applies a finished command of the card.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    if let Some(card) = app.timer.card.as_mut() {
        card.busy = false;
    }
    match intent {
        Intent::Start { customer, device, opened } => match &c.result {
            Ok(v) => started(app, v, opened),
            Err(e) => refused(app, &e.to_string(), customer, device, Mode::Start),
        },
        Intent::Log => match &c.result {
            Ok(v) => {
                app.timer.card = None;
                app.selected_intervention = v["intervention"]["id"].as_u64();
                app.toast(app.tr("Intervention saved").to_string());
            }
            Err(e) => refused(app, &e.to_string(), None, None, Mode::Log),
        },
        Intent::Stop { id } => match &c.result {
            Ok(v) => {
                let i = &v["intervention"];
                let duration = format::duration(i["minutes"].as_u64().unwrap_or(0));
                let customer = i["customer_name"].as_str().unwrap_or_default().to_string();
                app.timer.card = None;
                app.selected_intervention = Some(id);
                app.toast(app.language.fill("Stopped · {duration} · {customer}", &[("duration", &duration), ("customer", &customer)]));
            }
            Err(e) => {
                // Closed by someone else meanwhile: the status bar has the message.
                let m = e.to_string();
                if m.contains("already closed") || m.starts_with("no intervention") {
                    app.timer.card = None;
                } else {
                    set_error(app, app.language.tr_message(&m));
                }
            }
        },
        Intent::Consent => match &c.result {
            Ok(_) => {
                let mode = app.timer.card.as_mut().map(|card| {
                    card.refusal = None;
                    card.mode
                });
                match mode {
                    Some(Mode::Log) => submit_log(app, false),
                    Some(_) => submit_start(app, false),
                    None => {}
                }
            }
            Err(e) => {
                let text = app.language.tr_message(&e.to_string());
                if let Some(r) = app.timer.card.as_mut().and_then(|card| card.refusal.as_mut()) {
                    r.error = text;
                }
            }
        },
    }
}

/// A start succeeded: the card follows the intervention in Running mode.
fn started(app: &mut DeskApp, v: &Value, opened: bool) {
    let i = &v["intervention"];
    let text = |key: &str| i[key].as_str().unwrap_or_default().to_string();
    let running = Running {
        id: i["id"].as_u64().unwrap_or(0),
        customer_id: i["customer_id"].as_u64().unwrap_or(0),
        device_id: i["device_id"].as_u64(),
        started_at: text("started_at"),
        customer_name: text("customer_name"),
        device_name: text("device_name"),
    };
    let warnings: Vec<String> =
        v["warnings"].as_array().map(|a| a.iter().filter_map(|w| w.as_str()).map(|w| app.language.tr_message(w)).collect()).unwrap_or_default();
    if !opened && let Some(uri) = v["connect"]["uri"].as_str() {
        app.requests.push(Request::OpenUrl(uri.to_string()));
    }
    let target = Target { customer: Some(running.customer_id), device: running.device_id, text: String::new() };
    let fresh = Card::new(app, Mode::Running, target.clone());
    let mut card = app.timer.card.take().unwrap_or(fresh);
    card.mode = Mode::Running;
    card.target = target;
    card.summary = text("summary");
    card.notes.clear();
    card.billable = i["billable"].as_bool().unwrap_or(true);
    card.running = Some(running);
    card.warnings = warnings;
    card.error.clear();
    card.refusal = None;
    card.minimized = false;
    card.refocus = Some(Field::Summary);
    app.timer.card = Some(card);
    app.toast(app.tr("Timer started").to_string());
}

/// A start or a log failed: the consent refusal panel, or the message under the form. The
/// card opens when it was not (the fast path from a row), so the technician sees why.
fn refused(app: &mut DeskApp, message: &str, customer: Option<u64>, device: Option<u64>, mode: Mode) {
    if app.timer.card.is_none() {
        let customer = device.and_then(|d| app.data().device(d)).map(|d| d.customer_id).or(customer);
        let card = Card::new(app, mode, Target { customer, device, text: String::new() });
        app.timer.card = Some(card);
    }
    let consent = message.contains("remote_assistance consent is");
    let translated = app.language.tr_message(message);
    let Some(card) = app.timer.card.as_mut() else { return };
    card.minimized = false;
    if consent {
        card.error.clear();
        card.refusal = Some(Refusal { state: refused_state(message), ..Refusal::default() });
        card.refocus = Some(Field::Reason);
    } else {
        card.error = translated;
    }
}

/// Drops a card that follows an intervention the desk no longer runs (stopped from the CLI,
/// another technician, deleted).
fn sync(app: &mut DeskApp) {
    let Some(c) = app.timer.card.as_ref() else { return };
    if !matches!(c.mode, Mode::Running | Mode::Stop) || c.busy {
        return;
    }
    let gone = match c.running.as_ref() {
        None => true,
        Some(r) => match app.data().intervention(r.id) {
            Some(i) => !i.running(),
            // A remote mirror may lag behind the answer; a local desk cannot.
            None => app.session.server().is_empty(),
        },
    };
    if gone {
        app.timer.card = None;
    }
}

// ---------------------------------------------------------------------------------------------
// The chip

/// The running-interventions chip in the top bar (drawn right-to-left): a pulsing red dot,
/// the elapsed time, the customer, a stop glyph. Click = the card in Running mode; click on
/// the glyph = the card in Stop mode. Only the actor's own timers show here.
pub fn chip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let now = app.now();
    let actor = app.session.actor().to_string();
    let mine = mine_running(app.data(), &actor);
    let Some((id, customer, device, started)) = mine.first().cloned() else { return };
    let hover = app.data().intervention(id).map(|i| format!("{} · {}", i.summary, device)).unwrap_or(device);
    let extra = mine.len().saturating_sub(1);
    let mut name = format::clipped(&customer, 18);
    if extra > 0 {
        name = format!("{name} +{extra}");
    }
    let p = ui.painter().clone();
    let clock = p.layout_no_wrap(format::elapsed(&started, now), FontId::monospace(13.0), t.ui_text);
    let label = p.layout_no_wrap(name, FontId::proportional(13.0), t.ui_text);
    let width = 24.0 + clock.size().x + 8.0 + label.size().x + 34.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let (fill, _) = widgets::tone_colors(t, Tone::Bad);
    let red = widgets::danger(t);
    p.rect_filled(rect, 15.0, if resp.hovered() { fill.gamma_multiply(0.85) } else { fill });
    // The dot pulses: full one second, faint the next (the desk's clock, not a local one).
    let on = now.timestamp().rem_euclid(2) == 0;
    p.circle_filled(pos2(rect.left() + 12.0, rect.center().y), 4.0, if on { red } else { red.gamma_multiply(0.35) });
    let mut x = rect.left() + 22.0;
    p.galley(pos2(x, rect.center().y - clock.size().y / 2.0), clock.clone(), t.ui_text);
    x += clock.size().x + 8.0;
    p.galley(pos2(x, rect.center().y - label.size().y / 2.0), label, t.ui_text);
    let stop = Rect::from_center_size(pos2(rect.right() - 15.0, rect.center().y), vec2(14.0, 14.0));
    icons::paint(&p, stop, Icon::Stop, red);
    let on_stop = resp.interact_pointer_pos().is_some_and(|q| stop.expand(7.0).contains(q));
    if resp.clicked() {
        if on_stop {
            open_running(app, id, true);
        } else {
            let shown =
                app.timer.card.as_ref().is_some_and(|c| c.mode == Mode::Running && !c.minimized && c.running.as_ref().is_some_and(|r| r.id == id));
            if shown {
                close(app);
            } else {
                open_running(app, id, false);
            }
        }
    }
    resp.on_hover_text(hover);
}

// ---------------------------------------------------------------------------------------------
// The card

/// The docked card (drawn after the screens, like MailCraft's compose card).
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    sync(app);
    let Some(mut c) = app.timer.card.clone() else { return };
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let width = CARD_W.min((screen.width() - 40.0).max(240.0));
    // Docked at the bottom right, dragged by its title bar anywhere on the screen.
    let min_x = -(screen.width() - width - 48.0).max(0.0);
    let min_y = -(screen.height() - 120.0).max(0.0);
    c.offset = vec2(c.offset.x.clamp(min_x, 0.0), c.offset.y.clamp(min_y, 0.0));
    let home = vec2(-24.0, -34.0) + c.offset;
    let mut acts: Vec<Action> = Vec::new();
    // Ctrl+Enter inside one of the card's own boxes submits.
    let focused_here = ctx.memory(|m| m.focused()).is_some_and(|f| Field::ALL.iter().any(|x| x.id() == f));
    if focused_here && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
        acts.push(Action::Submit);
    }
    let now = app.now();
    let app_ref: &DeskApp = app;
    egui::Area::new(egui::Id::new("work-card")).anchor(egui::Align2::RIGHT_BOTTOM, home).order(egui::Order::Middle).fade_in(false).show(&ctx, |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(12)
            .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 18, spread: 0, color: t.ui_text.gamma_multiply(0.18) })
            .show(ui, |ui| {
                ui.set_width(width);
                title_bar(app_ref, ui, t, &mut c, width, now, &mut acts);
                if c.minimized {
                    return;
                }
                egui::Frame::NONE.inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    body(app_ref, ui, t, &mut c, now, &mut acts);
                });
            });
    });
    app.timer.card = Some(c);
    for a in acts {
        apply(app, a);
    }
}

fn apply(app: &mut DeskApp, action: Action) {
    let mode = app.timer.card.as_ref().map(|c| c.mode);
    match action {
        Action::Submit => match mode {
            Some(Mode::Start) => submit_start(app, false),
            Some(Mode::Log) => submit_log(app, false),
            Some(Mode::Stop) => submit_stop(app),
            Some(Mode::Running) => {
                if let Some(id) = app.timer.card.as_ref().and_then(|c| c.running.as_ref()).map(|r| r.id) {
                    open_running(app, id, true);
                }
            }
            None => {}
        },
        Action::Anyway => match mode {
            Some(Mode::Log) => submit_log(app, true),
            Some(_) => submit_start(app, true),
            None => {}
        },
        Action::Consent => submit_consent(app),
        Action::Cancel => match mode {
            Some(Mode::Stop) => {
                if let Some(c) = app.timer.card.as_mut() {
                    c.mode = Mode::Running;
                    c.error.clear();
                }
            }
            _ => app.timer.card = None,
        },
        Action::Close => app.timer.card = None,
        Action::Stop(id) => open_running(app, id, true),
        Action::Follow(id) => open_running(app, id, false),
        Action::Connect(device) => app.connect(device),
    }
}

/// The dark title bar: the mode (and the ticking time), dragged to move the card, with the
/// minimise and close buttons; double-click minimises.
fn title_bar(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, width: f32, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    let (bar, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click_and_drag());
    if resp.dragged() {
        c.offset += resp.drag_delta();
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if resp.double_clicked() {
        c.minimized = !c.minimized;
    }
    let radius = if c.minimized { egui::CornerRadius::same(12) } else { egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 } };
    ui.painter().rect_filled(bar, radius, if t.dark { t.chrome } else { t.ui_text });
    let (icon, title) = match c.mode {
        Mode::Start => (Icon::Play, app.tr("Start timer").to_string()),
        Mode::Log => (Icon::Edit, app.tr("Log past work").to_string()),
        Mode::Running | Mode::Stop => {
            let (customer, _, started, _) = c.running.as_ref().map(|r| running_view(app.data(), r)).unwrap_or_default();
            let word = if c.mode == Mode::Stop { app.tr("Stop") } else { app.tr("Running") };
            (Icon::Timer, format!("{word} · {} · {customer}", format::elapsed(&started, now)))
        }
    };
    let on_bar = t.on_accent;
    icons::paint(ui.painter(), Rect::from_center_size(bar.left_center() + vec2(22.0, 0.0), vec2(18.0, 18.0)), icon, on_bar);
    let galley = widgets::truncate(ui.painter(), &title, FontId::proportional(14.0), on_bar, (width - 120.0).max(40.0));
    ui.painter().galley(pos2(bar.left() + 40.0, bar.center().y - galley.size().y / 2.0), galley, on_bar);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 4.0))).layout(Layout::right_to_left(Align::Center)));
    let light = Tokens { ui_text: on_bar, hover: on_bar.gamma_multiply(0.15), ..*t };
    if icon_button(&mut child, &light, Icon::Close, app.tr("Close"), true, false).clicked() {
        acts.push(Action::Close);
    }
    if icon_button(&mut child, &light, Icon::Chevron, app.tr("Minimize"), true, false).clicked() {
        c.minimized = !c.minimized;
    }
}

fn body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    match c.mode {
        Mode::Start => start_body(app, ui, t, c, now, acts),
        Mode::Running => running_body(app, ui, t, c, now, acts),
        Mode::Stop => stop_body(app, ui, t, c, now, acts),
        Mode::Log => log_body(app, ui, t, c, now, acts),
    }
    // Focus a box once the pointer is quiet (MailCraft's refocus trick: egui takes the focus
    // away from a text box that is not under the click that opened it).
    if let Some(f) = c.refocus {
        if ui.input(|i| i.pointer.any_click() || i.pointer.any_pressed() || i.pointer.any_down()) {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().memory_mut(|m| m.request_focus(f.id()));
            c.refocus = None;
        }
    }
}

fn start_body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    target_section(app, ui, t, c);
    kind_row(app, ui, t, c);
    text_row(ui, t, Field::Summary, app.tr("Summary"), &mut c.summary, app.tr("What is the request?"));
    text_row(ui, t, Field::Reference, app.tr("Reference"), &mut c.reference, app.tr("Ticket or job number"));
    let row =
        TimeRow { field: Field::Started, label: "Started at", hint: app.tr("now"), chips: &[("Now", 0), ("−15 min", -15), ("−30 min", -30)] };
    time_row(app, ui, t, &row, &mut c.started_at, now);
    ui.add_space(4.0);
    ui.checkbox(&mut c.billable, app.tr("Billable"));
    warning_lines(ui, t, &c.warnings);
    widgets::error_line(ui, t, &c.error);
    ui.add_space(6.0);
    let busy = c.busy;
    if let Some(r) = c.refusal.as_mut() {
        if let Some(f) = refusal_panel(app, ui, t, r, Mode::Start, busy, acts) {
            c.refocus = Some(f);
        }
        return;
    }
    ui.horizontal(|ui| {
        if action_button(ui, t, Some(Icon::Play), app.tr("Start"), c.target.customer.is_some() && !busy) {
            acts.push(Action::Submit);
        }
        if ui.button(app.tr("Cancel")).clicked() {
            acts.push(Action::Cancel);
        }
        if busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
        }
    });
}

fn running_body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    let mine = mine_running(app.data(), app.session.actor());
    let current = c.running.as_ref().map(|r| r.id);
    if mine.len() > 1 {
        for (id, customer, device, started) in &mine {
            let (rect, resp) = widgets::row(ui, t, 36.0, Some(*id) == current);
            let p = ui.painter();
            let text = if device.is_empty() { customer.clone() } else { format!("{customer} › {device}") };
            let galley = widgets::truncate(p, &text, FontId::proportional(13.0), t.ui_text, (rect.width() - 130.0).max(20.0));
            p.galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, t.ui_text);
            p.text(
                pos2(rect.right() - 44.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                format::elapsed(started, now),
                FontId::monospace(13.0),
                t.accent,
            );
            let stop_rect = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(28.0, 28.0));
            let stop = ui.interact(stop_rect, ui.id().with(("stop-row", *id)), Sense::click());
            if stop.hovered() {
                ui.painter().rect_filled(stop_rect, 8.0, t.hover);
            }
            icons::paint(ui.painter(), stop_rect.shrink(7.0), Icon::Stop, widgets::danger(t));
            if stop.on_hover_text(app.tr("Stop")).clicked() {
                acts.push(Action::Stop(*id));
            } else if resp.clicked() {
                acts.push(Action::Follow(*id));
            }
        }
        ui.add_space(6.0);
    }
    let rustdesk_id = header(app, ui, t, c, now);
    text_row(ui, t, Field::Summary, app.tr("Summary"), &mut c.summary, app.tr("What is the request?"));
    notes_block(app, ui, t, &mut c.notes);
    ui.add_space(4.0);
    ui.checkbox(&mut c.billable, app.tr("Billable"));
    warning_lines(ui, t, &c.warnings);
    widgets::error_line(ui, t, &c.error);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if action_button(ui, t, Some(Icon::Stop), app.tr("Stop"), !c.busy)
            && let Some(id) = current
        {
            acts.push(Action::Stop(id));
        }
        if !rustdesk_id.is_empty()
            && let Some(device) = c.target.device
            && widgets::icon_text_button(ui, t, Icon::Connect, app.tr("Connect again"), true).clicked()
        {
            acts.push(Action::Connect(device));
        }
    });
}

fn stop_body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    header(app, ui, t, c, now);
    notes_block(app, ui, t, &mut c.notes);
    text_row(ui, t, Field::Summary, app.tr("Summary"), &mut c.summary, app.tr("What is the request?"));
    let row = TimeRow { field: Field::Ended, label: "Ended at", hint: app.tr("now"), chips: &[("Now", 0), ("−15 min", -15), ("−30 min", -30)] };
    time_row(app, ui, t, &row, &mut c.ended_at, now);
    text_row(ui, t, Field::Minutes, app.tr("Minutes"), &mut c.minutes, app.tr("Leave empty to use the elapsed time"));
    ui.add_space(4.0);
    ui.checkbox(&mut c.billable, app.tr("Billable"));
    widgets::error_line(ui, t, &c.error);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if action_button(ui, t, Some(Icon::Stop), app.tr("Stop"), !c.busy) {
            acts.push(Action::Submit);
        }
        if ui.button(app.tr("Cancel")).clicked() {
            acts.push(Action::Cancel);
        }
        if c.busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
        }
    });
}

fn log_body(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card, now: DateTime<Utc>, acts: &mut Vec<Action>) {
    target_section(app, ui, t, c);
    kind_row(app, ui, t, c);
    text_row(ui, t, Field::Summary, app.tr("Summary"), &mut c.summary, app.tr("What is the request?"));
    let example = format::input(&time::stamp(now));
    let start = TimeRow { field: Field::Started, label: "Started at", hint: &example, chips: &[("Now", 0), ("−1 h", -60)] };
    time_row(app, ui, t, &start, &mut c.started_at, now);
    let end = TimeRow { field: Field::Ended, label: "Ended at", hint: &example, chips: &[("Now", 0)] };
    time_row(app, ui, t, &end, &mut c.ended_at, now);
    text_row(ui, t, Field::Minutes, app.tr("Minutes"), &mut c.minutes, "30");
    text_row(ui, t, Field::Technician, app.tr("Technician"), &mut c.technician, app.session.actor());
    text_row(ui, t, Field::Reference, app.tr("Reference"), &mut c.reference, app.tr("Ticket or job number"));
    ui.add_space(4.0);
    ui.checkbox(&mut c.billable, app.tr("Billable"));
    notes_block(app, ui, t, &mut c.notes);
    warning_lines(ui, t, &c.warnings);
    widgets::error_line(ui, t, &c.error);
    ui.add_space(6.0);
    let busy = c.busy;
    if let Some(r) = c.refusal.as_mut() {
        if let Some(f) = refusal_panel(app, ui, t, r, Mode::Log, busy, acts) {
            c.refocus = Some(f);
        }
        return;
    }
    ui.horizontal(|ui| {
        let ready = c.target.customer.is_some() && !c.summary.trim().is_empty() && !busy;
        if action_button(ui, t, None, app.tr("Save"), ready) {
            acts.push(Action::Submit);
        }
        if ui.button(app.tr("Cancel")).clicked() {
            acts.push(Action::Cancel);
        }
        if busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
        }
    });
}

/// The customer › PC line and the big elapsed time of the followed intervention; answers the
/// PC's RustDesk ID (empty when none).
fn header(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &Card, now: DateTime<Utc>) -> String {
    let Some(r) = c.running.as_ref() else { return String::new() };
    let (customer, device, started, rustdesk_id) = running_view(app.data(), r);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        let icon = if !rustdesk_id.is_empty() {
            Icon::Remote
        } else if device.is_empty() {
            Icon::Customer
        } else {
            Icon::Device
        };
        icons::paint(ui.painter(), rect, icon, t.accent);
        let text = if device.is_empty() { customer } else { format!("{customer} › {device}") };
        let w = ui.available_width().max(20.0);
        let galley = widgets::truncate(ui.painter(), &text, FontId::proportional(15.0), t.ui_text, w);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
        ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, t.ui_text);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(format::elapsed(&started, now)).font(FontId::monospace(28.0)).color(t.accent));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let when = format!("{} {}", app.tr("Started at"), format::short(&started, now, app.italian()));
            ui.label(RichText::new(when).small().color(t.muted));
            if !rustdesk_id.is_empty() {
                ui.label(RichText::new(rustdesk::display(&rustdesk_id)).font(FontId::monospace(12.0)).color(t.muted));
            }
        });
    });
    ui.add_space(4.0);
    rustdesk_id
}

/// The picked target as a line with a button to change it, or the picker.
fn target_section(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card) {
    let d = app.data();
    let Some(customer) = c.target.customer else {
        picker(app, ui, t, c);
        return;
    };
    let device = c.target.device.and_then(|x| d.device(x));
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        let icon = match device {
            Some(x) if !x.rustdesk_id.is_empty() => Icon::Remote,
            Some(_) => Icon::Device,
            None => Icon::Customer,
        };
        icons::paint(ui.painter(), rect, icon, t.accent);
        let mut text = crate::customer_name(d, customer);
        if let Some(x) = device {
            text = format!("{text} › {}", x.name);
            if !x.rustdesk_id.is_empty() {
                text = format!("{text}  {}", rustdesk::display(&x.rustdesk_id));
            }
        }
        let w = (ui.available_width() - 40.0).max(20.0);
        let galley = widgets::truncate(ui.painter(), &text, FontId::proportional(14.0), t.ui_text, w);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, t.ui_text);
        if icon_button(ui, t, Icon::Close, app.tr("Change"), true, false).clicked() {
            c.target = Target::default();
            c.refocus = Some(Field::Target);
        }
    });
    hairline(ui, t);
}

/// The target picker: a text box filtering the PCs and the customers of the mirror (a
/// RustDesk ID matches its PC exactly); click or Enter picks, the kind follows the pick.
fn picker(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card) {
    let resp = ui.add(
        egui::TextEdit::singleline(&mut c.target.text)
            .id(Field::Target.id())
            .hint_text(app.tr("Type a name or a RustDesk ID"))
            .desired_width(f32::INFINITY),
    );
    let q = core::fold(c.target.text.trim());
    if q.is_empty() {
        ui.label(RichText::new(app.tr("Choose a customer or a PC")).small().color(t.muted));
        return;
    }
    let d = app.data();
    let devices: Vec<(u64, u64, String)> = d
        .devices
        .iter()
        .filter(|x| !x.retired && views::device_matches(x, &q))
        .take(PICKER_ROWS)
        .map(|x| {
            let id = if x.rustdesk_id.is_empty() { String::new() } else { format!(" · {}", rustdesk::display(&x.rustdesk_id)) };
            (x.id, x.customer_id, format!("{} · {}{id}", x.name, crate::customer_name(d, x.customer_id)))
        })
        .collect();
    let customers: Vec<(u64, String)> = d
        .customers
        .iter()
        .filter(|x| x.erased_at.is_none() && !x.archived && views::customer_matches(x, &q))
        .take(PICKER_ROWS)
        .map(|x| (x.id, if x.tax_id.is_empty() { x.name.clone() } else { format!("{} · {}", x.name, x.tax_id) }))
        .collect();
    let mut pick: Option<(u64, Option<u64>)> = None;
    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        pick = devices.first().map(|(dev, cu, _)| (*cu, Some(*dev))).or_else(|| customers.first().map(|(cu, _)| (*cu, None)));
    }
    egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(4).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if devices.is_empty() && customers.is_empty() {
            ui.label(RichText::new(app.tr("No match")).small().color(t.muted));
        }
        for (dev, cu, text) in &devices {
            if picker_row(ui, t, Icon::Device, text).clicked() {
                pick = Some((*cu, Some(*dev)));
            }
        }
        for (cu, text) in &customers {
            if picker_row(ui, t, Icon::Customer, text).clicked() {
                pick = Some((*cu, None));
            }
        }
    });
    if let Some((customer, device)) = pick {
        c.target = Target { customer: Some(customer), device, text: String::new() };
        if !c.kind_touched {
            c.kind = kind_default(d, &c.target).to_string();
        }
        c.refocus = Some(Field::Summary);
    }
}

fn picker_row(ui: &mut egui::Ui, t: &Tokens, icon: Icon, text: &str) -> Response {
    let (rect, resp) = widgets::row(ui, t, 30.0, false);
    let p = ui.painter();
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(16.0, 16.0)), icon, t.muted);
    let galley = widgets::truncate(p, text, FontId::proportional(13.0), t.ui_text, (rect.width() - 40.0).max(20.0));
    p.galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, t.ui_text);
    resp
}

/// The kind of work as a row of chips.
fn kind_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &mut Card) {
    ui.horizontal(|ui| {
        label_box(ui, t, app.tr("Kind"));
        // The chips wrap in their own column, to the right of the label.
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (value, label, icon) in KINDS {
                    if choice_chip(ui, t, icon, app.tr(label), c.kind == value).clicked() {
                        c.kind = value.to_string();
                        c.kind_touched = true;
                    }
                }
            });
        });
    });
    hairline(ui, t);
}

/// A time box with quick chips: `Now` clears it (empty = now), the others set a time
/// relative to now.
struct TimeRow<'a> {
    field: Field,
    label: &'a str,
    hint: &'a str,
    chips: &'a [(&'a str, i64)],
}

fn time_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, row: &TimeRow<'_>, value: &mut String, now: DateTime<Utc>) {
    // Wrapped: on a narrow card the chips go to a second line instead of widening it.
    ui.horizontal_wrapped(|ui| {
        label_box(ui, t, app.tr(row.label));
        // The chips keep their width; the text box takes what is left of the card.
        let chips_w: f32 = row
            .chips
            .iter()
            .map(|(label, _)| ui.painter().layout_no_wrap(app.tr(label).to_string(), FontId::proportional(14.0), t.ui_text).size().x + 16.0)
            .sum();
        // In a wrapped row `available_width` is the whole line: measure from the cursor.
        let room = ui.max_rect().right() - ui.cursor().left();
        let w = (room - chips_w - 12.0).clamp(48.0, 180.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .id(row.field.id())
                .hint_text(row.hint)
                .desired_width(w)
                .frame(egui::Frame::NONE)
                .font(FontId::monospace(13.0)),
        );
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().button_padding = vec2(6.0, 1.0);
        for (label, minutes) in row.chips {
            if ui.small_button(app.tr(label)).clicked() {
                *value = if *minutes == 0 { String::new() } else { shifted(now, *minutes) };
            }
        }
    });
    hairline(ui, t);
}

/// A compose-style row: a muted label, a frameless text box, a hairline under both.
fn text_row(ui: &mut egui::Ui, t: &Tokens, field: Field, label: &str, value: &mut String, hint: &str) -> Response {
    let r = ui
        .horizontal(|ui| {
            label_box(ui, t, label);
            let w = (ui.available_width() - 4.0).max(40.0);
            ui.add(egui::TextEdit::singleline(value).id(field.id()).hint_text(hint).desired_width(w).frame(egui::Frame::NONE))
        })
        .inner;
    hairline(ui, t);
    r
}

/// The notes box: a few lines on the panel tone.
fn notes_block(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, notes: &mut String) {
    ui.add_space(4.0);
    ui.label(RichText::new(app.tr("Notes")).small().color(t.muted));
    egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(6).show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(notes)
                .id(Field::Notes.id())
                .hint_text(app.tr("What was done, found, left to do"))
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .frame(egui::Frame::NONE),
        );
    });
}

/// The red-outlined panel of a refused start: a reason to go ahead, or a verbal consent to
/// record first. Answers the field to focus, if any.
fn refusal_panel(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, r: &mut Refusal, mode: Mode, busy: bool, acts: &mut Vec<Action>) -> Option<Field> {
    let (fill, _) = widgets::tone_colors(t, Tone::Bad);
    let red = widgets::danger(t);
    let mut focus = None;
    egui::Frame::NONE.fill(fill).stroke(Stroke::new(1.5, red)).corner_radius(8).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Warning, red);
            let state = app.tr(&r.state).to_string();
            ui.label(RichText::new(app.language.fill("Remote-assistance consent is {state}", &[("state", &state)])).strong().color(red));
        });
        ui.label(RichText::new(app.tr("Reason to proceed without consent")).small().color(t.muted));
        ui.add(
            egui::TextEdit::singleline(&mut r.reason)
                .id(Field::Reason.id())
                .hint_text(app.tr("The person agreed on the phone"))
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            let label = if mode == Mode::Log { "Log anyway" } else { "Start anyway" };
            if action_button(ui, t, Some(Icon::Play), app.tr(label), !r.reason.trim().is_empty() && !busy) {
                acts.push(Action::Anyway);
            }
            if ui.button(app.tr("Record consent first")).clicked() {
                r.record = !r.record;
                if r.record {
                    focus = Some(Field::Signer);
                }
            }
        });
        if r.record {
            text_row(ui, t, Field::Signer, app.tr("Who agreed"), &mut r.signer, "");
            text_row(ui, t, Field::Note, app.tr("Evidence or note"), &mut r.note, "");
            ui.add_space(4.0);
            if action_button(ui, t, Some(Icon::Signature), app.tr("Record verbal consent and start"), !r.signer.trim().is_empty() && !busy) {
                acts.push(Action::Consent);
            }
        }
        widgets::error_line(ui, t, &r.error);
        if busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
        }
    });
    focus
}

// ---------------------------------------------------------------------------------------------
// Small widgets of the card

/// A fixed-width muted label at the left of a row.
fn label_box(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(LABEL_W, 24.0), Sense::hover());
    let galley = widgets::truncate(ui.painter(), text, FontId::proportional(13.0), t.muted, LABEL_W - 6.0);
    ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, t.muted);
}

/// A hairline under a row.
fn hairline(ui: &mut egui::Ui, t: &Tokens) {
    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.border));
}

/// A selectable pill with a glyph (the kinds of work).
fn choice_chip(ui: &mut egui::Ui, t: &Tokens, icon: Icon, text: &str, selected: bool) -> Response {
    let color = if selected { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 36.0, 26.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 13.0, t.active);
    } else {
        if resp.hovered() {
            p.rect_filled(rect, 13.0, t.hover);
        }
        p.rect_stroke(rect, 13.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(15.0, 15.0)), icon, color);
    p.galley(pos2(rect.left() + 27.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// The accent button of a bar, muted and inert when disabled; answers whether it was clicked.
fn action_button(ui: &mut egui::Ui, t: &Tokens, icon: Option<Icon>, text: &str, enabled: bool) -> bool {
    if enabled {
        return widgets::primary_button(ui, t, icon, text).clicked();
    }
    let muted = Tokens { accent: t.accent.gamma_multiply(0.45), ..*t };
    widgets::primary_button(ui, &muted, icon, text);
    false
}

/// The engine's warnings as amber lines.
fn warning_lines(ui: &mut egui::Ui, t: &Tokens, lines: &[String]) {
    let (fill, color) = widgets::tone_colors(t, Tone::Warn);
    for w in lines {
        egui::Frame::NONE.fill(fill).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Warning, color);
                ui.label(RichText::new(w).small().color(color));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, Screen};

    /// Renders one frame of the app at 1440×900 (no window, no GPU).
    fn frame(app: &mut DeskApp) {
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    /// The demo desk with the interface in English (the desk's own language is Italian).
    fn demo_en() -> DeskApp {
        let mut app = DeskApp::demo();
        app.language = Language::En;
        app
    }

    fn card(app: &DeskApp) -> &Card {
        app.timer.card.as_ref().expect("the card is open")
    }

    fn edit(app: &mut DeskApp, f: impl FnOnce(&mut Card)) {
        f(app.timer.card.as_mut().expect("the card is open"));
    }

    fn edit_refusal(app: &mut DeskApp, f: impl FnOnce(&mut Refusal)) {
        f(app.timer.card.as_mut().and_then(|c| c.refusal.as_mut()).expect("the refusal panel is open"));
    }

    #[test]
    fn every_mode_renders_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = DeskApp::demo();
                app.language = lang;
                app.dark = dark;
                app.go(Screen::Customers);
                open_start(&mut app, Some(1), Some(1));
                frame(&mut app);
                assert_eq!(card(&app).kind, "remote");
                open_start(&mut app, None, None);
                edit(&mut app, |c| c.target.text = "lun".into());
                frame(&mut app);
                assert_eq!(card(&app).kind, "phone");
                open_log(&mut app, Some(2), Some(4));
                frame(&mut app);
                assert!(!card(&app).started_at.is_empty(), "log mode prefills the start");
                // Running, stop, the refusal panel and its consent form.
                start_now(&mut app, 4);
                frame(&mut app);
                assert_eq!(card(&app).mode, Mode::Running);
                frame(&mut app);
                open_running(&mut app, 11, true);
                frame(&mut app);
                assert_eq!(card(&app).mode, Mode::Stop);
                start_now(&mut app, 5);
                frame(&mut app);
                assert!(card(&app).refusal.is_some());
                edit_refusal(&mut app, |r| r.record = true);
                frame(&mut app);
                assert!(close(&mut app) && card(&app).minimized);
                frame(&mut app);
            }
        }
    }

    #[test]
    fn start_now_opens_the_link_in_the_click_frame_and_the_card_on_the_answer() {
        let mut app = demo_en();
        start_now(&mut app, 4);
        assert_eq!(app.requests, vec![Request::OpenUrl("rustdesk://connection/new/200300400".into())]);
        assert!(app.timer.card.is_none(), "the card waits for the engine");
        frame(&mut app);
        let c = card(&app);
        assert_eq!(c.mode, Mode::Running);
        assert_eq!(c.running.as_ref().map(|r| r.id), Some(11));
        assert_eq!(c.target, Target { customer: Some(2), device: Some(4), text: String::new() });
        assert_eq!(app.requests.len(), 1, "the link is not opened twice");
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("Timer started"));
        assert!(app.data().intervention(11).is_some_and(|i| i.running()));
        // The chip toggles the card; the stop glyph goes to Stop mode.
        close(&mut app);
        assert!(card(&app).minimized);
        open_running(&mut app, 11, true);
        let c = app.timer.card.as_mut().expect("card");
        assert_eq!(c.mode, Mode::Stop);
        assert!(!c.minimized);
        c.notes = "Driver aggiornato".into();
        c.minutes = "x".into();
        submit_stop(&mut app);
        assert!(card(&app).error.contains("number"));
        edit(&mut app, |c| c.minutes = "20".into());
        submit_stop(&mut app);
        assert!(card(&app).busy);
        frame(&mut app);
        assert!(app.timer.card.is_none());
        assert_eq!(app.selected_intervention, Some(11));
        let i = app.data().intervention(11).expect("stopped");
        assert!(!i.running());
        assert_eq!(i.minutes, 20);
        assert!(i.notes.contains("Driver aggiornato"));
        let toast = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
        assert!(toast.starts_with("Stopped · 20 min · Ferramenta Corvo"), "{toast}");
    }

    #[test]
    fn a_refused_start_offers_a_reason_or_a_verbal_consent() {
        // Elio Brina (3) has no remote-assistance consent and the desk requires one.
        let mut app = demo_en();
        start_now(&mut app, 5);
        assert!(app.requests.is_empty(), "no link without consent");
        frame(&mut app);
        let c = card(&app);
        assert_eq!(c.mode, Mode::Start);
        assert_eq!(c.refusal.as_ref().map(|r| r.state.as_str()), Some("missing"));
        assert_eq!(c.target.device, Some(5));
        assert_eq!(c.kind, "remote");
        // Start anyway, with the reason.
        edit_refusal(&mut app, |r| r.reason = "agreed on the phone".into());
        submit_start(&mut app, true);
        frame(&mut app);
        assert_eq!(card(&app).mode, Mode::Running);
        assert_eq!(app.requests, vec![Request::OpenUrl("rustdesk://connection/new/300400500".into())], "opened from the answer");
        assert_eq!(app.data().intervention(11).map(|i| i.consent_override.as_str()), Some("agreed on the phone"));
        assert!(card(&app).warnings.iter().any(|w| w.contains("consent is missing")), "{:?}", card(&app).warnings);
        // Record a verbal consent instead: the start is sent again by itself.
        let mut app = DeskApp::demo();
        app.language = Language::It;
        start_now(&mut app, 5);
        frame(&mut app);
        let r = app.timer.card.as_mut().and_then(|c| c.refusal.as_mut()).expect("refusal");
        r.record = true;
        r.signer = "Elio Brina".into();
        r.note = "detto al telefono".into();
        submit_consent(&mut app);
        frame(&mut app); // the consent answers: the start is re-sent
        frame(&mut app); // the start answers
        assert_eq!(card(&app).mode, Mode::Running);
        assert!(card(&app).refusal.is_none());
        assert_eq!(app.requests, vec![Request::OpenUrl("rustdesk://connection/new/300400500".into())], "opened when re-sent");
        let events = app.data().privacy.iter().filter(|e| e.customer_id == 3 && e.purpose == "remote_assistance").count();
        assert_eq!(events, 1);
        assert_eq!(app.data().intervention(11).map(|i| i.consent_override.as_str()), Some(""));
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("Timer avviato"));
    }

    #[test]
    fn the_start_form_validates_and_submits() {
        let mut app = demo_en();
        open_start(&mut app, None, None);
        submit_start(&mut app, false);
        assert_eq!(card(&app).error, "Choose a customer or a PC");
        let c = app.timer.card.as_mut().expect("card");
        c.target = Target { customer: Some(1), device: None, text: String::new() };
        c.kind = "phone".into();
        c.summary = "Domanda sulla posta".into();
        c.started_at = "ieri".into();
        submit_start(&mut app, false);
        assert!(card(&app).error.starts_with("Not a time"));
        edit(&mut app, |c| c.started_at = "09:00".into());
        submit_start(&mut app, false);
        assert!(app.requests.is_empty(), "a phone call opens no link");
        frame(&mut app);
        let c = card(&app);
        assert_eq!(c.mode, Mode::Running);
        assert_eq!(c.summary, "Domanda sulla posta");
        let i = app.data().intervention(11).expect("started");
        assert_eq!(i.kind, core::InterventionKind::Phone);
        assert!(i.started_at.ends_with("Z"));
    }

    #[test]
    fn past_work_is_logged_from_the_card() {
        let mut app = demo_en();
        open_log(&mut app, Some(1), None);
        assert_eq!(card(&app).technician, "riccardo");
        let c = app.timer.card.as_mut().expect("card");
        c.summary = "Backup verificato".into();
        c.started_at = "09:00".into();
        submit_log(&mut app, false);
        assert_eq!(card(&app).error, "Give the end time or the minutes");
        edit(&mut app, |c| c.ended_at = "08:00".into());
        submit_log(&mut app, false);
        assert_eq!(card(&app).error, "The end is before the start");
        let c = app.timer.card.as_mut().expect("card");
        c.ended_at.clear();
        c.minutes = "30".into();
        submit_log(&mut app, false);
        assert!(card(&app).busy, "the card waits for the engine's answer");
        frame(&mut app);
        assert!(app.timer.card.is_none());
        assert_eq!(app.selected_intervention, Some(11));
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("Intervention saved"));
        let i = app.data().intervention(11).expect("logged");
        assert_eq!(i.minutes, 30);
        assert_eq!(i.kind, core::InterventionKind::Phone);
        assert!(!i.running());
        // A required summary stops the submit locally.
        open_log(&mut app, Some(1), None);
        submit_log(&mut app, false);
        assert_eq!(card(&app).error, "Summary is required");
    }

    #[test]
    fn escape_minimises_then_closes_and_an_external_stop_drops_the_card() {
        let mut app = demo_en();
        assert!(!close(&mut app));
        open_start(&mut app, Some(1), None);
        assert!(close(&mut app));
        assert!(card(&app).minimized);
        assert!(close(&mut app));
        assert!(app.timer.card.is_none());
        start_now(&mut app, 1);
        frame(&mut app);
        assert_eq!(card(&app).mode, Mode::Running);
        app.run("intervention.stop", json!({"id": 11}));
        frame(&mut app);
        assert!(app.timer.card.is_none(), "stopped elsewhere");
        // `x`: stop my timer right now, without notes (nothing to stop first: a no-op).
        stop_mine(&mut app);
        frame(&mut app);
        assert!(app.timer.card.is_none());
        assert!(!app.toast.as_ref().is_some_and(|t| t.0.starts_with("Stopped")), "nothing to stop");
        start_now(&mut app, 4);
        frame(&mut app);
        stop_mine(&mut app);
        frame(&mut app);
        assert!(app.timer.card.is_none());
        assert!(app.data().intervention(12).is_some_and(|i| !i.running()));
        assert!(app.toast.as_ref().is_some_and(|t| t.0.starts_with("Stopped")));
    }

    #[test]
    fn elapsed_and_the_refused_state() {
        let now = time::parse_stamp("2026-10-13T12:00:04Z").expect("stamp");
        // The chip and the card show `h:mm:ss`, never negative.
        assert_eq!(format::elapsed("2026-10-13T09:58:30Z", now), "2:01:34");
        assert_eq!(format::elapsed("2026-10-13T13:00:00Z", now), "0:00:00");
        assert_eq!(refused_state("the customer's remote_assistance consent is expired: record it first"), "expired");
        assert_eq!(refused_state("nothing"), "");
        assert!(shifted(now, -15).ends_with(":45"));
        assert_eq!(mine_running(DeskApp::demo().data(), "riccardo").len(), 0);
    }
}
