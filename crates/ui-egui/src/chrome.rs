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
            // Cmd+Enter keeps the focus in a one-line box: consumed here (the top bar draws
            // before the screens), it starts on the PC found and never saves an open form.
            if te.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
                app.search_start();
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
                            pick_language(app, l);
                            ui.close();
                        }
                    }
                });
                if app.session.jobs_pending() > 0 {
                    ui.add(egui::Spinner::new().size(18.0).color(t.accent));
                }
                let findings = app.derived.findings.len();
                if findings > 0 {
                    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 32.0), Sense::click());
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(rect, 8.0, t.hover);
                    }
                    let (fill, color) = crate::widgets::tone_colors(t, crate::widgets::Tone::Bad);
                    icons::paint(p, Rect::from_center_size(rect.center() - vec2(3.0, 0.0), vec2(19.0, 19.0)), Icon::Privacy, color);
                    let disc = pos2(rect.right() - 8.0, rect.top() + 8.0);
                    p.circle_filled(disc, 8.0, color);
                    p.text(disc, egui::Align2::CENTER_CENTER, findings.min(99).to_string(), FontId::proportional(10.0), fill);
                    if resp.on_hover_text(findings_tip(app.language, findings)).clicked() {
                        app.privacy_review();
                    }
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
                Some("connecting") | None => None,
                Some(p) => Some(problem_text(app.language, &app.session.server(), p)),
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
                let text = counts_text(app.language, customers, devices, d.interventions.len());
                ui.label(RichText::new(text).small().color(t.muted));
            });
        });
    });
}

/// The top bar's language menu picked `l`: kept for the next start on a local desk. On a server
/// the preference is desk-wide (the language of the purpose texts people read), and the web app
/// keeps the choice in the browser instead.
fn pick_language(app: &mut DeskApp, l: Language) {
    app.language = l;
    if app.session.server().is_empty() {
        app.run("prefs.set", json!({"language": l.code()}));
    }
}

/// The privacy shield's tooltip.
fn findings_tip(lang: Language, n: usize) -> String {
    if n == 1 { lang.tr("Privacy review: 1 finding").to_string() } else { lang.fill("Privacy review: {n} findings", &[("n", &n.to_string())]) }
}

/// The status bar's counts.
fn counts_text(lang: Language, customers: usize, devices: usize, interventions: usize) -> String {
    let count = |n: usize, one: &str, many: &str| if n == 1 { lang.tr(one).to_string() } else { lang.fill(many, &[("n", &n.to_string())]) };
    format!(
        "{} · {} · {}",
        count(customers, "1 customer", "{n} customers"),
        count(devices, "1 device", "{n} devices"),
        count(interventions, "1 intervention", "{n} interventions"),
    )
}

/// The status bar's text for a problem of a remote desk. The session classifies it:
/// `unauthorized: …` (the token was refused), `incompatible: …` (the server answered what this
/// app cannot read) or `unreachable: …` (no answer); the detail is translated.
fn problem_text(lang: Language, url: &str, problem: &str) -> String {
    let (template, detail) = if let Some(d) = problem.strip_prefix("unauthorized: ") {
        ("The server refused the token ({url}): {error}. Open RemoteCraft again from the framework's link, or check the token", d)
    } else if let Some(d) = problem.strip_prefix("incompatible: ") {
        ("The server and this app are different versions ({url}): {error}. Reload the page or update the app", d)
    } else if let Some(d) = problem.strip_prefix("unreachable: ") {
        ("Server unreachable ({url}): {error}. Start it with: remotecraft-cli serve", d)
    } else {
        ("Server problem ({url}): {error}", problem)
    };
    lang.fill(template, &[("url", url), ("error", &lang.tr_message(detail))])
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

/// The keyboard shortcuts, in a window (`?`).
pub fn shortcuts_help(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if !app.shortcuts_open {
        return;
    }
    let mut open = true;
    egui::Window::new(app.tr("Keyboard shortcuts"))
        .id(egui::Id::new("shortcuts"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            let os = ui.ctx().os();
            egui::Grid::new("shortcuts-grid").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
                for (key, what) in crate::SHORTCUTS {
                    ui.label(RichText::new(crate::shortcut_key(os, key)).font(FontId::monospace(13.0)).color(t.accent));
                    ui.label(RichText::new(app.tr(what)).color(t.ui_text));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            if ui.button(app.tr("Close")).clicked() {
                app.shortcuts_open = false;
            }
        });
    if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.shortcuts_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completed, DeskSession, Session};
    use remotecraft_engine::core::Data;
    use remotecraft_engine::core::chrono::{DateTime, Utc};
    use serde_json::Value;

    /// The demo desk as a server would show it: a URL and, maybe, a problem.
    struct Served(Session, Option<&'static str>);

    impl DeskSession for Served {
        fn data(&self) -> &Data {
            self.0.data()
        }
        fn actor(&self) -> &str {
            self.0.actor()
        }
        fn now(&self) -> DateTime<Utc> {
            self.0.now()
        }
        fn execute(&mut self, command: &str, params: &Value) -> u64 {
            self.0.execute(command, params)
        }
        fn pump(&mut self) -> Vec<Completed> {
            self.0.pump()
        }
        fn jobs_pending(&self) -> usize {
            0
        }
        fn revision(&self) -> u64 {
            self.0.revision()
        }
        fn problem(&self) -> Option<String> {
            self.1.map(str::to_string)
        }
        fn server(&self) -> String {
            "https://desk.test".into()
        }
    }

    fn frame(app: &mut DeskApp) {
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    /// The texts one frame paints, on a context set to `os`.
    fn texts_on(app: &mut DeskApp, os: egui::os::OperatingSystem) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        ctx.set_os(os);
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut out = Vec::new();
        // Two frames: a window sizes itself in the first.
        for _ in 0..2 {
            let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
            output.textures_delta.clear();
            out.clear();
            for c in &output.shapes {
                walk(&c.shape, &mut out);
            }
        }
        out
    }

    fn texts_of(app: &mut DeskApp) -> Vec<String> {
        texts_on(app, egui::os::OperatingSystem::Nix)
    }

    #[test]
    fn a_remote_problem_gives_the_advice_of_its_class() {
        let it = Language::It;
        let url = "https://desk.test";
        let auth = problem_text(it, url, "unauthorized: unknown token");
        assert!(auth.contains("token sconosciuto") && auth.contains("framework"), "{auth}");
        assert!(!auth.contains("remotecraft-cli serve"), "{auth}");
        let old = problem_text(it, url, "incompatible: bad snapshot: missing field `data`");
        assert!(old.contains("versioni diverse") && old.contains("snapshot non valido"), "{old}");
        assert!(!old.contains("remotecraft-cli serve"), "{old}");
        let down = problem_text(it, url, "unreachable: connection refused");
        assert!(
            down.contains("Server non raggiungibile (https://desk.test): connection refused") && down.contains("remotecraft-cli serve"),
            "{down}"
        );
        // A poll given up after its window (the connection dropped while the PC slept).
        let silent = problem_text(it, url, "unreachable: the server does not answer");
        assert_eq!(silent, "Server non raggiungibile (https://desk.test): il server non risponde. Avvialo con: remotecraft-cli serve");
        let other = problem_text(Language::En, url, "no intervention 7");
        assert_eq!(other, "Server problem (https://desk.test): no intervention 7");
        // The status bar draws each, and "connecting" as the spinner.
        for p in ["connecting", "unauthorized: unknown token", "incompatible: the snapshot is too large", "unreachable: timeout", "odd"] {
            let mut app = DeskApp::new(Box::new(Served(Session::demo(), Some(p))));
            frame(&mut app);
        }
        // The status bar of an Italian web app whose server went silent reads all in Italian.
        let mut app = DeskApp::new(Box::new(Served(Session::demo(), Some("unreachable: the server does not answer"))));
        app.language = Language::It;
        let texts = texts_of(&mut app);
        assert!(texts.iter().any(|t| t.contains("il server non risponde")), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("does not answer")), "{texts:?}");
    }

    #[test]
    fn the_help_window_names_the_platforms_modifier() {
        use egui::os::OperatingSystem;
        let mut app = DeskApp::demo();
        app.language = Language::It;
        app.shortcuts_open = true;
        let texts = texts_on(&mut app, OperatingSystem::Windows);
        assert!(texts.iter().any(|t| t == "Ctrl+,") && texts.iter().any(|t| t == "Ctrl+Enter"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Scorciatoie da tastiera"), "{texts:?}");
        // A Mac listens for Cmd (`Modifiers::COMMAND`): the window says Cmd, never Ctrl.
        let texts = texts_on(&mut app, OperatingSystem::Mac);
        assert!(texts.iter().any(|t| t == "Cmd+,") && texts.iter().any(|t| t == "Cmd+R"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.starts_with("Ctrl+")), "{texts:?}");
        assert_eq!(crate::shortcut_key(OperatingSystem::Mac, "j / k"), "j / k");
        assert_eq!(crate::shortcut_key(OperatingSystem::Nix, "Ctrl+R"), "Ctrl+R");
        assert_eq!(crate::shortcut_key_in(OperatingSystem::Mac, false, "Ctrl+R"), "Cmd+R", "the desktop app on a Mac");
        // In a browser Ctrl works on a Mac too, and Cmd+R would reload the page (what was typed
        // is lost): the web app says Ctrl everywhere.
        for os in [OperatingSystem::Mac, OperatingSystem::IOS, OperatingSystem::Windows, OperatingSystem::Nix] {
            for (key, _) in crate::SHORTCUTS {
                assert_eq!(crate::shortcut_key_in(os, true, key), *key, "{os:?}");
            }
        }
    }

    #[test]
    fn counts_say_one_in_the_singular() {
        assert_eq!(counts_text(Language::It, 1, 1, 1), "1 cliente · 1 PC · 1 intervento");
        assert_eq!(counts_text(Language::It, 3, 0, 2), "3 clienti · 0 PC · 2 interventi");
        assert_eq!(counts_text(Language::En, 1, 2, 1), "1 customer · 2 devices · 1 intervention");
        assert_eq!(findings_tip(Language::It, 1), "Revisione privacy: 1 segnalazione");
        assert_eq!(findings_tip(Language::En, 4), "Privacy review: 4 findings");
    }

    #[test]
    fn the_language_menu_is_kept_on_a_local_desk_only() {
        let mut app = DeskApp::demo();
        pick_language(&mut app, Language::En);
        frame(&mut app);
        assert_eq!(app.language, Language::En);
        assert_eq!(app.data().preferences.language, "en", "kept for the next start");
        // On a server the preference is the whole desk's: only this session changes.
        let mut app = DeskApp::new(Box::new(Served(Session::demo(), None)));
        let before = app.data().preferences.language.clone();
        pick_language(&mut app, Language::En);
        frame(&mut app);
        assert_eq!(app.language, Language::En);
        assert_eq!(app.data().preferences.language, before);
    }
}
