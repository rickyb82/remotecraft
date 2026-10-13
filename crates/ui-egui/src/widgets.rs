//! RemoteCraft's widgets: icon buttons, the accent button, chips, form rows, empty states.

use egui::{Color32, FontId, Rect, Response, RichText, Sense, Ui, vec2};

use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// A square icon button with a hover tint; `selected` keeps it tinted.
pub fn icon_button(ui: &mut Ui, t: &Tokens, icon: Icon, tip: &str, enabled: bool, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 8.0, t.active);
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, 8.0, t.hover);
    }
    let color = if !enabled {
        t.muted.gamma_multiply(0.55)
    } else if selected {
        t.accent
    } else {
        t.ui_text
    };
    icons::paint(p, Rect::from_center_size(rect.center(), vec2(19.0, 19.0)), icon, color);
    resp.on_hover_text(tip)
}

/// A labelled button with a leading icon, tinted on hover.
pub fn icon_text_button(ui: &mut Ui, t: &Tokens, icon: Icon, text: &str, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(14.0), t.ui_text);
    let size = vec2(galley.size().x + 44.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(rect, 8.0, t.hover);
    }
    let color = if enabled { t.ui_text } else { t.muted.gamma_multiply(0.55) };
    icons::paint(p, Rect::from_center_size(rect.left_center() + vec2(20.0, 0.0), vec2(18.0, 18.0)), icon, color);
    p.galley_with_override_text_color(rect.left_center() + vec2(36.0, -galley.size().y / 2.0), galley, color);
    resp
}

/// The filled accent button (the main action of a bar).
pub fn primary_button(ui: &mut Ui, t: &Tokens, icon: Option<Icon>, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(14.0), t.on_accent);
    let extra = if icon.is_some() { 26.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 28.0 + extra, 34.0), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { t.accent.gamma_multiply(0.9) } else { t.accent };
    p.rect_filled(rect, 8.0, fill);
    let mut x = rect.left() + 14.0;
    if let Some(i) = icon {
        icons::paint(p, Rect::from_center_size(egui::pos2(x + 9.0, rect.center().y), vec2(17.0, 17.0)), i, t.on_accent);
        x += extra;
    }
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, t.on_accent);
    resp
}

/// A thin vertical divider.
pub fn divider(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, t.border));
}

// ---------------------------------------------------------------------------------------------
// Forms, chips and empty states shared by every screen.

/// A small rounded label (a tag, a state): `fill` behind `text`-coloured text.
pub fn chip(ui: &mut Ui, text: &str, fill: Color32, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), color);
    let size = vec2(galley.size().x + 16.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 11.0, fill);
    p.galley(rect.left_center() + vec2(8.0, -galley.size().y / 2.0), galley, color);
    resp
}

/// The colours of a chip for a state: `ok`, `warn`, `bad` or neutral.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Neutral,
    Accent,
}

/// `(fill, text)` of a tone in the current theme.
pub fn tone_colors(t: &Tokens, tone: Tone) -> (Color32, Color32) {
    match (tone, t.dark) {
        (Tone::Ok, false) => (Color32::from_rgb(0xdc, 0xf5, 0xe6), Color32::from_rgb(0x12, 0x70, 0x3e)),
        (Tone::Ok, true) => (Color32::from_rgb(0x1e, 0x3f, 0x2e), Color32::from_rgb(0x7f, 0xe0, 0xa6)),
        (Tone::Warn, false) => (Color32::from_rgb(0xfd, 0xf0, 0xd5), Color32::from_rgb(0x8a, 0x5a, 0x00)),
        (Tone::Warn, true) => (Color32::from_rgb(0x45, 0x37, 0x16), Color32::from_rgb(0xff, 0xcf, 0x6b)),
        (Tone::Bad, false) => (Color32::from_rgb(0xfc, 0xe2, 0xe3), Color32::from_rgb(0xa8, 0x1f, 0x24)),
        (Tone::Bad, true) => (Color32::from_rgb(0x4a, 0x22, 0x24), Color32::from_rgb(0xff, 0x9a, 0x9e)),
        (Tone::Accent, _) => (t.active, t.accent),
        (Tone::Neutral, _) => (t.hover, t.muted),
    }
}

/// A state chip in a tone.
pub fn tone_chip(ui: &mut Ui, t: &Tokens, text: &str, tone: Tone) -> Response {
    let (fill, color) = tone_colors(t, tone);
    chip(ui, text, fill, color)
}

/// The red of errors and danger buttons.
pub fn danger(t: &Tokens) -> Color32 {
    if t.dark { Color32::from_rgb(0xff, 0x7a, 0x7e) } else { Color32::from_rgb(0xc5, 0x22, 0x1f) }
}

/// A filled red button for actions that cannot be undone.
pub fn danger_button(ui: &mut Ui, t: &Tokens, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(14.0), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 28.0, 34.0), Sense::click());
    let p = ui.painter();
    let red = danger(t);
    p.rect_filled(rect, 8.0, if resp.hovered() { red.gamma_multiply(0.9) } else { red });
    p.galley(egui::pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    resp
}

/// A section heading inside a pane or a form.
pub fn section(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).small().strong().color(t.muted));
    ui.add_space(2.0);
}

/// A form row: a muted label on the left, a single-line text field on the right. Call inside an
/// `egui::Grid` (two columns); ends the row.
pub fn field(ui: &mut Ui, t: &Tokens, label: &str, value: &mut String, hint: &str) -> Response {
    ui.label(RichText::new(label).color(t.muted));
    let r = ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(f32::INFINITY));
    ui.end_row();
    r
}

/// A form row with a multi-line text field of `rows` lines.
pub fn multiline(ui: &mut Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, rows: usize) -> Response {
    ui.label(RichText::new(label).color(t.muted));
    let r = ui.add(egui::TextEdit::multiline(value).hint_text(hint).desired_width(f32::INFINITY).desired_rows(rows));
    ui.end_row();
    r
}

/// A form row with a checkbox (the label is the checkbox's text).
pub fn check(ui: &mut Ui, t: &Tokens, label: &str, value: &mut bool, hint: &str) -> Response {
    ui.label(RichText::new("").color(t.muted));
    let r = ui.checkbox(value, label).on_hover_text(hint);
    ui.end_row();
    r
}

/// A form row with a combo box over `(value, label)` pairs; answers the value picked, if any.
pub fn combo(ui: &mut Ui, t: &Tokens, id: &str, label: &str, current: &str, options: &[(&str, &str)]) -> Option<String> {
    ui.label(RichText::new(label).color(t.muted));
    let shown = options.iter().find(|(v, _)| *v == current).map(|(_, l)| l.to_string()).unwrap_or_else(|| current.to_string());
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width().min(260.0)).show_ui(ui, |ui| {
        for (value, label) in options {
            if ui.selectable_label(current == *value, *label).clicked() {
                picked = Some(value.to_string());
            }
        }
    });
    ui.end_row();
    picked
}

/// A key and a value on one line of a detail pane (skipped when the value is empty).
pub fn key_value(ui: &mut Ui, t: &Tokens, key: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(key).small().color(t.muted));
        ui.label(RichText::new(value).color(t.ui_text));
    });
}

/// An error line under a form (nothing when empty).
pub fn error_line(ui: &mut Ui, t: &Tokens, text: &str) {
    if !text.is_empty() {
        ui.horizontal_wrapped(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Warning, danger(t));
            ui.label(RichText::new(text).color(danger(t)));
        });
    }
}

/// The empty state of a list or a pane: an icon, a title and a hint, centred.
pub fn empty_state(ui: &mut Ui, t: &Tokens, icon: Icon, title: &str, hint: &str) {
    ui.add_space(ui.available_height() * 0.3);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, t.muted.gamma_multiply(0.7));
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(16.0).color(t.ui_text));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).color(t.muted));
        }
    });
}

/// A card: the pane's surface with a border and rounded corners.
pub fn card(t: &Tokens) -> egui::Frame {
    egui::Frame::NONE.fill(t.card).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(12).inner_margin(egui::Margin::symmetric(16, 12))
}

/// A list row background: selected, hovered or plain; answers the row's response.
pub fn row(ui: &mut Ui, t: &Tokens, height: f32, selected: bool) -> (Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 0.0, t.active);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.hover);
    }
    p.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, t.border));
    (rect, resp)
}

/// A single line of text truncated with `…` at `max_w` pixels.
pub fn truncate(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(10.0));
    painter.layout_job(job)
}
