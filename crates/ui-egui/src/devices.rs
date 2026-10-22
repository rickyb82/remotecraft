//! The Devices screen: every PC the desk looks after, found by name, host name or the RustDesk
//! ID a customer reads on the phone. The list sorts by customer and name (a searched RustDesk
//! ID first); the right pane is the PC's card — Connect, the RustDesk ID to copy, the consents
//! the connection depends on, the details and the interventions on it — or the new/edit form.
//! Everything goes through `device.add`, `device.update`, `device.retire`, `device.remove` and
//! `device.connect`; retiring and deleting always ask first.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{Align, Color32, FontId, Galley, Layout, Rect, Response, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;

use crate::core::privacy::PurposeState;
use crate::core::{Data, Device, fold, rustdesk};
use crate::derived;
use crate::engine::views;
use crate::forms::{self, FormAction, Picker};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone, icon_button};
use crate::{Completed, Confirm, DeskApp, Screen, format, interventions, privacy, timer};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("Pick the customer to save the PC", "Scegli il cliente per salvare il PC"),
    ("New PC", "Nuovo PC"),
    ("Edit PC", "Modifica PC"),
    ("PC saved", "PC salvato"),
    ("Customer: {name}", "Cliente: {name}"),
    ("1 device", "1 PC"),
    ("Show retired", "Mostra dismessi"),
    ("Unattended only", "Solo non presidiati"),
    ("No PCs yet", "Ancora nessun PC"),
    (
        "A PC needs its RustDesk ID, shown in the RustDesk window; search by that ID when a customer calls.",
        "Un PC ha bisogno del suo ID RustDesk, quello mostrato nella finestra di RustDesk: cercalo quando un cliente chiama.",
    ),
    ("No PC matches \"{query}\"", "Nessun PC corrisponde a \"{query}\""),
    ("No PC has the RustDesk ID {id}", "Nessun PC ha l'ID RustDesk {id}"),
    ("Add a PC with this ID", "Aggiungi un PC con questo ID"),
    ("Connected without a recorded consent", "Collegato senza un consenso registrato"),
    ("Copy ID", "Copia ID"),
    ("Copy", "Copia"),
    ("Command line: {command}", "Riga di comando: {command}"),
    ("No RustDesk ID", "Nessun ID RustDesk"),
    ("RustDesk ID", "ID RustDesk"),
    ("As shown in the RustDesk window", "Come appare nella finestra di RustDesk"),
    ("Not a RustDesk ID: 6–16 digits or letters", "Non è un ID RustDesk: 6–16 cifre o lettere"),
    ("Already on {device} of {customer}", "Già su {device} di {customer}"),
    ("Open that PC", "Apri quel PC"),
    ("Name", "Nome"),
    ("PC reception, Notebook Anna…", "PC reception, Notebook Anna…"),
    ("Hostname", "Nome host"),
    ("OS", "Sistema operativo"),
    ("Serial", "Numero di serie"),
    ("Location", "Posizione"),
    ("Used by", "Usato da"),
    ("Added", "Aggiunto"),
    ("Updated", "Aggiornato"),
    ("Details", "Dettagli"),
    ("Notes", "Note"),
    (
        "Unattended access (a permanent password is set in the client; it is never stored here)",
        "Accesso non presidiato (nel client è impostata una password permanente, che qui non viene mai salvata)",
    ),
    ("The customer's unattended-access consent is {state}", "Il consenso del cliente all'accesso non presidiato è {state}"),
    (
        "Unattended access without consent: remove the permanent password from the RustDesk client or record the consent.",
        "Accesso non presidiato senza consenso: togli la password permanente dal client RustDesk o registra il consenso.",
    ),
    ("Remote assistance consent: {state}", "Consenso all'assistenza remota: {state}"),
    ("Record the consent, or start with a reason", "Registra il consenso, oppure avvia indicando un motivo"),
    ("Record consent", "Registra consenso"),
    ("Not unattended any more", "Rimuovi l'accesso non presidiato"),
    ("Unattended", "Non presidiato"),
    ("Retired", "Dismesso"),
    ("Retire", "Dismetti"),
    ("Put back in service", "Rimetti in servizio"),
    ("Retire {name}?", "Dismettere {name}?"),
    ("The PC leaves the lists; its history is kept.", "Il PC esce dagli elenchi; il suo storico resta."),
    ("Device retired", "PC dismesso"),
    ("Device back in service", "PC rimesso in servizio"),
    ("Move to another customer", "Sposta a un altro cliente"),
    ("Move", "Sposta"),
    ("PC moved to {customer}", "PC spostato a {customer}"),
    ("Delete PC", "Elimina PC"),
    ("Delete the PC {name}?", "Eliminare il PC {name}?"),
    ("Deleting a PC cannot be undone.", "L'eliminazione di un PC non si può annullare."),
    ("PC deleted", "PC eliminato"),
    ("Only a PC without interventions can be deleted; retire it instead", "Si può eliminare solo un PC senza interventi; dismettilo invece"),
    ("Interventions on this device", "Interventi su questo PC"),
    ("No interventions on this PC yet", "Ancora nessun intervento su questo PC"),
    ("More", "Altro"),
];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `device.connect` answered: the consent state and the warnings.
    Connect,
    /// `device.add` / `device.update` (the form, or a one-click change such as unticking
    /// unattended access) answered.
    Saved,
    /// `device.retire` answered: retired, or back in service.
    Retired(bool),
    /// `device.remove` answered.
    Removed,
    /// `device.update {device, customer}` answered: the PC moved to another customer.
    Moved,
    /// `customer.add` or `customer.import` from the form's customer field: the form picks it.
    CustomerAdded,
    /// `intranet.search` from the form's customer field answered.
    IntranetSearched,
}

/// The new/edit PC form, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    /// The PC being edited; `None` for a new one.
    pub id: Option<u64>,
    pub customer: Picker,
    /// The customer is fixed (opened from a card or a filtered list, or editing: moving a PC is
    /// its own action).
    pub customer_locked: bool,
    pub name: String,
    /// As typed (`123 456 789`); the engine stores the normalised form.
    pub rustdesk_id: String,
    pub hostname: String,
    pub os: String,
    pub serial: String,
    pub location: String,
    pub assigned_to: String,
    pub unattended: bool,
    pub notes: String,
    /// The engine's refusal, translated.
    pub error: String,
    /// The PC a refusal says already has this RustDesk ID (the "Open that PC" button).
    pub conflict: Option<u64>,
}

/// The card's "Move to another customer" panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Move {
    pub device: u64,
    pub customer: Picker,
    /// The `device.update` in flight.
    pub job: Option<u64>,
    pub error: String,
}

/// What the rows read for every PC, rebuilt when the data's revision changes (lookups by id in
/// the data are linear; rows are drawn every frame).
#[derive(Default)]
struct Cache {
    revision: Option<u64>,
    /// Device id → position in `data.devices`.
    index: HashMap<u64, usize>,
    /// Customer id → name.
    names: HashMap<u64, String>,
    /// Device id → stamp of its latest intervention.
    last: HashMap<u64, String>,
    /// Devices with an intervention running.
    running: HashSet<u64>,
}

/// What the list was computed for: revision, search text, Show retired, Unattended only, the
/// customer filter.
type RowsKey = (u64, String, bool, bool, Option<u64>);

/// This screen's own state.
#[derive(Default)]
pub struct State {
    /// The open new/edit form (drawn in the right pane).
    pub form: Option<Form>,
    /// The form's save in flight: Save is disabled until it answers.
    pub busy: Option<u64>,
    /// The list shows retired PCs too.
    pub show_retired: bool,
    /// The list shows only PCs with unattended access.
    pub unattended_only: bool,
    /// Only this customer's PCs (a customer card's "Show all PCs").
    pub customer: Option<u64>,
    /// The open "Move to another customer" panel.
    pub move_to: Option<Move>,
    /// The warnings of the last save of a PC, shown on its card until another PC is opened.
    pub warnings: Option<(u64, Vec<String>)>,
    /// The intranet behind the form's customer field.
    pub intranet: forms::IntranetLookup,
    rows: Vec<u64>,
    rows_key: Option<RowsKey>,
    cache: Cache,
    /// A row to bring into view (after `j`/`k`), and the list's last scroll offset.
    scroll_to: Option<usize>,
    offset: f32,
}

/// The height of a row (two lines).
const ROW_H: f32 = 48.0;
/// Below this window width the pane replaces the list: the navigation (235 px), the pane (520)
/// and a list wide enough to read (about 480).
const NARROW: f32 = 1240.0;
/// The least width of the form's label column (the two grids line up).
const LABEL_W: f32 = 124.0;
/// The width the four hover actions cover at a row's right end.
const ROW_ACTIONS: f32 = 136.0;

// ---------------------------------------------------------------------------------------------
// The screen

/// What the right pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    None,
    /// A privacy form opened from the card or the form ("Record consent").
    Privacy,
    Form,
    Card(u64),
}

/// Draws the screen: the right pane (the privacy form, the PC form or the card), then the list.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    refresh_cache(app);
    refresh_rows(app);
    // The warnings of a save belong to the PC saved: drop them once another one is open.
    if app.pending_open.is_none() && app.devices.warnings.as_ref().is_some_and(|(id, _)| app.selected_device != Some(*id)) {
        app.devices.warnings = None;
    }
    let narrow = ui.ctx().content_rect().width() < NARROW;
    let pane = if privacy::form_customer(app).is_some() {
        Pane::Privacy
    } else if app.devices.form.is_some() {
        Pane::Form
    } else {
        match app.selected_device.filter(|id| app.data().device(*id).is_some()) {
            Some(id) => Pane::Card(id),
            None => Pane::None,
        }
    };
    match pane {
        Pane::None => {}
        Pane::Privacy => pane_ui(ui, t, narrow, |ui| {
            privacy::form_ui(app, ui, t);
        }),
        Pane::Form => pane_ui(ui, t, narrow, |ui| form_ui(app, ui, t)),
        Pane::Card(id) => pane_ui(ui, t, narrow, |ui| card(app, ui, t, id, narrow)),
    }
    if !narrow || pane == Pane::None {
        crate::central_panel(ui, t, |ui| list(app, ui, t));
    }
}

/// The right pane, or the whole centre when the window is narrow.
fn pane_ui(ui: &mut egui::Ui, t: &Tokens, narrow: bool, add: impl FnOnce(&mut egui::Ui)) {
    if !narrow {
        crate::detail_panel(ui, t, add);
        return;
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("device-pane").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                add(ui);
            });
        });
    });
}

/// Applies a finished command of this screen.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    match intent {
        Intent::Connect => connected(app, c),
        Intent::Saved => saved(app, c),
        Intent::Retired(retired) => {
            // A refusal is already in the status bar (`pump`).
            if c.result.is_ok() {
                let text = if retired { "Device retired" } else { "Device back in service" };
                app.toast(app.tr(text).to_string());
            }
        }
        Intent::Removed => {
            if let Ok(v) = &c.result {
                if v["removed"].as_u64().is_some_and(|id| app.selected_device == Some(id)) {
                    app.selected_device = None;
                }
                app.toast(app.tr("PC deleted").to_string());
            }
        }
        Intent::Moved => moved(app, c),
        Intent::CustomerAdded => customer_added(app, c),
        Intent::IntranetSearched => {
            app.devices.intranet.cached.finish(c);
        }
    }
}

/// The customer created (or imported from the intranet) from the form's customer field: the
/// form picks it while the field still names it (on a remote desk the answer can come after
/// more typing); a refusal shows in the form.
fn customer_added(app: &mut DeskApp, c: &Completed) {
    if app.devices.busy != Some(c.job) {
        return;
    }
    app.devices.busy = None;
    let Some(f) = app.devices.form.as_mut() else { return };
    match &c.result {
        Ok(v) => {
            let name = v["customer"]["name"].as_str().unwrap_or_default();
            if let Some(id) = forms::new_id(v, "customer")
                && f.customer.id.is_none()
                && fold(f.customer.text.trim()) == fold(name)
            {
                f.customer = Picker { text: name.to_string(), id: Some(id) };
                f.error.clear();
            }
        }
        Err(e) => f.error = app.language.tr_message(&e.to_string()),
    }
}

/// `device.connect` answered: a connection without a recorded consent is said aloud, the
/// warnings go to the status bar (an error is already there).
fn connected(app: &mut DeskApp, c: &Completed) {
    let Ok(v) = &c.result else { return };
    if v["consent_ok"].as_bool() == Some(false) {
        app.toast(app.tr("Connected without a recorded consent").to_string());
    }
    let warnings = forms::warnings(app, v);
    if !warnings.is_empty() {
        app.status = warnings.join(" · ");
    }
}

/// `device.add` / `device.update` answered. The form's own save closes it and opens the PC;
/// a one-click change (unticking unattended access) only toasts.
fn saved(app: &mut DeskApp, c: &Completed) {
    let from_form = app.devices.busy == Some(c.job);
    if from_form {
        app.devices.busy = None;
    }
    match &c.result {
        Ok(v) => {
            let warnings = forms::warnings(app, v);
            let id = forms::new_id(v, "device");
            if from_form {
                app.devices.form = None;
                if let Some(id) = id {
                    app.open_when_ready(Screen::Devices, id);
                }
            }
            if let Some(id) = id {
                app.devices.warnings = if warnings.is_empty() { None } else { Some((id, warnings)) };
            }
            app.toast(app.tr("PC saved").to_string());
        }
        Err(e) => {
            let raw = e.to_string();
            let message = app.language.tr_message(&raw);
            if from_form && let Some(f) = app.devices.form.as_mut() {
                f.error = message;
                f.conflict = conflict_of(&raw);
            }
        }
    }
}

/// `device.update {device, customer}` answered.
fn moved(app: &mut DeskApp, c: &Completed) {
    let mine = app.devices.move_to.as_ref().is_some_and(|m| m.job == Some(c.job));
    match &c.result {
        Ok(v) => {
            if mine {
                app.devices.move_to = None;
            }
            let id = forms::new_id(v, "device");
            let warnings = forms::warnings(app, v);
            if let Some(id) = id {
                app.devices.warnings = if warnings.is_empty() { None } else { Some((id, warnings)) };
                if app.selected_device == Some(id) {
                    app.selected_customer = v["device"]["customer_id"].as_u64();
                }
            }
            let customer = v["device"]["customer_name"].as_str().unwrap_or_default().to_string();
            app.toast(app.language.fill("PC moved to {customer}", &[("customer", &customer)]));
        }
        Err(e) => {
            let message = app.language.tr_message(&e.to_string());
            if mine && let Some(m) = app.devices.move_to.as_mut() {
                m.job = None;
                m.error = message;
            }
        }
    }
}

/// The id of the PC an "already on device N" refusal names.
fn conflict_of(message: &str) -> Option<u64> {
    let (_, rest) = message.split_once("is already on device ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).take(20).collect();
    digits.parse().ok()
}

/// Closes what this screen has open: the form, the move panel, or a privacy form drawn in this
/// screen's pane. Answers whether something was open.
pub fn close(app: &mut DeskApp) -> bool {
    if app.devices.form.take().is_some() {
        app.devices.busy = None;
        return true;
    }
    if app.devices.move_to.take().is_some() {
        return true;
    }
    privacy::form_customer(app).is_some() && privacy::close(app)
}

/// Opens the new-PC form (`n`, the header's button): for the customer the list is filtered by,
/// with the searched RustDesk ID when no PC has it yet.
pub fn new(app: &mut DeskApp) {
    let rid = derived::rustdesk_query(&app.search).filter(|r| !app.data().devices.iter().any(|d| !d.retired && &d.rustdesk_id == r));
    let customer = app.devices.customer;
    open_new(app, customer, &rid.unwrap_or_default());
}

/// Moves the selection by `delta` rows of the list (`j`/`k`); the pane follows.
pub fn step(app: &mut DeskApp, delta: i32) {
    refresh_cache(app);
    refresh_rows(app);
    let rows = &app.devices.rows;
    let Some(last) = rows.len().checked_sub(1) else { return };
    let current = app.selected_device.and_then(|id| rows.iter().position(|r| *r == id));
    let i = match current {
        Some(i) => {
            let moved = i64::try_from(i).unwrap_or(0).saturating_add(i64::from(delta));
            usize::try_from(moved.max(0)).unwrap_or(0).min(last)
        }
        None if delta < 0 => last,
        None => 0,
    };
    let Some(id) = rows.get(i).copied() else { return };
    select(app, id);
    app.devices.scroll_to = Some(i);
}

/// Selects a PC in the list (its customer too, for `p` and `l`).
fn select(app: &mut DeskApp, id: u64) {
    app.selected_device = Some(id);
    if let Some(d) = app.data().device(id) {
        app.selected_customer = Some(d.customer_id);
    }
    if app.devices.move_to.as_ref().is_some_and(|m| m.device != id) {
        app.devices.move_to = None;
    }
}

/// Opens the new-PC form on the Devices screen: for `customer` (the picker is locked then; a
/// picker when `None`), with a RustDesk ID prefilled (empty for none).
pub fn open_new(app: &mut DeskApp, customer: Option<u64>, rustdesk_id: &str) {
    let customer = customer.filter(|c| app.data().customer(*c).is_some_and(|c| c.erased_at.is_none()));
    let typed = crate::core::clip(rustdesk_id.trim(), 64).to_string();
    let shown = match rustdesk::normalize(&typed) {
        Ok(r) => rustdesk::display(&r),
        Err(_) => typed,
    };
    app.devices.form =
        Some(Form { customer: Picker::customer(app, customer), customer_locked: customer.is_some(), rustdesk_id: shown, ..Form::default() });
    app.devices.busy = None;
    app.devices.move_to = None;
    app.devices.intranet = forms::IntranetLookup::default();
    app.go(Screen::Devices);
}

/// Opens the edit form of a PC on the Devices screen (nothing when it does not exist).
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let Some(d) = app.data().device(id).cloned() else { return };
    app.open_device(id);
    app.devices.form = Some(Form {
        id: Some(id),
        customer: Picker::customer(app, Some(d.customer_id)),
        customer_locked: true,
        rustdesk_id: rustdesk::display(&d.rustdesk_id),
        name: d.name,
        hostname: d.hostname,
        os: d.os,
        serial: d.serial,
        location: d.location,
        assigned_to: d.assigned_to,
        unattended: d.unattended,
        notes: d.notes,
        error: String::new(),
        conflict: None,
    });
    app.devices.busy = None;
    app.devices.move_to = None;
}

// ---------------------------------------------------------------------------------------------
// The list

/// Rebuilds the rows' lookups when the data changed.
fn refresh_cache(app: &mut DeskApp) {
    let revision = app.session.revision();
    if app.devices.cache.revision == Some(revision) {
        return;
    }
    let data = app.session.data();
    let mut cache = Cache {
        revision: Some(revision),
        index: data.devices.iter().enumerate().map(|(i, d)| (d.id, i)).collect(),
        names: data.customers.iter().map(|c| (c.id, c.name.clone())).collect(),
        last: HashMap::new(),
        running: HashSet::new(),
    };
    for i in &data.interventions {
        let Some(device) = i.device_id else { continue };
        let last = cache.last.entry(device).or_default();
        if i.started_at > *last {
            last.clone_from(&i.started_at);
        }
        if i.running() {
            cache.running.insert(device);
        }
    }
    app.devices.cache = cache;
}

/// Recomputes the rows when the data, the search or a filter changed.
fn refresh_rows(app: &mut DeskApp) {
    let s = &app.devices;
    let key: RowsKey = (app.session.revision(), app.search.trim().to_string(), s.show_retired, s.unattended_only, s.customer);
    if s.rows_key.as_ref() == Some(&key) {
        return;
    }
    let rows = compute_rows(app.session.data(), &key.1, key.2, key.3, key.4);
    app.devices.rows = rows;
    app.devices.rows_key = Some(key);
}

/// The PCs to list, in order: a PC whose RustDesk ID is the searched one first, then by
/// customer and by name. The search matches the PC's fields (`views::device_matches`), its
/// RustDesk ID, or its customer's name.
fn compute_rows(data: &Data, search: &str, show_retired: bool, unattended_only: bool, customer: Option<u64>) -> Vec<u64> {
    let fq = fold(search);
    let rid = derived::rustdesk_query(search);
    let names: HashMap<u64, String> = data.customers.iter().map(|c| (c.id, fold(&c.name))).collect();
    let empty = String::new();
    let mut rows: Vec<(bool, &String, String, u64)> = data
        .devices
        .iter()
        .filter(|d| show_retired || !d.retired)
        .filter(|d| !unattended_only || d.unattended)
        .filter(|d| customer.is_none_or(|c| d.customer_id == c))
        .filter_map(|d| {
            let owner = names.get(&d.customer_id).unwrap_or(&empty);
            let by_id = rid.as_ref().is_some_and(|r| &d.rustdesk_id == r);
            (fq.is_empty() || by_id || views::device_matches(d, &fq) || owner.contains(fq.as_str())).then(|| (by_id, owner, fold(&d.name), d.id))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)).then_with(|| a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
    rows.into_iter().map(|r| r.3).collect()
}

/// The list: the header, then the rows (or an empty state).
fn list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    header(app, ui, t);
    let rows = std::mem::take(&mut app.devices.rows);
    if rows.is_empty() {
        empty(app, ui, t);
    } else {
        let viewport = ui.available_height();
        let mut area = egui::ScrollArea::vertical().id_salt("devices-list").auto_shrink([false, false]);
        if let Some(i) = app.devices.scroll_to.take() {
            let top = i as f32 * ROW_H;
            let offset = app.devices.offset;
            let wanted = if top < offset {
                top
            } else if top + ROW_H > offset + viewport {
                top + ROW_H - viewport
            } else {
                offset
            };
            area = area.vertical_scroll_offset(wanted.max(0.0));
        }
        let out = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
                    for id in rows.get(range).unwrap_or_default() {
                        if row(app, ui, t, *id, true).clicked() {
                            select(app, *id);
                        }
                    }
                })
            })
            .inner;
        if out.state.offset.y.is_finite() {
            app.devices.offset = out.state.offset.y;
        }
    }
    app.devices.rows = rows;
}

/// The header: the title, the count and New PC; under it the filter chips.
fn header(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let count = app.devices.rows.len();
    egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            ui.label(RichText::new(app.tr("Devices")).size(16.0).strong().color(t.ui_text));
            let query = app.search.trim();
            if !query.is_empty() {
                ui.label(RichText::new(format!("· \"{}\"", format::clipped(query, 40))).color(t.muted));
            }
            ui.label(RichText::new(count_text(app, count)).small().color(t.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::primary_button(ui, t, Some(Icon::Plus), app.tr("New PC")).on_hover_text(app.tip("device.add")).clicked() {
                    new(app);
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(c) = app.devices.customer {
                let name = app.devices.cache.names.get(&c).cloned().unwrap_or_default();
                let text = app.language.fill("Customer: {name}", &[("name", &format::clipped(&name, 32))]);
                if removable_chip(ui, t, &text).on_hover_text(app.tr("Close")).clicked() {
                    app.devices.customer = None;
                }
            }
            if toggle_chip(ui, t, app.tr("Show retired"), app.devices.show_retired).clicked() {
                app.devices.show_retired = !app.devices.show_retired;
            }
            if toggle_chip(ui, t, app.tr("Unattended only"), app.devices.unattended_only).clicked() {
                app.devices.unattended_only = !app.devices.unattended_only;
            }
        });
    });
}

/// The number of PCs listed, singular for one.
fn count_text(app: &DeskApp, count: usize) -> String {
    let pattern = if count == 1 { "1 device" } else { "{n} devices" };
    app.language.fill(pattern, &[("n", &count.to_string())])
}

/// A filter chip that toggles: accent with a check when on, outlined when off.
fn toggle_chip(ui: &mut egui::Ui, t: &Tokens, text: &str, on: bool) -> Response {
    let color = if on { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.5), color);
    let check = if on { 17.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0 + check, 26.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 13.0, t.active);
    } else {
        p.rect_filled(rect, 13.0, if resp.hovered() { t.hover } else { t.chrome });
        p.rect_stroke(rect, 13.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    let mut x = rect.left() + 11.0;
    if on {
        icons::paint(p, Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(13.0, 13.0)), Icon::Check, color);
        x += check;
    }
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// An accent chip with a × that removes the filter it names.
fn removable_chip(ui: &mut egui::Ui, t: &Tokens, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.5), t.accent);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 40.0, 26.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 13.0, if resp.hovered() { t.active.gamma_multiply(0.85) } else { t.active });
    p.galley(pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0), galley, t.accent);
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 15.0, rect.center().y), vec2(12.0, 12.0)), Icon::Close, t.accent);
    resp
}

/// The empty list: connecting, a RustDesk ID nobody has (with a button to add it), no match,
/// no PC at all, or nothing under the filters.
fn empty(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if app.session.problem().as_deref() == Some("connecting") && app.session.revision() == 0 {
        widgets::empty_state(ui, t, Icon::Sync, app.tr("Connecting…"), "");
        return;
    }
    let query = app.search.trim().to_string();
    if let Some(id) = derived::rustdesk_query(&query) {
        let title = app.language.fill("No PC has the RustDesk ID {id}", &[("id", &rustdesk::display(&id))]);
        widgets::empty_state(ui, t, Icon::Remote, &title, "");
        ui.add_space(10.0);
        let mut connect = false;
        ui.vertical_centered(|ui| {
            if app.search_quick_offer().is_some() {
                connect = widgets::primary_button(ui, t, Some(Icon::Remote), app.tr("Connect now")).clicked();
                ui.add_space(6.0);
            }
            if ui.button(app.tr("Add a PC with this ID")).clicked() {
                let customer = app.devices.customer;
                open_new(app, customer, &id);
            }
        });
        if connect {
            app.quick_connect(&id);
        }
    } else if !query.is_empty() {
        let title = app.language.fill("No PC matches \"{query}\"", &[("query", &format::clipped(&query, 60))]);
        widgets::empty_state(ui, t, Icon::Search, &title, "");
    } else if app.devices.customer.is_none() && !app.data().devices.iter().any(|d| !d.retired) {
        widgets::empty_state(
            ui,
            t,
            Icon::Device,
            app.tr("No PCs yet"),
            app.tr("A PC needs its RustDesk ID, shown in the RustDesk window; search by that ID when a customer calls."),
        );
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if widgets::primary_button(ui, t, Some(Icon::Plus), app.tr("New PC")).clicked() {
                new(app);
            }
        });
    } else {
        widgets::empty_state(ui, t, Icon::Device, app.tr("Nothing here yet"), "");
    }
}

// ---------------------------------------------------------------------------------------------
// A row

/// What a row shows of a PC.
struct RowInfo {
    name: String,
    customer: String,
    /// `123 456 789`, empty when the PC has no RustDesk ID.
    rustdesk: String,
    os: String,
    assigned_to: String,
    /// The Unattended chip's tone (Ok when the customer's unattended-access consent is given).
    unattended: Option<Tone>,
    retired: bool,
    running: bool,
    /// The latest intervention, as a short date.
    last: String,
}

fn row_info(app: &DeskApp, id: u64) -> Option<RowInfo> {
    let data = app.data();
    let cache = &app.devices.cache;
    let d = cache.index.get(&id).and_then(|i| data.devices.get(*i)).filter(|d| d.id == id).or_else(|| data.device(id))?;
    let unattended =
        d.unattended.then(|| if app.derived.purpose(d.customer_id, "unattended_access") == PurposeState::Given { Tone::Ok } else { Tone::Warn });
    Some(RowInfo {
        name: d.name.clone(),
        customer: cache.names.get(&d.customer_id).cloned().unwrap_or_else(|| crate::customer_name(data, d.customer_id)),
        rustdesk: rustdesk::display(&d.rustdesk_id),
        os: d.os.clone(),
        assigned_to: d.assigned_to.clone(),
        unattended,
        retired: d.retired,
        running: cache.running.contains(&id),
        last: cache.last.get(&id).map(|s| format::short(s, app.now(), app.italian())).unwrap_or_default(),
    })
}

/// A hover action of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAction {
    Connect,
    Start,
    Edit,
    Retire,
    PutBack,
}

/// Paints a galley centred on `cy` at `x` (twice, half a pixel apart, for bold); answers its
/// width.
fn put(p: &egui::Painter, x: f32, cy: f32, galley: Arc<Galley>, color: Color32, bold: bool) -> f32 {
    let size = galley.size();
    let at = pos2(x, cy - size.y / 2.0);
    if bold {
        p.galley(at + vec2(0.5, 0.0), galley.clone(), color);
    }
    p.galley(at, galley, color);
    size.x
}

/// A chip painted at `x` centred on `cy`; answers its width.
fn paint_chip(p: &egui::Painter, t: &Tokens, x: f32, cy: f32, text: &str, tone: Tone) -> f32 {
    let (fill, color) = widgets::tone_colors(t, tone);
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(11.5), color);
    let w = galley.size().x + 14.0;
    p.rect_filled(Rect::from_min_size(pos2(x, cy - 9.0), vec2(w, 18.0)), 9.0, fill);
    p.galley(pos2(x + 7.0, cy - galley.size().y / 2.0), galley, color);
    w
}

/// One PC row, 48 px: the Remote icon (Device without a RustDesk ID), the name in bold, the
/// customer when `with_customer`, the chips; under it the RustDesk ID in monospace, the OS and
/// who uses it; at the right the last intervention, replaced on hover by Connect, Start
/// intervention, Edit and Retire. Used by the list, customer cards and the "Who is" group; the
/// caller handles a click on the row (the actions are handled here).
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, with_customer: bool) -> Response {
    refresh_cache(app);
    let selected = app.screen == Screen::Devices && app.selected_device == Some(id);
    let (rect, resp) = widgets::row(ui, t, ROW_H, selected);
    let Some(info) = row_info(app, id) else { return resp };
    let hovering = ui.rect_contains_pointer(rect);
    let p = ui.painter().clone();
    if hovering && !selected && !resp.hovered() {
        // The pointer is on an action: keep the row's hover tint.
        p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 1.0)), 0.0, t.hover);
    }
    let has_id = !info.rustdesk.is_empty();
    let left = rect.left() + 14.0;
    let icon_color = if selected { t.accent } else { t.muted };
    icons::paint(
        &p,
        Rect::from_center_size(pos2(left + 9.0, rect.center().y), vec2(18.0, 18.0)),
        if has_id { Icon::Remote } else { Icon::Device },
        icon_color,
    );
    let x0 = left + 30.0;
    // The date keeps its column; the hover actions cover the row's right end instead.
    let date = (!info.last.is_empty()).then(|| p.layout_no_wrap(info.last.clone(), FontId::proportional(12.5), t.muted));
    let date_w = date.as_ref().map_or(0.0, |g| g.size().x + 14.0);
    let end = (rect.right() - 14.0 - date_w).max(x0 + 40.0);
    let avail = end - x0;
    let (y1, y2) = (rect.top() + 16.0, rect.top() + 33.0);
    // Line 1: the name, the customer, the chips (the customer gives way first, then chips).
    let mut chips: Vec<(&str, Tone)> = Vec::new();
    if info.running {
        chips.push((app.tr("Running"), Tone::Ok));
    }
    if let Some(tone) = info.unattended {
        chips.push((app.tr("Unattended"), tone));
    }
    if info.retired {
        chips.push((app.tr("Retired"), Tone::Neutral));
    }
    let name_color = if info.retired { t.muted } else { t.ui_text };
    let with_customer = with_customer && !info.customer.is_empty();
    let name_max = if with_customer { avail * 0.6 } else { avail };
    let name = widgets::truncate(&p, &info.name, FontId::proportional(14.0), name_color, name_max);
    let mut x = x0 + put(&p, x0, y1, name, name_color, true) + 8.0;
    let mut room = end - x;
    let chips: Vec<(&str, Tone, f32)> = chips
        .into_iter()
        .map(|(text, tone)| (text, tone, p.layout_no_wrap(text.to_string(), FontId::proportional(11.5), t.muted).size().x + 20.0))
        .filter(|(_, _, w)| {
            let fits = *w <= room;
            if fits {
                room -= w;
            }
            fits
        })
        .collect();
    if with_customer && room > 40.0 {
        let g = widgets::truncate(&p, &info.customer, FontId::proportional(13.0), t.muted, room - 8.0);
        x += put(&p, x, y1, g, t.muted, false) + 8.0;
    }
    for (text, tone, _) in chips {
        x += paint_chip(&p, t, x, y1, text, tone) + 6.0;
    }
    // Line 2: the RustDesk ID, the OS, who uses it.
    let mut x = x0;
    let (id_text, id_font, id_color) = if has_id {
        (info.rustdesk.as_str(), FontId::monospace(12.5), name_color)
    } else {
        (app.tr("No RustDesk ID"), FontId::proportional(12.0), t.muted)
    };
    let g = widgets::truncate(&p, id_text, id_font, id_color, avail);
    x += put(&p, x, y2, g, id_color, false) + 10.0;
    let details: Vec<&str> = [info.os.as_str(), info.assigned_to.as_str()].into_iter().filter(|s| !s.trim().is_empty()).collect();
    if !details.is_empty() && end - x > 30.0 {
        let g = widgets::truncate(&p, &details.join(" · "), FontId::proportional(12.0), t.muted, end - x);
        put(&p, x, y2, g, t.muted, false);
    }
    // The right column: the last intervention, or the actions on hover over the row's end.
    if !hovering {
        if let Some(g) = date {
            put(&p, rect.right() - 14.0 - g.size().x, rect.center().y, g, t.muted, false);
        }
        return resp;
    }
    let cover = Rect::from_min_max(pos2((rect.right() - ROW_ACTIONS).max(rect.left()), rect.top()), pos2(rect.right(), rect.bottom() - 1.0));
    p.rect_filled(cover, 0.0, if selected { t.active } else { t.hover });
    let retire = if info.retired { (RowAction::PutBack, "Put back in service") } else { (RowAction::Retire, "Retire") };
    let actions = [
        (Icon::Archive, retire.1, true, retire.0),
        (Icon::Edit, "Edit PC", true, RowAction::Edit),
        (Icon::Play, "Start intervention", true, RowAction::Start),
        (Icon::Connect, if has_id { "Connect" } else { "No RustDesk ID" }, has_id, RowAction::Connect),
    ];
    let mut clicked = None;
    let mut hx = rect.right() - 10.0;
    for (icon, tip, enabled, action) in actions {
        let r = Rect::from_center_size(pos2(hx - 13.0, rect.center().y), vec2(26.0, 26.0));
        let hr = ui.interact(r, ui.id().with(("device-row-action", id, tip)), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && hr.hovered() {
            p.rect_filled(r, 6.0, t.active);
        }
        icons::paint(&p, r.shrink(5.0), icon, if enabled { t.ui_text } else { t.muted.gamma_multiply(0.55) });
        if hr.on_hover_text(app.tr(tip)).clicked() && enabled {
            clicked = Some(action);
        }
        hx -= 30.0;
    }
    match clicked {
        Some(RowAction::Connect) => app.connect(id),
        Some(RowAction::Start) => start(app, id),
        Some(RowAction::Edit) => open_edit(app, id),
        Some(RowAction::Retire) => ask_retire(app, id),
        Some(RowAction::PutBack) => put_back(app, id),
        None => {}
    }
    resp
}

// ---------------------------------------------------------------------------------------------
// Actions

/// Starts work on a PC: the fast path (timer and link at once) when it has a RustDesk ID and is
/// in service, else the start card on it.
fn start(app: &mut DeskApp, id: u64) {
    let Some((customer, remote)) = app.data().device(id).map(|d| (d.customer_id, !d.rustdesk_id.is_empty() && !d.retired)) else { return };
    if remote {
        timer::start_now(app, id);
    } else {
        timer::open_start(app, Some(customer), Some(id));
    }
}

/// Asks before retiring a PC.
fn ask_retire(app: &mut DeskApp, id: u64) {
    let Some(name) = app.data().device(id).map(|d| d.name.clone()) else { return };
    app.ask(Confirm {
        title: app.language.fill("Retire {name}?", &[("name", &format::clipped(&name, 60))]),
        text: app.tr("The PC leaves the lists; its history is kept.").to_string(),
        ok_label: app.tr("Retire").to_string(),
        danger: true,
        command: "device.retire".into(),
        params: json!({"device": id, "retired": true}),
        intent: crate::Intent::Devices(Intent::Retired(true)),
    });
}

/// Puts a retired PC back in service (nothing to confirm: nothing is lost).
fn put_back(app: &mut DeskApp, id: u64) {
    app.run_as("device.retire", json!({"device": id, "retired": false}), crate::Intent::Devices(Intent::Retired(false)));
}

/// Asks before deleting a PC (the engine refuses one with interventions).
fn ask_delete(app: &mut DeskApp, id: u64) {
    let Some(name) = app.data().device(id).map(|d| d.name.clone()) else { return };
    app.ask(Confirm {
        title: app.language.fill("Delete the PC {name}?", &[("name", &format::clipped(&name, 60))]),
        text: app.tr("Deleting a PC cannot be undone.").to_string(),
        ok_label: app.tr("Delete").to_string(),
        danger: true,
        command: "device.remove".into(),
        params: json!({"device": id}),
        intent: crate::Intent::Devices(Intent::Removed),
    });
}

/// Unticks unattended access (the permanent password was removed from the client).
fn untick(app: &mut DeskApp, id: u64) {
    app.run_as("device.update", json!({"device": id, "unattended": false}), crate::Intent::Devices(Intent::Saved));
}

/// Moves a PC to another customer.
fn move_device(app: &mut DeskApp, id: u64, customer: u64) {
    let job = app.run_as("device.update", json!({"device": id, "customer": customer}), crate::Intent::Devices(Intent::Moved));
    if let Some(m) = app.devices.move_to.as_mut().filter(|m| m.device == id) {
        m.job = Some(job);
        m.error.clear();
    }
}

/// The RustDesk ID the form would save differs from the edited PC's (always for a new PC): only
/// then is it sent and checked against the other PCs. An unchanged ID is the PC's own, even
/// when another PC in service has it too (the old PC retired, the ID reused).
fn rustdesk_changed(data: &Data, f: &Form) -> bool {
    let stored = f.id.and_then(|id| data.device(id)).map(|d| d.rustdesk_id.as_str());
    stored.is_none_or(|stored| !rustdesk::normalize(f.rustdesk_id.trim()).is_ok_and(|r| r == stored))
}

/// Sends the form: `device.add`, or `device.update` with every field (the RustDesk ID only when
/// it changed, the customer only when it changed).
fn submit(app: &mut DeskApp) {
    let Some(f) = app.devices.form.clone() else { return };
    let Some(customer) = f.customer.id else { return };
    let mut params = json!({
        "name": f.name.trim(),
        "hostname": f.hostname.trim(),
        "os": f.os.trim(),
        "serial": f.serial.trim(),
        "location": f.location.trim(),
        "assigned_to": f.assigned_to.trim(),
        "unattended": f.unattended,
        "notes": f.notes,
    });
    if rustdesk_changed(app.data(), &f) {
        params["rustdesk_id"] = json!(f.rustdesk_id.trim());
    }
    let command = match f.id {
        Some(id) => {
            params["device"] = json!(id);
            if app.data().device(id).is_some_and(|d| d.customer_id != customer) {
                params["customer"] = json!(customer);
            }
            "device.update"
        }
        None => {
            params["customer"] = json!(customer);
            "device.add"
        }
    };
    let job = app.run_as(command, params, crate::Intent::Devices(Intent::Saved));
    app.devices.busy = Some(job);
    if let Some(f) = app.devices.form.as_mut() {
        f.error.clear();
        f.conflict = None;
    }
}

/// An active PC other than `except` that already has the RustDesk ID `rid`: `(id, name,
/// customer)`.
fn duplicate(data: &Data, rid: &str, except: Option<u64>) -> Option<(u64, String, String)> {
    if rid.is_empty() {
        return None;
    }
    data.devices
        .iter()
        .find(|d| Some(d.id) != except && !d.retired && d.rustdesk_id == rid)
        .map(|d| (d.id, d.name.clone(), crate::customer_name(data, d.customer_id)))
}

/// `widgets::banner`'s look laid out to the pane's width: the text wraps, the actions go on the
/// line under it (long labels would push the pane wider), and the tone's bar is as tall as the
/// banner. Answers the index of the action clicked.
fn banner(ui: &mut egui::Ui, t: &Tokens, tone: Tone, icon: Icon, text: &str, actions: &[&str]) -> Option<usize> {
    widgets::banner(ui, t, tone, icon, text, actions)
}

/// A consent state as a lowercase word for sentences (`missing`, `mancante`).
fn state_word(app: &DeskApp, state: PurposeState) -> String {
    let label = match state {
        PurposeState::Given => "Given",
        PurposeState::Expired => "Expired",
        PurposeState::Withdrawn => "Withdrawn",
        PurposeState::Missing => "Missing",
    };
    app.tr(label).to_lowercase()
}

// ---------------------------------------------------------------------------------------------
// The form

/// What the form asked for besides Save and Cancel.
enum FormRequest {
    OpenConflict(u64),
    RecordConsent(u64),
}

/// Draws the open new/edit PC form (in the right pane): the customer, the name, the RustDesk ID
/// normalised as you type and checked against the other PCs, the other fields, unattended
/// access with the consent it needs, the notes; Save and Cancel.
pub fn form_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(mut f) = app.devices.form.clone() else { return };
    let busy = app.devices.busy.is_some();
    ui.label(RichText::new(app.tr(if f.id.is_some() { "Edit PC" } else { "New PC" })).size(20.0).strong().color(t.ui_text));
    if let Some(d) = f.id.and_then(|id| app.data().device(id)) {
        widgets::key_value(ui, t, app.tr("Updated"), &format::long(&d.updated_at, app.italian()));
    }
    ui.add_space(8.0);
    let mut request = None;
    let mut enabled = false;
    let mut picker = forms::PickerAnswer::default();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("device-form-grid").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            let customer_label = format!("{} *", app.tr("Customer"));
            let options = forms::PickOptions { locked: f.customer_locked, create: !busy, intranet: Some(&app.devices.intranet) };
            picker = forms::customer_picker(app, ui, t, &customer_label, &mut f.customer, options);
            widgets::field(ui, t, app.tr("Name"), &mut f.name, app.tr("PC reception, Notebook Anna…"));
            widgets::field(ui, t, app.tr("RustDesk ID"), &mut f.rustdesk_id, app.tr("As shown in the RustDesk window"));
            let rid = rustdesk::normalize(&f.rustdesk_id);
            // Only a new or changed ID is checked (and sent): the PC keeps its own.
            let dup = rid.as_ref().ok().filter(|_| rustdesk_changed(app.data(), &f)).and_then(|r| duplicate(app.data(), r, f.id));
            match (&rid, &dup) {
                (Err(_), _) => {
                    ui.label("");
                    ui.label(RichText::new(app.tr("Not a RustDesk ID: 6–16 digits or letters")).small().color(widgets::danger(t)));
                    ui.end_row();
                }
                (Ok(_), Some((other, device, customer))) => {
                    ui.label("");
                    ui.horizontal_wrapped(|ui| {
                        let text =
                            app.language.fill("Already on {device} of {customer}", &[("device", device.as_str()), ("customer", customer.as_str())]);
                        ui.label(RichText::new(text).small().color(widgets::danger(t)));
                        if ui.small_button(app.tr("Open that PC")).clicked() {
                            request = Some(FormRequest::OpenConflict(*other));
                        }
                    });
                    ui.end_row();
                }
                (Ok(r), None) if !r.is_empty() => {
                    ui.label("");
                    ui.label(RichText::new(format!("→ {}", rustdesk::display(r))).font(FontId::monospace(12.0)).color(t.muted));
                    ui.end_row();
                }
                (Ok(_), None) => {}
            }
            widgets::field(ui, t, app.tr("Hostname"), &mut f.hostname, "");
            widgets::field(ui, t, app.tr("OS"), &mut f.os, "");
            widgets::field(ui, t, app.tr("Serial"), &mut f.serial, "");
            widgets::field(ui, t, app.tr("Location"), &mut f.location, "");
            widgets::field(ui, t, app.tr("Used by"), &mut f.assigned_to, "");
            enabled = f.customer.id.is_some() && rid.is_ok() && dup.is_none();
        });
        // Outside the grid, so the long label wraps instead of widening the pane.
        ui.add_space(8.0);
        ui.checkbox(&mut f.unattended, app.tr("Unattended access (a permanent password is set in the client; it is never stored here)"));
        if let Some(c) = f.customer.id.filter(|_| f.unattended) {
            let state = app.derived.purpose(c, "unattended_access");
            if state != PurposeState::Given {
                ui.horizontal_wrapped(|ui| {
                    let (_, warn) = widgets::tone_colors(t, Tone::Warn);
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Warning, warn);
                    let text = app.language.fill("The customer's unattended-access consent is {state}", &[("state", &state_word(app, state))]);
                    ui.label(RichText::new(text).small().color(warn));
                    if ui.small_button(app.tr("Record consent")).clicked() {
                        request = Some(FormRequest::RecordConsent(c));
                    }
                });
            }
        }
        ui.add_space(8.0);
        egui::Grid::new("device-form-notes").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            widgets::multiline(ui, t, app.tr("Notes"), &mut f.notes, "", 4);
        });
        ui.add_space(4.0);
        widgets::error_line(ui, t, &f.error);
        if let Some(other) = f.conflict.filter(|o| app.data().device(*o).is_some())
            && ui.small_button(app.tr("Open that PC")).clicked()
        {
            request = Some(FormRequest::OpenConflict(other));
        }
        // Why Save is off while no customer is picked (the field says why under it).
        if !busy && f.customer.id.is_none() {
            ui.label(RichText::new(app.tr("Pick the customer to save the PC")).small().color(t.muted));
        }
    });
    let action = forms::submit_row(app, ui, t, app.tr("Save"), busy, enabled);
    if let Some(name) = picker.create {
        let job = app.run_as("customer.add", json!({"name": name}), crate::Intent::Devices(Intent::CustomerAdded));
        app.devices.busy = Some(job);
        app.devices.form = Some(f);
        return;
    }
    if let Some(query) = picker.search {
        // A click is a request: searched again even after an error, and a search still out is
        // forgotten (its answer would be another text's).
        app.devices.intranet = forms::IntranetLookup { query: query.clone(), cached: derived::Cached::default() };
        derived::refresh(
            app,
            |a| &mut a.devices.intranet.cached,
            "intranet.search",
            json!({"query": query}),
            crate::Intent::Devices(Intent::IntranetSearched),
        );
        app.devices.form = Some(f);
        return;
    }
    if let Some((intranet_id, _)) = picker.import {
        let job = app.run_as("customer.import", json!({"intranet_id": intranet_id}), crate::Intent::Devices(Intent::CustomerAdded));
        app.devices.busy = Some(job);
        app.devices.form = Some(f);
        return;
    }
    match request {
        Some(FormRequest::OpenConflict(other)) => {
            app.devices.form = None;
            app.devices.busy = None;
            app.open_device(other);
            return;
        }
        Some(FormRequest::RecordConsent(c)) => {
            app.devices.form = Some(f);
            let purposes: &[&str] =
                if app.derived.gate(c).0 == PurposeState::Given { &["unattended_access"] } else { &["remote_assistance", "unattended_access"] };
            privacy::open_consent(app, c, purposes);
            return;
        }
        None => {}
    }
    match action {
        FormAction::Cancel => {
            app.devices.form = None;
            app.devices.busy = None;
        }
        FormAction::Save => {
            app.devices.form = Some(f);
            submit(app);
        }
        FormAction::None => app.devices.form = Some(f),
    }
}

// ---------------------------------------------------------------------------------------------
// The card

/// The PC's card: the action bar, the title, the RustDesk block, the details, the interventions
/// on it and the notes.
fn card(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, narrow: bool) {
    let Some(d) = app.data().device(id).cloned() else { return };
    let mut work: Vec<(&str, u64)> =
        app.data().interventions.iter().filter(|i| i.device_id == Some(id)).map(|i| (i.started_at.as_str(), i.id)).collect();
    work.sort_by(|a, b| b.0.cmp(a.0).then(b.1.cmp(&a.1)));
    let total = work.len();
    let recent: Vec<u64> = work.into_iter().take(20).map(|w| w.1).collect();
    action_bar(app, ui, t, &d, total, narrow);
    ui.add_space(10.0);
    if let Some((_, warnings)) = app.devices.warnings.clone().filter(|(w, _)| *w == id) {
        for w in &warnings {
            if banner(ui, t, Tone::Warn, Icon::Warning, w, &[app.tr("Close")]).is_some() {
                app.devices.warnings = None;
            }
        }
    }
    move_panel(app, ui, t, &d);
    title(app, ui, t, &d);
    ui.add_space(8.0);
    rustdesk_block(app, ui, t, &d);
    widgets::section(ui, t, app.tr("Details"));
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let italian = app.italian();
        widgets::key_value(ui, t, app.tr("Hostname"), &d.hostname);
        widgets::key_value(ui, t, app.tr("OS"), &d.os);
        widgets::key_value(ui, t, app.tr("Serial"), &d.serial);
        widgets::key_value(ui, t, app.tr("Location"), &d.location);
        widgets::key_value(ui, t, app.tr("Used by"), &d.assigned_to);
        widgets::key_value(ui, t, app.tr("Added"), &format::long(&d.created_at, italian));
        widgets::key_value(ui, t, app.tr("Updated"), &format::long(&d.updated_at, italian));
    });
    ui.horizontal(|ui| {
        widgets::section(ui, t, app.tr("Interventions on this device"));
        if total > 0 {
            ui.label(RichText::new(total.to_string()).small().color(t.muted));
        }
    });
    if recent.is_empty() {
        ui.label(RichText::new(app.tr("No interventions on this PC yet")).color(t.muted));
    }
    for iid in recent {
        if interventions::row(app, ui, t, iid, false).clicked() {
            app.open_intervention(iid);
        }
    }
    if !d.notes.trim().is_empty() {
        widgets::section(ui, t, app.tr("Notes"));
        ui.add(egui::Label::new(RichText::new(&d.notes).color(t.ui_text)).wrap().selectable(true));
    }
    ui.add_space(12.0);
}

/// The card's action bar: Connect (disabled without a RustDesk ID), Start intervention, Edit,
/// More (retire or put back, move, delete) and Close; Back first when the window is narrow.
fn action_bar(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, d: &Device, interventions: usize, narrow: bool) {
    let id = d.id;
    ui.horizontal(|ui| {
        if narrow && icon_button(ui, t, Icon::Back, app.tr("Back"), true, false).clicked() {
            app.selected_device = None;
            app.devices.move_to = None;
        }
        if d.rustdesk_id.is_empty() {
            widgets::icon_text_button(ui, t, Icon::Connect, app.tr("Connect"), false).on_hover_text(app.tr("No RustDesk ID"));
        } else if widgets::primary_button(ui, t, Some(Icon::Connect), app.tr("Connect"))
            .on_hover_text(rustdesk::connect_uri(&d.rustdesk_id))
            .clicked()
        {
            app.connect(id);
        }
        ui.add_space(4.0);
        if icon_button(ui, t, Icon::Play, app.tr("Start intervention"), true, false).clicked() {
            start(app, id);
        }
        if icon_button(ui, t, Icon::Edit, app.tr("Edit PC"), true, false).clicked() {
            open_edit(app, id);
        }
        let more = icon_button(ui, t, Icon::More, app.tr("More"), true, false);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(220.0);
            if d.retired {
                if ui.button(app.tr("Put back in service")).clicked() {
                    put_back(app, id);
                    ui.close();
                }
            } else if ui.button(app.tr("Retire")).clicked() {
                ask_retire(app, id);
                ui.close();
            }
            if ui.button(app.tr("Move to another customer")).clicked() {
                app.devices.move_to = Some(Move { device: id, ..Move::default() });
                ui.close();
            }
            ui.separator();
            let can_delete = interventions == 0;
            let color = if can_delete { widgets::danger(t) } else { t.muted };
            let r = ui
                .add_enabled(can_delete, egui::Button::new(RichText::new(app.tr("Delete PC")).color(color)))
                .on_disabled_hover_text(app.tr("Only a PC without interventions can be deleted; retire it instead"));
            if r.clicked() {
                ask_delete(app, id);
                ui.close();
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, t, Icon::Close, app.tr("Close"), true, false).clicked() {
                app.selected_device = None;
                app.devices.move_to = None;
            }
        });
    });
}

/// The "Move to another customer" panel, when open for this PC.
fn move_panel(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, d: &Device) {
    let Some(mut m) = app.devices.move_to.clone().filter(|m| m.device == d.id) else { return };
    let mut go = false;
    let mut cancel = false;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(app.tr("Move to another customer")).strong().color(t.ui_text));
        ui.add_space(4.0);
        egui::Grid::new("device-move-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            forms::customer_picker(app, ui, t, app.tr("Customer"), &mut m.customer, forms::PickOptions::default());
        });
        widgets::error_line(ui, t, &m.error);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let target = m.customer.id.filter(|c| *c != d.customer_id);
                if m.job.is_some() {
                    ui.add_enabled(false, egui::Button::new(app.tr("Saving…")));
                } else if target.is_some() {
                    go = widgets::primary_button(ui, t, None, app.tr("Move")).clicked();
                } else {
                    ui.add_enabled(false, egui::Button::new(app.tr("Move")));
                }
                cancel = ui.button(app.tr("Cancel")).clicked();
            });
        });
    });
    ui.add_space(8.0);
    if cancel {
        app.devices.move_to = None;
        return;
    }
    let target = m.customer.id;
    app.devices.move_to = Some(m);
    if go && let Some(c) = target {
        move_device(app, d.id, c);
    }
}

/// The title and the subtitle: the customer (a link), host name, OS, `#id`, the chips.
fn title(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, d: &Device) {
    ui.add(egui::Label::new(RichText::new(&d.name).size(20.0).strong().color(t.ui_text)).wrap());
    let customer = crate::customer_name(app.data(), d.customer_id);
    let unattended = d.unattended.then(|| app.derived.purpose(d.customer_id, "unattended_access") == PurposeState::Given);
    let running = app.devices.cache.running.contains(&d.id);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !customer.is_empty() && ui.link(RichText::new(&customer).color(t.accent)).on_hover_text(app.tip("customer.get")).clicked() {
            app.open_customer(d.customer_id);
        }
        for part in [d.hostname.as_str(), d.os.as_str()].into_iter().filter(|s| !s.trim().is_empty()) {
            ui.label(RichText::new("·").color(t.muted));
            ui.label(RichText::new(part).color(t.muted));
        }
        ui.label(RichText::new(format!("· #{}", d.id)).color(t.muted));
        if running {
            widgets::tone_chip(ui, t, app.tr("Running"), Tone::Ok);
        }
        if let Some(given) = unattended {
            widgets::tone_chip(ui, t, app.tr("Unattended"), if given { Tone::Ok } else { Tone::Warn });
        }
        if d.retired {
            widgets::tone_chip(ui, t, app.tr("Retired"), Tone::Neutral);
        }
    });
}

/// The RustDesk block: the ID to read and copy, the remote-assistance consent the connection
/// depends on, the unattended-access warning, and the command line for when nothing opens.
fn rustdesk_block(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, d: &Device) {
    let customer = d.customer_id;
    let (state, event) = {
        let (state, row) = app.derived.gate(customer);
        (state, row.and_then(|r| r.event))
    };
    let consent = event.and_then(|e| app.data().privacy.iter().find(|p| p.id == e)).map(|e| {
        let method = app.tr(forms::method_label(e.method)).to_string();
        let parts: Vec<String> =
            [method, format::date(&e.at, app.italian()), e.signer.clone()].into_iter().filter(|s| !s.trim().is_empty()).collect();
        parts.join(", ")
    });
    let unattended_state = app.derived.purpose(customer, "unattended_access");
    let has_id = !d.rustdesk_id.is_empty();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(app.tr("RustDesk ID")).small().color(t.muted));
        if has_id {
            ui.horizontal(|ui| {
                widgets::mono(ui, t, &rustdesk::display(&d.rustdesk_id), 24.0);
                if widgets::copy_button(ui, t, app.tr("Copy ID"), &d.rustdesk_id) {
                    app.toast(app.tr("Copied").to_string());
                }
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(app.tr("No RustDesk ID")).size(16.0).color(t.muted));
                if ui.small_button(app.tr("Edit PC")).clicked() {
                    open_edit(app, d.id);
                }
            });
        }
        ui.add_space(8.0);
        // The remote-assistance consent.
        let tone = match state {
            PurposeState::Given => Tone::Ok,
            PurposeState::Expired => Tone::Warn,
            PurposeState::Withdrawn => Tone::Bad,
            PurposeState::Missing if has_id => Tone::Bad,
            PurposeState::Missing => Tone::Neutral,
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            widgets::tone_chip(ui, t, &app.language.fill("Remote assistance consent: {state}", &[("state", &state_word(app, state))]), tone);
            if state == PurposeState::Given
                && let Some(text) = &consent
            {
                ui.label(RichText::new(text).small().color(t.muted));
            }
        });
        if state != PurposeState::Given {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(app.tr("Record the consent, or start with a reason")).small().color(t.muted));
                if ui.small_button(app.tr("Record consent")).clicked() {
                    privacy::open_consent(app, customer, &["remote_assistance"]);
                }
            });
        }
        // Unattended access without its consent.
        if d.unattended && unattended_state != PurposeState::Given {
            ui.add_space(6.0);
            let text = app.tr("Unattended access without consent: remove the permanent password from the RustDesk client or record the consent.");
            match banner(ui, t, Tone::Warn, Icon::Lock, text, &[app.tr("Record consent"), app.tr("Not unattended any more")]) {
                Some(0) => {
                    let purposes: &[&str] =
                        if state == PurposeState::Given { &["unattended_access"] } else { &["remote_assistance", "unattended_access"] };
                    privacy::open_consent(app, customer, purposes);
                }
                Some(_) => untick(app, d.id),
                None => {}
            }
        }
        // The command line, for when the link opens nothing.
        if has_id {
            ui.add_space(4.0);
            let command = rustdesk::connect_command(&d.rustdesk_id);
            ui.horizontal(|ui| {
                let text = app.language.fill("Command line: {command}", &[("command", &command)]);
                ui.add(egui::Label::new(RichText::new(text).font(FontId::monospace(12.0)).color(t.muted)).selectable(true));
                if widgets::copy_button(ui, t, app.tr("Copy"), &command) {
                    app.toast(app.tr("Copied").to_string());
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

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

    fn app() -> DeskApp {
        let mut app = DeskApp::demo();
        app.go(Screen::Devices);
        frame(&mut app);
        app
    }

    fn toast(app: &DeskApp) -> String {
        app.toast.as_ref().map(|(t, _)| t.clone()).unwrap_or_default()
    }

    /// Answers the open confirmation as the OK button does.
    fn confirm(app: &mut DeskApp) {
        let c = app.confirm.take().expect("a confirmation");
        app.run_as(&c.command, c.params, c.intent);
        frame(app);
    }

    #[test]
    fn the_screen_renders_with_every_pane_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                for width in [1440.0, 900.0] {
                    let mut app = app();
                    app.language = lang;
                    app.dark = dark;
                    frame_at(&mut app, width);
                    for id in 1..=5 {
                        app.selected_device = Some(id);
                        frame_at(&mut app, width);
                    }
                    app.devices.move_to = Some(Move { device: 5, ..Move::default() });
                    app.devices.warnings = Some((5, vec!["device 5 has unattended access".into()]));
                    frame_at(&mut app, width);
                    new(&mut app);
                    frame_at(&mut app, width);
                    open_edit(&mut app, 1);
                    if let Some(f) = app.devices.form.as_mut() {
                        f.rustdesk_id = "100 200 301".into();
                        f.error = "refused".into();
                        f.conflict = Some(2);
                    }
                    frame_at(&mut app, width);
                    if let Some(f) = app.devices.form.as_mut() {
                        f.rustdesk_id = "abc/def".into();
                    }
                    frame_at(&mut app, width);
                    assert!(close(&mut app));
                    app.search = "999 888 777".into();
                    app.selected_device = None;
                    frame_at(&mut app, width);
                    app.search = "nessuno".into();
                    frame_at(&mut app, width);
                    app.search.clear();
                    app.devices.customer = Some(2);
                    app.devices.show_retired = true;
                    app.devices.unattended_only = true;
                    frame_at(&mut app, width);
                }
            }
        }
    }

    #[test]
    fn every_label_reads_in_italian() {
        for (en, itx) in LABELS_IT {
            assert_eq!(Language::It.tr(en), *itx, "{en} is translated elsewhere differently");
            assert_eq!(Language::En.tr(en), *en);
        }
    }

    #[test]
    fn an_empty_desk_shows_the_empty_state() {
        let mut app = DeskApp::new(Box::new(crate::Session::new(crate::engine::Desk::in_memory(Data::new()))));
        app.go(Screen::Devices);
        frame(&mut app);
        assert!(app.devices.rows.is_empty());
        step(&mut app, 1);
        assert_eq!(app.selected_device, None);
        new(&mut app);
        assert!(app.devices.form.as_ref().is_some_and(|f| f.customer.id.is_none() && !f.customer_locked));
        frame(&mut app);
    }

    #[test]
    fn rows_sort_by_customer_then_name_and_a_searched_id_comes_first() {
        let mut app = app();
        assert_eq!(app.devices.rows, vec![5, 4, 3, 1, 2], "Elio, Corvo, Lunaria (NAS, PC segreteria, Workstation)");
        app.search = "windows".into();
        refresh_rows(&mut app);
        assert_eq!(app.devices.rows, vec![5, 4, 1, 2]);
        app.search = "200 300 400".into();
        refresh_rows(&mut app);
        assert_eq!(app.devices.rows.first(), Some(&4));
        app.search = "lunaria".into();
        refresh_rows(&mut app);
        assert_eq!(app.devices.rows, vec![3, 1, 2], "the customer's name matches");
        app.search.clear();
        app.devices.unattended_only = true;
        refresh_rows(&mut app);
        assert_eq!(app.devices.rows, vec![1, 2]);
        app.devices.unattended_only = false;
        app.devices.customer = Some(2);
        refresh_rows(&mut app);
        assert_eq!(app.devices.rows, vec![4]);
    }

    #[test]
    fn j_and_k_walk_the_list() {
        let mut app = app();
        step(&mut app, 1);
        assert_eq!(app.selected_device, Some(5));
        assert_eq!(app.selected_customer, Some(3));
        step(&mut app, 1);
        assert_eq!(app.selected_device, Some(4));
        step(&mut app, -10);
        assert_eq!(app.selected_device, Some(5));
        step(&mut app, 100);
        assert_eq!(app.selected_device, Some(2));
        frame(&mut app);
        app.selected_device = None;
        step(&mut app, -1);
        assert_eq!(app.selected_device, Some(2));
    }

    #[test]
    fn a_new_pc_is_saved_opened_and_warned_about() {
        let mut app = app();
        open_new(&mut app, Some(2), "555-666-777");
        let f = app.devices.form.as_mut().expect("the form");
        assert!(f.customer_locked);
        assert_eq!(f.rustdesk_id, "555 666 777");
        f.name = "PC magazzino".into();
        f.unattended = true;
        frame(&mut app);
        submit(&mut app);
        assert!(app.devices.busy.is_some());
        frame(&mut app);
        assert!(app.devices.form.is_none(), "closed on success");
        assert!(app.devices.busy.is_none());
        let d = app.data().devices.iter().find(|d| d.name == "PC magazzino").expect("added");
        assert_eq!((d.customer_id, d.rustdesk_id.as_str(), d.unattended), (2, "555666777", true));
        let id = d.id;
        assert_eq!(app.selected_device, Some(id));
        assert_eq!(toast(&app), app.tr("PC saved"));
        assert!(app.devices.warnings.as_ref().is_some_and(|(w, list)| *w == id && !list.is_empty()), "no unattended consent: warned");
        frame(&mut app);
        // "Not unattended any more" from the card.
        untick(&mut app, id);
        frame(&mut app);
        assert!(app.data().device(id).is_some_and(|d| !d.unattended));
        assert!(app.devices.warnings.is_none());
    }

    #[test]
    fn a_duplicate_rustdesk_id_is_caught_locally_and_by_the_engine() {
        let mut app = app();
        open_new(&mut app, Some(2), "100 200 300");
        assert_eq!(duplicate(app.data(), "100200300", None).map(|d| d.0), Some(1));
        assert_eq!(duplicate(app.data(), "100200300", Some(1)), None);
        frame(&mut app);
        submit(&mut app);
        frame(&mut app);
        let f = app.devices.form.as_ref().expect("still open");
        assert!(f.error.contains("100 200 300"), "{}", f.error);
        assert_eq!(f.conflict, Some(1));
        assert!(app.devices.busy.is_none());
        assert_eq!(app.data().devices.len(), 5, "nothing added");
        // A form without a customer is not sent.
        open_new(&mut app, None, "");
        submit(&mut app);
        assert!(app.devices.busy.is_none());
        assert_eq!(conflict_of("RustDesk ID 1 is already on device 42 `x` of y"), Some(42));
        assert_eq!(conflict_of("is already on device "), None);
        assert_eq!(conflict_of("something else"), None);
    }

    #[test]
    fn editing_sends_every_field_and_keeps_the_customer() {
        let mut app = app();
        open_edit(&mut app, 4);
        assert_eq!(app.selected_device, Some(4));
        assert_eq!(app.screen, Screen::Devices);
        let f = app.devices.form.as_mut().expect("the form");
        assert_eq!(f.rustdesk_id, "200 300 400");
        f.os = "Windows 11 Pro".into();
        f.location = "Cassa".into();
        submit(&mut app);
        frame(&mut app);
        let d = app.data().device(4).expect("still there");
        assert_eq!((d.os.as_str(), d.location.as_str(), d.customer_id, d.rustdesk_id.as_str()), ("Windows 11 Pro", "Cassa", 2, "200300400"));
        assert!(app.devices.form.is_none());
        open_edit(&mut app, 999);
        assert!(app.devices.form.is_none(), "no such PC: no form");
    }

    #[test]
    fn retire_put_back_move_and_delete() {
        let mut app = app();
        ask_retire(&mut app, 2);
        assert!(app.confirm.as_ref().is_some_and(|c| c.command == "device.retire" && c.danger));
        confirm(&mut app);
        assert!(app.data().device(2).is_some_and(|d| d.retired));
        assert_eq!(toast(&app), app.tr("Device retired"));
        assert!(!app.devices.rows.contains(&2), "retired PCs leave the list");
        put_back(&mut app, 2);
        frame(&mut app);
        assert!(app.data().device(2).is_some_and(|d| !d.retired));
        assert_eq!(toast(&app), app.tr("Device back in service"));
        // Move.
        app.selected_device = Some(4);
        app.devices.move_to = Some(Move { device: 4, customer: Picker::customer(&app, Some(3)), ..Move::default() });
        move_device(&mut app, 4, 3);
        assert!(app.devices.move_to.as_ref().is_some_and(|m| m.job.is_some()));
        frame(&mut app);
        assert!(app.devices.move_to.is_none());
        assert!(app.data().device(4).is_some_and(|d| d.customer_id == 3));
        assert_eq!(app.selected_customer, Some(3));
        assert!(toast(&app).contains("Elio Brina"));
        // Delete: refused with interventions, done without.
        ask_delete(&mut app, 4);
        confirm(&mut app);
        assert!(app.data().device(4).is_some());
        open_new(&mut app, Some(1), "");
        submit(&mut app);
        frame(&mut app);
        let id = app.selected_device.expect("the new PC");
        ask_delete(&mut app, id);
        confirm(&mut app);
        assert!(app.data().device(id).is_none());
        assert_eq!(app.selected_device, None);
        assert_eq!(toast(&app), app.tr("PC deleted"));
    }

    #[test]
    fn connecting_without_consent_is_said() {
        let mut app = app();
        app.connect(5);
        assert!(app.requests.iter().any(|r| matches!(r, crate::Request::OpenUrl(u) if u.ends_with("300400500"))));
        frame(&mut app);
        assert_eq!(toast(&app), app.tr("Connected without a recorded consent"));
        assert!(!app.status.is_empty());
        app.toast = None;
        app.connect(1);
        frame(&mut app);
        assert_eq!(toast(&app), "", "consent given: nothing to say");
        app.connect(3);
        frame(&mut app);
        assert!(app.status.contains('3'), "no RustDesk ID: the refusal in the status bar: {}", app.status);
    }

    #[test]
    fn hostile_input_does_not_crash() {
        let mut app = app();
        open_new(&mut app, Some(999), &"9".repeat(100_000));
        let f = app.devices.form.as_ref().expect("the form");
        assert!(f.customer.id.is_none() && !f.customer_locked);
        assert!(f.rustdesk_id.len() <= 64);
        frame(&mut app);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let t = Tokens::for_dark_mode(false);
            row(&mut app, ui, &t, 999, true);
            row(&mut app, ui, &t, 1, false);
        });
        output.textures_delta.clear();
        assert_eq!(state_word(&app, PurposeState::Missing), app.tr("Missing").to_lowercase());
        ask_retire(&mut app, 999);
        ask_delete(&mut app, 999);
        assert!(app.confirm.is_none());
        start(&mut app, 999);
        app.devices.customer = Some(999);
        frame(&mut app);
        assert!(app.devices.rows.is_empty());
    }

    #[test]
    fn a_pc_keeps_its_own_rustdesk_id_when_another_pc_in_service_has_it() {
        let mut app = app();
        // The PC segreteria (1, 100 200 300) is retired and its ID goes to a new PC.
        app.run("device.retire", json!({"device": 1, "retired": true}));
        frame(&mut app);
        app.run("device.add", json!({"customer": 1, "name": "PC segreteria nuovo", "rustdesk_id": "100200300"}));
        frame(&mut app);
        let new_pc = app.data().devices.iter().find(|d| d.name == "PC segreteria nuovo").map(|d| d.id).expect("added");
        // The retired PC is edited with its ID unchanged: nothing to check, nothing refused.
        open_edit(&mut app, 1);
        let f = app.devices.form.as_mut().expect("the form");
        f.notes = "In magazzino".into();
        let f = f.clone();
        assert!(!rustdesk_changed(app.data(), &f));
        frame(&mut app);
        submit(&mut app);
        frame(&mut app);
        assert!(app.devices.form.is_none(), "saved: {:?}", app.devices.form.as_ref().map(|f| f.error.clone()));
        let d = app.data().device(1).expect("there");
        assert_eq!((d.notes.as_str(), d.rustdesk_id.as_str(), d.retired), ("In magazzino", "100200300", true));
        // A changed ID is still checked against the PCs in service.
        open_edit(&mut app, 1);
        if let Some(f) = app.devices.form.as_mut() {
            f.rustdesk_id = "100 200 301".into();
        }
        let f = app.devices.form.clone().expect("the form");
        assert!(rustdesk_changed(app.data(), &f));
        assert_eq!(duplicate(app.data(), "100200301", f.id).map(|d| d.0), Some(2));
        assert!(close(&mut app));
        // Back in service it would share the ID with the new PC: refused, said in the status bar.
        app.toast = None;
        put_back(&mut app, 1);
        frame(&mut app);
        assert!(app.data().device(1).is_some_and(|d| d.retired), "still retired");
        assert!(!app.status.is_empty());
        assert_ne!(toast(&app), app.tr("Device back in service"));
        // The new PC keeps its ID too.
        open_edit(&mut app, new_pc);
        if let Some(f) = app.devices.form.as_mut() {
            f.location = "Reception".into();
        }
        submit(&mut app);
        frame(&mut app);
        assert!(app.devices.form.is_none());
        assert!(app.data().device(new_pc).is_some_and(|d| d.location == "Reception" && d.rustdesk_id == "100200300"));
        // A new PC is always checked.
        open_new(&mut app, Some(1), "100 200 300");
        let f = app.devices.form.clone().expect("the form");
        assert!(rustdesk_changed(app.data(), &f));
    }

    /// The texts painted in one frame of the app at `w`×`h`.
    fn texts_at(app: &mut DeskApp, w: f32, h: f32) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(w, h))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
        let mut out = Vec::new();
        for c in &output.shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// One frame of a context that lives across frames (the focus), with these events: the
    /// texts painted and where.
    fn frame_with(ctx: &egui::Context, app: &mut DeskApp, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect())),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0))), events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
        let mut out = Vec::new();
        for c in &output.shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// Frames until no command waits on another thread (the intranet), then one more: the
    /// texts painted.
    fn settle(ctx: &egui::Context, app: &mut DeskApp) -> Vec<(String, Rect)> {
        for _ in 0..500 {
            if app.session.jobs_pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            frame_with(ctx, app, Vec::new());
        }
        frame_with(ctx, app, Vec::new())
    }

    /// Clicks, as a mouse does (press, then release a frame later), the rightmost text painted
    /// as `text` (the pane's, not the list's); answers the texts after it.
    fn click_text(ctx: &egui::Context, app: &mut DeskApp, text: &str) -> Vec<(String, Rect)> {
        let texts = frame_with(ctx, app, Vec::new());
        let at = texts
            .iter()
            .filter(|(t, _)| t == text)
            .map(|(_, r)| r.center())
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap_or_else(|| panic!("{text:?} is not painted: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame_with(ctx, app, vec![egui::Event::PointerMoved(at), button(true)]);
        frame_with(ctx, app, vec![button(false)]);
        frame_with(ctx, app, Vec::new())
    }

    #[test]
    fn connect_now_is_offered_for_digits_only() {
        let mut app = app();
        let connect = app.tr("Connect now").to_string();
        let add = app.tr("Add a PC with this ID").to_string();
        app.search = "555 666 777".into();
        let texts = texts_at(&mut app, 1440.0, 900.0);
        assert!(texts.contains(&connect) && texts.contains(&add), "{texts:?}");
        // A word reads as a RustDesk ID too: it can be added, never dialled from the search.
        app.search = "Zanzibar".into();
        let texts = texts_at(&mut app, 1440.0, 900.0);
        assert!(!texts.contains(&connect) && texts.contains(&add), "{texts:?}");
        // Nor a customer's phone number, nor the ID of a retired PC (hidden from the list).
        app.run("device.retire", json!({"device": 1}));
        frame(&mut app);
        for search in ["0471 000002", "100 200 300"] {
            app.search = search.into();
            let texts = texts_at(&mut app, 1440.0, 900.0);
            assert!(!texts.contains(&connect), "{search}: {texts:?}");
        }
    }

    #[test]
    fn the_intranet_search_is_asked_again_on_a_click_and_says_archived() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        // An intranet that fails until told otherwise, counting the calls.
        let calls = Arc::new(AtomicUsize::new(0));
        let down = Arc::new(Mutex::new(true));
        let mut desk = crate::engine::Desk::demo();
        let (c, d) = (calls.clone(), down.clone());
        let long = "Lunghissima Società Cooperativa Agricola di Trasformazione Prodotti Ortofrutticoli del Nuorese S.c.a.r.l.";
        desk.set_intranet(Box::new(move |_, command, params| {
            c.fetch_add(1, Ordering::SeqCst);
            if *d.lock().unwrap() {
                return Err("the intranet is unreachable (refused)".into());
            }
            let row = json!({"id": 9, "name": long, "vat": "IT09999999999", "private": false});
            match command {
                "customers.search" => Ok(json!({"customers": [row], "count": 1})),
                _ if params["id"] == 9 => Ok(json!({"customer": row})),
                _ => Err("no".into()),
            }
        }));
        let mut app = DeskApp::new(Box::new(crate::engine::Session::new(desk)));
        app.go(Screen::Devices);
        let ctx = egui::Context::default();
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Paste("Lunghiss\tima".into())]);
        let search = format!("» {}", app.language.fill("Search the intranet for \"{name}\"", &[("name", "Lunghiss\tima")]));
        click_text(&ctx, &mut app, &search);
        let texts = settle(&ctx, &mut app);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            texts.iter().any(|(t, _)| t.contains("non risponde") || t.contains("unreachable")),
            "{:?}",
            texts.iter().map(|t| &t.0).collect::<Vec<_>>()
        );
        assert_eq!(app.devices.form.as_ref().map(|f| f.customer.text.as_str()), Some("Lunghiss ima"), "the field reads as the engine does");
        // The intranet is back: the search row is offered again, and a click asks again.
        *down.lock().unwrap() = false;
        let search = format!("» {}", app.language.fill("Search the intranet for \"{name}\"", &[("name", "Lunghiss ima")]));
        assert!(texts.iter().any(|(t, _)| *t == search), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        click_text(&ctx, &mut app, &search);
        let texts = settle(&ctx, &mut app);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let painted: Vec<&String> = texts.iter().map(|t| &t.0).collect();
        assert!(painted.iter().any(|t| t.starts_with("Lunghissima Società")), "{painted:?}");
        // The long name is imported and picked (the field holds the whole name).
        let row = painted.iter().find(|t| t.starts_with("Lunghissima Società")).unwrap().to_string();
        click_text(&ctx, &mut app, &row);
        settle(&ctx, &mut app);
        let imported = app.data().customers.iter().find(|c| c.intranet_id == Some(9)).map(|c| c.id).expect("imported");
        let f = app.devices.form.clone().unwrap();
        assert_eq!((f.customer.id, f.customer.text.as_str()), (Some(imported), long));
        // Archived in the desk: the intranet row says so instead of picking it.
        app.run("customer.archive", json!({"customer": imported}));
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Text("Lunghissima".into())]);
        let search = format!("» {}", app.language.fill("Search the intranet for \"{name}\"", &[("name", "Lunghissima")]));
        click_text(&ctx, &mut app, &search);
        let texts = settle(&ctx, &mut app);
        let archived = app.language.fill("The customer \"{name}\" is archived: restore it to pick it", &[("name", &format::clipped(long, 60))]);
        assert!(texts.iter().any(|(t, _)| *t == archived), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        assert!(!texts.iter().any(|(t, _)| t.starts_with("Lunghissima Società")), "no row to pick");
        assert_eq!(app.devices.form.as_ref().and_then(|f| f.customer.id), None);
    }

    #[test]
    fn a_customer_is_imported_from_the_intranet_in_the_pc_form() {
        let mut app = DeskApp::demo_with_intranet();
        app.go(Screen::Devices);
        let ctx = egui::Context::default();
        open_new(&mut app, None, "987654321");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        let texts = frame_with(&ctx, &mut app, vec![egui::Event::Text("Rossi Imp".into())]);
        let search = format!("» {}", app.language.fill("Search the intranet for \"{name}\"", &[("name", "Rossi Imp")]));
        assert!(texts.iter().any(|(t, _)| *t == search), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        let customers = app.data().customers.len();
        click_text(&ctx, &mut app, &search);
        let texts = settle(&ctx, &mut app);
        assert!(texts.iter().any(|(t, _)| t == "Rossi Impianti S.r.l."), "the intranet's rows: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        assert!(texts.iter().any(|(t, _)| t == "IT05555555555 · Nuoro"));
        click_text(&ctx, &mut app, "Rossi Impianti S.r.l.");
        settle(&ctx, &mut app);
        assert_eq!(app.data().customers.len(), customers + 1, "imported");
        let imported = app.data().customers.iter().find(|c| c.intranet_id == Some(501)).map(|c| c.id).expect("the import");
        let f = app.devices.form.clone().expect("the form stays open");
        assert_eq!((f.customer.id, f.customer.text.as_str()), (Some(imported), "Rossi Impianti S.r.l."));
        assert_eq!(app.devices.busy, None);
        let save = app.tr("Save").to_string();
        click_text(&ctx, &mut app, &save);
        assert!(app.data().devices.iter().any(|d| d.rustdesk_id == "987654321" && d.customer_id == imported));
        // Searched again: the imported one is marked as the desk's and a click picks it.
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Text("Rossi".into())]);
        let search = format!("» {}", app.language.fill("Search the intranet for \"{name}\"", &[("name", "Rossi")]));
        click_text(&ctx, &mut app, &search);
        let texts = settle(&ctx, &mut app);
        let already = app.tr("Already in the desk").to_string();
        assert!(texts.iter().any(|(t, _)| *t == already), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        assert!(texts.iter().any(|(t, _)| t == "Rossi Anna"));
        click_text(&ctx, &mut app, "Rossi Impianti S.r.l.");
        settle(&ctx, &mut app);
        assert_eq!(app.devices.form.as_ref().and_then(|f| f.customer.id), Some(imported));
        assert_eq!(app.data().customers.len(), customers + 1, "not imported twice");
        // Without an intranet the field offers no search.
        let mut app = self::app();
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        click_text(&ctx, &mut app, &field);
        let texts = frame_with(&ctx, &mut app, vec![egui::Event::Text("Rossi".into())]);
        assert!(!texts.iter().any(|(t, _)| t.starts_with('»')), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
    }

    #[test]
    fn a_customer_is_picked_with_a_click_while_the_field_has_the_focus() {
        let mut app = app();
        let ctx = egui::Context::default();
        open_new(&mut app, None, "987654321");
        frame_with(&ctx, &mut app, Vec::new());
        // Click the empty customer field (its hint), then type: the matches show under it.
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Text("Lunar".into())]);
        assert_eq!(app.devices.form.as_ref().map(|f| f.customer.text.as_str()), Some("Lunar"));
        assert!(ctx.memory(|m| m.focused().is_some()), "the field has the focus");
        // The click takes the focus from the field: the match must still be there to take it.
        click_text(&ctx, &mut app, "Studio Tecnico Lunaria");
        let f = app.devices.form.clone().expect("the form");
        assert_eq!((f.customer.id, f.customer.text.as_str()), (Some(1), "Studio Tecnico Lunaria"));
        let save = app.tr("Save").to_string();
        click_text(&ctx, &mut app, &save);
        assert!(app.devices.form.is_none(), "saved");
        assert!(app.data().devices.iter().any(|d| d.rustdesk_id == "987654321" && d.customer_id == 1));
    }

    #[test]
    fn a_customer_the_desk_does_not_have_is_created_from_the_field() {
        let mut app = app();
        let ctx = egui::Context::default();
        open_new(&mut app, None, "987654321");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        let texts = frame_with(&ctx, &mut app, vec![egui::Event::Text("Emilian".into())]);
        let hint = app.tr("Pick the customer to save the PC");
        assert!(texts.iter().any(|(t, _)| t == hint), "why Save is off");
        let customers = app.data().customers.len();
        let create = format!("+ {}", app.language.fill("Create the customer \"{name}\"", &[("name", "Emilian")]));
        click_text(&ctx, &mut app, &create);
        assert_eq!(app.data().customers.len(), customers + 1);
        let created = app.data().customers.iter().find(|c| c.name == "Emilian").map(|c| c.id).expect("Emilian");
        let f = app.devices.form.clone().expect("the form stays open");
        assert_eq!(f.customer.id, Some(created));
        assert_eq!(app.devices.busy, None);
        let save = app.tr("Save").to_string();
        click_text(&ctx, &mut app, &save);
        assert!(app.data().devices.iter().any(|d| d.rustdesk_id == "987654321" && d.customer_id == created));
        // A name pasted with a tab is created as the engine stores it, and picked.
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Paste("Rossi\tMario".into())]);
        let create = format!("+ {}", app.language.fill("Create the customer \"{name}\"", &[("name", "Rossi\tMario")]));
        click_text(&ctx, &mut app, &create);
        let rossi = app.data().customers.iter().find(|c| c.name == "Rossi Mario").map(|c| c.id);
        assert!(rossi.is_some());
        assert_eq!(app.devices.form.as_ref().and_then(|f| f.customer.id), rossi);
        // An archived customer's name: neither picked nor created, and the field says why, even
        // under another customer it finds.
        app.run("customer.add", json!({"name": "Elio Brina Impianti"}));
        app.run("customer.archive", json!({"customer": 3}));
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        let texts = frame_with(&ctx, &mut app, vec![egui::Event::Text("Elio Brina".into())]);
        let archived = app.language.fill("The customer \"{name}\" is archived: restore it to pick it", &[("name", "Elio Brina")]);
        assert!(texts.iter().any(|(t, _)| *t == archived), "{:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>());
        assert!(texts.iter().any(|(t, _)| t == "Elio Brina Impianti"));
        assert!(!texts.iter().any(|(t, _)| t.starts_with('+')), "no create row");
        assert_eq!(app.devices.form.as_ref().and_then(|f| f.customer.id), None);
        // A name typed exactly as the only customer it finds picks that customer.
        open_new(&mut app, None, "");
        frame_with(&ctx, &mut app, Vec::new());
        let field = app.tr("Type a name…").to_string();
        click_text(&ctx, &mut app, &field);
        frame_with(&ctx, &mut app, vec![egui::Event::Text("ferramenta corvo".into())]);
        assert_eq!(app.devices.form.as_ref().and_then(|f| f.customer.id), Some(2));
    }

    #[test]
    fn the_pane_replaces_the_list_until_both_fit() {
        let mut app = app();
        app.selected_device = Some(1);
        let new_pc = app.tr("New PC").to_string();
        assert!(texts_at(&mut app, 1440.0, 900.0).contains(&new_pc), "wide: the list next to the card");
        assert!(!texts_at(&mut app, 1100.0, 900.0).contains(&new_pc), "the list would be squeezed under the card: the card alone");
        app.selected_device = None;
        assert!(texts_at(&mut app, 1100.0, 900.0).contains(&new_pc), "nothing open: the list");
    }

    #[test]
    fn the_count_says_one_pc() {
        let mut app = app();
        app.language = Language::En;
        assert_eq!(count_text(&app, 1), "1 device");
        assert_eq!(count_text(&app, 5), "5 devices");
        app.language = Language::It;
        assert_eq!(count_text(&app, 1), "1 PC");
        app.devices.customer = Some(2);
        assert!(texts_at(&mut app, 1440.0, 900.0).contains(&"1 PC".to_string()));
    }
}
