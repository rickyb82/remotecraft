//! What every form shares: the customer and PC pickers, the time field with its quick
//! buttons, the kind and method combos, the Save/Cancel row, and readers of command answers.

use egui::{FontId, Response, RichText, vec2};
use remotecraft_engine::core::chrono::{DateTime, Duration, Utc};
use remotecraft_engine::core::privacy::Method;
use remotecraft_engine::core::{InterventionKind, fold, rustdesk};
use remotecraft_engine::views;
use serde_json::Value;

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

fn pick_row(ui: &mut egui::Ui, t: &Tokens, title: &str, sub: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 6.0, t.hover);
    }
    let name = widgets::truncate(p, title, FontId::proportional(14.0), t.ui_text, rect.width() * 0.55);
    p.galley(egui::pos2(rect.left() + 8.0, rect.center().y - name.size().y / 2.0), name, t.ui_text);
    let sub = widgets::truncate(p, sub, FontId::proportional(12.0), t.muted, rect.width() * 0.4);
    p.galley(egui::pos2(rect.right() - 8.0 - sub.size().x, rect.center().y - sub.size().y / 2.0), sub, t.muted);
    resp
}

/// A form row picking a customer by name, tax id, e-mail or contact. `locked` shows the name
/// without a field (the customer is fixed). Call inside a two-column `Grid`; ends the row.
pub fn customer_picker(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, p: &mut Picker, locked: bool) {
    ui.label(RichText::new(label).color(t.muted));
    if locked {
        ui.label(RichText::new(&p.text).color(t.ui_text));
        ui.end_row();
        return;
    }
    ui.vertical(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.text).hint_text(app.tr("Type a name…")).desired_width(f32::INFINITY));
        if r.changed() {
            p.id = None;
        }
        let fq = fold(p.text.trim());
        if p.id.is_none() && r.has_focus() && fq.chars().count() >= 2 {
            let rid = crate::derived::rustdesk_query(&p.text);
            let data = app.data();
            let hits: Vec<(u64, String, String)> = data
                .customers
                .iter()
                .filter(|c| !c.archived && c.erased_at.is_none() && crate::derived::customer_filter(data, c, &fq, rid.as_deref()))
                .take(MATCHES)
                .map(|c| (c.id, c.name.clone(), if c.tax_id.is_empty() { c.email.clone() } else { c.tax_id.clone() }))
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
        if p.id.is_none() && r.has_focus() && (fq.chars().count() >= 2 || allow_none) {
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
/// `busy`); `Cmd+Enter` anywhere in the window saves too.
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
    if action == FormAction::None && !busy && enabled && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        action = FormAction::Save;
    }
    action
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
}
