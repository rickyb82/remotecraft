//! What every form shares: the customer and PC pickers, the time field with its quick
//! buttons, the kind and method combos, the Save/Cancel row, and readers of command answers.

use egui::{FontId, Response, RichText, vec2};
use remotecraft_engine::core::chrono::{DateTime, Duration, Utc};
use remotecraft_engine::core::privacy::Method;
use remotecraft_engine::core::{InterventionKind, fold, rustdesk};
use remotecraft_engine::views;
use serde_json::Value;

use crate::derived::Cached;
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, DeskApp, format};

/// A record picked by name: the text typed and the id picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picker {
    pub text: String,
    pub id: Option<u64>,
}

impl Picker {
    pub fn customer(app: &DeskApp, id: Option<u64>) -> Self {
        Self { text: id.map(|i| crate::customer_name(app.data(), i)).unwrap_or_default(), id }
    }

    pub fn device(app: &DeskApp, id: Option<u64>) -> Self {
        Self { text: crate::device_name(app.data(), id), id }
    }
}

/// How many matches a picker lists.
const MATCHES: usize = 6;

/// The operator's intranet behind a customer field: the text searched there on request and
/// the answer of `intranet.search` (a [`Cached`] slot: the screen runs it and applies it).
#[derive(Clone, Debug, Default)]
pub struct IntranetLookup {
    /// The text searched (empty: nothing asked yet).
    pub query: String,
    pub cached: Cached,
}

/// A row of the intranet's answer, as the pickers list it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntranetRow {
    pub intranet_id: u64,
    pub name: String,
    /// The tax id and the city, for the second column.
    pub detail: String,
    pub ceased: bool,
    /// The desk's customer when already imported (or with the same tax id).
    pub customer: Option<u64>,
}

impl IntranetLookup {
    /// The rows of the last answer (the intranet's text clipped: it is not the desk's).
    pub fn rows(&self) -> Vec<IntranetRow> {
        let Some(list) = self.cached.value.as_ref().and_then(|v| v["customers"].as_array()) else { return Vec::new() };
        list.iter()
            .take(50)
            .filter_map(|r| {
                let intranet_id = r["intranet_id"].as_u64().filter(|i| *i > 0)?;
                let name = format::clipped(r["name"].as_str().unwrap_or_default(), 80);
                let detail = [r["tax_id"].as_str().unwrap_or_default(), r["city"].as_str().unwrap_or_default()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| format::clipped(s, 40))
                    .collect::<Vec<_>>()
                    .join(" · ");
                Some(IntranetRow { intranet_id, name, detail, ceased: r["ceased"].as_bool().unwrap_or(false), customer: r["customer"].as_u64() })
            })
            .collect()
    }

    /// Whether the last search was for this (folded) text.
    fn is_for(&self, fq: &str) -> bool {
        !self.query.is_empty() && fold(&self.query) == fq
    }
}

/// How a customer field behaves: `locked` shows the name without a field (the customer is
/// fixed); `create` offers to create a customer with the name typed; `intranet` (with
/// `create`, when the desk can reach it) offers to search the intranet and import from it.
#[derive(Clone, Copy, Default)]
pub struct PickOptions<'a> {
    pub locked: bool,
    pub create: bool,
    pub intranet: Option<&'a IntranetLookup>,
}

/// What a customer field asked for besides a pick: a customer to create with this name, a
/// search of the intranet for this text, a customer of the intranet to import (its id and
/// name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PickerAnswer {
    pub create: Option<String>,
    pub search: Option<String>,
    pub import: Option<(u64, String)>,
}

pub(crate) fn pick_row(ui: &mut egui::Ui, t: &Tokens, title: &str, sub: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 6.0, t.hover);
    }
    // Without a second column the title takes the row.
    let room = if sub.is_empty() { rect.width() - 16.0 } else { rect.width() * 0.55 };
    let name = widgets::truncate(p, title, FontId::proportional(14.0), t.ui_text, room);
    p.galley(egui::pos2(rect.left() + 8.0, rect.center().y - name.size().y / 2.0), name, t.ui_text);
    let sub = widgets::truncate(p, sub, FontId::proportional(12.0), t.muted, rect.width() * 0.4);
    p.galley(egui::pos2(rect.right() - 8.0 - sub.size().x, rect.center().y - sub.size().y / 2.0), sub, t.muted);
    resp
}

/// A form row picking a customer by name, tax id, e-mail or contact. The matches stay listed
/// until one is picked (a click takes the focus from the field, so they cannot follow it), and
/// a name typed exactly as the only customer it finds picks that customer; see [`PickOptions`]
/// for creating and importing. Call inside a two-column `Grid`; ends the row.
pub fn customer_picker(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, p: &mut Picker, options: PickOptions) -> PickerAnswer {
    let mut answer = PickerAnswer::default();
    ui.label(RichText::new(label).color(t.muted));
    if options.locked {
        ui.label(RichText::new(&p.text).color(t.ui_text));
        ui.end_row();
        return answer;
    }
    let create = options.create;
    let intranet = options.intranet.filter(|_| create && app.session.intranet());
    ui.vertical(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.text).hint_text(app.tr("Type a name…")).desired_width(f32::INFINITY));
        let fq = fold(p.text.trim());
        let data = app.data();
        let pickable = |c: &&remotecraft_engine::core::Customer| !c.archived && c.erased_at.is_none();
        let rid = crate::derived::rustdesk_query(&p.text);
        if r.changed() {
            // Only the one customer the text finds, so `Rossi` never picks itself while
            // `Rossi Mario` is being typed.
            let mut found =
                data.customers.iter().filter(pickable).filter(|c| !fq.is_empty() && crate::derived::customer_filter(data, c, &fq, rid.as_deref()));
            p.id = match (found.next(), found.next()) {
                (Some(c), None) if fold(&c.name) == fq => Some(c.id),
                _ => None,
            };
        }
        if p.id.is_none() && fq.chars().count() >= 2 {
            let hits: Vec<(u64, String, String)> = data
                .customers
                .iter()
                .filter(pickable)
                .filter(|c| crate::derived::customer_filter(data, c, &fq, rid.as_deref()))
                .take(MATCHES)
                .map(|c| (c.id, c.name.clone(), if c.tax_id.is_empty() { c.email.clone() } else { c.tax_id.clone() }))
                .collect();
            let none = hits.is_empty();
            for (id, name, sub) in hits {
                if pick_row(ui, t, &name, &sub).clicked() {
                    p.id = Some(id);
                    p.text = name;
                }
            }
            // A name the desk already has (an archived customer too) is not offered: the
            // engine would refuse it as a duplicate.
            let taken = data.customers.iter().any(|c| c.erased_at.is_none() && fold(&c.name) == fq);
            let archived = data.customers.iter().any(|c| c.archived && c.erased_at.is_none() && fold(&c.name) == fq);
            let name = p.text.trim().to_string();
            let shown = format::clipped(&name, 60);
            if create && !taken {
                let title = app.language.fill("Create the customer \"{name}\"", &[("name", &name)]);
                if pick_row(ui, t, &format!("+ {title}"), "").clicked() {
                    // As the engine stores it (a pasted tab becomes a space), so the answer
                    // finds the field still naming it.
                    let clean = remotecraft_engine::core::line(&name);
                    p.text = clean.clone();
                    answer.create = Some(clean);
                }
            } else if archived {
                let text = app.language.fill("The customer \"{name}\" is archived: restore it to pick it", &[("name", &shown)]);
                ui.label(RichText::new(text).small().color(t.muted));
            } else if none {
                let text = app.language.fill("No customer matches \"{query}\"", &[("query", &shown)]);
                ui.label(RichText::new(text).small().color(t.muted));
            }
            if let Some(lookup) = intranet {
                intranet_rows(app, ui, t, lookup, p, &mut answer);
            }
        }
    });
    ui.end_row();
    answer
}

/// Under a customer field: the row that searches the intranet for the text, or, once searched,
/// what it found (a customer already in the desk is picked; another is imported).
fn intranet_rows(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, lookup: &IntranetLookup, p: &mut Picker, answer: &mut PickerAnswer) {
    let name = p.text.trim().to_string();
    if !lookup.is_for(&fold(&name)) {
        let title = app.language.fill("Search the intranet for \"{name}\"", &[("name", &format::clipped(&name, 60))]);
        if pick_row(ui, t, &format!("» {title}"), "").clicked() {
            answer.search = Some(remotecraft_engine::core::line(&name));
        }
        return;
    }
    if lookup.cached.loading() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0).color(t.accent));
            ui.label(RichText::new(app.tr("Searching the intranet…")).small().color(t.muted));
        });
        return;
    }
    if !lookup.cached.error.is_empty() {
        widgets::error_line(ui, t, &app.language.tr_message(&lookup.cached.error));
        return;
    }
    let rows = lookup.rows();
    if rows.is_empty() {
        ui.label(RichText::new(app.tr("Nothing in the intranet")).small().color(t.muted));
        return;
    }
    ui.label(RichText::new(app.tr("From the intranet")).small().color(t.muted));
    let data = app.data();
    for row in rows.iter().take(MATCHES) {
        let sub = match row.customer {
            Some(_) => app.tr("Already in the desk").to_string(),
            None if row.ceased => format!("{} · {}", row.detail, app.tr("Ceased")),
            None => row.detail.clone(),
        };
        if pick_row(ui, t, &row.name, &sub).clicked() {
            match row.customer.filter(|c| data.customer(*c).is_some_and(|c| c.erased_at.is_none())) {
                Some(id) => {
                    p.id = Some(id);
                    p.text = crate::customer_name(data, id);
                }
                None => {
                    p.text.clone_from(&row.name);
                    answer.import = Some((row.intranet_id, row.name.clone()));
                }
            }
        }
    }
}

/// A form row picking a PC by name, host name or RustDesk ID, among `customer`'s PCs when
/// given; typing a RustDesk ID finds the PC anywhere. `allow_none` shows a "No PC" choice.
pub fn device_picker(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, p: &mut Picker, customer: Option<u64>, allow_none: bool) {
    ui.label(RichText::new(label).color(t.muted));
    ui.vertical(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.text).hint_text(app.tr("Name or RustDesk ID…")).desired_width(f32::INFINITY));
        if r.changed() {
            p.id = None;
        }
        let fq = fold(p.text.trim());
        // The matches stay listed until one is picked: a click takes the focus from the field.
        if p.id.is_none() && (fq.chars().count() >= 2 || (allow_none && r.has_focus())) {
            let data = app.data();
            if allow_none && pick_row(ui, t, app.tr("No PC"), "").clicked() {
                p.id = None;
                p.text.clear();
            }
            let rid = crate::derived::rustdesk_query(&p.text);
            let hits: Vec<(u64, String, String)> = data
                .devices
                .iter()
                .filter(|d| !d.retired)
                .filter(|d| customer.is_none_or(|c| d.customer_id == c) || rid.as_ref().is_some_and(|r| &d.rustdesk_id == r))
                .filter(|d| !fq.is_empty() && views::device_matches(d, &fq))
                .take(MATCHES)
                .map(|d| (d.id, d.name.clone(), format!("{} · {}", crate::customer_name(data, d.customer_id), rustdesk::display(&d.rustdesk_id))))
                .collect();
            for (id, name, sub) in hits {
                if pick_row(ui, t, &name, &sub).clicked() {
                    p.id = Some(id);
                    p.text = name;
                }
            }
        }
    });
    ui.end_row();
}

/// A form row for a time typed as the engine accepts it, with quick buttons; answers the stamp
/// to send when the text is valid (empty and not required → `Some("")`), and shows the error
/// line otherwise. Ends the row.
pub fn time_field(
    app: &DeskApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &str,
    value: &mut String,
    now: DateTime<Utc>,
    required: bool,
) -> Option<String> {
    ui.label(RichText::new(label).color(t.muted));
    let mut out = None;
    ui.vertical(|ui| {
        // The box takes the column's width (at most 220 px); the quick buttons wrap under it
        // when the pane is narrow, so the field never widens the pane.
        ui.horizontal_wrapped(|ui| {
            let room = ui.max_rect().right() - ui.cursor().left();
            ui.add(egui::TextEdit::singleline(value).hint_text("13/10/2026 09:30").desired_width(room.clamp(120.0, 220.0)));
            if ui.small_button(app.tr("Now")).clicked() {
                *value = format::input(&remotecraft_engine::core::time::stamp(now));
            }
            if ui.small_button(app.tr("−1 h")).clicked() {
                let base = format::stamp_from_input(value, now).and_then(|s| remotecraft_engine::core::time::parse_stamp(&s)).unwrap_or(now);
                *value = format::input(&remotecraft_engine::core::time::stamp(base - Duration::hours(1)));
            }
            if ui.small_button(app.tr("Yesterday")).clicked() {
                *value = format::input(&remotecraft_engine::core::time::stamp(now - Duration::days(1)));
            }
        });
        let text = value.trim();
        if text.is_empty() {
            if required {
                ui.label(RichText::new(app.tr("Required")).small().color(widgets::danger(t)));
            } else {
                out = Some(String::new());
            }
        } else {
            match format::stamp_from_input(text, now) {
                Some(s) => {
                    ui.label(RichText::new(format::long(&s, app.italian())).small().color(t.muted));
                    out = Some(s);
                }
                None => {
                    ui.label(RichText::new(app.tr("Not a time: use 2026-10-13 09:30, 13/10/2026 09:30 or 09:30")).small().color(widgets::danger(t)));
                }
            }
        }
    });
    ui.end_row();
    out
}

/// The kinds of intervention with their labels.
pub const KINDS: &[(InterventionKind, &str)] = &[
    (InterventionKind::Remote, "Remote"),
    (InterventionKind::Onsite, "On site"),
    (InterventionKind::Phone, "Phone"),
    (InterventionKind::Lab, "Lab"),
    (InterventionKind::Other, "Other"),
];

/// The engine's name of a kind (`remote`, `onsite`…).
pub fn kind_id(kind: InterventionKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The label of a kind, to translate.
pub fn kind_label(kind: InterventionKind) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l).unwrap_or("Other")
}

/// A form row with the kind combo; ends the row.
pub fn kind_combo(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, current: &mut InterventionKind) {
    ui.label(RichText::new(label).color(t.muted));
    egui::ComboBox::from_id_salt(("kind", label)).selected_text(app.tr(kind_label(*current))).show_ui(ui, |ui| {
        for (k, l) in KINDS {
            ui.selectable_value(current, *k, app.tr(l));
        }
    });
    ui.end_row();
}

/// The methods of a privacy event with their labels.
pub const METHODS: &[(Method, &str)] =
    &[(Method::Paper, "Paper"), (Method::Email, "E-mail"), (Method::Online, "Online"), (Method::Verbal, "Verbal"), (Method::Other, "Other")];

/// The engine's name of a method (`paper`, `email`…).
pub fn method_id(m: Method) -> String {
    crate::derived::method_name(m)
}

pub fn method_label(m: Method) -> &'static str {
    METHODS.iter().find(|(k, _)| *k == m).map(|(_, l)| *l).unwrap_or("Other")
}

/// A form row with the method combo; ends the row.
pub fn method_combo(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, current: &mut Method) {
    ui.label(RichText::new(label).color(t.muted));
    egui::ComboBox::from_id_salt(("method", label)).selected_text(app.tr(method_label(*current))).show_ui(ui, |ui| {
        for (k, l) in METHODS {
            ui.selectable_value(current, *k, app.tr(l));
        }
    });
    ui.end_row();
}

/// What the Save/Cancel row answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormAction {
    Save,
    Cancel,
    None,
}

/// The bottom row of a form: Cancel and the accent Save (`Saving…` and disabled while
/// `busy`); `Cmd+Enter` saves too while one of the form's own fields has the focus (call it in
/// the `ui` that holds the form's fields, after them).
pub fn submit_row(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, save_label: &str, busy: bool, enabled: bool) -> FormAction {
    let mut action = FormAction::None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if busy {
                ui.add_enabled(false, egui::Button::new(app.tr("Saving…")));
            } else if enabled {
                if widgets::primary_button(ui, t, None, save_label).clicked() {
                    action = FormAction::Save;
                }
            } else {
                ui.add_enabled(false, egui::Button::new(save_label));
            }
            if ui.button(app.tr("Cancel")).clicked() {
                action = FormAction::Cancel;
            }
        });
    });
    if action == FormAction::None
        && !busy
        && enabled
        && focus_inside(ui)
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
    {
        action = FormAction::Save;
    }
    action
}

/// Whether the focused widget lies in `ui` (its layer, its rectangle so far): `Cmd+Enter` typed
/// in the work card, the search box or another pane is not this form's.
fn focus_inside(ui: &egui::Ui) -> bool {
    let ctx = ui.ctx();
    let Some(id) = ctx.memory(|m| m.focused()) else { return false };
    ctx.read_response(id).is_some_and(|r| r.layer_id == ui.layer_id() && ui.min_rect().contains(r.rect.center()))
}

/// The message of a refusal or an error, translated, or `None` on success.
pub fn error_of(app: &DeskApp, c: &Completed) -> Option<String> {
    c.result.as_ref().err().map(|e| app.language.tr_message(&e.to_string()))
}

/// Whether the answer is the engine refusing for a missing remote-assistance consent.
pub fn is_consent_refusal(c: &Completed) -> bool {
    c.result.as_ref().err().is_some_and(|e| e.to_string().contains("remote_assistance consent is"))
}

/// The `warnings` of an answer, translated.
pub fn warnings(app: &DeskApp, v: &Value) -> Vec<String> {
    v["warnings"].as_array().map(|l| l.iter().filter_map(Value::as_str).map(|w| app.language.tr_message(w)).collect()).unwrap_or_default()
}

/// The id of the record an answer carries under `key` (`customer`, `device`, `intervention`…).
pub fn new_id(v: &Value, key: &str) -> Option<u64> {
    v[key]["id"].as_u64()
}

/// A CSV field: quoted when it holds the separator, a quote or a newline.
pub fn csv_escape(text: &str) -> String {
    if text.contains([';', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_string() }
}

/// A consent state as a tone chip.
pub fn state_chip(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, state: remotecraft_engine::core::privacy::PurposeState) -> Response {
    use remotecraft_engine::core::privacy::PurposeState as S;
    let (label, tone) = match state {
        S::Given => ("Given", Tone::Ok),
        S::Expired => ("Expired", Tone::Warn),
        S::Withdrawn => ("Withdrawn", Tone::Bad),
        S::Missing => ("Missing", Tone::Neutral),
    };
    widgets::tone_chip(ui, t, app.tr(label), tone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(kind_id(InterventionKind::Onsite), "onsite");
        assert_eq!(kind_label(InterventionKind::Lab), "Lab");
        assert_eq!(method_id(Method::Email), "email");
        assert_eq!(csv_escape("a;b"), "\"a;b\"");
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        let app = DeskApp::demo();
        assert_eq!(Picker::customer(&app, Some(2)).text, "Ferramenta Corvo");
        assert_eq!(Picker::device(&app, Some(4)).text, "PC cassa");
        assert_eq!(Picker::device(&app, None), Picker::default());
        assert_eq!(new_id(&serde_json::json!({"customer": {"id": 7}}), "customer"), Some(7));
        assert_eq!(warnings(&app, &serde_json::json!({"warnings": ["the customer is archived"]})).len(), 1);
    }

    #[test]
    fn cmd_enter_saves_only_from_the_forms_own_fields() {
        let app = DeskApp::demo();
        let t = Tokens::for_dark_mode(false);
        let ctx = egui::Context::default();
        let (inside, before, over) = (egui::Id::new("form-field"), egui::Id::new("search-field"), egui::Id::new("card-field"));
        let ctrl_enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE },
        };
        // One frame: a box above the form, the form (a box and the row), and a box floating
        // over the form in its own layer (the work card); `focus` is given at the end.
        let run = |focus: Option<egui::Id>, events: Vec<egui::Event>| {
            let mut action = FormAction::None;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let (mut a, mut b, mut c) = (String::new(), String::new(), String::new());
                ui.add(egui::TextEdit::singleline(&mut b).id(before));
                let form = ui.group(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut a).id(inside));
                    action = submit_row(&app, ui, &t, "Save", false, true);
                });
                egui::Area::new(egui::Id::new("card")).fixed_pos(form.response.rect.min).show(ui.ctx(), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut c).id(over));
                });
                if let Some(f) = focus {
                    ui.memory_mut(|m| m.request_focus(f));
                }
            });
            output.textures_delta.clear();
            action
        };
        run(None, Vec::new());
        assert_eq!(run(None, vec![ctrl_enter.clone()]), FormAction::None, "nothing focused");
        for (focus, expected) in [(before, FormAction::None), (over, FormAction::None), (inside, FormAction::Save)] {
            run(Some(focus), Vec::new());
            assert_eq!(run(None, vec![ctrl_enter.clone()]), expected, "{focus:?}");
        }
    }
}
