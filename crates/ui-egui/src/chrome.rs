//! The chrome around the screens: the top bar (logo, search, the actor, settings, theme,
//! language), the status bar and the toast.

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::icon_button;
use crate::{DeskApp, Language, Screen};

/// A coloured circle with an initial, for customers and technicians.
pub fn avatar(ui: &mut egui::Ui, t: &Tokens, name: &str, color: Color32, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let p = ui.painter();
    p.circle_filled(rect.center(), size / 2.0, color);
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    p.text(rect.center(), egui::Align2::CENTER_CENTER, initial, FontId::proportional(size * 0.5), t.on_accent);
    resp
}

/// A stable colour for a name.
pub fn color_for(name: &str) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(0x4a, 0x6c, 0xf7),
        Color32::from_rgb(0xe0, 0x6a, 0x2c),
        Color32::from_rgb(0x22, 0xa0, 0x6b),
        Color32::from_rgb(0xb0, 0x4a, 0xd6),
        Color32::from_rgb(0xd6, 0x3a, 0x5e),
        Color32::from_rgb(0x1f, 0x9a, 0xb8),
        Color32::from_rgb(0xc9, 0x8a, 0x00),
        Color32::from_rgb(0x5a, 0x6e, 0x8c),
    ];
    let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    PALETTE.get((h % 8) as usize).copied().unwrap_or(Color32::GRAY)
}

/// The top bar.
pub fn top_bar(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let frame = egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(14, 8));
    egui::Panel::top("top").frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            icons::paint(ui.painter(), rect.shrink(2.0), Icon::Logo, t.accent);
            if resp.on_hover_text("RemoteCraft").clicked() {
                app.go(Screen::Customers);
            }
            ui.label(RichText::new("RemoteCraft").size(18.0).strong().color(t.ui_text));
            ui.add_space(18.0);
            // The search field: a painted pill with a text edit inside.
            let width = (ui.available_width() - 440.0).clamp(240.0, 720.0);
            let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
            ui.painter().rect_filled(rect, 18.0, t.panel);
            ui.painter().rect_stroke(rect, 18.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), Icon::Search, t.muted);
            let field = Rect::from_min_max(pos2(rect.left() + 38.0, rect.top() + 4.0), pos2(rect.right() - 34.0, rect.bottom() - 4.0));
            let hint = app.tr("Search customers, PCs, RustDesk IDs…").to_string();
            let te = ui.put(
                field,
                egui::TextEdit::singleline(&mut app.search)
                    .hint_text(hint)
                    .frame(egui::Frame::NONE)
                    .desired_width(field.width())
                    .font(FontId::proportional(14.0)),
            );
            if app.search_focus {
                te.request_focus();
                app.search_focus = false;
            }
            if te.changed() {
                app.search_changed();
            }
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                app.search_submit();
            }
            if !app.search.is_empty() {
                let close = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(24.0, 24.0));
                let r = ui.interact(close, ui.id().with("clear-search"), Sense::click());
                icons::paint(ui.painter(), close.shrink(4.0), Icon::Close, t.muted);
                if r.clicked() {
                    app.search.clear();
                    app.search_changed();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The technician at work.
                let actor = app.session.actor().to_string();
                let shown = if actor.is_empty() { "?".to_string() } else { actor.clone() };
                avatar(ui, t, &shown, color_for(&shown), 30.0).on_hover_text(app.language.fill("Working as {actor}", &[("actor", &shown)]));
                ui.label(RichText::new(&shown).color(t.muted));
                ui.add_space(6.0);
                if icon_button(ui, t, Icon::Settings, app.tr("Settings"), true, app.screen == Screen::Settings).clicked() {
                    if app.screen == Screen::Settings { app.go_back() } else { app.go(Screen::Settings) }
                }
                let (icon, tip) = if app.dark { (Icon::Sun, "Light theme") } else { (Icon::Moon, "Dark theme") };
                if icon_button(ui, t, icon, app.tr(tip), true, false).clicked() {
                    app.dark = !app.dark;
                    app.run("prefs.set", json!({"theme": if app.dark { "dark" } else { "light" }}));
                }
                let lang = icon_button(ui, t, Icon::Globe, &format!("{}: {}", app.tr("Language"), app.language.name()), true, false);
                egui::Popup::menu(&lang).show(|ui| {
                    for l in Language::ALL {
                        if ui.selectable_label(app.language == l, l.name()).clicked() {
                            app.language = l;
                            app.run("prefs.set", json!({"language": l.code()}));
                            ui.close();
                        }
                    }
                });
                if app.session.jobs_pending() > 0 {
                    ui.add(egui::Spinner::new().size(18.0).color(t.accent));
                }
                app.timer_chip(ui, t);
            });
        });
    });
}

/// The status bar: a problem or the last message on the left, counts on the right.
pub fn status_bar(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let frame = egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(14, 4));
    egui::Panel::bottom("status").frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            let problem = app.session.problem();
            let error = match problem.as_deref() {
                Some("connecting") => None,
                Some(p) => Some(app.language.fill(
                    "Server unreachable ({url}): {error}. Start it with: remotecraft-cli serve",
                    &[("url", &app.session.server()), ("error", &app.language.tr_message(p))],
                )),
                None => None,
            };
            match error {
                Some(e) => {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Warning, crate::widgets::danger(t));
                    ui.label(RichText::new(e).small().color(crate::widgets::danger(t)));
                }
                None if problem.is_some() => {
                    ui.add(egui::Spinner::new().size(12.0).color(t.accent));
                    ui.label(RichText::new(app.tr("Connecting…")).small().color(t.muted));
                }
                None => {
                    ui.label(RichText::new(&app.status).small().color(t.muted));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let d = app.session.data();
                let customers = d.customers.iter().filter(|c| !c.archived && c.erased_at.is_none()).count();
                let devices = d.devices.iter().filter(|x| !x.retired).count();
                let text = format!(
                    "{} · {} · {}",
                    app.language.fill("{n} customers", &[("n", &customers.to_string())]),
                    app.language.fill("{n} devices", &[("n", &devices.to_string())]),
                    app.language.fill("{n} interventions", &[("n", &d.interventions.len().to_string())]),
                );
                ui.label(RichText::new(text).small().color(t.muted));
            });
        });
    });
}

/// A transient message at the bottom centre.
pub fn toast(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some((text, since)) = app.toast.clone() else { return };
    if since.elapsed().as_secs_f32() > 4.0 {
        app.toast = None;
        return;
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -44.0)).order(egui::Order::Foreground).show(
        ui.ctx(),
        |ui| {
            egui::Frame::NONE
                .fill(if t.dark { Color32::from_rgb(0xe8, 0xe9, 0xee) } else { Color32::from_rgb(0x2a, 0x2c, 0x35) })
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).color(if t.dark { Color32::from_rgb(0x22, 0x25, 0x30) } else { Color32::WHITE }));
                });
        },
    );
}

/// A centred confirmation for actions that cannot be undone.
pub fn confirm(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(c) = app.confirm.clone() else { return };
    let mut answer: Option<bool> = None;
    egui::Window::new(&c.title)
        .id(egui::Id::new("confirm"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(&c.text).color(t.ui_text));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let yes = if c.danger {
                        crate::widgets::danger_button(ui, t, &c.ok_label)
                    } else {
                        crate::widgets::primary_button(ui, t, None, &c.ok_label)
                    };
                    if yes.clicked() {
                        answer = Some(true);
                    }
                    if ui.button(app.tr("Cancel")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            app.confirm = None;
            app.run_as(&c.command, c.params.clone(), c.intent.clone());
        }
        Some(false) => app.confirm = None,
        None => {}
    }
}
