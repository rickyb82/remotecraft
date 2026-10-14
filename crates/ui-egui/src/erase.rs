//! Erasing a customer's personal data (GDPR art. 17): the only caller of `gdpr.erase`. A
//! red-bordered card replaces the customer card: what erasure does and keeps, then three steps —
//! (1) export a copy first if the person wants one, (2) the reason, (3) the customer's name
//! typed to confirm (that is the confirmation: no dialog follows). Erase stays disabled while
//! the customer has a running intervention (the engine refuses it too).

use egui::{Align, Layout, RichText, Sense, Stroke, vec2};
use remotecraft_engine::core::chrono::{DateTime, Utc};
use remotecraft_engine::core::{clip, fold, slug, time};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, DeskApp, Request, format};

/// The labels this card shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("Erase {name}'s personal data", "Cancella i dati personali di {name}"),
    (
        "Erasing anonymises the customer (name, contacts, address, notes) and their PCs (RustDesk ID, hostname, serial, user, notes) and removes the text of their interventions. Times, durations, the technician and the privacy record stay as proof, without names.",
        "La cancellazione anonimizza il cliente (nome, contatti, indirizzo, note) e i suoi PC (ID RustDesk, nome host, seriale, utente, note) e toglie il testo dei suoi interventi. Tempi, durate, il tecnico e il registro privacy restano come prova, senza nomi.",
    ),
    ("1. Export a copy first (if the person wants one)", "1. Prima esporta una copia (se la persona la vuole)"),
    ("Exported and saved", "Esportato e salvato"),
    ("2. Reason", "2. Motivo"),
    ("Who asked, when and how", "Chi l'ha chiesto, quando e come"),
    ("3. Type the customer's name to confirm", "3. Scrivi il nome del cliente per confermare"),
    ("Erase", "Cancella"),
    ("Erasing…", "Cancellazione…"),
    ("Stop the running interventions first", "Chiudi prima gli interventi in corso"),
    ("Personal data erased", "Dati personali cancellati"),
];

/// What a command queued by this card was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `gdpr.export` from step 1: the answer is saved as a file.
    Exported,
    /// `gdpr.erase`.
    Erased,
}

/// The card's state: open for `customer`, with what was typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub customer: Option<u64>,
    pub reason: String,
    /// The customer's name as typed to confirm.
    pub typed: String,
    /// Step 1 was done: the export was saved.
    pub exported: bool,
    /// `gdpr.erase` in flight.
    pub busy: Option<u64>,
    /// `gdpr.export` in flight.
    pub exporting: Option<u64>,
    pub error: String,
}

/// A label of this card in the interface language (this table first, then the shared ones).
fn tr<'a>(app: &DeskApp, text: &'a str) -> &'a str {
    if app.italian()
        && let Some((_, it)) = LABELS_IT.iter().find(|(en, _)| *en == text)
    {
        return it;
    }
    app.tr(text)
}

/// A label with `{name}` placeholders filled.
fn fill(app: &DeskApp, text: &str, values: &[(&str, &str)]) -> String {
    let mut out = tr(app, text).to_string();
    for (k, v) in values {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// The file name of a customer's export: `export-{slug}-{YYYY-MM-DD}.json` (local date).
pub fn export_name(name: &str, now: DateTime<Utc>) -> String {
    let s = slug(name);
    let s = if s.is_empty() { "customer".to_string() } else { clip(&s, 60).trim_end_matches('-').to_string() };
    let day = format::input(&time::stamp(now));
    format!("export-{s}-{}.json", day.get(..10).unwrap_or_default())
}

/// A name compared for the confirmation: folded, spaces collapsed.
fn same_name(typed: &str, name: &str) -> bool {
    let norm = |s: &str| fold(s).split_whitespace().collect::<Vec<_>>().join(" ");
    let name = norm(name);
    !name.is_empty() && norm(typed) == name
}

/// Opens the erase card for a customer: the Customers screen with the customer selected.
pub fn open(app: &mut DeskApp, customer: u64) {
    if app.data().customer(customer).is_none_or(|c| c.erased_at.is_some()) {
        return;
    }
    app.erase = State { customer: Some(customer), ..State::default() };
    app.customers.form = None;
    app.open_customer(customer);
}

/// Whether the customer has a running intervention (erasure must wait).
fn running(app: &DeskApp, customer: u64) -> bool {
    app.data().interventions.iter().any(|i| i.customer_id == customer && i.running())
}

/// Whether Erase can be pressed: nothing in flight, nothing running, a reason, the name typed.
pub fn ready(app: &DeskApp) -> bool {
    let e = &app.erase;
    let Some(id) = e.customer else { return false };
    let Some(c) = app.data().customer(id) else { return false };
    e.busy.is_none() && c.erased_at.is_none() && !running(app, id) && !e.reason.trim().is_empty() && same_name(&e.typed, &c.name)
}

/// What the card's buttons asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Export,
    Erase,
    Cancel,
}

/// A numbered step's heading.
fn step(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(text).strong().color(t.ui_text));
    ui.add_space(2.0);
}

/// Draws the card in the customer's pane; answers whether it is open (and drawn).
pub fn ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let Some(id) = app.erase.customer else { return false };
    let Some(c) = app.data().customer(id) else {
        app.erase = State::default();
        return false;
    };
    if c.erased_at.is_some() && app.erase.busy.is_none() {
        app.erase = State::default();
        return false;
    }
    let name = c.name.clone();
    let blocked = running(app, id);
    let can = ready(app);
    let (busy, exporting, exported) = (app.erase.busy.is_some(), app.erase.exporting.is_some(), app.erase.exported);
    let mut reason = app.erase.reason.clone();
    let mut typed = app.erase.typed.clone();
    let error = app.erase.error.clone();
    let mut act = None;
    let red = widgets::danger(t);
    egui::Frame::NONE.fill(t.card).stroke(Stroke::new(1.0, red)).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Erase, red);
            ui.add(egui::Label::new(RichText::new(fill(app, "Erase {name}'s personal data", &[("name", &name)])).size(18.0).strong().color(t.ui_text)).wrap());
        });
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(tr(
                    app,
                    "Erasing anonymises the customer (name, contacts, address, notes) and their PCs (RustDesk ID, hostname, serial, user, notes) and removes the text of their interventions. Times, durations, the technician and the privacy record stay as proof, without names.",
                ))
                .color(t.muted),
            )
            .wrap(),
        );
        step(ui, t, tr(app, "1. Export a copy first (if the person wants one)"));
        ui.horizontal(|ui| {
            if exporting {
                ui.add(egui::Spinner::new().size(16.0).color(t.accent));
                ui.label(RichText::new(tr(app, "Loading…")).color(t.muted));
            } else if widgets::icon_text_button(ui, t, Icon::Export, tr(app, "Export personal data"), !busy).clicked() {
                act = Some(Act::Export);
            }
            if exported {
                let ok = widgets::tone_colors(t, Tone::Ok).1;
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Check, ok);
                ui.label(RichText::new(tr(app, "Exported and saved")).color(ok));
            }
        });
        step(ui, t, &format!("{} *", tr(app, "2. Reason")));
        ui.add(egui::TextEdit::multiline(&mut reason).id_salt("erase-reason").hint_text(tr(app, "Who asked, when and how")).desired_rows(3).desired_width(f32::INFINITY));
        step(ui, t, tr(app, "3. Type the customer's name to confirm"));
        ui.add(egui::Label::new(RichText::new(&name).font(egui::FontId::monospace(13.0)).color(t.ui_text)).selectable(true));
        ui.add(egui::TextEdit::singleline(&mut typed).id_salt("erase-typed").desired_width(f32::INFINITY));
        ui.add_space(8.0);
        if blocked {
            crate::customers::banner(ui, t, Tone::Bad, Icon::Timer, tr(app, "Stop the running interventions first"), &[]);
        }
        widgets::error_line(ui, t, &error);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if busy {
                    ui.add_enabled(false, egui::Button::new(tr(app, "Erasing…")));
                } else if can {
                    if widgets::danger_button(ui, t, tr(app, "Erase")).clicked() {
                        act = Some(Act::Erase);
                    }
                } else {
                    let r = ui.add_enabled(false, egui::Button::new(tr(app, "Erase")));
                    if blocked {
                        r.on_disabled_hover_text(tr(app, "Stop the running interventions first"));
                    }
                }
                if ui.button(tr(app, "Cancel")).clicked() {
                    act = Some(Act::Cancel);
                }
            });
        });
    });
    app.erase.reason = reason;
    app.erase.typed = typed;
    match act {
        Some(Act::Export) => {
            app.erase.error.clear();
            let job = app.run_as("gdpr.export", json!({"customer": id}), crate::Intent::Erase(Intent::Exported));
            app.erase.exporting = Some(job);
        }
        Some(Act::Erase) => erase(app),
        Some(Act::Cancel) => {
            close(app);
        }
        None => {}
    }
    true
}

/// Runs `gdpr.erase` when the card is ready (the button's action).
fn erase(app: &mut DeskApp) {
    if !ready(app) {
        return;
    }
    let Some(id) = app.erase.customer else { return };
    app.erase.error.clear();
    let reason = app.erase.reason.trim().to_string();
    let job = app.run_as("gdpr.erase", json!({"customer": id, "confirm": true, "reason": reason}), crate::Intent::Erase(Intent::Erased));
    app.erase.busy = Some(job);
}

/// Applies a finished command of this card.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    match intent {
        Intent::Exported => {
            let mine = app.erase.exporting == Some(c.job);
            if mine {
                app.erase.exporting = None;
            }
            match &c.result {
                Ok(v) => {
                    let name = export_name(v["customer"]["name"].as_str().unwrap_or_default(), app.now());
                    let data = serde_json::to_vec_pretty(v).unwrap_or_default();
                    app.requests.push(Request::SaveFile { name, data });
                    if mine {
                        app.erase.exported = true;
                    }
                    app.toast(tr(app, "Export saved (recorded in the audit log)").to_string());
                }
                Err(e) => {
                    if mine {
                        app.erase.error = app.language.tr_message(&e.to_string());
                    }
                }
            }
        }
        Intent::Erased => {
            let mine = app.erase.busy == Some(c.job);
            if mine {
                app.erase.busy = None;
            }
            match &c.result {
                Ok(_) => {
                    if mine {
                        app.erase = State::default();
                    }
                    app.toast(tr(app, "Personal data erased").to_string());
                }
                Err(e) => {
                    if mine {
                        app.erase.error = app.language.tr_message(&e.to_string());
                    }
                }
            }
        }
    }
}

/// Closes the card; answers whether it was open.
pub fn close(app: &mut DeskApp) -> bool {
    let open = app.erase.customer.is_some();
    app.erase = State::default();
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, Screen};

    /// The demo app in English (the demo desk's language is Italian).
    fn demo() -> DeskApp {
        let mut app = DeskApp::demo();
        app.language = Language::En;
        app
    }

    fn frame(app: &mut DeskApp) {
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    #[test]
    fn the_card_renders_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = demo();
                app.language = lang;
                app.dark = dark;
                open(&mut app, 3);
                assert_eq!(app.screen, Screen::Customers);
                assert_eq!(app.selected_customer, Some(3));
                frame(&mut app);
                app.erase.reason = "Richiesta".into();
                app.erase.typed = "elio  BRINA".into();
                app.erase.exported = true;
                app.erase.error = "x".into();
                frame(&mut app);
                assert!(ready(&app));
                assert!(app.erase.customer.is_some(), "still open");
            }
        }
    }

    #[test]
    fn export_then_erase() {
        let mut app = demo();
        frame(&mut app);
        open(&mut app, 3);
        frame(&mut app);
        assert!(!ready(&app), "no reason, no name");
        // Step 1: the export is saved as a file.
        let job = app.run_as("gdpr.export", json!({"customer": 3}), crate::Intent::Erase(Intent::Exported));
        app.erase.exporting = Some(job);
        frame(&mut app);
        assert!(app.erase.exported);
        assert!(app.erase.exporting.is_none());
        assert!(matches!(app.requests.last(), Some(Request::SaveFile { name, data }) if name.starts_with("export-elio-brina-") && !data.is_empty()));
        // Steps 2 and 3.
        app.erase.reason = "Richiesta dell'interessato".into();
        app.erase.typed = "Elio Brin".into();
        assert!(!ready(&app), "the name must match");
        app.erase.typed = "élio brina".into();
        assert!(ready(&app));
        erase(&mut app);
        assert!(app.erase.busy.is_some());
        frame(&mut app);
        let c = app.data().customer(3).unwrap();
        assert!(c.erased_at.is_some());
        assert_eq!(app.erase, State::default(), "closed");
        assert!(app.toast.as_ref().is_some_and(|(text, _)| text == "Personal data erased"));
        assert_eq!(app.selected_customer, Some(3), "the card shows the erased state");
        frame(&mut app);
        // An erased customer cannot be opened again for erasure.
        open(&mut app, 3);
        assert!(app.erase.customer.is_none());
    }

    #[test]
    fn a_running_intervention_blocks_erasure() {
        let mut app = demo();
        app.run("intervention.start", json!({"customer": 3, "kind": "phone"}));
        frame(&mut app);
        open(&mut app, 3);
        app.erase.reason = "Richiesta".into();
        app.erase.typed = "Elio Brina".into();
        assert!(!ready(&app));
        frame(&mut app);
        // The engine refuses as well: the message lands in the card.
        let job = app.run_as("gdpr.erase", json!({"customer": 3, "confirm": true, "reason": "x"}), crate::Intent::Erase(Intent::Erased));
        app.erase.busy = Some(job);
        frame(&mut app);
        assert!(app.erase.busy.is_none());
        assert!(app.erase.error.contains("stop the running"), "{}", app.erase.error);
        assert!(app.data().customer(3).unwrap().erased_at.is_none());
        assert!(close(&mut app));
        assert!(!close(&mut app));
    }

    #[test]
    fn names_and_labels() {
        assert!(same_name(" ELIO   brina ", "Elio Brina"));
        assert!(!same_name("", ""));
        assert!(!same_name("Elio", "Elio Brina"));
        let now = time::parse_stamp("2026-10-13T10:00:00Z").unwrap();
        let name = export_name("Studio Tecnico Lunaria", now);
        assert!(name.starts_with("export-studio-tecnico-lunaria-2026-10-1") && name.ends_with(".json"), "{name}");
        assert!(export_name("???", now).starts_with("export-customer-"));
        // Every label is translated, once, and the same way as anywhere else.
        let mut seen = Vec::new();
        for (en, it) in LABELS_IT {
            assert!(!en.is_empty() && !it.is_empty(), "{en}");
            assert!(!seen.contains(en), "duplicate {en}");
            seen.push(*en);
            assert!(!crate::i18n::LABELS.contains(en), "{en} is already shared");
            if let Some((_, other)) = crate::module_labels().iter().find(|(e, _)| e == en) {
                assert_eq!(other, it, "{en} translated two ways");
            }
        }
        let mut app = demo();
        app.language = Language::It;
        assert_eq!(tr(&app, "Erase"), "Cancella");
        assert_eq!(tr(&app, "Cancel"), "Annulla");
        assert_eq!(fill(&app, "Erase {name}'s personal data", &[("name", "Elio")]), "Cancella i dati personali di Elio");
    }
}
