//! The Interventions screen: the work done for customers, the running timers on top and then
//! newest first, filtered by the period, the technician, the kind, billable or running only, a
//! customer or a PC (the filter the Reports drill down with) and by the search as you type. The
//! header sums the hours and the billable hours of what is listed; the right pane is the
//! intervention's detail — times, billing, the consent override, the notes — or the form that
//! logs past work and edits an intervention. Everything goes through `intervention.add`,
//! `intervention.update`, `intervention.stop` and `intervention.remove`; stopping a colleague's
//! timer and deleting always ask first, and the technician's own timer is stopped in the timer
//! card, where the notes are typed.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align, Color32, FontId, Galley, Layout, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::core::chrono::{DateTime, Duration, Local, Utc};
use crate::core::privacy::PurposeState;
use crate::core::{Data, Intervention, InterventionKind, fold, time};
use crate::engine::views;
use crate::forms::{self, FormAction, Picker};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone, icon_button};
use crate::{Completed, Confirm, DeskApp, Screen, WorkFilter, format, privacy, timer};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("Log work", "Registra lavoro"),
    ("Edit intervention", "Modifica intervento"),
    ("Intervention saved", "Intervento salvato"),
    ("Intervention logged: {duration}", "Intervento registrato: {duration}"),
    ("Period", "Periodo"),
    ("This month", "Questo mese"),
    ("Last month", "Mese scorso"),
    ("Last 30 days", "Ultimi 30 giorni"),
    ("All", "Tutti"),
    ("Custom…", "Personalizzato…"),
    ("From", "Dal"),
    ("To", "Al"),
    ("The end of the period is excluded", "La fine del periodo è esclusa"),
    ("Technician", "Tecnico"),
    ("Everyone", "Tutti"),
    ("Me", "Io"),
    ("Kind", "Tipo"),
    ("All kinds", "Tutti i tipi"),
    ("Billable only", "Solo fatturabili"),
    ("Running only", "Solo in corso"),
    ("Customer: {name}", "Cliente: {name}"),
    ("PC: {name}", "PC: {name}"),
    ("Remove this filter", "Togli questo filtro"),
    ("{n} interventions · {hours} h · {billable} h billable", "{n} interventi · {hours} h · {billable} h fatturabili"),
    ("1 intervention · {hours} h · {billable} h billable", "1 intervento · {hours} h · {billable} h fatturabili"),
    ("No interventions in this period", "Nessun intervento in questo periodo"),
    (
        "Start the timer with Start intervention, or log past work with Log work.",
        "Avvia il timer con Avvia intervento, oppure registra il lavoro svolto con Registra lavoro.",
    ),
    ("No intervention matches \"{query}\"", "Nessun intervento corrisponde a \"{query}\""),
    ("Nothing running", "Niente in corso"),
    ("(no summary)", "(senza oggetto)"),
    ("PC", "PC"),
    ("Summary", "Oggetto"),
    ("Summary is required", "L'oggetto è obbligatorio"),
    ("Choose a customer or a PC", "Scegli un cliente o un PC"),
    ("Pick the customer from the list", "Scegli il cliente dall'elenco"),
    ("Pick the PC from the list", "Scegli il PC dall'elenco"),
    ("Reference", "Riferimento"),
    ("Ticket or job number in another system", "Numero di ticket o commessa in un altro sistema"),
    ("Started", "Inizio"),
    ("Ended", "Fine"),
    ("Duration", "Durata"),
    ("Minutes", "Minuti"),
    ("From the times: {duration}", "Dagli orari: {duration}"),
    ("Tick to give the minutes instead of the end", "Spunta per indicare i minuti al posto della fine"),
    ("Instead of the span (pauses)", "Al posto dell'intervallo (pause)"),
    ("Billable", "Fatturabile"),
    ("Not billable", "Non fatturabile"),
    ("Billable minutes: {n}", "Minuti fatturabili: {n}"),
    ("Notes", "Note"),
    ("Running since {time}", "In corso da {time}"),
    ("Running {elapsed}", "In corso {elapsed}"),
    ("No consent", "Senza consenso"),
    ("Carried out without a recorded consent: {reason}", "Svolto senza un consenso registrato: {reason}"),
    ("Time", "Tempo"),
    ("Record", "Registrazione"),
    ("Created", "Creato"),
    ("Updated", "Aggiornato"),
    ("Give the end time or the minutes", "Indica la fine o i minuti"),
    ("The end is before the start", "La fine è prima dell'inizio"),
    ("More than a week between start and end: check the times", "Più di una settimana tra inizio e fine: controlla gli orari"),
    ("Remote-assistance consent is {state}", "Il consenso all'assistenza remota è {state}"),
    ("Record consent", "Registra consenso"),
    ("Proceed with a reason", "Procedi indicando un motivo"),
    ("Reason for proceeding without consent", "Motivo per procedere senza consenso"),
    ("The reason stays in the intervention and in the audit log", "Il motivo resta nell'intervento e nel registro di audit"),
    ("Stop", "Ferma"),
    ("Stop {technician}'s intervention?", "Fermare l'intervento di {technician}?"),
    ("The timer stops now and the time worked so far is recorded.", "Il timer si ferma adesso e il tempo lavorato finora viene registrato."),
    ("Delete intervention", "Elimina intervento"),
    ("Delete this intervention? It cannot be undone.", "Eliminare questo intervento? Non si può annullare."),
    ("Intervention closed: {duration}", "Intervento chiuso: {duration}"),
    ("Intervention deleted", "Intervento eliminato"),
];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `intervention.add` from the log-work form.
    Logged,
    /// `intervention.update` from the edit form.
    Saved,
    /// `intervention.remove`, after the confirmation.
    Removed,
    /// `intervention.stop {id}` of a colleague's timer, after the confirmation.
    Stopped,
}

/// The log-work / edit form, drawn in the right pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    /// `None` logs past work (`intervention.add`); else the intervention edited.
    pub id: Option<u64>,
    pub customer: Picker,
    pub device: Picker,
    pub kind: InterventionKind,
    /// The kind was picked by hand: the default that follows the PC no longer applies.
    pub kind_touched: bool,
    pub summary: String,
    /// Form text of the start (required).
    pub started: String,
    /// Form text of the end; empty = not given (an edited running intervention keeps running).
    pub ended: String,
    /// Minutes worked instead of the span between start and end (pauses); `None` = the span.
    pub minutes: Option<u32>,
    pub technician: String,
    pub reference: String,
    pub billable: bool,
    pub notes: String,
    /// "Proceed with a reason" was clicked on the red consent gate: the reason field shows.
    pub proceed: bool,
    /// Why remote work went ahead without the remote-assistance consent (`consent_override`).
    pub consent_override: String,
    /// The edited intervention was running when the form opened.
    pub running: bool,
    /// The stored stamps of an edited intervention: sent back as they are when the times were
    /// not changed (the form shows minutes, the desk keeps seconds).
    pub stored_started: String,
    pub stored_ended: String,
    pub error: String,
}

/// What the rows read for every intervention, rebuilt when the data's revision changes (lookups
/// by id in the data are linear; rows are drawn every frame).
#[derive(Default)]
struct Cache {
    revision: Option<u64>,
    /// Intervention id → position in `data.interventions`.
    index: HashMap<u64, usize>,
    /// Customer id → name.
    customers: HashMap<u64, String>,
    /// Device id → (name, has a RustDesk ID).
    devices: HashMap<u64, (String, bool)>,
}

/// The sums of the listed interventions, for the header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Totals {
    count: usize,
    minutes: u64,
    billable: u64,
}

/// What the list was computed for: revision, folded search, the shared filter.
type RowsKey = (u64, String, WorkFilter);

/// This screen's own state.
#[derive(Default)]
pub struct State {
    /// The open log-work / edit form (drawn in the right pane).
    pub form: Option<Form>,
    /// The form's save in flight: Save is disabled until it answers.
    pub busy: Option<u64>,
    /// The Period menu shows the From / To fields.
    pub custom_period: bool,
    rows: Vec<u64>,
    rows_key: Option<RowsKey>,
    totals: Totals,
    cache: Cache,
    /// A row to bring into view (after `j`/`k`), and the list's last scroll offset.
    scroll_to: Option<usize>,
    offset: f32,
}

/// The height of a row (two lines and the technician's avatar).
const ROW_H: f32 = 52.0;
/// Below this window width the pane replaces the list.
const NARROW: f32 = 980.0;
/// The least width of the form's label column (the form's grids line up).
const LABEL_W: f32 = 110.0;
/// The width the hover actions cover at a row's right end.
const ROW_ACTIONS: f32 = 104.0;
/// The most minutes an intervention can count (a week, as the engine).
const MAX_MINUTES: u32 = 10_080;

// ---------------------------------------------------------------------------------------------
// The screen

/// What the right pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    None,
    /// A privacy form ("Record consent" from the consent gate).
    Privacy,
    Form,
    Detail(u64),
    /// A record just logged that the mirror does not hold yet.
    Loading,
}

/// Draws the screen: the right pane (a privacy form, the form, the detail), then the list.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    refresh_cache(app);
    refresh_rows(app);
    let narrow = ui.ctx().content_rect().width() < NARROW;
    let pane = if privacy::form_customer(app).is_some() {
        Pane::Privacy
    } else if app.interventions.form.is_some() {
        Pane::Form
    } else if let Some(id) = app.selected_intervention.filter(|id| app.data().intervention(*id).is_some()) {
        Pane::Detail(id)
    } else if app.pending_open.as_ref().is_some_and(|p| p.screen == Screen::Interventions) {
        Pane::Loading
    } else {
        Pane::None
    };
    match pane {
        Pane::None => {}
        Pane::Privacy => pane_ui(ui, t, narrow, |ui| {
            privacy::form_ui(app, ui, t);
        }),
        Pane::Form => pane_ui(ui, t, narrow, |ui| form_ui(app, ui, t)),
        Pane::Detail(id) => pane_ui(ui, t, narrow, |ui| detail(app, ui, t, id, narrow)),
        Pane::Loading => pane_ui(ui, t, narrow, |ui| widgets::empty_state(ui, t, Icon::Sync, app.tr("Loading…"), "")),
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
        egui::ScrollArea::vertical().id_salt("intervention-pane").auto_shrink([false, false]).show(ui, |ui| {
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
        Intent::Logged => saved(app, c, true),
        Intent::Saved => saved(app, c, false),
        Intent::Removed => {
            // A refusal is already in the status bar (`pump`).
            if let Ok(v) = &c.result {
                if v["removed"].as_u64().is_some_and(|id| app.selected_intervention == Some(id)) {
                    app.selected_intervention = None;
                }
                app.toast(app.tr("Intervention deleted").to_string());
            }
        }
        Intent::Stopped => {
            if let Ok(v) = &c.result {
                let minutes = v["intervention"]["minutes"].as_u64().unwrap_or(0);
                app.toast(app.language.fill("Intervention closed: {duration}", &[("duration", &format::duration_words(minutes))]));
            }
        }
    }
}

/// `intervention.add` / `intervention.update` answered: the form closes and the intervention
/// opens (once the mirror holds it), or the refusal shows in the form, which stays open.
fn saved(app: &mut DeskApp, c: &Completed, logged: bool) {
    let from_form = app.interventions.busy == Some(c.job);
    if from_form {
        app.interventions.busy = None;
    }
    match &c.result {
        Ok(v) => {
            if from_form {
                app.interventions.form = None;
                if let Some(id) = forms::new_id(v, "intervention") {
                    app.open_when_ready(Screen::Interventions, id);
                }
            }
            let warnings = forms::warnings(app, v);
            if !warnings.is_empty() {
                app.status = warnings.join(" · ");
            }
            let text = if logged {
                let minutes = v["intervention"]["minutes"].as_u64().unwrap_or(0);
                app.language.fill("Intervention logged: {duration}", &[("duration", &format::duration_words(minutes))])
            } else {
                app.tr("Intervention saved").to_string()
            };
            app.toast(text);
        }
        Err(e) => {
            let message = app.language.tr_message(&e.to_string());
            if from_form && let Some(f) = app.interventions.form.as_mut() {
                f.error = message;
            }
        }
    }
}

/// Closes what this screen has open: a privacy form drawn in its pane (on top), else the form.
/// Answers whether something was open.
pub fn close(app: &mut DeskApp) -> bool {
    if privacy::form_customer(app).is_some() && privacy::close(app) {
        return true;
    }
    if app.interventions.form.take().is_some() {
        app.interventions.busy = None;
        return true;
    }
    false
}

/// Opens the log-work form (`n`, the header's button), for the customer or the PC the list is
/// filtered on.
pub fn new(app: &mut DeskApp) {
    let (customer, device) = (app.work_filter.customer, app.work_filter.device);
    open_log(app, customer, device);
}

/// Moves the selection by `delta` rows of the list (`j`/`k`); the pane follows.
pub fn step(app: &mut DeskApp, delta: i32) {
    refresh_cache(app);
    refresh_rows(app);
    let rows = &app.interventions.rows;
    let Some(last) = rows.len().checked_sub(1) else { return };
    let current = app.selected_intervention.and_then(|id| rows.iter().position(|r| *r == id));
    let i = match current {
        Some(i) => {
            let moved = i64::try_from(i).unwrap_or(0).saturating_add(i64::from(delta));
            usize::try_from(moved.max(0)).unwrap_or(0).min(last)
        }
        None if delta < 0 => last,
        None => 0,
    };
    let Some(id) = rows.get(i).copied() else { return };
    app.selected_intervention = Some(id);
    app.interventions.scroll_to = Some(i);
}

/// Opens the log-past-work form on the Interventions screen, prefilled with a customer and/or a
/// PC (the PC's customer wins); the times are left to type (Now is one click away), so a
/// minutes-only entry is not tied to an end it did not mean.
pub fn open_log(app: &mut DeskApp, customer: Option<u64>, device: Option<u64>) {
    let data = app.data();
    let device = device.filter(|d| data.device(*d).is_some());
    let customer =
        device.and_then(|d| data.device(d)).map(|d| d.customer_id).or(customer).filter(|c| data.customer(*c).is_some_and(|c| c.erased_at.is_none()));
    let device = device.filter(|d| customer.is_some() && data.device(*d).map(|x| x.customer_id) == customer);
    let form = Form {
        customer: Picker::customer(app, customer),
        device: Picker::device(app, device),
        kind: default_kind(app.data(), device),
        technician: app.session.actor().to_string(),
        billable: true,
        ..Form::default()
    };
    app.interventions.form = Some(form);
    app.interventions.busy = None;
    app.go(Screen::Interventions);
}

/// Opens the edit form of an intervention on the Interventions screen (nothing when it does not
/// exist).
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let Some(i) = app.data().intervention(id).cloned() else { return };
    // Minutes that differ from the span are an override (pauses): shown, and sent back.
    let span = i.ended_at.as_deref().and_then(|e| time::minutes_between(&i.started_at, e));
    let minutes = match (&i.ended_at, span) {
        (Some(_), Some(s)) if s == i.minutes => None,
        (Some(_), _) => Some(i.minutes.min(MAX_MINUTES)),
        (None, _) => None,
    };
    app.open_intervention(id);
    let form = Form {
        id: Some(id),
        customer: Picker::customer(app, Some(i.customer_id)),
        device: Picker::device(app, i.device_id),
        kind: i.kind,
        kind_touched: true,
        summary: i.summary,
        started: format::input(&i.started_at),
        ended: i.ended_at.as_deref().map(format::input).unwrap_or_default(),
        minutes,
        technician: i.technician,
        reference: i.reference,
        billable: i.billable,
        notes: i.notes,
        running: i.ended_at.is_none(),
        stored_started: i.started_at,
        stored_ended: i.ended_at.unwrap_or_default(),
        ..Form::default()
    };
    app.interventions.form = Some(form);
    app.interventions.busy = None;
}

/// The kind the engine picks when none is given: remote on a PC with a RustDesk ID, on site on
/// one without, phone without a PC.
fn default_kind(data: &Data, device: Option<u64>) -> InterventionKind {
    match device.and_then(|d| data.device(d)) {
        Some(d) if !d.rustdesk_id.is_empty() => InterventionKind::Remote,
        Some(_) => InterventionKind::Onsite,
        None => InterventionKind::Phone,
    }
}

// ---------------------------------------------------------------------------------------------
// The list

/// Rebuilds the rows' lookups when the data changed.
fn refresh_cache(app: &mut DeskApp) {
    let revision = app.session.revision();
    if app.interventions.cache.revision == Some(revision) {
        return;
    }
    let data = app.session.data();
    app.interventions.cache = Cache {
        revision: Some(revision),
        index: data.interventions.iter().enumerate().map(|(i, x)| (x.id, i)).collect(),
        customers: data.customers.iter().map(|c| (c.id, c.name.clone())).collect(),
        devices: data.devices.iter().map(|d| (d.id, (d.name.clone(), !d.rustdesk_id.is_empty()))).collect(),
    };
}

/// Recomputes the rows and the totals when the data, the search or the filter changed.
fn refresh_rows(app: &mut DeskApp) {
    let key: RowsKey = (app.session.revision(), app.query(), app.work_filter.clone());
    if app.interventions.rows_key.as_ref() == Some(&key) {
        return;
    }
    let (rows, totals) = compute_rows(app.session.data(), app.now(), &key.1, &key.2);
    app.interventions.rows = rows;
    app.interventions.totals = totals;
    app.interventions.rows_key = Some(key);
}

/// A period bound typed in the filter, as a stamp; `None` when empty or unreadable (ignored).
fn bound(text: &str, now: DateTime<Utc>) -> Option<String> {
    let text = text.trim();
    if text.is_empty() { None } else { format::stamp_from_input(text, now) }
}

/// The interventions to list, in order (running first, then newest first), with their sums.
/// `fq` is the folded search, matched as the engine matches (`views::intervention_matches`);
/// the period's end is excluded, as in `intervention.list`.
fn compute_rows(data: &Data, now: DateTime<Utc>, fq: &str, f: &WorkFilter) -> (Vec<u64>, Totals) {
    let from = bound(&f.from, now);
    let to = bound(&f.to, now);
    let technician = fold(f.technician.trim());
    let kind = InterventionKind::parse(&f.kind);
    let keeps = |i: &Intervention| {
        f.customer.is_none_or(|c| i.customer_id == c)
            && f.device.is_none_or(|d| i.device_id == Some(d))
            && (technician.is_empty() || fold(&i.technician) == technician)
            && kind.is_none_or(|k| i.kind == k)
            && from.as_ref().is_none_or(|s| i.started_at >= *s)
            && to.as_ref().is_none_or(|s| i.started_at < *s)
            && (!f.running_only || i.running())
            && (!f.billable_only || i.billable)
            && (fq.is_empty() || views::intervention_matches(data, i, fq))
    };
    let mut rows: Vec<&Intervention> = data.interventions.iter().filter(|i| keeps(i)).collect();
    rows.sort_by(|a, b| b.running().cmp(&a.running()).then_with(|| b.started_at.cmp(&a.started_at)).then(b.id.cmp(&a.id)));
    let totals = Totals {
        count: rows.len(),
        minutes: rows.iter().map(|i| u64::from(i.minutes)).sum(),
        billable: rows.iter().map(|i| data.preferences.billable_minutes(i)).sum(),
    };
    (rows.into_iter().map(|i| i.id).collect(), totals)
}

/// Minutes as hours with one decimal (`5.3`, `5,3` in Italian).
fn hours(minutes: u64, italian: bool) -> String {
    let h = format!("{:.1}", minutes as f64 / 60.0);
    if italian { h.replace('.', ",") } else { h }
}

/// The list: the header, then the rows (or an empty state).
fn list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    header(app, ui, t);
    let rows = std::mem::take(&mut app.interventions.rows);
    if rows.is_empty() {
        empty(app, ui, t);
    } else {
        let viewport = ui.available_height();
        let mut area = egui::ScrollArea::vertical().id_salt("interventions-list").auto_shrink([false, false]);
        if let Some(i) = app.interventions.scroll_to.take() {
            let top = i as f32 * ROW_H;
            let offset = app.interventions.offset;
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
                            app.selected_intervention = Some(*id);
                        }
                    }
                })
            })
            .inner;
        if out.state.offset.y.is_finite() {
            app.interventions.offset = out.state.offset.y;
        }
    }
    app.interventions.rows = rows;
}

/// The header: the title, the totals and Log work; under it the filter chips.
fn header(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let totals = app.interventions.totals;
    egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            ui.label(RichText::new(app.tr("Interventions")).size(16.0).strong().color(t.ui_text));
            let query = app.search.trim();
            if !query.is_empty() {
                ui.label(RichText::new(format!("· \"{}\"", format::clipped(query, 40))).color(t.muted));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::primary_button(ui, t, Some(Icon::Interventions), app.tr("Log work")).on_hover_text(app.tip("intervention.add")).clicked()
                {
                    new(app);
                }
                ui.add_space(6.0);
                let pattern = if totals.count == 1 {
                    "1 intervention · {hours} h · {billable} h billable"
                } else {
                    "{n} interventions · {hours} h · {billable} h billable"
                };
                let text = app.language.fill(
                    pattern,
                    &[
                        ("n", &totals.count.to_string()),
                        ("hours", &hours(totals.minutes, app.italian())),
                        ("billable", &hours(totals.billable, app.italian())),
                    ],
                );
                let room = ui.available_width() - 8.0;
                if totals.count > 0 && room > 60.0 {
                    let g = widgets::truncate(ui.painter(), &text, FontId::proportional(12.5), t.muted, room);
                    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(r.min, g, t.muted);
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            period_chip(app, ui, t);
            technician_chip(app, ui, t);
            kind_chip(app, ui, t);
            if toggle_chip(ui, t, app.tr("Billable only"), app.work_filter.billable_only).clicked() {
                app.work_filter.billable_only = !app.work_filter.billable_only;
            }
            if toggle_chip(ui, t, app.tr("Running only"), app.work_filter.running_only).clicked() {
                app.work_filter.running_only = !app.work_filter.running_only;
            }
            if let Some(c) = app.work_filter.customer {
                let name = app.interventions.cache.customers.get(&c).cloned().unwrap_or_default();
                let text = app.language.fill("Customer: {name}", &[("name", &format::clipped(&name, 32))]);
                if removable_chip(ui, t, &text).on_hover_text(app.tr("Remove this filter")).clicked() {
                    app.work_filter.customer = None;
                }
            }
            if let Some(d) = app.work_filter.device {
                let name = app.interventions.cache.devices.get(&d).map(|x| x.0.clone()).unwrap_or_default();
                let text = app.language.fill("PC: {name}", &[("name", &format::clipped(&name, 32))]);
                if removable_chip(ui, t, &text).on_hover_text(app.tr("Remove this filter")).clicked() {
                    app.work_filter.device = None;
                }
            }
        });
    });
}

/// The period presets of the filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Period {
    ThisMonth,
    LastMonth,
    Last30,
    All,
    Custom,
}

/// The presets the Period menu lists, with their labels.
const PERIODS: [(Period, &str); 4] =
    [(Period::ThisMonth, "This month"), (Period::LastMonth, "Last month"), (Period::Last30, "Last 30 days"), (Period::All, "All")];

/// The local date `days` days before `now`, as form text (`2026-09-14`).
fn days_ago(now: DateTime<Utc>, days: i64) -> String {
    (now - Duration::days(days)).with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// The form-text bounds of a preset (the end excluded); empty for All and Custom.
fn period_bounds(period: Period, now: DateTime<Utc>) -> (String, String) {
    match period {
        Period::ThisMonth => format::month_bounds(now, 0),
        Period::LastMonth => format::month_bounds(now, 1),
        Period::Last30 => (days_ago(now, 30), String::new()),
        Period::All | Period::Custom => (String::new(), String::new()),
    }
}

/// Which preset the filter's bounds are.
fn current_period(f: &WorkFilter, now: DateTime<Utc>) -> Period {
    let (from, to) = (f.from.trim(), f.to.trim());
    if from.is_empty() && to.is_empty() {
        return Period::All;
    }
    for p in [Period::ThisMonth, Period::LastMonth, Period::Last30] {
        let (a, b) = period_bounds(p, now);
        if a == from && b == to {
            return p;
        }
    }
    Period::Custom
}

/// The Period chip and its menu: the presets, then Custom… with the From / To fields.
fn period_chip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let now = app.now();
    let current = current_period(&app.work_filter, now);
    let text = match current {
        Period::All => app.tr("Period").to_string(),
        Period::Custom => {
            let side = |s: &str| if s.trim().is_empty() { "…".to_string() } else { format::clipped(s.trim(), 16) };
            format!("{} – {}", side(&app.work_filter.from), side(&app.work_filter.to))
        }
        p => PERIODS.iter().find(|(q, _)| *q == p).map(|(_, l)| app.tr(l).to_string()).unwrap_or_default(),
    };
    let chip = menu_chip(ui, t, &text, current != Period::All).on_hover_text(app.tr("Period"));
    let shown = egui::Popup::menu(&chip).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(200.0);
        for (p, label) in PERIODS {
            if ui.selectable_label(current == p && !app.interventions.custom_period, app.tr(label)).clicked() {
                let (from, to) = period_bounds(p, now);
                app.work_filter.from = from;
                app.work_filter.to = to;
                app.interventions.custom_period = false;
                ui.close();
            }
        }
        let custom = current == Period::Custom || app.interventions.custom_period;
        if ui.selectable_label(custom, app.tr("Custom…")).clicked() && !app.interventions.custom_period {
            app.interventions.custom_period = true;
            if current == Period::All {
                let (from, to) = format::month_bounds(now, 0);
                app.work_filter.from = from;
                app.work_filter.to = to;
            }
        }
        if custom {
            ui.separator();
            let (mut from, mut to) = (app.work_filter.from.clone(), app.work_filter.to.clone());
            egui::Grid::new("intervention-period-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                forms::time_field(app, ui, t, app.tr("From"), &mut from, now, false);
                forms::time_field(app, ui, t, app.tr("To"), &mut to, now, false);
            });
            ui.label(RichText::new(app.tr("The end of the period is excluded")).small().color(t.muted));
            app.work_filter.from = crate::core::clip(&from, 64).to_string();
            app.work_filter.to = crate::core::clip(&to, 64).to_string();
        }
    });
    // Closed on a preset: the next opening starts from the presets.
    if shown.is_none() && current_period(&app.work_filter, now) != Period::Custom {
        app.interventions.custom_period = false;
    }
}

/// The Technician chip and its menu: everyone, me, each technician of the desk.
fn technician_chip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let actor = app.session.actor().to_string();
    let current = app.work_filter.technician.trim().to_string();
    let mine = !actor.is_empty() && fold(&current) == fold(&actor);
    let text = if current.is_empty() {
        app.tr("Technician").to_string()
    } else if mine {
        app.tr("Me").to_string()
    } else {
        format::clipped(&current, 24)
    };
    let chip = menu_chip(ui, t, &text, !current.is_empty()).on_hover_text(app.tr("Technician"));
    let others: Vec<String> = app.derived.technicians.iter().filter(|n| fold(n) != fold(&actor)).cloned().collect();
    egui::Popup::menu(&chip).show(|ui| {
        ui.set_min_width(180.0);
        if ui.selectable_label(current.is_empty(), app.tr("Everyone")).clicked() {
            app.work_filter.technician.clear();
        }
        if !actor.is_empty() && ui.selectable_label(mine, app.tr("Me")).clicked() {
            app.work_filter.technician.clone_from(&actor);
        }
        if !others.is_empty() {
            ui.separator();
        }
        for name in others {
            let on = !current.is_empty() && fold(&current) == fold(&name);
            if ui.selectable_label(on, format::clipped(&name, 40)).clicked() {
                app.work_filter.technician = name;
            }
        }
    });
}

/// The Kind chip and its menu: every kind, or one.
fn kind_chip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let current = InterventionKind::parse(&app.work_filter.kind);
    let text = match current {
        Some(k) => app.tr(forms::kind_label(k)).to_string(),
        None => app.tr("Kind").to_string(),
    };
    let chip = menu_chip(ui, t, &text, current.is_some()).on_hover_text(app.tr("Kind"));
    egui::Popup::menu(&chip).show(|ui| {
        ui.set_min_width(160.0);
        if ui.selectable_label(current.is_none(), app.tr("All kinds")).clicked() {
            app.work_filter.kind.clear();
        }
        ui.separator();
        for (k, label) in forms::KINDS {
            if ui.selectable_label(current == Some(*k), app.tr(label)).clicked() {
                app.work_filter.kind = forms::kind_id(*k);
            }
        }
    });
}

/// A filter chip that opens a menu: accent when the filter is set, outlined when not; a small
/// chevron after the text.
fn menu_chip(ui: &mut egui::Ui, t: &Tokens, text: &str, on: bool) -> Response {
    let color = if on { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.5), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 38.0, 26.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 13.0, if resp.hovered() { t.active.gamma_multiply(0.85) } else { t.active });
    } else {
        p.rect_filled(rect, 13.0, if resp.hovered() { t.hover } else { t.chrome });
        p.rect_stroke(rect, 13.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    p.galley(pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0), galley, color);
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0)), Icon::Chevron, color);
    resp
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

/// The empty list: connecting, no match for the search, nothing running, or nothing in the
/// period (with Log work).
fn empty(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if app.session.problem().as_deref() == Some("connecting") && app.session.revision() == 0 {
        widgets::empty_state(ui, t, Icon::Sync, app.tr("Connecting…"), "");
        return;
    }
    let query = app.search.trim().to_string();
    if !query.is_empty() {
        let title = app.language.fill("No intervention matches \"{query}\"", &[("query", &format::clipped(&query, 60))]);
        widgets::empty_state(ui, t, Icon::Search, &title, "");
    } else if app.work_filter.running_only {
        widgets::empty_state(ui, t, Icon::Timer, app.tr("Nothing running"), "");
    } else {
        widgets::empty_state(
            ui,
            t,
            Icon::Interventions,
            app.tr("No interventions in this period"),
            app.tr("Start the timer with Start intervention, or log past work with Log work."),
        );
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if widgets::primary_button(ui, t, Some(Icon::Interventions), app.tr("Log work")).clicked() {
                new(app);
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// A row

/// What a row shows of an intervention.
struct RowInfo {
    customer: String,
    summary: String,
    device: String,
    technician: String,
    kind: InterventionKind,
    running: bool,
    billable: bool,
    /// Carried out without a recorded consent (`consent_override` set).
    no_consent: bool,
    /// The start, as a short date.
    started: String,
    /// The minutes worked (empty while running: the chip ticks instead).
    duration: String,
    /// `h:mm:ss` since the start, while running.
    elapsed: String,
}

fn row_info(app: &DeskApp, id: u64) -> Option<RowInfo> {
    let data = app.data();
    let cache = &app.interventions.cache;
    let i = cache.index.get(&id).and_then(|p| data.interventions.get(*p)).filter(|i| i.id == id).or_else(|| data.intervention(id))?;
    let now = app.now();
    let running = i.running();
    Some(RowInfo {
        customer: cache.customers.get(&i.customer_id).cloned().unwrap_or_else(|| crate::customer_name(data, i.customer_id)),
        summary: i.summary.clone(),
        device: i
            .device_id
            .map(|d| cache.devices.get(&d).map(|x| x.0.clone()).unwrap_or_else(|| crate::device_name(data, Some(d))))
            .unwrap_or_default(),
        technician: i.technician.clone(),
        kind: i.kind,
        running,
        billable: i.billable,
        no_consent: !i.consent_override.trim().is_empty(),
        started: format::short(&i.started_at, now, app.italian()),
        duration: if running { String::new() } else { format::duration(u64::from(i.minutes)) },
        elapsed: if running { format::elapsed(&i.started_at, now) } else { String::new() },
    })
}

/// A hover action of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAction {
    Stop,
    Edit,
    Delete,
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

/// A technician's avatar painted at `center`: a disc in the name's colour with the initial.
fn paint_avatar(p: &egui::Painter, t: &Tokens, center: Pos2, size: f32, name: &str) {
    p.circle_filled(center, size / 2.0, crate::chrome::color_for(name));
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    p.text(center, egui::Align2::CENTER_CENTER, initial, FontId::proportional(size * 0.5), t.on_accent);
}

/// One intervention row, 52 px: the kind icon, the customer in bold (when `with_customer`; the
/// summary in bold otherwise) and the summary, the chips (Running with the ticking elapsed time,
/// Not billable, No consent); under it the PC and the technician's avatar and name; at the right
/// the start and the duration, replaced on hover by Stop (while running), Edit and Delete. Used
/// by the list and by the customer and PC cards; the caller handles a click on the row (the
/// actions are handled here).
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, with_customer: bool) -> Response {
    refresh_cache(app);
    let selected = app.screen == Screen::Interventions && app.selected_intervention == Some(id);
    let (rect, resp) = widgets::row(ui, t, ROW_H, selected);
    let Some(info) = row_info(app, id) else { return resp };
    let hovering = ui.rect_contains_pointer(rect);
    let p = ui.painter().clone();
    if hovering && !selected && !resp.hovered() {
        // The pointer is on an action: keep the row's hover tint.
        p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 1.0)), 0.0, t.hover);
    }
    let left = rect.left() + 14.0;
    let icon_color = if selected { t.accent } else { t.muted };
    icons::paint(&p, Rect::from_center_size(pos2(left + 9.0, rect.center().y), vec2(18.0, 18.0)), widgets::kind_icon(info.kind), icon_color);
    let x0 = left + 30.0;
    let (y1, y2) = (rect.top() + 17.0, rect.top() + 36.0);
    // The right column (start and duration) keeps its width; hover actions cover it.
    let date = p.layout_no_wrap(info.started.clone(), FontId::proportional(12.5), t.muted);
    let duration = (!info.duration.is_empty()).then(|| p.layout_no_wrap(info.duration.clone(), FontId::proportional(12.5), t.ui_text));
    let right_w = date.size().x.max(duration.as_ref().map_or(0.0, |g| g.size().x)) + 14.0;
    let end = (rect.right() - 14.0 - right_w).max(x0 + 40.0);
    let avail = end - x0;
    // Line 1: the chips claim up to about half the room, the texts share the rest.
    let mut chips: Vec<(String, Tone)> = Vec::new();
    if info.running {
        chips.push((app.language.fill("Running {elapsed}", &[("elapsed", &info.elapsed)]), Tone::Ok));
    }
    if info.no_consent {
        chips.push((app.tr("No consent").to_string(), Tone::Warn));
    }
    if !info.billable {
        chips.push((app.tr("Not billable").to_string(), Tone::Neutral));
    }
    let mut room = avail * 0.55;
    let chips: Vec<(String, Tone, f32)> = chips
        .into_iter()
        .map(|(text, tone)| {
            let w = p.layout_no_wrap(text.clone(), FontId::proportional(11.5), t.muted).size().x + 20.0;
            (text, tone, w)
        })
        .filter(|(_, _, w)| {
            let fits = *w <= room;
            if fits {
                room -= w;
            }
            fits
        })
        .collect();
    let chips_w: f32 = chips.iter().map(|c| c.2).sum();
    let text_end = (end - chips_w).max(x0 + 20.0);
    let no_summary = info.summary.trim().is_empty();
    let summary_text = if no_summary { app.tr("(no summary)").to_string() } else { info.summary.clone() };
    let summary_color = if no_summary { t.muted } else { t.ui_text };
    let mut x = x0;
    if with_customer && !info.customer.is_empty() {
        let max = ((text_end - x0) * 0.45).max(20.0);
        let g = widgets::truncate(&p, &info.customer, FontId::proportional(14.0), t.ui_text, max);
        x += put(&p, x, y1, g, t.ui_text, true) + 10.0;
        if text_end - x > 30.0 {
            let g = widgets::truncate(&p, &summary_text, FontId::proportional(13.5), summary_color, text_end - x);
            x += put(&p, x, y1, g, summary_color, false) + 8.0;
        }
    } else {
        let g = widgets::truncate(&p, &summary_text, FontId::proportional(14.0), summary_color, text_end - x0);
        x += put(&p, x, y1, g, summary_color, !no_summary) + 8.0;
    }
    for (text, tone, _) in chips {
        x += paint_chip(&p, t, x, y1, &text, tone) + 6.0;
    }
    // Line 2: the PC, then the technician's avatar and name.
    let mut x = x0;
    if !info.device.is_empty() {
        let g = widgets::truncate(&p, &info.device, FontId::proportional(12.0), t.muted, avail * 0.5);
        x += put(&p, x, y2, g, t.muted, false) + 12.0;
    }
    if !info.technician.is_empty() && end - x > 40.0 {
        paint_avatar(&p, t, pos2(x + 10.0, y2), 20.0, &info.technician);
        x += 26.0;
        let g = widgets::truncate(&p, &info.technician, FontId::proportional(12.0), t.muted, end - x);
        put(&p, x, y2, g, t.muted, false);
    }
    // The right column: the start and the duration, or the actions on hover.
    if !hovering {
        put(&p, rect.right() - 14.0 - date.size().x, y1, date, t.muted, false);
        if let Some(g) = duration {
            put(&p, rect.right() - 14.0 - g.size().x, y2, g, t.ui_text, false);
        }
        return resp;
    }
    let cover = Rect::from_min_max(pos2((rect.right() - ROW_ACTIONS).max(rect.left()), rect.top()), pos2(rect.right(), rect.bottom() - 1.0));
    p.rect_filled(cover, 0.0, if selected { t.active } else { t.hover });
    let mut actions = vec![(Icon::Trash, "Delete intervention", RowAction::Delete), (Icon::Edit, "Edit intervention", RowAction::Edit)];
    if info.running {
        actions.push((Icon::Stop, "Stop", RowAction::Stop));
    }
    let mut clicked = None;
    let mut hx = rect.right() - 10.0;
    for (icon, tip, action) in actions {
        let r = Rect::from_center_size(pos2(hx - 13.0, rect.center().y), vec2(26.0, 26.0));
        let hr = ui.interact(r, ui.id().with(("intervention-row-action", id, tip)), Sense::click());
        if hr.hovered() {
            p.rect_filled(r, 6.0, t.active);
        }
        let color = if action == RowAction::Delete && hr.hovered() { widgets::danger(t) } else { t.ui_text };
        icons::paint(&p, r.shrink(5.0), icon, color);
        if hr.on_hover_text(app.tr(tip)).clicked() {
            clicked = Some(action);
        }
        hx -= 30.0;
    }
    match clicked {
        Some(RowAction::Stop) => stop(app, id),
        Some(RowAction::Edit) => open_edit(app, id),
        Some(RowAction::Delete) => ask_delete(app, id),
        None => {}
    }
    resp
}

// ---------------------------------------------------------------------------------------------
// Actions

/// Stops a running intervention: the technician's own in the timer card (notes, end, minutes),
/// a colleague's after a confirmation (`intervention.stop {id}`).
fn stop(app: &mut DeskApp, id: u64) {
    let Some((technician, running)) = app.data().intervention(id).map(|i| (i.technician.clone(), i.running())) else { return };
    if !running {
        return;
    }
    if technician == app.session.actor() {
        timer::open_running(app, id, true);
        return;
    }
    app.ask(Confirm {
        title: app.language.fill("Stop {technician}'s intervention?", &[("technician", &format::clipped(&technician, 40))]),
        text: app.tr("The timer stops now and the time worked so far is recorded.").to_string(),
        ok_label: app.tr("Stop").to_string(),
        danger: true,
        command: "intervention.stop".into(),
        params: json!({"id": id}),
        intent: crate::Intent::Interventions(Intent::Stopped),
    });
}

/// Asks before deleting an intervention.
fn ask_delete(app: &mut DeskApp, id: u64) {
    if app.data().intervention(id).is_none() {
        return;
    }
    app.ask(Confirm {
        title: app.tr("Delete intervention").to_string(),
        text: app.tr("Delete this intervention? It cannot be undone.").to_string(),
        ok_label: app.tr("Delete").to_string(),
        danger: true,
        command: "intervention.remove".into(),
        params: json!({"id": id}),
        intent: crate::Intent::Interventions(Intent::Removed),
    });
}

// ---------------------------------------------------------------------------------------------
// The form

/// The consent gate of the form: when logging remote work for a customer whose
/// remote-assistance consent is not given, the state and whether it blocks (`require_consent`;
/// a reason then overrides). Edits are not gated: the engine checks the consent when work is
/// started or logged.
fn gate(app: &DeskApp, f: &Form) -> Option<(PurposeState, bool)> {
    if f.id.is_some() || f.kind != InterventionKind::Remote {
        return None;
    }
    let customer = f.customer.id.or_else(|| f.device.id.and_then(|d| app.data().device(d)).map(|d| d.customer_id))?;
    let state = app.derived.gate(customer).0;
    (state != PurposeState::Given).then(|| (state, app.data().preferences.require_consent))
}

/// Why the form cannot be sent yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blocker {
    /// Something to fill in or fix, said by this label (red when `true`: a contradiction).
    Say(&'static str, bool),
    /// A time is empty or unreadable: its field says so under itself.
    Time,
    /// The consent gate is red and no reason is given: the banner says so.
    Gate,
}

/// The stamp a time field of the form sends: the stored one when the text still shows it (the
/// form shows minutes, the desk keeps seconds); `None` when unreadable, `Some("")` when empty.
fn stamp_of(text: &str, stored: &str, now: DateTime<Utc>) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return Some(String::new());
    }
    if !stored.is_empty() && text == format::input(stored) {
        return Some(stored.to_string());
    }
    format::stamp_from_input(text, now)
}

/// What stops the form from being sent, if anything.
fn blocker(app: &DeskApp, f: &Form) -> Option<Blocker> {
    let now = app.now();
    if f.customer.id.is_none() && f.device.id.is_none() {
        if !f.customer.text.trim().is_empty() {
            return Some(Blocker::Say("Pick the customer from the list", false));
        }
        if !f.device.text.trim().is_empty() {
            return Some(Blocker::Say("Pick the PC from the list", false));
        }
        return Some(Blocker::Say("Choose a customer or a PC", false));
    }
    // A name typed but not picked would be dropped silently: pick it, or clear it.
    if f.customer.id.is_none() && !f.customer.text.trim().is_empty() {
        return Some(Blocker::Say("Pick the customer from the list", false));
    }
    if f.device.id.is_none() && !f.device.text.trim().is_empty() {
        return Some(Blocker::Say("Pick the PC from the list", false));
    }
    if f.summary.trim().is_empty() {
        return Some(Blocker::Say("Summary is required", false));
    }
    let started = stamp_of(&f.started, &f.stored_started, now).filter(|s| !s.is_empty());
    let ended = stamp_of(&f.ended, &f.stored_ended, now);
    let (Some(started), Some(ended)) = (started, ended) else { return Some(Blocker::Time) };
    // A closed intervention needs an end (or the minutes); an edited running one may keep running.
    let needs_end = f.id.is_none() || !f.running;
    if needs_end && ended.is_empty() && f.minutes.is_none() {
        return Some(Blocker::Say("Give the end time or the minutes", false));
    }
    if !ended.is_empty() && ended < started {
        return Some(Blocker::Say("The end is before the start", true));
    }
    // The engine refuses a span over a week even with the minutes given (a mistyped date).
    if !ended.is_empty() && time::minutes_between(&started, &ended).is_none_or(|m| m > MAX_MINUTES) {
        return Some(Blocker::Say("More than a week between start and end: check the times", true));
    }
    if gate(app, f).is_some_and(|(_, blocks)| blocks) && f.consent_override.trim().is_empty() {
        return Some(Blocker::Gate);
    }
    None
}

/// Sends the form: `intervention.add`, or `intervention.update` with every field (`device: null`
/// for No PC). Nothing is sent while the form is incomplete or a save is in flight.
fn submit(app: &mut DeskApp) {
    let Some(f) = app.interventions.form.clone() else { return };
    if app.interventions.busy.is_some() || blocker(app, &f).is_some() {
        return;
    }
    let now = app.now();
    let Some(started) = stamp_of(&f.started, &f.stored_started, now).filter(|s| !s.is_empty()) else { return };
    let ended = stamp_of(&f.ended, &f.stored_ended, now).unwrap_or_default();
    let mut params = json!({
        "kind": forms::kind_id(f.kind),
        "summary": f.summary.trim(),
        "notes": f.notes,
        "reference": f.reference.trim(),
        "started_at": started,
        "billable": f.billable,
    });
    if !f.technician.trim().is_empty() {
        params["technician"] = json!(f.technician.trim());
    }
    if !ended.is_empty() {
        params["ended_at"] = json!(ended);
    }
    let intent = match f.id {
        Some(id) => {
            params["id"] = json!(id);
            params["device"] = f.device.id.map_or(Value::Null, |d| json!(d));
            // A running intervention left running counts no minutes yet.
            if let Some(m) = f.minutes.filter(|_| !(f.running && ended.is_empty())) {
                params["minutes"] = json!(m.min(MAX_MINUTES));
            }
            Intent::Saved
        }
        None => {
            if let Some(c) = f.customer.id {
                params["customer"] = json!(c);
            }
            if let Some(d) = f.device.id {
                params["device"] = json!(d);
            }
            if let Some(m) = f.minutes {
                params["minutes"] = json!(m.min(MAX_MINUTES));
            }
            let reason = f.consent_override.trim();
            if f.proceed && !reason.is_empty() && gate(app, &f).is_some() {
                params["consent_override"] = json!(reason);
            }
            Intent::Logged
        }
    };
    let command = if f.id.is_some() { "intervention.update" } else { "intervention.add" };
    let job = app.run_as(command, params, crate::Intent::Interventions(intent));
    app.interventions.busy = Some(job);
    if let Some(f) = app.interventions.form.as_mut() {
        f.error.clear();
    }
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

/// `widgets::banner`'s look laid out to the pane's width: the text wraps and the actions go on
/// the line under it (long labels would push the pane wider); the tone's bar is as tall as the
/// banner. Answers the index of the action clicked.
fn banner(ui: &mut egui::Ui, t: &Tokens, tone: Tone, icon: Icon, text: &str, actions: &[&str]) -> Option<usize> {
    widgets::banner(ui, t, tone, icon, text, actions)
}

/// The Minutes row: ticked, the minutes worked instead of the span (pauses); unticked, the span
/// the times give. Ends the row.
fn minutes_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, f: &mut Form, span: Option<u32>) {
    ui.label(RichText::new(app.tr("Minutes")).color(t.muted));
    ui.horizontal_wrapped(|ui| {
        let mut on = f.minutes.is_some();
        if ui.checkbox(&mut on, "").changed() {
            f.minutes = on.then(|| span.unwrap_or(30).min(MAX_MINUTES));
        }
        match f.minutes.as_mut() {
            Some(m) => {
                ui.add(egui::DragValue::new(m).range(0..=MAX_MINUTES).speed(1.0).suffix(" min"));
                ui.label(RichText::new(app.tr("Instead of the span (pauses)")).small().color(t.muted));
            }
            None => {
                let text = match span {
                    Some(m) => app.language.fill("From the times: {duration}", &[("duration", &format::duration_words(u64::from(m)))]),
                    None => app.tr("Tick to give the minutes instead of the end").to_string(),
                };
                ui.label(RichText::new(text).small().color(t.muted));
            }
        }
    });
    ui.end_row();
}

/// What the form asked for besides Save and Cancel.
enum FormRequest {
    /// Record the remote-assistance consent of this customer (the privacy form opens on top).
    Consent(u64),
}

/// Draws the open log-work / edit form (in the right pane): the customer and the PC (one of the
/// two; No PC when editing), the kind with the consent gate for remote work, the summary, the
/// start, the end or the minutes, the technician, the reference, billable, the notes; Save and
/// Cancel.
pub fn form_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(mut f) = app.interventions.form.clone() else { return };
    let busy = app.interventions.busy.is_some();
    let now = app.now();
    let editing = f.id.is_some();
    ui.label(RichText::new(app.tr(if editing { "Edit intervention" } else { "Log past work" })).size(20.0).strong().color(t.ui_text));
    if let Some(i) = f.id.and_then(|id| app.data().intervention(id)) {
        widgets::key_value(ui, t, app.tr("Updated"), &format::long(&i.updated_at, app.italian()));
    }
    ui.add_space(8.0);
    let mut request = None;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("intervention-form-who").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            let before = f.customer.id;
            forms::customer_picker(app, ui, t, app.tr("Customer"), &mut f.customer, editing);
            if f.customer.id != before
                && let Some(c) = f.customer.id
                && f.device.id.and_then(|d| app.data().device(d)).is_some_and(|d| d.customer_id != c)
            {
                f.device = Picker::default();
            }
            let before = f.device.id;
            forms::device_picker(app, ui, t, app.tr("PC"), &mut f.device, f.customer.id, editing);
            if f.device.id != before && !editing {
                if let Some(owner) = f.device.id.and_then(|d| app.data().device(d)).map(|d| d.customer_id)
                    && f.customer.id != Some(owner)
                {
                    f.customer = Picker::customer(app, Some(owner));
                }
                if !f.kind_touched {
                    f.kind = default_kind(app.data(), f.device.id);
                }
            }
            let before = f.kind;
            forms::kind_combo(app, ui, t, app.tr("Kind"), &mut f.kind);
            if f.kind != before {
                f.kind_touched = true;
            }
        });
        if let Some((state, blocks)) = gate(app, &f) {
            let customer = f.customer.id.or_else(|| f.device.id.and_then(|d| app.data().device(d)).map(|d| d.customer_id));
            ui.add_space(8.0);
            let text = app.language.fill("Remote-assistance consent is {state}", &[("state", &state_word(app, state))]);
            let mut actions = vec![app.tr("Record consent")];
            if blocks && !f.proceed {
                actions.push(app.tr("Proceed with a reason"));
            }
            let (tone, icon) = if blocks { (Tone::Bad, Icon::ShieldOff) } else { (Tone::Warn, Icon::Warning) };
            match banner(ui, t, tone, icon, &text, &actions) {
                Some(0) => request = customer.map(FormRequest::Consent),
                Some(_) => f.proceed = true,
                None => {}
            }
            if blocks && f.proceed {
                ui.label(RichText::new(app.tr("Reason for proceeding without consent")).color(t.muted));
                ui.add(
                    egui::TextEdit::singleline(&mut f.consent_override)
                        .hint_text(app.tr("The reason stays in the intervention and in the audit log"))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
            }
        }
        ui.add_space(8.0);
        egui::Grid::new("intervention-form-what").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            widgets::field(ui, t, &format!("{} *", app.tr("Summary")), &mut f.summary, "");
            let started = forms::time_field(app, ui, t, &format!("{} *", app.tr("Started")), &mut f.started, now, true);
            let ended = forms::time_field(app, ui, t, app.tr("Ended"), &mut f.ended, now, false);
            let span = match (started.as_deref(), ended.as_deref()) {
                (Some(s), Some(e)) if !s.is_empty() && !e.is_empty() => time::minutes_between(s, e),
                _ => None,
            };
            if !(f.running && f.ended.trim().is_empty()) {
                minutes_row(app, ui, t, &mut f, span);
            }
            let mut names = app.derived.technicians.clone();
            if !f.technician.is_empty() && !names.contains(&f.technician) {
                names.push(f.technician.clone());
            }
            let options: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), n.as_str())).collect();
            if let Some(picked) = widgets::combo(ui, t, "intervention-form-technician", app.tr("Technician"), &f.technician, &options) {
                f.technician = picked;
            }
            widgets::field(ui, t, app.tr("Reference"), &mut f.reference, app.tr("Ticket or job number in another system"));
            widgets::check(ui, t, app.tr("Billable"), &mut f.billable, "");
            widgets::multiline(ui, t, app.tr("Notes"), &mut f.notes, "", 4);
        });
        ui.add_space(4.0);
        if let Some(Blocker::Say(text, red)) = blocker(app, &f) {
            ui.label(RichText::new(app.tr(text)).small().color(if red { widgets::danger(t) } else { t.muted }));
        }
        widgets::error_line(ui, t, &f.error);
    });
    let enabled = blocker(app, &f).is_none();
    let save = if editing { app.tr("Save") } else { app.tr("Log work") };
    let action = forms::submit_row(app, ui, t, save, busy, enabled);
    if let Some(FormRequest::Consent(customer)) = request {
        app.interventions.form = Some(f);
        privacy::open_consent(app, customer, &["remote_assistance"]);
        return;
    }
    match action {
        FormAction::Cancel => {
            app.interventions.form = None;
            app.interventions.busy = None;
        }
        FormAction::Save => {
            app.interventions.form = Some(f);
            submit(app);
        }
        FormAction::None => app.interventions.form = Some(f),
    }
}

// ---------------------------------------------------------------------------------------------
// The detail

/// What the detail's action bar asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailAction {
    Back,
    Stop,
    Edit,
    Connect(u64),
    Delete,
}

/// The intervention's detail: the action bar, the title and the subtitle, the consent override,
/// the times and the billing, the notes and the record.
fn detail(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, narrow: bool) {
    let Some(i) = app.data().intervention(id).cloned() else { return };
    let now = app.now();
    let italian = app.italian();
    let running = i.running();
    let device = i.device_id.and_then(|d| app.data().device(d)).map(|d| (d.id, d.name.clone(), !d.rustdesk_id.is_empty()));
    let customer = crate::customer_name(app.data(), i.customer_id);
    let mut act = None;
    ui.horizontal(|ui| {
        if narrow && icon_button(ui, t, Icon::Back, app.tr("Back"), true, false).clicked() {
            act = Some(DetailAction::Back);
        }
        if running && widgets::primary_button(ui, t, Some(Icon::Stop), app.tr("Stop")).on_hover_text(app.tip("intervention.stop")).clicked() {
            act = Some(DetailAction::Stop);
        }
        ui.add_space(4.0);
        if icon_button(ui, t, Icon::Edit, app.tr("Edit intervention"), true, false).clicked() {
            act = Some(DetailAction::Edit);
        }
        if let Some((d, _, true)) = &device
            && icon_button(ui, t, Icon::Connect, app.tr("Connect"), true, false).clicked()
        {
            act = Some(DetailAction::Connect(*d));
        }
        if icon_button(ui, t, Icon::Trash, app.tr("Delete intervention"), true, false).clicked() {
            act = Some(DetailAction::Delete);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, t, Icon::Close, app.tr("Close"), true, false).clicked() {
                act = Some(DetailAction::Back);
            }
        });
    });
    match act {
        Some(DetailAction::Back) => {
            app.selected_intervention = None;
            return;
        }
        Some(DetailAction::Stop) => stop(app, id),
        Some(DetailAction::Edit) => {
            open_edit(app, id);
            return;
        }
        Some(DetailAction::Connect(d)) => app.connect(d),
        Some(DetailAction::Delete) => ask_delete(app, id),
        None => {}
    }
    ui.add_space(10.0);
    // The title and the subtitle.
    if i.summary.trim().is_empty() {
        ui.label(RichText::new(app.tr("(no summary)")).size(20.0).color(t.muted));
    } else {
        ui.add(egui::Label::new(RichText::new(&i.summary).size(20.0).strong().color(t.ui_text)).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !customer.is_empty() && ui.link(RichText::new(&customer).color(t.accent)).on_hover_text(app.tip("customer.get")).clicked() {
            app.open_customer(i.customer_id);
        }
        if let Some((d, name, _)) = &device {
            ui.label(RichText::new("·").color(t.muted));
            if ui.link(RichText::new(name).color(t.accent)).on_hover_text(app.tip("device.get")).clicked() {
                app.open_device(*d);
            }
        }
        ui.label(RichText::new("·").color(t.muted));
        widgets::tone_chip(ui, t, app.tr(forms::kind_label(i.kind)), Tone::Neutral);
        if !i.technician.is_empty() {
            ui.label(RichText::new("·").color(t.muted));
            crate::chrome::avatar(ui, t, &i.technician, crate::chrome::color_for(&i.technician), 20.0);
            ui.label(RichText::new(&i.technician).color(t.muted));
        }
    });
    ui.add_space(8.0);
    if !i.consent_override.trim().is_empty() {
        let text = app.language.fill("Carried out without a recorded consent: {reason}", &[("reason", i.consent_override.trim())]);
        banner(ui, t, Tone::Warn, Icon::Warning, &text, &[]);
    }
    // Time and billing.
    widgets::section(ui, t, app.tr("Time"));
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::key_value(ui, t, app.tr("Started"), &format::long(&i.started_at, italian));
        let worked = match &i.ended_at {
            Some(end) => {
                widgets::key_value(ui, t, app.tr("Ended"), &format::long(end, italian));
                u64::from(i.minutes)
            }
            None => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    widgets::dot(ui, t, Tone::Ok, 8.0);
                    let since = app.language.fill("Running since {time}", &[("time", &format::short(&i.started_at, now, italian))]);
                    ui.label(RichText::new(since).color(widgets::tone_colors(t, Tone::Ok).1));
                    widgets::mono(ui, t, &format::elapsed(&i.started_at, now), 15.0);
                });
                u64::from(time::minutes_between(&i.started_at, &time::stamp(now)).unwrap_or(0))
            }
        };
        widgets::key_value(ui, t, app.tr("Duration"), &format::duration_words(worked));
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if i.billable {
                widgets::tone_chip(ui, t, app.tr("Billable"), Tone::Ok);
                if !running {
                    let n = app.data().preferences.billable_minutes(&i);
                    ui.label(RichText::new(app.language.fill("Billable minutes: {n}", &[("n", &n.to_string())])).color(t.muted));
                }
            } else {
                widgets::tone_chip(ui, t, app.tr("Not billable"), Tone::Neutral);
            }
        });
        widgets::key_value(ui, t, app.tr("Reference"), &i.reference);
    });
    if !i.notes.trim().is_empty() {
        widgets::section(ui, t, app.tr("Notes"));
        widgets::card(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&i.notes).color(t.ui_text)).wrap().selectable(true));
        });
    }
    widgets::section(ui, t, app.tr("Record"));
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::key_value(ui, t, app.tr("Created"), &format::long(&i.created_at, italian));
        widgets::key_value(ui, t, app.tr("Updated"), &format::long(&i.updated_at, italian));
        ui.label(RichText::new(format!("#{}", i.id)).small().color(t.muted));
    });
    ui.add_space(12.0);
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
        app.go(Screen::Interventions);
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

    /// Starts a timer on a PC as the actor; answers its id.
    fn start(app: &mut DeskApp, device: u64) -> u64 {
        app.run("intervention.start", json!({"device": device, "summary": "In corso"}));
        frame(app);
        app.data().interventions.iter().filter(|i| i.running()).map(|i| i.id).max().expect("started")
    }

    #[test]
    fn the_screen_renders_with_every_pane_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                for width in [1440.0, 900.0] {
                    let mut app = app();
                    app.language = lang;
                    app.dark = dark;
                    let running = start(&mut app, 1);
                    app.run("intervention.update", json!({"id": 7, "billable": false}));
                    frame_at(&mut app, width);
                    for id in 1..=running {
                        app.selected_intervention = Some(id);
                        frame_at(&mut app, width);
                    }
                    new(&mut app);
                    frame_at(&mut app, width);
                    open_log(&mut app, Some(3), Some(5));
                    if let Some(f) = app.interventions.form.as_mut() {
                        f.kind = InterventionKind::Remote;
                        f.proceed = true;
                        f.started = "ieri".into();
                        f.error = "refused".into();
                        f.minutes = Some(45);
                    }
                    frame_at(&mut app, width);
                    open_edit(&mut app, running);
                    frame_at(&mut app, width);
                    open_edit(&mut app, 2);
                    frame_at(&mut app, width);
                    assert!(close(&mut app));
                    for (from, to) in [period_bounds(Period::ThisMonth, app.now()), period_bounds(Period::Last30, app.now()), ("x".into(), "".into())]
                    {
                        app.work_filter.from = from;
                        app.work_filter.to = to;
                        frame_at(&mut app, width);
                    }
                    app.work_filter = WorkFilter {
                        customer: Some(1),
                        device: Some(1),
                        technician: "riccardo".into(),
                        kind: "remote".into(),
                        billable_only: true,
                        running_only: true,
                        ..WorkFilter::default()
                    };
                    app.selected_intervention = None;
                    frame_at(&mut app, width);
                    app.work_filter = WorkFilter { running_only: true, customer: Some(2), ..WorkFilter::default() };
                    frame_at(&mut app, width);
                    app.work_filter = WorkFilter::default();
                    app.search = "nessuno".into();
                    frame_at(&mut app, width);
                    app.search.clear();
                    app.work_filter.from = "2000-01-01".into();
                    app.work_filter.to = "2000-02-01".into();
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
        app.go(Screen::Interventions);
        frame(&mut app);
        assert!(app.interventions.rows.is_empty());
        assert_eq!(app.interventions.totals, Totals::default());
        step(&mut app, 1);
        assert_eq!(app.selected_intervention, None);
        new(&mut app);
        assert!(app.interventions.form.as_ref().is_some_and(|f| f.customer.id.is_none() && f.kind == InterventionKind::Phone));
        frame(&mut app);
        let f = app.interventions.form.clone().expect("the form");
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Choose a customer or a PC", false)));
    }

    #[test]
    fn rows_put_running_first_then_newest_and_follow_the_filter() {
        let mut app = app();
        assert_eq!(app.interventions.rows, vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        let t = app.interventions.totals;
        assert_eq!((t.count, t.minutes), (10, 470));
        // Billable: rounded up to 15 minutes each, the phone call (6) is not billable.
        assert_eq!(t.billable, 30 + 75 + 120 + 45 + 15 + 90 + 30 + 45 + 45);
        let running = start(&mut app, 4);
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows.first(), Some(&running));
        app.work_filter.running_only = true;
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![running]);
        app.work_filter = WorkFilter { customer: Some(2), ..WorkFilter::default() };
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![running, 9, 6, 5]);
        app.work_filter.billable_only = true;
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![running, 9, 5]);
        app.work_filter = WorkFilter { kind: "lab".into(), ..WorkFilter::default() };
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![7]);
        app.work_filter = WorkFilter { device: Some(3), ..WorkFilter::default() };
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![4, 3]);
        app.work_filter = WorkFilter { technician: "Anna".into(), ..WorkFilter::default() };
        refresh_rows(&mut app);
        assert!(app.interventions.rows.is_empty());
        app.work_filter = WorkFilter { technician: "RICCARDO".into(), ..WorkFilter::default() };
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows.len(), 11);
        // The last 30 days: 20 days ago and newer (the end excluded, none given).
        app.work_filter = WorkFilter::default();
        let (from, to) = period_bounds(Period::Last30, app.now());
        app.work_filter.from = from;
        app.work_filter.to = to;
        assert_eq!(current_period(&app.work_filter, app.now()), Period::Last30);
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![running, 10, 9, 8, 7, 6, 5, 4]);
        app.work_filter.from = "garbage".into();
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows.len(), 11, "an unreadable bound is ignored");
        assert_eq!(current_period(&app.work_filter, app.now()), Period::Custom);
        app.work_filter = WorkFilter::default();
        assert_eq!(current_period(&app.work_filter, app.now()), Period::All);
        // The search, as the engine matches.
        app.search = "plotter".into();
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![10]);
        app.search = "corvo".into();
        refresh_rows(&mut app);
        assert_eq!(app.interventions.rows, vec![running, 9, 6, 5]);
        assert_eq!(hours(90, true), "1,5");
        assert_eq!(hours(90, false), "1.5");
    }

    #[test]
    fn j_and_k_walk_the_list() {
        let mut app = app();
        step(&mut app, 1);
        assert_eq!(app.selected_intervention, Some(10));
        step(&mut app, 2);
        assert_eq!(app.selected_intervention, Some(8));
        step(&mut app, -10);
        assert_eq!(app.selected_intervention, Some(10));
        step(&mut app, 100);
        assert_eq!(app.selected_intervention, Some(1));
        frame(&mut app);
        app.selected_intervention = None;
        step(&mut app, -1);
        assert_eq!(app.selected_intervention, Some(1));
    }

    #[test]
    fn past_work_is_logged_and_opened() {
        let mut app = app();
        open_log(&mut app, None, Some(4));
        let started = format::input(&time::stamp(app.now() - Duration::minutes(50)));
        let f = app.interventions.form.as_mut().expect("the form");
        assert_eq!((f.customer.id, f.device.id, f.kind), (Some(2), Some(4), InterventionKind::Remote));
        assert_eq!(f.technician, "riccardo");
        assert!(f.ended.is_empty() && f.started.is_empty());
        f.summary = "Stampante di rete".into();
        f.started = started;
        f.reference = "T-42".into();
        f.minutes = Some(40);
        let job_before = app.interventions.busy;
        assert!(job_before.is_none());
        frame(&mut app);
        submit(&mut app);
        assert!(app.interventions.busy.is_some());
        frame(&mut app);
        assert!(app.interventions.form.is_none(), "closed on success");
        assert!(app.interventions.busy.is_none());
        let i = app.data().interventions.iter().find(|i| i.summary == "Stampante di rete").expect("logged").clone();
        assert_eq!((i.customer_id, i.device_id, i.minutes, i.reference.as_str()), (2, Some(4), 40, "T-42"));
        assert!(!i.running());
        assert_eq!(app.selected_intervention, Some(i.id));
        assert_eq!(app.screen, Screen::Interventions);
        assert_eq!(toast(&app), app.language.fill("Intervention logged: {duration}", &[("duration", "40 min")]));
        frame(&mut app);
    }

    #[test]
    fn an_incomplete_form_is_not_sent() {
        let mut app = app();
        open_log(&mut app, Some(1), None);
        let f = app.interventions.form.clone().expect("the form");
        assert_eq!(f.kind, InterventionKind::Phone);
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Summary is required", false)));
        submit(&mut app);
        assert!(app.interventions.busy.is_none());
        let mut f = f;
        f.summary = "Telefonata".into();
        assert_eq!(blocker(&app, &f), Some(Blocker::Time), "no start");
        f.started = "domani forse".into();
        assert_eq!(blocker(&app, &f), Some(Blocker::Time), "an unreadable start");
        f.started = "2026-10-01 10:00".into();
        f.ended = "2026-10-01 09:00".into();
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("The end is before the start", true)));
        f.ended = "2026-10-09 10:00".into();
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("More than a week between start and end: check the times", true)));
        f.ended.clear();
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Give the end time or the minutes", false)));
        f.minutes = Some(20);
        assert_eq!(blocker(&app, &f), None);
        f.device.text = "qualcosa".into();
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Pick the PC from the list", false)));
        f.device = Picker::default();
        f.customer = Picker { text: "Lunaria".into(), id: None };
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Pick the customer from the list", false)));
        f.device = Picker::device(&app, Some(1));
        assert_eq!(blocker(&app, &f), Some(Blocker::Say("Pick the customer from the list", false)), "the PC alone does not hide it");
    }

    #[test]
    fn remote_work_without_consent_needs_a_reason() {
        let mut app = app();
        // Elio Brina (3) has no remote-assistance consent; his notebook (5) has a RustDesk ID.
        open_log(&mut app, None, Some(5));
        let f = app.interventions.form.as_mut().expect("the form");
        assert_eq!(f.kind, InterventionKind::Remote);
        f.summary = "Notebook lento".into();
        f.started = "2026-10-01 10:00".into();
        f.ended = "2026-10-01 10:30".into();
        let f = f.clone();
        assert_eq!(gate(&app, &f), Some((PurposeState::Missing, true)));
        assert_eq!(blocker(&app, &f), Some(Blocker::Gate));
        frame(&mut app);
        // The engine refuses it too (a form sent without the gate, e.g. a stale mirror).
        let job = app.run_as(
            "intervention.add",
            json!({"device": 5, "kind": "remote", "summary": "x", "started_at": "2026-10-01T08:00:00Z", "minutes": 5}),
            crate::Intent::Interventions(Intent::Logged),
        );
        app.interventions.busy = Some(job);
        frame(&mut app);
        let f = app.interventions.form.as_ref().expect("still open");
        assert!(!f.error.is_empty(), "the refusal shows in the form");
        assert!(app.interventions.busy.is_none());
        // With a reason it goes through, and the reason is kept.
        let f = app.interventions.form.as_mut().expect("the form");
        f.proceed = true;
        f.consent_override = "Ha acconsentito al telefono".into();
        let f = f.clone();
        assert_eq!(blocker(&app, &f), None);
        submit(&mut app);
        frame(&mut app);
        assert!(app.interventions.form.is_none());
        let i = app.data().interventions.iter().find(|i| i.summary == "Notebook lento" && i.id > 10).expect("logged");
        assert_eq!(i.consent_override, "Ha acconsentito al telefono");
        assert_eq!(i.minutes, 30);
        let id = i.id;
        frame(&mut app);
        assert_eq!(app.selected_intervention, Some(id));
        // Not remote: no gate.
        open_log(&mut app, Some(3), None);
        let f = app.interventions.form.clone().expect("the form");
        assert_eq!(gate(&app, &f), None);
        // Consent given (Lunaria): no gate either.
        open_log(&mut app, None, Some(1));
        let f = app.interventions.form.clone().expect("the form");
        assert_eq!(gate(&app, &f), None);
    }

    #[test]
    fn editing_keeps_the_stored_times_and_can_drop_the_pc() {
        let mut app = app();
        let before = app.data().intervention(2).cloned().expect("intervention 2");
        open_edit(&mut app, 2);
        assert_eq!(app.selected_intervention, Some(2));
        let f = app.interventions.form.as_mut().expect("the form");
        assert_eq!(f.minutes, None, "the minutes are the span");
        assert!(f.customer.id == Some(1) && f.device.id == Some(2));
        f.summary = "CAD aggiornato".into();
        f.device = Picker::default();
        f.billable = false;
        submit(&mut app);
        frame(&mut app);
        assert!(app.interventions.form.is_none());
        let after = app.data().intervention(2).cloned().expect("still there");
        assert_eq!(after.summary, "CAD aggiornato");
        assert_eq!(after.device_id, None);
        assert!(!after.billable);
        assert_eq!((after.started_at, after.ended_at, after.minutes), (before.started_at, before.ended_at, before.minutes));
        assert_eq!(toast(&app), app.tr("Intervention saved"));
        // Minutes that differ from the span are an override: shown and kept.
        open_edit(&mut app, 3);
        if let Some(f) = app.interventions.form.as_mut() {
            f.minutes = Some(100);
        }
        submit(&mut app);
        frame(&mut app);
        assert_eq!(app.data().intervention(3).map(|i| i.minutes), Some(100));
        open_edit(&mut app, 3);
        assert_eq!(app.interventions.form.as_ref().and_then(|f| f.minutes), Some(100));
        // A refusal keeps the form open with the message.
        if let Some(f) = app.interventions.form.as_mut() {
            f.device = Picker { text: "PC cassa".into(), id: Some(4) };
        }
        submit(&mut app);
        frame(&mut app);
        assert!(app.interventions.form.as_ref().is_some_and(|f| !f.error.is_empty()), "PC cassa belongs to another customer");
        open_edit(&mut app, 999);
        assert_eq!(app.interventions.form.as_ref().and_then(|f| f.id), Some(3), "no such intervention: the form is unchanged");
    }

    #[test]
    fn a_running_intervention_is_edited_without_closing_it() {
        let mut app = app();
        let id = start(&mut app, 1);
        open_edit(&mut app, id);
        let f = app.interventions.form.as_mut().expect("the form");
        assert!(f.running && f.ended.is_empty());
        f.summary = "Ancora in corso".into();
        f.minutes = Some(5);
        let f = f.clone();
        assert_eq!(blocker(&app, &f), None);
        submit(&mut app);
        frame(&mut app);
        let i = app.data().intervention(id).expect("there");
        assert!(i.running(), "still running");
        assert_eq!((i.summary.as_str(), i.minutes), ("Ancora in corso", 0));
    }

    #[test]
    fn delete_asks_first_and_a_colleagues_timer_is_stopped_after_a_confirmation() {
        let mut app = app();
        app.selected_intervention = Some(4);
        ask_delete(&mut app, 4);
        assert!(app.confirm.as_ref().is_some_and(|c| c.command == "intervention.remove" && c.danger));
        confirm(&mut app);
        assert!(app.data().intervention(4).is_none());
        assert_eq!(app.selected_intervention, None);
        assert_eq!(toast(&app), app.tr("Intervention deleted"));
        // Mine: the timer card opens in Stop mode, nothing is asked.
        let id = start(&mut app, 4);
        stop(&mut app, id);
        assert!(app.confirm.is_none());
        assert!(app.timer.card.is_some());
        // A colleague's: asked first.
        app.run("intervention.update", json!({"id": id, "technician": "anna"}));
        frame(&mut app);
        stop(&mut app, id);
        let c = app.confirm.as_ref().expect("asked");
        assert!(c.title.contains("anna") && c.danger && c.command == "intervention.stop");
        confirm(&mut app);
        assert!(app.data().intervention(id).is_some_and(|i| !i.running()));
        assert!(toast(&app).starts_with(&app.tr("Intervention closed: {duration}").replace("{duration}", "")));
        // Closed or missing: nothing happens.
        stop(&mut app, id);
        stop(&mut app, 999);
        ask_delete(&mut app, 999);
        assert!(app.confirm.is_none());
    }

    #[test]
    fn hostile_input_does_not_crash() {
        let mut app = app();
        open_log(&mut app, Some(999), Some(999));
        let f = app.interventions.form.as_ref().expect("the form");
        assert!(f.customer.id.is_none() && f.device.id.is_none());
        app.work_filter = WorkFilter {
            from: "9".repeat(10_000),
            to: "∞".into(),
            customer: Some(999),
            device: Some(u64::MAX),
            technician: "x".repeat(1000),
            kind: "nonsense".into(),
            ..WorkFilter::default()
        };
        frame(&mut app);
        assert!(app.interventions.rows.is_empty());
        if let Some(f) = app.interventions.form.as_mut() {
            f.summary = "é".repeat(5000);
            f.started = "\u{0}".into();
            f.minutes = Some(u32::MAX);
        }
        frame(&mut app);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let t = Tokens::for_dark_mode(false);
            row(&mut app, ui, &t, 999, true);
            row(&mut app, ui, &t, 1, false);
        });
        output.textures_delta.clear();
        assert_eq!(state_word(&app, PurposeState::Missing), app.tr("Missing").to_lowercase());
        assert_eq!(stamp_of("", "", app.now()), Some(String::new()));
        assert_eq!(stamp_of("nonsense", "", app.now()), None);
        open_edit(&mut app, 999);
        app.selected_intervention = Some(999);
        frame(&mut app);
        assert!(close(&mut app));
        assert!(!close(&mut app));
    }
}
