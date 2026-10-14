//! The left navigation: the main action and the screens, with counts.

use egui::{Align2, FontId, RichText, Sense, vec2};

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::primary_button;
use crate::{DeskApp, Screen};

const NAV_W: f32 = 236.0;

/// One navigation row: an icon, a label and a count, in a pill when active.
pub fn nav_item(ui: &mut egui::Ui, t: &Tokens, icon: Option<Icon>, label: &str, count: usize, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    let pill = rect.shrink2(vec2(0.0, 1.0));
    if active {
        p.rect_filled(pill, 16.0, t.active);
    } else if resp.hovered() {
        p.rect_filled(pill, 16.0, t.hover);
    }
    let color = if active { t.accent } else { t.ui_text };
    let mut x = rect.left() + 14.0;
    if let Some(i) = icon {
        icons::paint(p, egui::Rect::from_center_size(egui::pos2(x + 9.0, rect.center().y), vec2(18.0, 18.0)), i, color);
        x += 28.0;
    }
    let max_w = (rect.right() - 44.0 - x).max(20.0);
    let galley = crate::widgets::truncate(p, label, FontId::proportional(14.0), color, max_w);
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    if count > 0 {
        p.text(egui::pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(12.5), t.muted);
    }
    resp
}

/// A navigation row with a text (an elapsed time) instead of a count on the right.
pub fn nav_item_text(ui: &mut egui::Ui, t: &Tokens, icon: Option<Icon>, label: &str, right: &str, active: bool) -> egui::Response {
    let resp = nav_item(ui, t, icon, label, 0, active);
    if !right.is_empty() {
        ui.painter().text(egui::pos2(resp.rect.right() - 14.0, resp.rect.center().y), Align2::RIGHT_CENTER, right, FontId::monospace(12.0), t.muted);
    }
    resp
}

/// The navigation panel.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let frame = egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 });
    egui::Panel::left("nav").exact_size(NAV_W).resizable(false).frame(frame).show(ui, |ui| {
        if primary_button(ui, t, Some(Icon::Play), app.tr("Start intervention")).on_hover_text(app.tip("intervention.start")).clicked() {
            app.start_intervention();
        }
        ui.add_space(10.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let d = app.session.data();
            let customers = d.customers.iter().filter(|c| !c.archived && c.erased_at.is_none()).count();
            let devices = d.devices.iter().filter(|x| !x.retired).count();
            let running = d.interventions.iter().filter(|i| i.running()).count();
            let entries = [
                (Screen::Customers, Icon::Customers, "Customers", customers),
                (Screen::Devices, Icon::Device, "Devices", devices),
                (Screen::Interventions, Icon::Interventions, "Interventions", running),
                (Screen::Privacy, Icon::Privacy, "Privacy", app.privacy_findings),
                (Screen::Reports, Icon::Reports, "Reports", 0),
            ];
            for (screen, icon, label, count) in entries {
                if nav_item(ui, t, Some(icon), app.tr(label), count, app.screen == screen).clicked() {
                    app.go(screen);
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new(app.tr("Running")).small().strong().color(t.muted));
            let actor = app.session.actor().to_string();
            let now = app.now();
            let mut rows: Vec<(u64, String, String, String, bool)> = app
                .derived
                .running
                .iter()
                .filter_map(|id| app.session.data().intervention(*id))
                .map(|i| {
                    (
                        i.id,
                        crate::customer_name(app.session.data(), i.customer_id),
                        i.technician.clone(),
                        crate::format::elapsed_short(&i.started_at, now),
                        i.technician == actor,
                    )
                })
                .collect();
            rows.sort_by_key(|r| !r.4);
            if rows.is_empty() {
                ui.label(RichText::new(app.tr("No running interventions")).small().color(t.muted));
            }
            for (id, customer, tech, elapsed, mine) in rows {
                let label = format!("{customer} · {tech}");
                let active = app.selected_intervention == Some(id) && app.screen == Screen::Interventions;
                if nav_item_text(ui, t, Some(Icon::Timer), &label, &elapsed, active).clicked() {
                    app.open_intervention(id);
                    if mine {
                        crate::timer::open_running(app, id, false);
                    }
                }
            }
        });
    });
}
