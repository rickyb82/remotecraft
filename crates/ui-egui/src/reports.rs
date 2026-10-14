//! The reports screen: hours and billable hours per customer, technician, month, PC or kind
//! over a period, for invoicing. Centre only: a controls row (period chips, group by, the
//! customer and technician pickers), the table answered by `report.time`, the totals and the
//! rounding note. A row drills down into the Interventions screen with the same period and that
//! group; the period and the pickers are shared with it through `app.work_filter`. Export CSV
//! and Copy as text hand the table to the invoicing program.

use egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::core::chrono::{DateTime, Datelike, Local, Utc};
use crate::core::fold;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, DeskApp, Intent as AppIntent, Request, Screen, format};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("This month", "Questo mese"),
    ("Last month", "Mese scorso"),
    ("This year", "Quest'anno"),
    ("Custom", "Personalizzato"),
    ("From", "Dal"),
    ("To", "Al"),
    ("Group by", "Raggruppa per"),
    ("By customer", "Per cliente"),
    ("By technician", "Per tecnico"),
    ("By month", "Per mese"),
    ("By device", "Per PC"),
    ("By kind", "Per tipo"),
    ("Every customer", "Tutti i clienti"),
    ("Every technician", "Tutti i tecnici"),
    ("Technician", "Tecnico"),
    ("Month", "Mese"),
    ("Kind", "Tipo"),
    ("Hours", "Ore"),
    ("Billable hours", "Ore fatturabili"),
    ("Total", "Totale"),
    (
        "Billable minutes are rounded up to {unit} min per intervention, minimum {min} min",
        "I minuti fatturabili sono arrotondati per eccesso a {unit} min per intervento, minimo {min} min",
    ),
    ("{n} running interventions excluded", "{n} interventi in corso esclusi"),
    ("1 running intervention excluded", "1 intervento in corso escluso"),
    ("No closed interventions between {from} and {to}", "Nessun intervento chiuso tra {from} e {to}"),
    ("Export CSV", "Esporta CSV"),
    ("CSV saved", "CSV salvato"),
    ("Copy as text", "Copia come testo"),
    ("Report copied", "Report copiato"),
    ("Show interventions", "Mostra gli interventi"),
];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `report.time` for the table (the job in flight is `State::pending`).
    Load,
}

/// How the hours are grouped: the `group_by` of `report.time`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupBy {
    #[default]
    Customer,
    Technician,
    Month,
    Device,
    Kind,
}

impl GroupBy {
    pub const ALL: [Self; 5] = [Self::Customer, Self::Technician, Self::Month, Self::Device, Self::Kind];

    /// The `group_by` parameter of `report.time`.
    pub const fn param(self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Technician => "technician",
            Self::Month => "month",
            Self::Device => "device",
            Self::Kind => "kind",
        }
    }

    /// Reads the engine's `group_by` value.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.param() == text.trim())
    }

    /// The label of the segment in the Group by control.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Customer => "By customer",
            Self::Technician => "By technician",
            Self::Month => "By month",
            Self::Device => "By device",
            Self::Kind => "By kind",
        }
    }

    /// The title of the table's first column.
    pub const fn column(self) -> &'static str {
        match self {
            Self::Customer => "Customer",
            Self::Technician => "Technician",
            Self::Month => "Month",
            Self::Device => "Device",
            Self::Kind => "Kind",
        }
    }
}

/// A period chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    ThisMonth,
    LastMonth,
    ThisYear,
    /// The From / To fields.
    Custom,
}

impl Period {
    pub const ALL: [Self; 4] = [Self::ThisMonth, Self::LastMonth, Self::ThisYear, Self::Custom];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ThisMonth => "This month",
            Self::LastMonth => "Last month",
            Self::ThisYear => "This year",
            Self::Custom => "Custom",
        }
    }
}

/// The period and the pickers as last shared with `app.work_filter`: `(from, to, customer,
/// technician)`.
type Shared = (String, String, Option<u64>, String);

/// This screen's own state: the controls, the loaded report and the job that fetches it.
#[derive(Default)]
pub struct State {
    /// The period as form text (`2026-10-01`), empty = no bound; the preset chips fill it.
    pub from: String,
    pub to: String,
    /// The Custom chip was chosen: the From / To fields show even on a preset's bounds.
    pub custom: bool,
    pub group_by: GroupBy,
    /// Only this customer's interventions (`None` = every customer).
    pub customer: Option<u64>,
    /// Only this technician's interventions (empty = every technician).
    pub technician: String,
    /// The last report loaded (`report.time`'s answer) and the key — parameters and revision —
    /// it answers: a different key greys the table until the new answer arrives.
    pub report: Option<Value>,
    pub loaded_key: Option<String>,
    /// The job in flight and its key.
    pending: Option<(u64, String)>,
    /// The engine's refusal of the last run, translated.
    pub error: String,
    /// What was last shared with `app.work_filter`, to notice changes made on other screens.
    shared: Option<Shared>,
}

/// One line of the table.
struct Row {
    /// The group's key as the engine gives it (a customer or device id, a technician, a
    /// `YYYY-MM`, a kind).
    key: String,
    label: String,
    interventions: u64,
    hours: String,
    billable: String,
}

/// Where the table's columns sit in a row: the label from `label_x` over `label_w`, the
/// numbers right-aligned at `interventions`, `hours` and `billable`.
struct Columns {
    label_x: f32,
    label_w: f32,
    interventions: f32,
    hours: f32,
    billable: f32,
}

impl Columns {
    fn of(rect: Rect) -> Self {
        let num_w = (rect.width() * 0.13).clamp(100.0, 140.0);
        let billable = rect.right() - 16.0;
        let hours = billable - num_w;
        let interventions = hours - num_w;
        let label_x = rect.left() + 16.0;
        let label_w = (interventions - num_w - label_x - 8.0).max(40.0);
        Self { label_x, label_w, interventions, hours, billable }
    }
}

const ROW_H: f32 = 40.0;
/// The least widths of the customer and the technician pickers.
const CUSTOMER_W: f32 = 220.0;
const TECHNICIAN_W: f32 = 180.0;

/// Draws the screen: the header, the controls, the table.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    adopt_shared(app);
    // Before the header, so a run caused by a change of the data shows its spinner at once;
    // again after the controls, for a change made this frame (the same key runs nothing).
    refresh(app);
    crate::central_panel(ui, t, |ui| {
        // The panel's width before anything is laid out: a control that did not fit widens
        // the panel's rect, and the table's right-aligned numbers would follow it off-screen.
        let width = ui.available_width();
        header(app, ui, t);
        controls(app, ui, t);
        publish_shared(app);
        refresh(app);
        let rect = Rect::from_min_size(ui.cursor().min, vec2(width.min(ui.available_width()).max(0.0), ui.available_height().max(0.0)));
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| table(app, ui, t));
    });
}

/// Applies a finished `report.time`: the table, or the error under the controls.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    match intent {
        Intent::Load => {
            let Some((job, key)) = app.reports.pending.clone() else { return };
            if job != c.job {
                // An older run, overtaken by a change of the controls.
                return;
            }
            app.reports.pending = None;
            app.reports.loaded_key = Some(key);
            match &c.result {
                Ok(v) => {
                    app.reports.report = Some(v.clone());
                    app.reports.error.clear();
                }
                Err(e) => app.reports.error = app.language.tr_message(&e.to_string()),
            }
        }
    }
}

/// Closes an open form; the reports have none, so never.
pub fn close(_app: &mut DeskApp) -> bool {
    false
}

// ---------------------------------------------------------------------------------------------
// The period

/// The bounds of a preset period as form text, the last day included (empty for Custom).
fn preset_bounds(period: Period, now: DateTime<Utc>) -> (String, String) {
    match period {
        Period::ThisMonth => format::month_bounds(now, 0),
        Period::LastMonth => format::month_bounds(now, 1),
        Period::ThisYear => {
            let y = now.with_timezone(&Local).year();
            (format!("{y:04}-01-01"), format!("{y:04}-12-31"))
        }
        Period::Custom => (String::new(), String::new()),
    }
}

/// The chip the bounds match (Custom when none does).
fn period_of(from: &str, to: &str, now: DateTime<Utc>) -> Period {
    [Period::ThisMonth, Period::LastMonth, Period::ThisYear]
        .into_iter()
        .find(|p| {
            let (f, t) = preset_bounds(*p, now);
            f == from && t == to
        })
        .unwrap_or(Period::Custom)
}

/// The first and the last day of a `YYYY-MM` month, as form text.
fn month_span(key: &str) -> Option<(String, String)> {
    let (y, m) = key.trim().split_once('-')?;
    let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
    if !(1..=12).contains(&m) {
        return None;
    }
    let last = format::days_in_month(i64::from(y), m);
    Some((format!("{y:04}-{m:02}-01"), format!("{y:04}-{m:02}-{last:02}")))
}

/// Takes the period and the pickers from `app.work_filter` when another screen changed them
/// (and, on the first show with nothing shared, starts on this month).
fn adopt_shared(app: &mut DeskApp) {
    let now = app.now();
    let wf = &app.work_filter;
    let current: Shared = (wf.from.clone(), wf.to.clone(), wf.customer, wf.technician.clone());
    if app.reports.shared.as_ref() == Some(&current) {
        return;
    }
    let first = app.reports.shared.is_none();
    let (from, to, customer, technician) = current;
    let s = &mut app.reports;
    if first && from.is_empty() && to.is_empty() {
        let (f, t) = preset_bounds(Period::ThisMonth, now);
        s.from = f;
        s.to = t;
    } else {
        s.from = from;
        s.to = to;
    }
    s.custom = false;
    s.customer = customer;
    s.technician = technician;
}

/// Writes the period and the pickers to `app.work_filter`, so the Interventions screen shows
/// the same slice.
fn publish_shared(app: &mut DeskApp) {
    let s = &app.reports;
    let shared: Shared = (s.from.clone(), s.to.clone(), s.customer, s.technician.clone());
    let wf = &mut app.work_filter;
    wf.from.clone_from(&shared.0);
    wf.to.clone_from(&shared.1);
    wf.customer = shared.2;
    wf.technician.clone_from(&shared.3);
    app.reports.shared = Some(shared);
}

// ---------------------------------------------------------------------------------------------
// Loading

/// A bound typed in a field as the stamp to send: `Ok(None)` when empty, `Err` when unreadable.
/// The end (`end`) is included: a date alone sends the start of the next day, as the engine
/// excludes its `to`.
fn bound(text: &str, now: DateTime<Utc>, end: bool) -> Result<Option<String>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let stamp = if end { format::end_stamp_from_input(text, now) } else { format::stamp_from_input(text, now) };
    stamp.map(Some).ok_or(())
}

/// The parameters of `report.time` for the controls and the key they make with the revision;
/// `None` while a bound is unreadable.
fn params(app: &DeskApp) -> Option<(Value, String)> {
    let s = &app.reports;
    let now = app.now();
    let from = bound(&s.from, now, false).ok()?;
    let to = bound(&s.to, now, true).ok()?;
    let technician = Some(s.technician.trim().to_string()).filter(|t| !t.is_empty());
    let key = format!(
        "{}|{}|{}|{}|{}|{}",
        from.as_deref().unwrap_or(""),
        to.as_deref().unwrap_or(""),
        s.group_by.param(),
        s.customer.map(|c| c.to_string()).unwrap_or_default(),
        technician.as_deref().unwrap_or(""),
        app.session.revision()
    );
    let params = json!({"from": from, "to": to, "group_by": s.group_by.param(), "customer": s.customer, "technician": technician});
    Some((params, key))
}

/// Runs `report.time` when the controls or the data changed since the last answer.
fn refresh(app: &mut DeskApp) {
    let Some((params, key)) = params(app) else { return };
    if app.reports.loaded_key.as_deref() == Some(key.as_str()) || app.reports.pending.as_ref().is_some_and(|(_, k)| *k == key) {
        return;
    }
    let job = app.run_as("report.time", params, AppIntent::Reports(Intent::Load));
    app.reports.pending = Some((job, key));
}

/// Whether the table shown answers the current controls (else it is greyed while the new
/// answer is on its way).
fn stale(app: &DeskApp) -> bool {
    match params(app) {
        Some((_, key)) => app.reports.loaded_key.as_deref() != Some(key.as_str()),
        None => true,
    }
}

/// The kind of an intervention, for people (the engine's key: `remote`, `onsite`…).
fn kind_label(key: &str, italian: bool) -> String {
    let (en, it) = match key {
        "remote" => ("Remote", "Remoto"),
        "onsite" => ("On site", "In sede"),
        "phone" => ("Phone", "Telefono"),
        "lab" => ("Lab", "Laboratorio"),
        "other" => ("Other", "Altro"),
        _ => (key, key),
    };
    (if italian { it } else { en }).to_string()
}

/// The table's lines from the engine's answer, labelled for people.
fn rows_of(app: &DeskApp, report: &Value) -> Vec<Row> {
    let italian = app.italian();
    let group = GroupBy::parse(report["group_by"].as_str().unwrap_or_default()).unwrap_or(app.reports.group_by);
    report["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let key = r["key"].as_str().unwrap_or_default().to_string();
            let label = match group {
                GroupBy::Month => format::month_title(&key, italian),
                GroupBy::Kind => kind_label(&key, italian),
                _ => r["label"].as_str().unwrap_or_default().to_string(),
            };
            Row {
                key,
                label: if label.trim().is_empty() { "—".into() } else { label },
                interventions: r["interventions"].as_u64().unwrap_or(0),
                hours: r["hours"].as_str().unwrap_or("0:00").to_string(),
                billable: r["billable_hours"].as_str().unwrap_or("0:00").to_string(),
            }
        })
        .collect()
}

/// The totals line: `(interventions, hours, billable hours)`.
fn totals_of(report: &Value, rows: &[Row]) -> (u64, String, String) {
    let n = rows.iter().map(|r| r.interventions).fold(0u64, u64::saturating_add);
    let hours = report["totals"]["hours"].as_str().unwrap_or("0:00").to_string();
    let billable = report["totals"]["billable_hours"].as_str().unwrap_or("0:00").to_string();
    (n, hours, billable)
}

/// Opens the Interventions screen on the period and the group of a row.
fn drill(app: &mut DeskApp, key: &str) {
    let s = &app.reports;
    let wf = &mut app.work_filter;
    wf.from.clone_from(&s.from);
    wf.to.clone_from(&s.to);
    wf.customer = s.customer;
    wf.technician.clone_from(&s.technician);
    wf.device = None;
    wf.no_device = false;
    wf.kind.clear();
    wf.running_only = false;
    wf.billable_only = false;
    match s.group_by {
        GroupBy::Customer => wf.customer = key.parse().ok(),
        GroupBy::Technician => wf.technician = key.to_string(),
        GroupBy::Device => {
            // The "—" row (key "") holds the interventions without a PC.
            wf.device = key.parse().ok();
            wf.no_device = key.trim().is_empty();
        }
        GroupBy::Kind => wf.kind = key.to_string(),
        GroupBy::Month => {
            if let Some((from, to)) = month_span(key) {
                wf.from = from;
                wf.to = to;
            }
        }
    }
    app.selected_intervention = None;
    app.go(Screen::Interventions);
}

// ---------------------------------------------------------------------------------------------
// Export

/// The table as text: one line per row with `sep` between the cells, a header and the totals.
fn table_text(app: &DeskApp, sep: char, eol: &str) -> Option<String> {
    let report = app.reports.report.as_ref()?;
    let rows = rows_of(app, report);
    let group = GroupBy::parse(report["group_by"].as_str().unwrap_or_default()).unwrap_or(app.reports.group_by);
    let cell = |s: &str| -> String {
        if sep == ';' && (s.contains(sep) || s.contains('"') || s.contains('\n')) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.replace(['\t', '\n', '\r', sep], " ")
        }
    };
    let line = |cells: [&str; 4]| -> String { cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(&sep.to_string()) };
    let mut out = line([app.tr(group.column()), app.tr("Interventions"), app.tr("Hours"), app.tr("Billable hours")]);
    out.push_str(eol);
    for r in &rows {
        out.push_str(&line([&r.label, &r.interventions.to_string(), &r.hours, &r.billable]));
        out.push_str(eol);
    }
    let (n, hours, billable) = totals_of(report, &rows);
    out.push_str(&line([app.tr("Total"), &n.to_string(), &hours, &billable]));
    out.push_str(eol);
    Some(out)
}

/// A period bound as a piece of a file name (`13/10/2026` → `13-10-2026`; empty → `all`).
fn file_part(text: &str) -> String {
    let part: String = text
        .trim()
        .chars()
        .map(|c| if matches!(c, '/' | ' ' | ':' | '.') { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    if part.is_empty() { "all".into() } else { part }
}

/// Saves the table as `ore-{from}-{to}.csv` (`;` separated, with a BOM so spreadsheets in
/// Italy open it as UTF-8).
fn export_csv(app: &mut DeskApp) {
    let Some(text) = table_text(app, ';', "\r\n") else { return };
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(text.as_bytes());
    let name = format!("ore-{}-{}.csv", file_part(&app.reports.from), file_part(&app.reports.to));
    app.requests.push(Request::SaveFile { name, data });
    app.toast(app.tr("CSV saved").to_string());
}

/// Copies the table, tab-separated, to paste into the invoicing program.
fn copy_text(app: &mut DeskApp, ui: &egui::Ui) {
    let Some(text) = table_text(app, '\t', "\n") else { return };
    ui.ctx().copy_text(text);
    app.toast(app.tr("Report copied").to_string());
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The technicians of the desk: everyone with an intervention, and the actor.
fn technicians(app: &DeskApp) -> Vec<String> {
    let mut names: Vec<String> = app.data().interventions.iter().map(|i| i.technician.trim().to_string()).filter(|t| !t.is_empty()).collect();
    let actor = app.session.actor().trim().to_string();
    if !actor.is_empty() {
        names.push(actor);
    }
    names.sort_by_key(|n| fold(n));
    names.dedup_by(|a, b| fold(a) == fold(b));
    names
}

/// The customers to pick from, by name (archived and erased ones too: their hours were worked).
fn customers(app: &DeskApp) -> Vec<(u64, String)> {
    let mut list: Vec<(u64, String)> = app.data().customers.iter().map(|c| (c.id, c.name.clone())).collect();
    list.sort_by_key(|(_, n)| fold(n));
    list
}

/// Whether the loaded report has lines (the exports need some).
fn has_rows(app: &DeskApp) -> bool {
    app.reports.report.as_ref().and_then(|r| r["rows"].as_array()).is_some_and(|rows| !rows.is_empty())
}

/// The header strip: the title, the spinner while a run is in flight, the export buttons.
fn header(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(app.tr("Time Report")).strong().color(t.ui_text));
            if app.reports.pending.is_some() {
                ui.add(egui::Spinner::new().size(16.0).color(t.accent));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = has_rows(app);
                if widgets::icon_text_button(ui, t, Icon::Copy, app.tr("Copy as text"), ready).clicked() {
                    copy_text(app, ui);
                }
                if widgets::icon_text_button(ui, t, Icon::Export, app.tr("Export CSV"), ready).clicked() {
                    export_csv(app);
                }
            });
        });
    });
    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top() - 0.5, Stroke::new(1.0, t.border));
}

/// A clickable filter chip: accent on `t.active` when chosen, a hairline otherwise.
fn chip_button(ui: &mut egui::Ui, t: &Tokens, text: &str, active: bool) -> egui::Response {
    let color = if active { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0, 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 14.0, t.active);
    } else if resp.hovered() {
        p.rect_filled(rect, 14.0, t.hover);
    } else {
        p.rect_stroke(rect, 14.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    p.galley(pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Group by segmented control; answers the segment clicked.
fn segmented(ui: &mut egui::Ui, t: &Tokens, options: &[(GroupBy, String)], current: GroupBy) -> Option<GroupBy> {
    let font = FontId::proportional(13.0);
    let segments: Vec<(GroupBy, std::sync::Arc<egui::Galley>, f32)> = options
        .iter()
        .map(|(value, label)| {
            let color = if *value == current { t.accent } else { t.ui_text };
            let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), color);
            let w = galley.size().x + 24.0;
            (*value, galley, w)
        })
        .collect();
    let total: f32 = segments.iter().map(|(_, _, w)| *w).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(total.max(40.0), 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 8.0, t.card);
    p.rect_stroke(rect, 8.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut x = rect.left();
    let mut picked = None;
    for (i, (value, galley, w)) in segments.into_iter().enumerate() {
        let seg = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        let r = ui.interact(seg, ui.id().with(("report-group", i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let active = value == current;
        if active {
            p.rect_filled(seg.shrink(2.0), 6.0, t.active);
        } else if r.hovered() {
            p.rect_filled(seg.shrink(2.0), 6.0, t.hover);
        }
        let color = if active { t.accent } else { t.ui_text };
        p.galley(pos2(seg.center().x - galley.size().x / 2.0, seg.center().y - galley.size().y / 2.0), galley, color);
        if r.clicked() {
            picked = Some(value);
        }
        x += w;
    }
    picked
}

/// The controls: the period chips (with From / To for a custom one), Group by, the pickers.
fn controls(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let now = app.now();
    egui::Frame::NONE.inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 6 }).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let active = if app.reports.custom { Period::Custom } else { period_of(&app.reports.from, &app.reports.to, now) };
            for period in Period::ALL {
                if chip_button(ui, t, app.tr(period.label()), active == period).clicked() {
                    match period {
                        Period::Custom => app.reports.custom = true,
                        preset => {
                            let (from, to) = preset_bounds(preset, now);
                            app.reports.from = from;
                            app.reports.to = to;
                            app.reports.custom = false;
                        }
                    }
                }
            }
            if active == Period::Custom {
                widgets::divider(ui, t);
                let unreadable = app.language.tr_message("is not a time");
                for (label, is_from) in [("From", true), ("To", false)] {
                    ui.label(RichText::new(app.tr(label)).small().color(t.muted));
                    let value = if is_from { &mut app.reports.from } else { &mut app.reports.to };
                    let ok = bound(value, now, !is_from).is_ok();
                    ui.add(egui::TextEdit::singleline(value).hint_text("2026-10-01").desired_width(110.0));
                    if !ok {
                        ui.label(RichText::new(&unreadable).small().color(widgets::danger(t)));
                    }
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(app.tr("Group by")).small().color(t.muted));
            let options: Vec<(GroupBy, String)> = GroupBy::ALL.into_iter().map(|g| (g, app.tr(g.label()).to_string())).collect();
            if let Some(g) = segmented(ui, t, &options, app.reports.group_by) {
                app.reports.group_by = g;
            }
            // A ComboBox takes its width where it stands and never wraps by itself: each picker
            // goes to a new line when the rest of this one cannot hold it (`available_width` is
            // a whole line's in a wrapping layout; the rest of this one is before the wrap).
            let gap = ui.spacing().item_spacing.x;
            if ui.available_size_before_wrap().x < CUSTOMER_W + 9.0 + 2.0 * gap {
                ui.end_row();
            } else {
                widgets::divider(ui, t);
            }
            // The customer picker.
            let customers = customers(app);
            let every = app.tr("Every customer").to_string();
            let shown = app.reports.customer.and_then(|id| customers.iter().find(|(i, _)| *i == id)).map_or(every.clone(), |(_, n)| n.clone());
            let mut pick: Option<Option<u64>> = None;
            egui::ComboBox::from_id_salt("report-customer").selected_text(shown).width(CUSTOMER_W).show_ui(ui, |ui| {
                if ui.selectable_label(app.reports.customer.is_none(), &every).clicked() {
                    pick = Some(None);
                }
                for (id, name) in &customers {
                    if ui.selectable_label(app.reports.customer == Some(*id), name).clicked() {
                        pick = Some(Some(*id));
                    }
                }
            });
            if let Some(p) = pick {
                app.reports.customer = p;
            }
            // The technician picker.
            let technicians = technicians(app);
            let everyone = app.tr("Every technician").to_string();
            let current = app.reports.technician.trim().to_string();
            let shown = if current.is_empty() { everyone.clone() } else { current.clone() };
            let mut pick: Option<String> = None;
            if ui.available_size_before_wrap().x < TECHNICIAN_W + gap {
                ui.end_row();
            }
            egui::ComboBox::from_id_salt("report-technician").selected_text(shown).width(TECHNICIAN_W).show_ui(ui, |ui| {
                if ui.selectable_label(current.is_empty(), &everyone).clicked() {
                    pick = Some(String::new());
                }
                for name in &technicians {
                    if ui.selectable_label(fold(&current) == fold(name), name).clicked() {
                        pick = Some(name.clone());
                    }
                }
            });
            if let Some(p) = pick {
                app.reports.technician = p;
            }
        });
    });
}

/// Paints a line of text twice, half a pixel apart (no bold font is loaded).
fn bold_text(p: &egui::Painter, pos: egui::Pos2, align: Align2, text: &str, font: FontId, color: egui::Color32) {
    p.text(pos, align, text, font.clone(), color);
    p.text(pos + vec2(0.5, 0.0), align, text, font, color);
}

/// The column titles on a chrome strip.
fn column_titles(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, group: GroupBy) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.chrome);
    p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.border));
    let cols = Columns::of(rect);
    let font = FontId::proportional(12.0);
    let cy = rect.center().y;
    p.text(pos2(cols.label_x, cy), Align2::LEFT_CENTER, app.tr(group.column()), font.clone(), t.muted);
    for (right, title) in [(cols.interventions, "Interventions"), (cols.hours, "Hours"), (cols.billable, "Billable hours")] {
        p.text(pos2(right, cy), Align2::RIGHT_CENTER, app.tr(title), font.clone(), t.muted);
    }
}

/// The note on the running interventions the report leaves out, singular for one.
fn excluded_text(app: &DeskApp, running: u64) -> String {
    let pattern = if running == 1 { "1 running intervention excluded" } else { "{n} running interventions excluded" };
    app.language.fill(pattern, &[("n", &running.to_string())])
}

/// The table: the rows, the totals, the notes; or the empty state.
fn table(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if !app.reports.error.is_empty() {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| widgets::error_line(ui, t, &app.reports.error));
    }
    let Some(report) = app.reports.report.clone() else {
        if app.reports.pending.is_some() {
            widgets::empty_state(ui, t, Icon::Sync, app.tr("Loading…"), "");
        }
        return;
    };
    let rows = rows_of(app, &report);
    let running = report["running_excluded"].as_u64().unwrap_or(0);
    let excluded = excluded_text(app, running);
    if rows.is_empty() {
        let italian = app.italian();
        let shown = |stamp: Option<&str>, text: &str| -> String {
            let d = stamp.map(|s| format::date(s, italian)).filter(|d| !d.is_empty());
            d.unwrap_or_else(|| if text.trim().is_empty() { "…".into() } else { text.trim().to_string() })
        };
        let from = shown(report["from"].as_str(), &app.reports.from);
        // The engine's `to` is excluded: shown as the last day it covers.
        let to = report["to"].as_str().map(|s| format::end_text(s, italian)).filter(|d| !d.is_empty());
        let to = to.unwrap_or_else(|| shown(None, &app.reports.to));
        let title = app.language.fill("No closed interventions between {from} and {to}", &[("from", &from), ("to", &to)]);
        widgets::empty_state(ui, t, Icon::Reports, &title, if running > 0 { &excluded } else { "" });
        return;
    }
    let group = GroupBy::parse(report["group_by"].as_str().unwrap_or_default()).unwrap_or(app.reports.group_by);
    let greyed = stale(app);
    let color = if greyed { t.muted } else { t.ui_text };
    let font = FontId::proportional(14.0);
    column_titles(app, ui, t, group);
    let mut clicked: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for row in &rows {
            let (rect, resp) = widgets::row(ui, t, ROW_H, false);
            let cols = Columns::of(rect);
            let cy = rect.center().y;
            let p = ui.painter();
            let label = widgets::truncate(p, &row.label, font.clone(), color, cols.label_w);
            p.galley(pos2(cols.label_x, cy - label.size().y / 2.0), label, color);
            for (right, text) in
                [(cols.interventions, row.interventions.to_string()), (cols.hours, row.hours.clone()), (cols.billable, row.billable.clone())]
            {
                p.text(pos2(right, cy), Align2::RIGHT_CENTER, text, font.clone(), color);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(app.tr("Show interventions")).clicked() {
                clicked = Some(row.key.clone());
            }
        }
        // The totals.
        let (n, hours, billable) = totals_of(&report, &rows);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H + 4.0), Sense::hover());
        let cols = Columns::of(rect);
        let cy = rect.center().y;
        let p = ui.painter();
        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.border));
        bold_text(p, pos2(cols.label_x, cy), Align2::LEFT_CENTER, app.tr("Total"), font.clone(), color);
        for (right, text) in [(cols.interventions, n.to_string()), (cols.hours, hours), (cols.billable, billable)] {
            bold_text(p, pos2(right, cy), Align2::RIGHT_CENTER, &text, font.clone(), color);
        }
        // The notes.
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
            let unit = report["billing_unit_minutes"].as_u64().unwrap_or(0);
            let min = report["minimum_minutes"].as_u64().unwrap_or(0);
            if unit > 1 || min > 0 {
                let note = app.language.fill(
                    "Billable minutes are rounded up to {unit} min per intervention, minimum {min} min",
                    &[("unit", &unit.max(1).to_string()), ("min", &min.to_string())],
                );
                ui.label(RichText::new(note).small().color(t.muted));
            }
            if running > 0 {
                ui.label(RichText::new(&excluded).small().color(widgets::tone_colors(t, Tone::Warn).1));
            }
        });
    });
    if let Some(key) = clicked {
        drill(app, &key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    /// Renders one frame of the app at 1440×900 (no window, no GPU).
    fn frame(app: &mut DeskApp) {
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    /// The demo desk on the Reports screen, over its whole history.
    fn open(lang: Language) -> DeskApp {
        let mut app = DeskApp::demo();
        app.language = lang;
        app.work_filter.from = "2000-01-01".into();
        app.go(Screen::Reports);
        app
    }

    fn labels(app: &DeskApp) -> Vec<String> {
        let report = app.reports.report.clone().unwrap();
        rows_of(app, &report).into_iter().map(|r| r.label).collect()
    }

    #[test]
    fn the_report_loads_and_renders_in_both_languages_and_groupings() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = open(lang);
                app.dark = dark;
                frame(&mut app);
                assert!(app.reports.pending.is_some(), "the first frame sends report.time");
                frame(&mut app);
                assert!(app.reports.pending.is_none() && app.reports.error.is_empty(), "{}", app.reports.error);
                assert!(!stale(&app));
                assert_eq!(labels(&app).len(), 3, "one row per demo customer");
                assert!(labels(&app).iter().any(|l| l.contains("Corvo")));
                assert_eq!(app.reports.loaded_key.as_deref().map(|k| k.contains("|customer|")), Some(true));

                app.reports.group_by = GroupBy::Technician;
                frame(&mut app);
                assert!(stale(&app), "greyed until the new answer arrives");
                frame(&mut app);
                assert_eq!(labels(&app), vec!["riccardo".to_string()]);

                app.reports.group_by = GroupBy::Month;
                frame(&mut app);
                frame(&mut app);
                let months = labels(&app);
                assert!(!months.is_empty());
                let expected = if lang == Language::It {
                    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"]
                } else {
                    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
                };
                assert!(months.iter().all(|m| expected.iter().any(|e| m.starts_with(e))), "{months:?}");

                app.reports.group_by = GroupBy::Kind;
                frame(&mut app);
                frame(&mut app);
                let kinds = labels(&app);
                let onsite = if lang == Language::It { "In sede" } else { "On site" };
                assert!(kinds.iter().any(|k| k == onsite), "{kinds:?}");

                app.reports.group_by = GroupBy::Device;
                frame(&mut app);
                frame(&mut app);
                assert!(labels(&app).iter().any(|l| l == "—"), "the phone call has no PC");

                // Only one customer, one technician.
                app.reports.customer = Some(3);
                app.reports.technician = "RICCARDO".into();
                app.reports.group_by = GroupBy::Customer;
                frame(&mut app);
                frame(&mut app);
                assert_eq!(labels(&app), vec!["Elio Brina".to_string()]);
                assert_eq!(app.work_filter.customer, Some(3));
                assert_eq!(app.work_filter.technician, "RICCARDO");
            }
        }
    }

    #[test]
    fn the_period_is_shared_and_a_row_drills_down() {
        let mut app = DeskApp::demo();
        let now = app.now();
        app.go(Screen::Reports);
        frame(&mut app);
        let (from, to) = format::month_bounds(now, 0);
        assert_eq!((app.work_filter.from.as_str(), app.work_filter.to.as_str()), (from.as_str(), to.as_str()), "this month by default");
        assert_eq!(period_of(&from, &to, now), Period::ThisMonth);
        let (from, to) = preset_bounds(Period::LastMonth, now);
        assert_eq!(period_of(&from, &to, now), Period::LastMonth);
        app.reports.from.clone_from(&from);
        app.reports.to.clone_from(&to);
        frame(&mut app);
        assert_eq!(app.work_filter.from, from);
        frame(&mut app);
        assert!(!stale(&app));
        let (from, to) = preset_bounds(Period::ThisYear, now);
        assert!(from.ends_with("-01-01") && to.ends_with("-12-31") && from.get(..4) == to.get(..4), "{from} {to}");
        assert_eq!(period_of(&from, &to, now), Period::ThisYear);
        assert_eq!(period_of("2026-02-01", "", now), Period::Custom);

        // An unreadable bound runs nothing and shows no engine error.
        let loaded = app.reports.loaded_key.clone();
        app.reports.custom = true;
        app.reports.from = "ieri".into();
        frame(&mut app);
        frame(&mut app);
        assert_eq!(app.reports.loaded_key, loaded);
        assert!(app.reports.error.is_empty());
        assert!(stale(&app));

        // The whole history, then a row click.
        app.reports.from = "2000-01-01".into();
        app.reports.to.clear();
        frame(&mut app);
        frame(&mut app);
        assert_eq!(labels(&app).len(), 3);
        drill(&mut app, "2");
        assert_eq!(app.screen, Screen::Interventions);
        assert_eq!(app.work_filter.customer, Some(2));
        assert_eq!(app.work_filter.from, "2000-01-01");
        assert_eq!(app.work_filter.to, "");
        app.reports.group_by = GroupBy::Month;
        drill(&mut app, "2026-12");
        assert_eq!((app.work_filter.from.as_str(), app.work_filter.to.as_str()), ("2026-12-01", "2026-12-31"));
        drill(&mut app, "2028-02");
        assert_eq!((app.work_filter.from.as_str(), app.work_filter.to.as_str()), ("2028-02-01", "2028-02-29"));
        app.reports.group_by = GroupBy::Kind;
        drill(&mut app, "phone");
        assert_eq!(app.work_filter.kind, "phone");
        app.reports.group_by = GroupBy::Technician;
        drill(&mut app, "riccardo");
        assert_eq!(app.work_filter.technician, "riccardo");
        assert_eq!(app.work_filter.kind, "", "a drill-down keeps only its own group");
        app.reports.group_by = GroupBy::Device;
        drill(&mut app, "4");
        assert_eq!((app.work_filter.device, app.work_filter.no_device), (Some(4), false));
        drill(&mut app, "");
        assert_eq!((app.work_filter.device, app.work_filter.no_device), (None, true), "the \"—\" row: the work without a PC");
        app.reports.group_by = GroupBy::Customer;
        drill(&mut app, "1");
        assert!(!app.work_filter.no_device, "a drill-down keeps only its own group");

        // Filters set on the Interventions screen are adopted when the Reports show again.
        app.work_filter.customer = Some(1);
        app.work_filter.from = "2026-01-01".into();
        app.work_filter.to = "2026-02-01".into();
        app.go(Screen::Reports);
        frame(&mut app);
        assert_eq!(app.reports.customer, Some(1));
        assert_eq!(app.reports.from, "2026-01-01");
        assert_eq!(month_span("2026-13"), None);
        assert_eq!(month_span("x"), None);
    }

    /// The texts painted in one frame of the app at `w`×`h`, with their rects.
    fn texts_at(app: &mut DeskApp, w: f32, h: f32) -> Vec<(String, Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect())),
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

    #[test]
    fn the_hours_stay_on_screen_in_a_narrow_window() {
        for lang in Language::ALL {
            for w in [900.0, 1000.0, 1150.0, 1440.0] {
                let mut app = open(lang);
                frame(&mut app);
                frame(&mut app);
                assert!(has_rows(&app));
                let texts = texts_at(&mut app, w, 700.0);
                for label in ["Billable hours", "Hours", "Every technician", "Every customer"] {
                    let wanted = app.tr(label).to_string();
                    let found: Vec<&Rect> = texts.iter().filter(|(text, _)| *text == wanted).map(|(_, r)| r).collect();
                    assert!(!found.is_empty(), "{wanted} at {w}: {texts:?}");
                    assert!(found.iter().all(|r| r.right() <= w + 0.5), "{wanted} at {w}: {found:?}");
                }
                // The totals line's numbers too.
                let total = app.reports.report.as_ref().and_then(|r| r["totals"]["billable_hours"].as_str()).unwrap_or_default().to_string();
                assert!(texts.iter().filter(|(text, _)| *text == total).all(|(_, r)| r.right() <= w + 0.5), "{total} at {w}");
            }
        }
    }

    #[test]
    fn the_end_of_the_period_is_included_as_a_whole_day() {
        let mut app = open(Language::It);
        let now = app.now();
        // From and To on the day intervention 10 started: it is counted.
        let started = app.data().intervention(10).map(|i| format::input(&i.started_at)).expect("intervention 10");
        let day = started.get(..10).expect("a date").to_string();
        app.work_filter.from.clone_from(&day);
        app.work_filter.to.clone_from(&day);
        frame(&mut app);
        frame(&mut app);
        let report = app.reports.report.clone().expect("loaded");
        let counted: u64 = report["rows"].as_array().into_iter().flatten().filter_map(|r| r["interventions"].as_u64()).sum();
        let expected = app.data().interventions.iter().filter(|i| !i.running() && format::input(&i.started_at).starts_with(&day)).count();
        assert!(counted >= 1 && counted == expected as u64, "{day}: {counted} of {expected}");
        assert_eq!(report["to"].as_str().map(str::to_string), format::end_stamp_from_input(&day, now), "the start of the next day");
        // The empty state names the last day included, not the day after.
        app.work_filter.from = "1990-10-01".into();
        app.work_filter.to = "1990-10-31".into();
        frame(&mut app);
        frame(&mut app);
        assert!(labels(&app).is_empty());
        let texts: Vec<String> = texts_at(&mut app, 1440.0, 900.0).into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t.contains("31 ott 1990") && !t.contains("1 nov 1990")), "{texts:?}");
        // An unreadable end runs nothing.
        assert_eq!(bound("ieri", now, true), Err(()));
        assert_eq!(bound(" ", now, true), Ok(None));
    }

    #[test]
    fn the_no_pc_row_drills_into_the_work_without_a_pc() {
        let mut app = open(Language::It);
        app.reports.group_by = GroupBy::Device;
        frame(&mut app);
        frame(&mut app);
        let report = app.reports.report.clone().expect("loaded");
        let rows = rows_of(&app, &report);
        let none = rows.iter().find(|r| r.key.is_empty()).expect("the \"—\" row");
        assert_eq!(none.label, "—");
        let count = none.interventions;
        drill(&mut app, "");
        frame(&mut app);
        assert_eq!(app.screen, Screen::Interventions);
        assert_eq!(app.work_filter.device, None);
        assert!(app.work_filter.no_device);
        // The Interventions header counts what the row counted.
        let header = if count == 1 { "1 intervento ·".to_string() } else { format!("{count} interventi ·") };
        let texts = texts_at(&mut app, 1440.0, 900.0);
        assert!(texts.iter().any(|(t, _)| t.starts_with(&header)), "{header}: {texts:?}");
    }

    #[test]
    fn the_running_note_says_one_intervention() {
        let mut app = DeskApp::demo();
        app.language = Language::En;
        assert_eq!(excluded_text(&app, 1), "1 running intervention excluded");
        assert_eq!(excluded_text(&app, 2), "2 running interventions excluded");
        app.language = Language::It;
        assert_eq!(excluded_text(&app, 1), "1 intervento in corso escluso");
        assert_eq!(excluded_text(&app, 3), "3 interventi in corso esclusi");
    }

    #[test]
    fn csv_and_text_exports() {
        let mut app = open(Language::It);
        frame(&mut app);
        frame(&mut app);
        export_csv(&mut app);
        let Some(Request::SaveFile { name, data }) = app.requests.first() else { panic!("no file") };
        assert_eq!(name, "ore-2000-01-01-all.csv");
        assert!(data.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(data.get(3..).unwrap_or_default().to_vec()).unwrap();
        assert!(text.starts_with("Cliente;Interventi;Ore;Ore fatturabili\r\n"), "{text}");
        assert!(text.contains("Ferramenta Corvo;3;"), "{text}");
        assert!(text.lines().last().is_some_and(|l| l.starts_with("Totale;10;")), "{text}");
        let tsv = table_text(&app, '\t', "\n").unwrap();
        assert!(tsv.starts_with("Cliente\tInterventi\tOre\tOre fatturabili\n"), "{tsv}");
        assert_eq!(tsv.lines().count(), 5);
        assert_eq!(file_part("13/10/2026 09:30"), "13-10-2026-09-30");
        assert_eq!(file_part("  "), "all");
        assert_eq!(kind_label("lab", true), "Laboratorio");
        assert_eq!(kind_label("odd", false), "odd");
        // Nothing to export without a report.
        let mut empty = DeskApp::demo();
        export_csv(&mut empty);
        assert!(empty.requests.is_empty());
    }

    #[test]
    fn a_refused_run_shows_its_error_and_an_old_answer_is_ignored() {
        let mut app = DeskApp::demo();
        let job = app.run_as("report.time", json!({"group_by": "bogus"}), AppIntent::Reports(Intent::Load));
        app.reports.pending = Some((job, "k".into()));
        frame(&mut app);
        // The demo desk speaks Italian: the engine's refusal arrives translated.
        assert!(app.reports.error.contains("group_by") || app.reports.error.contains("raggruppamento"), "{}", app.reports.error);
        assert!(app.reports.pending.is_none());
        assert_eq!(app.reports.loaded_key.as_deref(), Some("k"));
        assert!(app.reports.report.is_none());
        // An answer to a job that was overtaken changes nothing.
        let old = app.run_as("report.time", json!({}), AppIntent::Reports(Intent::Load));
        app.reports.pending = Some((old.wrapping_add(100), "newer".into()));
        frame(&mut app);
        assert!(app.reports.report.is_none());
        assert!(app.reports.pending.is_some());
        // The empty state over a period without work.
        let mut app = DeskApp::demo();
        app.language = Language::It;
        app.work_filter.from = "1990-01-01".into();
        app.work_filter.to = "1991-01-01".into();
        app.go(Screen::Reports);
        frame(&mut app);
        frame(&mut app);
        assert!(labels(&app).is_empty());
        assert!(!has_rows(&app));
        frame(&mut app);
    }
}
