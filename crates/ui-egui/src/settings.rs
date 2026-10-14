//! The settings screen, full width like MailCraft's: a scrolled column with the setup guide
//! while something is missing (the company, the privacy notice, the RustDesk server), then the
//! preferences in sections — company and billing, privacy rules, the RustDesk server with its
//! check and the client setup — the appearance, the keyboard shortcuts and the status.
//!
//! Each section edits a draft taken from `data().preferences` (seeded when the screen opens,
//! merged field by field when the data changes under it) and saves only the keys that changed
//! with `prefs.set`; the guide's cards use `prefs.set` and `notice.add`; the server section runs
//! `rustdesk.check` on request and `rustdesk.setup` when the screen opens and after every change
//! of the data. Results come back through [`on_result`], never by reading the session's answer.

use egui::{Align, Align2, FontId, Layout, RichText, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Map, Value, json};

use crate::core::Preferences;
use crate::core::chrono::Local;
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, DeskApp, Intent as AppIntent, Language, Request};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    (
        "This browser keeps the token that opens the desk. On a shared or lost PC, forget it: the page reloads without it, and the framework's link brings you back.",
        "Questo browser conserva il token che apre il gestionale. Su un PC condiviso o smarrito, dimenticalo: la pagina si ricarica senza token e il link del framework ti riporta qui.",
    ),
    ("Forget the token on this browser", "Dimentica il token su questo browser"),
    ("Forget and reload", "Dimentica e ricarica"),
    ("Setup guide", "Guida iniziale"),
    ("Three things to set before the first call", "Tre cose da impostare prima della prima chiamata"),
    ("Your company", "La tua azienda"),
    ("Your privacy notice", "La tua informativa privacy"),
    ("Your RustDesk server", "Il tuo server RustDesk"),
    ("Version of the notice", "Versione dell'informativa"),
    ("Title of the notice", "Titolo dell'informativa"),
    ("Text of the notice", "Testo dell'informativa"),
    (
        "Who the controller is, which data, purposes and legal basis, retention, recipients, the rights of articles 15–22",
        "Chi è il titolare, quali dati, finalità e base giuridica, conservazione, destinatari, i diritti degli articoli 15–22",
    ),
    ("Privacy notice saved", "Informativa salvata"),
    ("From the container", "Dal container"),
    ("Company and billing", "Azienda e fatturazione"),
    ("Company name", "Nome dell'azienda"),
    ("Named as data controller in exports", "Indicato come titolare del trattamento nelle esportazioni"),
    ("Billing unit (minutes)", "Unità di fatturazione (minuti)"),
    ("Minimum billable minutes", "Minuti minimi fatturabili"),
    ("Privacy rules", "Regole privacy"),
    ("Require consent for remote work", "Richiedi il consenso per il lavoro da remoto"),
    ("Consent validity (months)", "Validità del consenso (mesi)"),
    ("0 = never expires", "0 = non scade mai"),
    ("Retention (years)", "Conservazione (anni)"),
    ("0 = never", "0 = mai"),
    ("Preferences saved", "Preferenze salvate"),
    ("RustDesk server", "Server RustDesk"),
    ("Server name", "Nome del server"),
    ("Public key", "Chiave pubblica"),
    ("Checking…", "Verifica in corso…"),
    ("Port {port} — {service}: OK ({ms} ms)", "Porta {port} — {service}: OK ({ms} ms)"),
    ("Port {port} — {service}: {error}", "Porta {port} — {service}: {error}"),
    ("hbbs: NAT type test (TCP)", "hbbs: test del tipo di NAT (TCP)"),
    ("hbbs: ID registration and connection setup (TCP and UDP)", "hbbs: registrazione degli ID e avvio delle connessioni (TCP e UDP)"),
    ("hbbr: relay (TCP)", "hbbr: relay (TCP)"),
    ("Key known", "Chiave nota"),
    ("Key missing", "Chiave mancante"),
    (
        "UDP 21116 cannot be tested: if clients stay offline while TCP works, check UDP forwarding on the router",
        "La porta UDP 21116 non si può verificare: se i client restano offline mentre il TCP funziona, controlla l'inoltro UDP sul router",
    ),
    ("ID server", "Server ID"),
    ("Relay server", "Server relay"),
    ("API server (leave empty)", "Server API (lascia vuoto)"),
    ("Enter in RustDesk › Settings › Network › ID/Relay server", "Da inserire in RustDesk › Impostazioni › Rete › Server ID/Relay"),
    ("Import string", "Stringa di importazione"),
    (
        "Paste in RustDesk › Settings › Network › Import server config",
        "Da incollare in RustDesk › Impostazioni › Rete › Importa configurazione server",
    ),
    ("Windows installer name", "Nome dell'installer Windows"),
    (
        "Rename the official Windows installer to this name: it installs already pointed at this server",
        "Rinomina così l'installer Windows ufficiale: si installa già configurato su questo server",
    ),
    ("Not available until the server is set", "Disponibile quando il server è impostato"),
    ("Ports to forward", "Porte da inoltrare"),
    ("Copy to the clipboard", "Copia negli appunti"),
    ("Copied to the clipboard", "Copiato negli appunti"),
    ("Missing: {items}", "Manca: {items}"),
    ("Appearance", "Aspetto"),
    ("Theme", "Tema"),
    ("Outside text fields; Escape leaves a field", "Fuori dai campi di testo; Esc esce da un campo"),
    ("Version {version}", "Versione {version}"),
    ("1 customer", "1 cliente"),
    ("1 device", "1 PC"),
    ("1 intervention", "1 intervento"),
    ("Server: {url}", "Server: {url}"),
    ("This computer", "Questo computer"),
    ("Problems loading the desk", "Problemi nel caricamento del gestionale"),
];

/// The width of the settings column.
const MAX_W: f32 = 860.0;

/// The engine's bounds for the numeric preferences (`prefs.set` refuses more).
const MAX_MINUTES: u32 = 480;
const MAX_MONTHS: u32 = 240;
const MAX_YEARS: u32 = 100;

/// A preferences section with its own Save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// Company name, billing unit, minimum billable minutes.
    Company,
    /// Consent required, consent validity, retention.
    Rules,
    /// The RustDesk server's name and key.
    Server,
}

/// A card of the setup guide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Company,
    Notice,
    Server,
}

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `prefs.set` from a section's Save.
    Prefs(Section),
    /// `prefs.set` or `notice.add` from a card of the setup guide.
    Guide(Card),
    /// `rustdesk.check` from the Check server button.
    Check,
    /// `rustdesk.setup` for the client setup card.
    Setup,
}

/// The editable preferences as typed; each section saves its keys when they differ from the
/// saved values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub company_name: String,
    pub billing_unit_minutes: u32,
    pub minimum_minutes: u32,
    pub require_consent: bool,
    pub consent_validity_months: u32,
    pub retention_years: u32,
    pub rustdesk_host: String,
    pub rustdesk_key: String,
}

impl Draft {
    /// The preferences as saved.
    pub fn of(p: &Preferences) -> Self {
        Self {
            company_name: p.company_name.clone(),
            billing_unit_minutes: p.billing_unit_minutes,
            minimum_minutes: p.minimum_minutes,
            require_consent: p.require_consent,
            consent_validity_months: p.consent_validity_months,
            retention_years: p.retention_years,
            rustdesk_host: p.rustdesk_host.clone(),
            rustdesk_key: p.rustdesk_key.clone(),
        }
    }

    /// The parameters of `prefs.set` for the keys of `section` that differ from `base`
    /// (numbers clamped to the engine's bounds): an empty object when nothing changed.
    pub fn changes(&self, base: &Self, section: Section) -> Value {
        let mut m = Map::new();
        let mut text = |key: &str, mine: &str, theirs: &str| {
            if mine.trim() != theirs {
                m.insert(key.to_string(), json!(mine.trim()));
            }
        };
        match section {
            Section::Company => {
                text("company_name", &self.company_name, &base.company_name);
                number(&mut m, "billing_unit_minutes", self.billing_unit_minutes, base.billing_unit_minutes, MAX_MINUTES);
                number(&mut m, "minimum_minutes", self.minimum_minutes, base.minimum_minutes, MAX_MINUTES);
            }
            Section::Rules => {
                if self.require_consent != base.require_consent {
                    m.insert("require_consent".into(), json!(self.require_consent));
                }
                number(&mut m, "consent_validity_months", self.consent_validity_months, base.consent_validity_months, MAX_MONTHS);
                number(&mut m, "retention_years", self.retention_years, base.retention_years, MAX_YEARS);
            }
            Section::Server => {
                text("rustdesk_host", &self.rustdesk_host, &base.rustdesk_host);
                text("rustdesk_key", &self.rustdesk_key, &base.rustdesk_key);
            }
        }
        Value::Object(m)
    }

    /// Whether `section` has something to save.
    pub fn dirty(&self, base: &Self, section: Section) -> bool {
        self.changes(base, section).as_object().is_some_and(|o| !o.is_empty())
    }

    /// Takes the saved values of `section`'s keys from `p` (after a save, so what the engine
    /// normalised is what the draft shows).
    fn adopt(&mut self, p: &Preferences, section: Section) {
        match section {
            Section::Company => {
                self.company_name.clone_from(&p.company_name);
                self.billing_unit_minutes = p.billing_unit_minutes;
                self.minimum_minutes = p.minimum_minutes;
            }
            Section::Rules => {
                self.require_consent = p.require_consent;
                self.consent_validity_months = p.consent_validity_months;
                self.retention_years = p.retention_years;
            }
            Section::Server => {
                self.rustdesk_host.clone_from(&p.rustdesk_host);
                self.rustdesk_key.clone_from(&p.rustdesk_key);
            }
        }
    }

    /// Takes the values of `new` for every field still equal to `base` (untouched); what the
    /// technician edited is kept, so a change made elsewhere never clobbers typing.
    fn merge(&mut self, base: &Self, new: &Self) {
        if self.company_name == base.company_name {
            self.company_name.clone_from(&new.company_name);
        }
        if self.billing_unit_minutes == base.billing_unit_minutes {
            self.billing_unit_minutes = new.billing_unit_minutes;
        }
        if self.minimum_minutes == base.minimum_minutes {
            self.minimum_minutes = new.minimum_minutes;
        }
        if self.require_consent == base.require_consent {
            self.require_consent = new.require_consent;
        }
        if self.consent_validity_months == base.consent_validity_months {
            self.consent_validity_months = new.consent_validity_months;
        }
        if self.retention_years == base.retention_years {
            self.retention_years = new.retention_years;
        }
        if self.rustdesk_host == base.rustdesk_host {
            self.rustdesk_host.clone_from(&new.rustdesk_host);
        }
        if self.rustdesk_key == base.rustdesk_key {
            self.rustdesk_key.clone_from(&new.rustdesk_key);
        }
    }
}

/// Adds `key` when the number changed, clamped to `max`.
fn number(m: &mut Map<String, Value>, key: &str, mine: u32, theirs: u32, max: u32) {
    if mine != theirs {
        m.insert(key.to_string(), json!(mine.min(max)));
    }
}

/// A Save in flight and the error of the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feedback {
    pub job: Option<u64>,
    /// The engine's refusal, translated; empty when the last save went through.
    pub error: String,
}

/// The setup guide's three cards: what is typed in each, and how each save went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guide {
    pub company_name: String,
    pub version: String,
    pub title: String,
    pub text: String,
    pub host: String,
    pub key: String,
    pub company: Feedback,
    pub notice: Feedback,
    pub server: Feedback,
    /// The fields were prefilled (once, the first time the screen showed).
    prefilled: bool,
}

impl Guide {
    fn feedback(&self, card: Card) -> &Feedback {
        match card {
            Card::Company => &self.company,
            Card::Notice => &self.notice,
            Card::Server => &self.server,
        }
    }

    fn feedback_mut(&mut self, card: Card) -> &mut Feedback {
        match card {
            Card::Company => &mut self.company,
            Card::Notice => &mut self.notice,
            Card::Server => &mut self.server,
        }
    }

    /// Whether a card has what its command requires.
    pub fn can_save(&self, card: Card) -> bool {
        match card {
            Card::Company => !self.company_name.trim().is_empty(),
            Card::Notice => !self.version.trim().is_empty() && !self.text.trim().is_empty(),
            Card::Server => !self.host.trim().is_empty(),
        }
    }
}

/// This screen's own state: the drafts, the guide, the answers of the RustDesk commands.
#[derive(Default)]
pub struct State {
    pub draft: Draft,
    /// The saved values the draft started from.
    base: Draft,
    /// The revision `base` was taken at.
    seeded: Option<u64>,
    /// The frame the screen was last drawn in: a gap means it was left and reopened.
    last_frame: Option<u64>,
    pub company: Feedback,
    pub rules: Feedback,
    pub server: Feedback,
    pub guide: Guide,
    /// The last `rustdesk.check` answer, the job in flight and the refusal of the last run.
    pub check: Option<Value>,
    pub checking: Option<u64>,
    pub check_error: String,
    /// The `rustdesk.setup` answer, the revision it was asked for and the job in flight.
    pub setup: Option<Value>,
    setup_rev: Option<u64>,
    setup_job: Option<u64>,
    pub setup_error: String,
    /// "Forget the token" was pressed once and waits for the second, confirming press.
    pub forget_armed: bool,
}

impl State {
    fn feedback(&self, section: Section) -> &Feedback {
        match section {
            Section::Company => &self.company,
            Section::Rules => &self.rules,
            Section::Server => &self.server,
        }
    }

    fn feedback_mut(&mut self, section: Section) -> &mut Feedback {
        match section {
            Section::Company => &mut self.company,
            Section::Rules => &mut self.rules,
            Section::Server => &mut self.server,
        }
    }
}

/// Draws the screen: one full-width panel with the scrolled column of sections.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    sync(app, ui.ctx().cumulative_frame_nr());
    crate::central_panel(ui, t, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 18)).show(ui, |ui| {
                ui.set_max_width(MAX_W);
                title_row(app, ui, t);
                if guide_needed(app) {
                    guide_section(app, ui, t);
                }
                company_section(app, ui, t);
                rules_section(app, ui, t);
                server_section(app, ui, t);
                appearance_section(app, ui, t);
                shortcuts_section(app, ui, t);
                status_section(app, ui, t);
                ui.add_space(24.0);
            });
        });
    });
}

/// Applies a finished command: a section's save, a guide card's, the server check, the setup.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    let error = match &c.result {
        Ok(_) => None,
        Err(e) => Some(app.language.tr_message(&e.to_string())),
    };
    match intent {
        Intent::Prefs(section) => {
            let s = &mut app.settings;
            s.feedback_mut(section).job = None;
            match error {
                Some(e) => s.feedback_mut(section).error = e,
                None => {
                    s.feedback_mut(section).error.clear();
                    // What the engine saved, from its answer: a remote desk's mirror still holds
                    // the old values until the next snapshot, and taking those would undo the
                    // edit on screen. Without them the draft stays as typed, and the snapshot's
                    // merge settles it.
                    if let Some(saved) = saved_preferences(&c.result) {
                        s.draft.adopt(&saved, section);
                        s.base.adopt(&saved, section);
                    }
                    app.toast(app.tr("Preferences saved").to_string());
                }
            }
        }
        Intent::Guide(card) => {
            let fb = app.settings.guide.feedback_mut(card);
            fb.job = None;
            match error {
                Some(e) => fb.error = e,
                None => {
                    fb.error.clear();
                    let text = if card == Card::Notice { "Privacy notice saved" } else { "Preferences saved" };
                    app.toast(app.tr(text).to_string());
                }
            }
        }
        Intent::Check => {
            let s = &mut app.settings;
            s.checking = None;
            match (error, &c.result) {
                (None, Ok(v)) => {
                    s.check = Some(v.clone());
                    s.check_error.clear();
                }
                (e, _) => {
                    s.check = None;
                    s.check_error = e.unwrap_or_default();
                }
            }
        }
        Intent::Setup => {
            let s = &mut app.settings;
            if s.setup_job != Some(c.job) {
                // An older request, overtaken by a change of the data.
                return;
            }
            s.setup_job = None;
            match (error, &c.result) {
                (None, Ok(v)) => {
                    s.setup = Some(v.clone());
                    s.setup_error.clear();
                }
                (e, _) => s.setup_error = e.unwrap_or_default(),
            }
        }
    }
}

/// The preferences in a `prefs.set` answer (`{"changed": […], "preferences": {…}}`).
fn saved_preferences(result: &crate::engine::Result<Value>) -> Option<Preferences> {
    let answer = result.as_ref().ok()?;
    serde_json::from_value(answer.get("preferences")?.clone()).ok()
}

/// Closes an open form; the settings have none (Escape leaves the screen), so never.
pub fn close(_app: &mut DeskApp) -> bool {
    false
}

// ---------------------------------------------------------------------------------------------
// Drafts and loading

/// Seeds the drafts when the screen opens, merges a change of the data into them, prefills the
/// guide once and asks for the client setup when the data changed.
fn sync(app: &mut DeskApp, frame: u64) {
    let rev = app.session.revision();
    let saved = Draft::of(&app.session.data().preferences);
    let s = &mut app.settings;
    let reopened = s.last_frame.is_none_or(|f| frame > f.wrapping_add(1));
    s.last_frame = Some(frame);
    if reopened {
        s.draft = saved.clone();
        s.base = saved;
        s.seeded = Some(rev);
        for fb in [&mut s.company, &mut s.rules, &mut s.server] {
            fb.error.clear();
        }
    } else if s.seeded != Some(rev) {
        s.draft.merge(&s.base, &saved);
        s.base = saved;
        s.seeded = Some(rev);
    }
    if !app.settings.guide.prefilled {
        prefill_guide(app);
    }
    request_setup(app);
}

/// The RustDesk server the clients use: the preferences, else what the container knows.
fn effective_server(app: &DeskApp) -> (String, String) {
    let p = &app.session.data().preferences;
    let (host, key) = app.session.rustdesk_defaults();
    (if p.rustdesk_host.is_empty() { host } else { p.rustdesk_host.clone() }, if p.rustdesk_key.is_empty() { key } else { p.rustdesk_key.clone() })
}

/// Whether a card of the guide is done, from the data.
fn card_done(app: &DeskApp, card: Card) -> bool {
    let d = app.session.data();
    match card {
        Card::Company => !d.preferences.company_name.trim().is_empty(),
        Card::Notice => !d.notices.is_empty(),
        Card::Server => {
            let (host, key) = effective_server(app);
            !host.is_empty() && !key.is_empty()
        }
    }
}

/// The guide shows while something is missing.
fn guide_needed(app: &DeskApp) -> bool {
    [Card::Company, Card::Notice, Card::Server].into_iter().any(|c| !card_done(app, c))
}

/// Fills the guide's fields with what the desk and the container know.
fn prefill_guide(app: &mut DeskApp) {
    let (host, key) = effective_server(app);
    let version = app.now().with_timezone(&Local).format("%Y-%m").to_string();
    let title = if app.italian() { "Informativa assistenza tecnica" } else { "Technical support privacy notice" };
    let company = app.session.data().preferences.company_name.clone();
    let g = &mut app.settings.guide;
    g.company_name = company;
    g.version = version;
    g.title = title.into();
    g.host = host;
    g.key = key;
    g.prefilled = true;
}

/// Runs `rustdesk.setup` unless its answer (or a request) already matches the data's revision.
fn request_setup(app: &mut DeskApp) {
    let rev = app.session.revision();
    if app.settings.setup_rev == Some(rev) {
        return;
    }
    let job = app.run_as("rustdesk.setup", json!({}), AppIntent::Settings(Intent::Setup));
    app.settings.setup_job = Some(job);
    app.settings.setup_rev = Some(rev);
}

/// Sends the changed keys of a section with `prefs.set`.
fn save_section(app: &mut DeskApp, section: Section) {
    let params = app.settings.draft.changes(&app.settings.base, section);
    if !app.settings.draft.dirty(&app.settings.base, section) || app.settings.feedback(section).job.is_some() {
        return;
    }
    let job = app.run_as("prefs.set", params, AppIntent::Settings(Intent::Prefs(section)));
    let fb = app.settings.feedback_mut(section);
    fb.job = Some(job);
    fb.error.clear();
}

/// Sends a guide card: the company name or the server with `prefs.set`, the notice with
/// `notice.add`.
fn save_card(app: &mut DeskApp, card: Card) {
    let g = &app.settings.guide;
    if !g.can_save(card) || g.feedback(card).job.is_some() {
        return;
    }
    let (command, params) = match card {
        Card::Company => ("prefs.set", json!({"company_name": g.company_name.trim()})),
        Card::Notice => ("notice.add", json!({"version": g.version.trim(), "title": g.title.trim(), "text": g.text})),
        Card::Server => ("prefs.set", json!({"rustdesk_host": g.host.trim(), "rustdesk_key": g.key.trim()})),
    };
    let job = app.run_as(command, params, AppIntent::Settings(Intent::Guide(card)));
    let fb = app.settings.guide.feedback_mut(card);
    fb.job = Some(job);
    fb.error.clear();
}

/// Runs `rustdesk.check` on the server the desk knows.
fn run_check(app: &mut DeskApp) {
    if app.settings.checking.is_some() {
        return;
    }
    let job = app.run_as("rustdesk.check", json!({}), AppIntent::Settings(Intent::Check));
    app.settings.checking = Some(job);
}

// ---------------------------------------------------------------------------------------------
// The sections

/// The title and the Done button.
fn title_row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let mut back = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.tr("Settings")).size(22.0).strong().color(t.ui_text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_text_button(ui, t, Icon::Back, app.tr("Done"), true).clicked() {
                back = true;
            }
        });
    });
    if back {
        app.go_back();
    }
}

/// The three onboarding cards, each a mini form with its own Save, checked when done.
fn guide_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    widgets::section(ui, t, app.tr("Setup guide"));
    ui.label(RichText::new(app.tr("Three things to set before the first call")).small().color(t.muted));
    guide_card(app, ui, t, Card::Company, 1, "Your company", |ui, t, g, lang| {
        egui::Grid::new("settings-guide-company").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::field(ui, t, lang.tr("Company name"), &mut g.company_name, "");
            hint_row(ui, t, lang.tr("Named as data controller in exports"));
        });
    });
    guide_card(app, ui, t, Card::Notice, 2, "Your privacy notice", |ui, t, g, lang| {
        egui::Grid::new("settings-guide-notice").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::field(ui, t, lang.tr("Version of the notice"), &mut g.version, "");
            widgets::field(ui, t, lang.tr("Title of the notice"), &mut g.title, "");
            let hint = lang.tr("Who the controller is, which data, purposes and legal basis, retention, recipients, the rights of articles 15–22");
            widgets::multiline(ui, t, lang.tr("Text of the notice"), &mut g.text, hint, 8);
        });
    });
    let (default_host, default_key) = app.session.rustdesk_defaults();
    guide_card(app, ui, t, Card::Server, 3, "Your RustDesk server", |ui, t, g, lang| {
        egui::Grid::new("settings-guide-server").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::field(ui, t, lang.tr("Server name"), &mut g.host, &default_host);
            widgets::field(ui, t, lang.tr("Public key"), &mut g.key, &default_key);
            if !default_host.is_empty() || !default_key.is_empty() {
                hint_row(ui, t, lang.tr("From the container"));
            }
        });
    });
}

/// One card of the guide: a numbered title, the check when done, else the form and its Save.
fn guide_card(
    app: &mut DeskApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    card: Card,
    n: usize,
    title: &str,
    form: impl FnOnce(&mut egui::Ui, &Tokens, &mut Guide, Language),
) {
    let done = card_done(app, card);
    let lang = app.language;
    let mut g = app.settings.guide.clone();
    let fb = g.feedback(card).clone();
    let ok = widgets::tone_colors(t, Tone::Ok).1;
    let (badge, digit) = badge_colors(t, done);
    let mut save = false;
    ui.add_space(6.0);
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 12.0, badge);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(13.0), digit);
            ui.label(RichText::new(lang.tr(title)).strong().color(t.ui_text));
            if done {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(lang.tr("Done")).small().color(ok));
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Check, ok);
                });
            }
        });
        if done {
            return;
        }
        ui.add_space(6.0);
        form(ui, t, &mut g, lang);
        widgets::error_line(ui, t, &fb.error);
        ui.add_space(6.0);
        save = save_row(ui, t, lang, g.can_save(card), fb.job.is_some());
    });
    app.settings.guide = g;
    if save {
        save_card(app, card);
    }
}

/// `(fill, digit)` of a guide card's number: the accent while to do; the Ok tone's colours
/// swapped once done, since the Ok text colour is light in the dark theme and a white digit on
/// it would be unreadable.
fn badge_colors(t: &Tokens, done: bool) -> (egui::Color32, egui::Color32) {
    if done {
        let (fill, text) = widgets::tone_colors(t, Tone::Ok);
        (text, fill)
    } else {
        (t.accent, t.on_accent)
    }
}

/// Company name, billing unit, minimum billable minutes.
fn company_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    section_card(app, ui, t, Section::Company, "Company and billing", |ui, t, d, lang| {
        egui::Grid::new("settings-company").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::field(ui, t, lang.tr("Company name"), &mut d.company_name, "");
            hint_row(ui, t, lang.tr("Named as data controller in exports"));
            number_row(ui, t, lang.tr("Billing unit (minutes)"), &mut d.billing_unit_minutes, MAX_MINUTES, "");
            number_row(ui, t, lang.tr("Minimum billable minutes"), &mut d.minimum_minutes, MAX_MINUTES, "");
        });
    });
}

/// Consent required, consent validity, retention.
fn rules_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    section_card(app, ui, t, Section::Rules, "Privacy rules", |ui, t, d, lang| {
        egui::Grid::new("settings-rules").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::check(ui, t, lang.tr("Require consent for remote work"), &mut d.require_consent, "");
            number_row(ui, t, lang.tr("Consent validity (months)"), &mut d.consent_validity_months, MAX_MONTHS, lang.tr("0 = never expires"));
            number_row(ui, t, lang.tr("Retention (years)"), &mut d.retention_years, MAX_YEARS, lang.tr("0 = never"));
        });
    });
}

/// The server's name and key, the check and the client setup.
fn server_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let (default_host, default_key) = app.session.rustdesk_defaults();
    section_card(app, ui, t, Section::Server, "RustDesk server", |ui, t, d, lang| {
        egui::Grid::new("settings-server").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            widgets::field(ui, t, lang.tr("Server name"), &mut d.rustdesk_host, &default_host);
            widgets::field(ui, t, lang.tr("Public key"), &mut d.rustdesk_key, &default_key);
            if (d.rustdesk_host.trim().is_empty() && !default_host.is_empty()) || (d.rustdesk_key.trim().is_empty() && !default_key.is_empty()) {
                hint_row(ui, t, lang.tr("From the container"));
            }
        });
    });
    check_block(app, ui, t);
    setup_card(app, ui, t);
}

/// The Check server button, the spinner while it runs, the port rows or the refusal.
fn check_block(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let lang = app.language;
    let busy = app.settings.checking.is_some();
    let mut run = false;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if widgets::icon_text_button(ui, t, Icon::Sync, lang.tr("Check RustDesk Server"), !busy).clicked() {
            run = true;
        }
        if busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
            ui.label(RichText::new(lang.tr("Checking…")).small().color(t.muted));
        }
    });
    if run {
        run_check(app);
    }
    widgets::error_line(ui, t, &app.settings.check_error);
    let Some(v) = app.settings.check.clone() else { return };
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for p in v["ports"].as_array().into_iter().flatten() {
            let port = p["port"].as_u64().unwrap_or(0).to_string();
            let service = lang.tr(p["service"].as_str().unwrap_or_default());
            if p["ok"].as_bool().unwrap_or(false) {
                let ms = p["ms"].as_u64().unwrap_or(0).to_string();
                let text = lang.fill("Port {port} — {service}: OK ({ms} ms)", &[("port", &port), ("service", service), ("ms", &ms)]);
                status_line(ui, t, Tone::Ok, Icon::Check, &text);
            } else {
                let error = lang.tr_message(p["error"].as_str().unwrap_or_default());
                let text = lang.fill("Port {port} — {service}: {error}", &[("port", &port), ("service", service), ("error", &error)]);
                status_line(ui, t, Tone::Bad, Icon::Close, &text);
            }
        }
        if v["key_known"].as_bool().unwrap_or(false) {
            status_line(ui, t, Tone::Ok, Icon::Check, lang.tr("Key known"));
        } else {
            status_line(ui, t, Tone::Warn, Icon::Warning, lang.tr("Key missing"));
        }
        ui.add_space(4.0);
        let note = lang.tr("UDP 21116 cannot be tested: if clients stay offline while TCP works, check UDP forwarding on the router");
        ui.add(egui::Label::new(RichText::new(note).small().color(t.muted)).wrap());
    });
}

/// The client setup card: the values to enter in each client, the import string, the
/// installer name, the ports, and what is missing.
fn setup_card(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let lang = app.language;
    ui.add_space(12.0);
    ui.label(RichText::new(lang.tr("RustDesk Client Setup")).strong().color(t.ui_text));
    let setup = app.settings.setup.clone();
    let error = app.settings.setup_error.clone();
    let loading = app.settings.setup_job.is_some();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let Some(v) = setup else {
            if !error.is_empty() {
                widgets::error_line(ui, t, &error);
            } else if loading {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(t.accent));
                    ui.label(RichText::new(lang.tr("Loading…")).color(t.muted));
                });
            }
            return;
        };
        let missing: Vec<String> = v["missing"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|m| missing_label(lang, m)).collect();
        if !missing.is_empty() {
            banner(ui, t, Tone::Warn, &[lang.fill("Missing: {items}", &[("items", &missing.join(", "))])]);
            ui.add_space(6.0);
        }
        let text = |key: &str| v[key].as_str().map(str::to_string);
        let labels =
            ["ID server", "Relay server", "API server (leave empty)", "Public key", "Import string", "Windows installer name"].map(|l| lang.tr(l));
        let [id, relay, api, key, import, installer] = labels;
        let w = label_width(ui.painter(), &labels);
        value_row(app, ui, t, id, w, text("id_server").as_deref());
        value_row(app, ui, t, relay, w, text("relay_server").as_deref());
        value_row(app, ui, t, api, w, text("api_server").as_deref());
        value_row(app, ui, t, key, w, text("key").as_deref());
        hint_line(ui, t, lang.tr("Enter in RustDesk › Settings › Network › ID/Relay server"));
        ui.add_space(8.0);
        value_row(app, ui, t, import, w, text("config_string").as_deref());
        hint_line(ui, t, lang.tr("Paste in RustDesk › Settings › Network › Import server config"));
        ui.add_space(8.0);
        value_row(app, ui, t, installer, w, text("windows_installer_name").as_deref());
        hint_line(ui, t, lang.tr("Rename the official Windows installer to this name: it installs already pointed at this server"));
        ui.add_space(10.0);
        ui.label(RichText::new(lang.tr("Ports to forward")).small().strong().color(t.muted));
        for p in v["ports"].as_array().into_iter().flatten() {
            let port = p["port"].as_u64().unwrap_or(0).to_string();
            let service = lang.tr(p["service"].as_str().unwrap_or_default());
            ui.horizontal(|ui| {
                ui.label(RichText::new(port).monospace().color(t.ui_text));
                ui.label(RichText::new(service).color(t.muted));
            });
        }
    });
}

/// Theme and interface language, applied at once.
fn appearance_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    widgets::section(ui, t, app.tr("Appearance"));
    let lang = app.language;
    let theme = app.session.data().preferences.theme.clone();
    let mut picked_theme = None;
    let mut picked_lang = None;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("settings-appearance").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            let themes = [("system", lang.tr("System")), ("light", lang.tr("Light")), ("dark", lang.tr("Dark"))];
            picked_theme = widgets::combo(ui, t, "settings-theme", lang.tr("Theme"), &theme, &themes);
            let languages: Vec<(&str, &str)> = Language::ALL.iter().map(|l| (l.code(), l.name())).collect();
            picked_lang = widgets::combo(ui, t, "settings-language", lang.tr("Language"), lang.code(), &languages);
        });
    });
    if let Some(v) = picked_theme {
        app.dark = v == "dark";
        app.run("prefs.set", json!({"theme": v}));
    }
    if let Some(v) = picked_lang {
        if let Some(l) = Language::parse(&v) {
            app.language = l;
        }
        app.run("prefs.set", json!({"language": v}));
    }
}

/// The keyboard shortcuts the shell answers to: the same list as the `?` window.
fn shortcuts_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    widgets::section(ui, t, app.tr("Keyboard shortcuts"));
    let lang = app.language;
    let os = ui.ctx().os();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("settings-shortcuts").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            for (key, action) in shortcut_rows(lang, os) {
                key_chip(ui, t, &key);
                ui.label(RichText::new(action).color(t.ui_text));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(lang.tr("Outside text fields; Escape leaves a field")).small().color(t.muted));
    });
}

/// The rows of the shortcuts table: [`crate::SHORTCUTS`], the keys as `os` names them (Cmd on
/// a Mac, like the `?` window), the descriptions translated.
fn shortcut_rows(lang: Language, os: egui::os::OperatingSystem) -> impl Iterator<Item = (std::borrow::Cow<'static, str>, &'static str)> {
    crate::SHORTCUTS.iter().map(move |(key, what)| (crate::shortcut_key(os, key), lang.tr(what)))
}

/// The status card's counts, `1 customer` in the singular.
fn counts_line(lang: Language, customers: usize, devices: usize, interventions: usize) -> String {
    let count = |n: usize, one: &str, many: &str| if n == 1 { lang.tr(one).to_string() } else { lang.fill(many, &[("n", &n.to_string())]) };
    format!(
        "{} · {} · {}",
        count(customers, "1 customer", "{n} customers"),
        count(devices, "1 device", "{n} devices"),
        count(interventions, "1 intervention", "{n} interventions"),
    )
}

/// Version, actor, where the desk is, counts and the problems found loading it.
fn status_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    widgets::section(ui, t, app.tr("Status"));
    let lang = app.language;
    let d = app.session.data();
    let actor = app.session.actor().to_string();
    let server = app.session.server();
    let customers = d.customers.iter().filter(|c| !c.archived && c.erased_at.is_none()).count();
    let devices = d.devices.iter().filter(|x| !x.retired).count();
    let running = d.interventions.iter().filter(|i| i.running()).count();
    let lines = [
        lang.fill("Version {version}", &[("version", env!("CARGO_PKG_VERSION"))]),
        lang.fill("Working as {actor}", &[("actor", if actor.is_empty() { "?" } else { &actor })]),
        if server.is_empty() { lang.tr("This computer").to_string() } else { lang.fill("Server: {url}", &[("url", &server)]) },
        counts_line(lang, customers, devices, d.interventions.len()),
        format!("{}: {} · {}: {}", lang.tr("Running"), running, lang.tr("Privacy Notices"), d.notices.len()),
    ];
    let problems = app.session.problems();
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for line in &lines {
            ui.label(RichText::new(line).color(t.ui_text));
        }
        if !problems.is_empty() {
            ui.add_space(8.0);
            let mut all = vec![lang.tr("Problems loading the desk").to_string()];
            all.extend(problems.iter().map(|p| lang.tr_message(p)));
            banner(ui, t, Tone::Warn, &all);
        }
    });
    if !server.is_empty() {
        browser_card(app, ui, t);
    }
}

/// On a remote desk: forget the token this browser keeps (a shared or lost PC), in two presses.
fn browser_card(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(8.0);
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let text = app.tr(
            "This browser keeps the token that opens the desk. On a shared or lost PC, forget it: the page reloads without it, and the framework's link brings you back.",
        );
        ui.label(RichText::new(text).color(t.muted));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if app.settings.forget_armed {
                if widgets::danger_button(ui, t, app.tr("Forget and reload")).clicked() {
                    forget_token(app);
                }
                if ui.button(app.tr("Cancel")).clicked() {
                    app.settings.forget_armed = false;
                }
            } else if ui.button(app.tr("Forget the token on this browser")).clicked() {
                app.settings.forget_armed = true;
            }
        });
    });
}

/// Asks the host to drop the browser's server and token and reload.
fn forget_token(app: &mut DeskApp) {
    app.settings.forget_armed = false;
    app.requests.push(Request::ForgetToken);
}

// ---------------------------------------------------------------------------------------------
// Pieces

/// A section's heading and card: the form on a copy of the draft, the error and the Save row,
/// which sends only the keys that changed.
fn section_card(
    app: &mut DeskApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    section: Section,
    title: &str,
    form: impl FnOnce(&mut egui::Ui, &Tokens, &mut Draft, Language),
) {
    widgets::section(ui, t, app.tr(title));
    let lang = app.language;
    let mut draft = app.settings.draft.clone();
    let base = app.settings.base.clone();
    let fb = app.settings.feedback(section).clone();
    let mut save = false;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        form(ui, t, &mut draft, lang);
        widgets::error_line(ui, t, &fb.error);
        ui.add_space(8.0);
        save = save_row(ui, t, lang, draft.dirty(&base, section), fb.job.is_some());
    });
    app.settings.draft = draft;
    if save {
        save_section(app, section);
    }
}

/// The Save row of a card: the accent button when there is something to save, a disabled one
/// when not, a spinner while the save is on its way. Answers whether Save was clicked.
fn save_row(ui: &mut egui::Ui, t: &Tokens, lang: Language, enabled: bool, busy: bool) -> bool {
    ui.horizontal(|ui| {
        if busy {
            ui.add(egui::Spinner::new().size(16.0).color(t.accent));
            ui.label(RichText::new(lang.tr("Loading…")).small().color(t.muted));
            false
        } else if enabled {
            widgets::primary_button(ui, t, None, lang.tr("Save")).clicked()
        } else {
            ui.add_enabled(false, egui::Button::new(lang.tr("Save")));
            false
        }
    })
    .inner
}

/// A grid row with a number field bounded to `0..=max` and a muted hint beside it.
fn number_row(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut u32, max: u32, hint: &str) {
    ui.label(RichText::new(label).color(t.muted));
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(value).range(0..=max).speed(1.0));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).small().color(t.muted));
        }
    });
    *value = (*value).min(max);
    ui.end_row();
}

/// A grid row with only a muted hint in the field column.
fn hint_row(ui: &mut egui::Ui, t: &Tokens, hint: &str) {
    ui.label("");
    ui.label(RichText::new(hint).small().color(t.muted));
    ui.end_row();
}

/// A muted hint under a value row.
fn hint_line(ui: &mut egui::Ui, t: &Tokens, hint: &str) {
    ui.add(egui::Label::new(RichText::new(hint).small().color(t.muted)).wrap());
}

/// The font of the client setup's labels.
const LABEL_FONT: f32 = 14.0;

/// The width of the client setup's label column: the widest label in this language with a gap,
/// within bounds so a long translation neither gets cut nor squeezes the values.
fn label_width(painter: &egui::Painter, labels: &[&str]) -> f32 {
    let width = |l: &&str| painter.layout_no_wrap(l.to_string(), FontId::proportional(LABEL_FONT), egui::Color32::WHITE).size().x;
    let widest = labels.iter().map(width).fold(0.0, f32::max);
    (widest + 12.0).clamp(140.0, 240.0)
}

/// A label in a column `label_w` wide, a read-only monospace box with the value and a Copy
/// button; a missing value shows why and cannot be copied. A label cut short shows whole on hover.
fn value_row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, label: &str, label_w: f32, value: Option<&str>) {
    ui.horizontal(|ui| {
        let (lr, resp) = ui.allocate_exact_size(vec2(label_w, 30.0), Sense::hover());
        let room = lr.width() - 4.0;
        let galley = widgets::truncate(ui.painter(), label, FontId::proportional(LABEL_FONT), t.muted, room);
        if ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(LABEL_FONT), t.muted).size().x > room {
            resp.on_hover_text(label);
        }
        ui.painter().galley(lr.left_center() - vec2(0.0, galley.size().y / 2.0), galley, t.muted);
        let w = (ui.available_width() - 40.0).max(80.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
        ui.painter().rect_filled(rect, 6.0, t.panel);
        ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        let (text, color) = match value {
            Some(v) => (v.to_string(), t.ui_text),
            None => (app.tr("Not available until the server is set").to_string(), t.muted),
        };
        let galley = widgets::truncate(ui.painter(), &text, FontId::monospace(13.0), color, rect.width() - 20.0);
        ui.painter().galley(rect.left_center() + vec2(10.0, -galley.size().y / 2.0), galley, color);
        let enabled = value.is_some_and(|v| !v.is_empty());
        if widgets::icon_button(ui, t, Icon::Copy, app.tr("Copy to the clipboard"), enabled, false).clicked()
            && let Some(v) = value
        {
            ui.ctx().copy_text(v.to_string());
            app.toast(app.tr("Copied to the clipboard").to_string());
        }
    });
}

/// A line with an icon and a text in a tone (a port that answers, a key that is missing).
fn status_line(ui: &mut egui::Ui, t: &Tokens, tone: Tone, icon: Icon, text: &str) {
    let color = widgets::tone_colors(t, tone).1;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, color);
        ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
    });
}

/// A filled banner in a tone with a warning icon and one or more lines.
fn banner(ui: &mut egui::Ui, t: &Tokens, tone: Tone, lines: &[String]) {
    let (fill, color) = widgets::tone_colors(t, tone);
    egui::Frame::NONE.fill(fill).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Warning, color);
            ui.vertical(|ui| {
                for line in lines {
                    ui.add(egui::Label::new(RichText::new(line).color(color)).wrap());
                }
            });
        });
    });
}

/// A key cap for the shortcuts table.
fn key_chip(ui: &mut egui::Ui, t: &Tokens, key: &str) {
    let galley = ui.painter().layout_no_wrap(key.to_string(), FontId::monospace(13.0), t.ui_text);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, 6.0, t.panel);
    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    ui.painter().galley(rect.left_center() + vec2(8.0, -galley.size().y / 2.0), galley, t.ui_text);
}

/// A `missing` item of `rustdesk.setup` as the field it names.
fn missing_label(lang: Language, item: &str) -> String {
    if item.starts_with("rustdesk_host") {
        lang.tr("Server name").to_string()
    } else if item.starts_with("rustdesk_key") {
        lang.tr("Public key").to_string()
    } else {
        item.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Data;
    use crate::{DeskSession, Screen, Session, engine};

    /// Renders one frame of the app at 1440×900 in `ctx` (no window, no GPU).
    fn frame_in(ctx: &egui::Context, app: &mut DeskApp) {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    fn frame(app: &mut DeskApp) {
        frame_in(&egui::Context::default(), app);
    }

    /// A desk with nothing in it: the app opens on Settings with the whole guide to do.
    fn empty() -> DeskApp {
        DeskApp::new(Box::new(Session::new(engine::Desk::in_memory(Data::new()))))
    }

    #[test]
    fn renders_in_both_languages_and_themes_with_the_guide_and_the_setup() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = DeskApp::demo();
                app.language = lang;
                app.dark = dark;
                app.go(Screen::Settings);
                frame(&mut app);
                // The demo has a company and a notice but no key: the guide shows its last card.
                assert!(guide_needed(&app));
                assert!(card_done(&app, Card::Company) && card_done(&app, Card::Notice) && !card_done(&app, Card::Server));
                assert!(app.settings.guide.prefilled);
                assert_eq!(app.settings.guide.host, "rustdesk.example.net");
                assert_eq!(app.settings.draft, app.settings.base);
                // egui may run a frame in two passes, so the answer can already be in.
                assert!(app.settings.setup_job.is_some() || app.settings.setup.is_some(), "the first frame asks for the client setup");
                frame(&mut app);
                let setup = app.settings.setup.clone().unwrap();
                assert_eq!(setup["id_server"], "rustdesk.example.net");
                assert_eq!(setup["missing"].as_array().map(Vec::len), Some(1));
                assert!(setup["config_string"].is_null());
                assert!(app.settings.setup_job.is_none() && app.settings.setup_error.is_empty());
                // Without a probe the check is refused: the refusal shows under the button.
                run_check(&mut app);
                assert!(app.settings.checking.is_some());
                run_check(&mut app);
                frame(&mut app);
                assert!(app.settings.checking.is_none());
                assert!(app.settings.check_error.contains("probe"), "{}", app.settings.check_error);
                assert!(app.settings.check.is_none());
                // A check that answered renders its port rows.
                app.settings.check = Some(json!({
                    "ports": [
                        {"port": 21115, "service": "hbbs: NAT type test (TCP)", "ok": true, "ms": 12},
                        {"port": 21117, "service": "hbbr: relay (TCP)", "ok": false, "error": "connection refused"},
                    ],
                    "key_known": false,
                    "note": "UDP…",
                }));
                app.settings.check_error.clear();
                frame(&mut app);
                // Problems found loading the desk, and the empty desk with everything to do.
                let mut app = empty();
                app.language = lang;
                assert_eq!(app.screen, Screen::Settings);
                frame(&mut app);
                assert!(!card_done(&app, Card::Company) && !card_done(&app, Card::Notice) && !card_done(&app, Card::Server));
                frame(&mut app);
                assert_eq!(app.settings.setup.as_ref().map(|v| v["missing"].as_array().map(Vec::len)), Some(Some(2)));
            }
        }
    }

    #[test]
    fn a_section_saves_only_the_keys_that_changed() {
        let mut app = DeskApp::demo();
        app.go(Screen::Settings);
        frame(&mut app);
        let base = app.settings.base.clone();
        assert!(!app.settings.draft.dirty(&base, Section::Company));
        app.settings.draft.company_name = " Sirius ".into();
        app.settings.draft.minimum_minutes = 30;
        app.settings.draft.retention_years = 9_999;
        assert_eq!(app.settings.draft.changes(&base, Section::Company), json!({"company_name": "Sirius", "minimum_minutes": 30}));
        assert_eq!(app.settings.draft.changes(&base, Section::Rules), json!({"retention_years": MAX_YEARS}), "clamped to the engine's bound");
        assert_eq!(app.settings.draft.changes(&base, Section::Server), json!({}));
        save_section(&mut app, Section::Company);
        assert!(app.settings.company.job.is_some());
        save_section(&mut app, Section::Company);
        frame(&mut app);
        assert!(app.settings.company.job.is_none() && app.settings.company.error.is_empty());
        let p = &app.session.data().preferences;
        assert_eq!((p.company_name.as_str(), p.minimum_minutes, p.retention_years), ("Sirius", 30, 10));
        assert!(!app.settings.draft.dirty(&app.settings.base, Section::Company), "the draft shows what was saved");
        assert_eq!(app.settings.draft.company_name, "Sirius");
        assert!(app.settings.draft.dirty(&app.settings.base, Section::Rules), "the other section keeps its edit");
        // The number field clamps what it shows to the engine's bound.
        assert_eq!(app.settings.draft.retention_years, MAX_YEARS);
        // An engine refusal lands under its section and keeps the draft.
        let job = app.run_as("prefs.set", json!({"retention_years": 1000}), AppIntent::Settings(Intent::Prefs(Section::Rules)));
        app.settings.rules.job = Some(job);
        frame(&mut app);
        assert!(app.settings.rules.job.is_none());
        assert!(app.settings.rules.error.contains("retention_years"), "{}", app.settings.rules.error);
        assert_eq!(app.settings.draft.retention_years, MAX_YEARS);
        // A change made elsewhere (the CLI, another technician) reaches the untouched fields
        // and leaves the edited ones alone.
        app.settings.draft.rustdesk_key = "typed".into();
        app.run("prefs.set", json!({"company_name": "Altro", "rustdesk_host": "rd.example.org", "retention_years": 7}));
        frame(&mut app);
        let d = &app.settings.draft;
        assert_eq!(
            (d.company_name.as_str(), d.rustdesk_host.as_str(), d.rustdesk_key.as_str(), d.retention_years),
            ("Altro", "rd.example.org", "typed", MAX_YEARS)
        );
        assert_eq!(app.settings.base.retention_years, 7);
        // Theme and language from the appearance combos.
        app.dark = true;
        app.run("prefs.set", json!({"theme": "dark"}));
        frame(&mut app);
        assert_eq!(app.session.data().preferences.theme, "dark");
        assert_eq!(missing_label(Language::It, "rustdesk_key (prefs.set; …)"), "Chiave pubblica");
        assert_eq!(missing_label(Language::En, "other"), "other");
    }

    #[test]
    fn the_guide_cards_set_up_an_empty_desk_and_reopening_resets_the_drafts() {
        let ctx = egui::Context::default();
        let mut app = empty();
        frame_in(&ctx, &mut app);
        let g = &app.settings.guide;
        assert!(g.prefilled);
        assert_eq!(g.version.len(), 7, "{}", g.version);
        assert_eq!(g.title, "Informativa assistenza tecnica", "the desk's default language is Italian");
        assert!(!g.can_save(Card::Company) && !g.can_save(Card::Notice) && !g.can_save(Card::Server));
        save_card(&mut app, Card::Company);
        assert!(app.settings.guide.company.job.is_none(), "nothing to send");
        // The company.
        app.settings.guide.company_name = "Sirius".into();
        save_card(&mut app, Card::Company);
        assert!(app.settings.guide.company.job.is_some());
        frame_in(&ctx, &mut app);
        assert!(card_done(&app, Card::Company));
        assert!(app.settings.guide.company.job.is_none() && app.settings.guide.company.error.is_empty());
        // The notice: once, then the same version is refused and the error stays in the card.
        app.settings.guide.text = "Testo dell'informativa".into();
        save_card(&mut app, Card::Notice);
        frame_in(&ctx, &mut app);
        assert!(card_done(&app, Card::Notice));
        let version = app.settings.guide.version.clone();
        assert_eq!(app.session.data().notices.first().map(|n| n.version.clone()), Some(version));
        assert!(!guide_needed(&app) || !card_done(&app, Card::Server));
        save_card(&mut app, Card::Notice);
        frame_in(&ctx, &mut app);
        assert!(app.settings.guide.notice.error.contains("exists"), "{}", app.settings.guide.notice.error);
        // The server: the guide goes away and the setup card has the import string.
        app.settings.guide.host = "rd.example.org".into();
        app.settings.guide.key = "KEY=".into();
        save_card(&mut app, Card::Server);
        frame_in(&ctx, &mut app);
        assert!(card_done(&app, Card::Server));
        assert!(!guide_needed(&app));
        frame_in(&ctx, &mut app);
        let setup = app.settings.setup.clone().unwrap();
        assert!(setup["config_string"].is_string() && setup["windows_installer_name"].is_string(), "{setup}");
        assert_eq!(setup["missing"].as_array().map(Vec::len), Some(0));
        assert_eq!(effective_server(&app), ("rd.example.org".to_string(), "KEY=".to_string()));
        // Leaving and coming back resets the drafts to what is saved.
        app.settings.draft.company_name = "dirty".into();
        app.settings.company.error = "old".into();
        frame_in(&ctx, &mut app);
        assert_eq!(app.settings.draft.company_name, "dirty", "staying keeps the edit");
        app.go(Screen::Customers);
        frame_in(&ctx, &mut app);
        frame_in(&ctx, &mut app);
        app.go(Screen::Settings);
        frame_in(&ctx, &mut app);
        assert_eq!(app.settings.draft.company_name, "Sirius");
        assert!(app.settings.company.error.is_empty());
        // Escape has nothing to close here.
        assert!(!close(&mut app));
    }

    /// The demo desk as if served: only a remote desk has a token to forget.
    struct Served(Session);

    impl DeskSession for Served {
        fn data(&self) -> &Data {
            self.0.data()
        }
        fn actor(&self) -> &str {
            self.0.actor()
        }
        fn now(&self) -> crate::core::chrono::DateTime<crate::core::chrono::Utc> {
            self.0.now()
        }
        fn execute(&mut self, command: &str, params: &Value) -> u64 {
            self.0.execute(command, params)
        }
        fn pump(&mut self) -> Vec<Completed> {
            self.0.pump()
        }
        fn jobs_pending(&self) -> usize {
            self.0.jobs_pending()
        }
        fn revision(&self) -> u64 {
            self.0.revision()
        }
        fn server(&self) -> String {
            "https://assistenza.example.it".into()
        }
    }

    #[test]
    fn a_remote_desk_offers_to_forget_the_browser_s_token_in_two_presses() {
        // A local desk: nothing to forget.
        let mut local = DeskApp::new(Box::new(Session::demo()));
        local.go(Screen::Settings);
        let label = local.tr("Forget the token on this browser").to_string();
        let texts = crate::privacy::tests::texts_in(&mut local, 1200.0, 4000.0);
        assert!(crate::privacy::tests::find(&texts, &label).is_none());
        // A served desk: the card, then the confirming press, then the request to the host.
        let mut app = DeskApp::new(Box::new(Served(Session::demo())));
        app.go(Screen::Settings);
        let texts = crate::privacy::tests::texts_in(&mut app, 1200.0, 4000.0);
        assert!(crate::privacy::tests::find(&texts, &label).is_some_and(crate::privacy::tests::whole), "{label}");
        assert!(app.requests.is_empty());
        app.settings.forget_armed = true;
        let texts = crate::privacy::tests::texts_in(&mut app, 1200.0, 4000.0);
        assert!(crate::privacy::tests::find(&texts, app.tr("Forget and reload")).is_some());
        assert!(app.requests.is_empty(), "arming asks nothing of the host");
        forget_token(&mut app);
        assert_eq!(app.requests, vec![Request::ForgetToken]);
        assert!(!app.settings.forget_armed);
    }

    /// A desk seen through a mirror, as from a server: a command's answer arrives before the
    /// snapshot with its effect, which comes only when `snapshot` is set.
    struct Mirrored {
        desk: Session,
        mirror: Data,
        revision: u64,
        snapshot: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl DeskSession for Mirrored {
        fn data(&self) -> &Data {
            &self.mirror
        }
        fn actor(&self) -> &str {
            self.desk.actor()
        }
        fn now(&self) -> crate::core::chrono::DateTime<crate::core::chrono::Utc> {
            self.desk.now()
        }
        fn execute(&mut self, command: &str, params: &Value) -> u64 {
            self.desk.execute(command, params)
        }
        fn pump(&mut self) -> Vec<Completed> {
            let done = self.desk.pump();
            if self.snapshot.get() {
                self.mirror = self.desk.data().clone();
                self.revision = self.desk.revision();
            }
            done
        }
        fn jobs_pending(&self) -> usize {
            self.desk.jobs_pending()
        }
        fn revision(&self) -> u64 {
            self.revision
        }
    }

    #[test]
    fn a_save_on_a_remote_desk_keeps_what_was_saved_until_the_snapshot_confirms_it() {
        let desk = Session::demo();
        let (mirror, revision) = (desk.data().clone(), desk.revision());
        let snapshot = std::rc::Rc::new(std::cell::Cell::new(false));
        let mut app = DeskApp::new(Box::new(Mirrored { desk, mirror, revision, snapshot: snapshot.clone() }));
        app.go(Screen::Settings);
        let ctx = egui::Context::default();
        frame_in(&ctx, &mut app);
        assert!(app.settings.draft.require_consent);
        app.settings.draft.require_consent = false;
        app.settings.draft.retention_years = 7;
        app.settings.draft.company_name = "  Sirius  ".into();
        save_section(&mut app, Section::Rules);
        save_section(&mut app, Section::Company);
        frame_in(&ctx, &mut app);
        // Both answers are in, the mirror still has the old preferences.
        assert!(app.settings.rules.job.is_none() && app.settings.company.job.is_none());
        assert!(app.session.data().preferences.require_consent);
        assert!(app.toast.as_ref().is_some_and(|(text, _)| text == app.tr("Preferences saved")));
        let d = &app.settings.draft;
        assert_eq!((d.require_consent, d.retention_years, d.company_name.as_str()), (false, 7, "Sirius"), "the draft shows what was saved, as saved");
        assert!(!d.dirty(&app.settings.base, Section::Rules) && !d.dirty(&app.settings.base, Section::Company));
        frame_in(&ctx, &mut app);
        assert!(!app.settings.draft.require_consent, "nothing reverts while the snapshot is on its way");
        // The snapshot lands: it confirms the values, nothing flips.
        snapshot.set(true);
        frame_in(&ctx, &mut app);
        assert!(!app.session.data().preferences.require_consent);
        let d = &app.settings.draft;
        assert_eq!((d.require_consent, d.retention_years, d.company_name.as_str()), (false, 7, "Sirius"));
        assert_eq!(app.settings.draft, app.settings.base);
        // An answer without the preferences leaves the draft as typed.
        assert!(saved_preferences(&Ok(json!({"changed": []}))).is_none());
        assert!(saved_preferences(&Ok(json!({"preferences": 3}))).is_none());
        assert_eq!(saved_preferences(&Ok(json!({"preferences": {"retention_years": 4}}))).map(|p| p.retention_years), Some(4));
    }

    /// The texts painted in one frame at `w`×`h`, each with whether it was cut short.
    fn texts_at(app: &mut DeskApp, w: f32, h: f32) -> Vec<(String, bool)> {
        texts_on(app, w, h, egui::os::OperatingSystem::Nix)
    }

    /// [`texts_at`] on a context that runs on `os` (the shortcut keys depend on it).
    fn texts_on(app: &mut DeskApp, w: f32, h: f32, os: egui::os::OperatingSystem) -> Vec<(String, bool)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, bool)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.elided)),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        ctx.set_os(os);
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))), ..Default::default() };
        let mut out = Vec::new();
        // Two frames: the client setup answers after the first.
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

    #[test]
    fn the_client_setup_labels_fit_in_italian_and_show_whole_on_hover() {
        let mut app = DeskApp::demo();
        app.language = Language::It;
        app.go(Screen::Settings);
        let texts = texts_at(&mut app, 1100.0, 2600.0);
        for label in ["Server ID", "Server relay", "Server API (lascia vuoto)", "Stringa di importazione", "Nome dell'installer Windows"] {
            let row = texts.iter().find(|(t, _)| t == label);
            assert_eq!(row.map(|(_, elided)| *elided), Some(false), "{label}: {texts:?}");
        }
        // The column follows the widest label, within bounds.
        let ctx = egui::Context::default();
        let mut widths = (0.0, 0.0, 0.0, 0.0);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter();
            let installer = p.layout_no_wrap("Nome dell'installer Windows".into(), FontId::proportional(LABEL_FONT), egui::Color32::WHITE).size().x;
            widths =
                (label_width(p, &["ID", "Nome dell'installer Windows"]), installer, label_width(p, &["ID"]), label_width(p, &[&"x".repeat(200)]));
        });
        output.textures_delta.clear();
        let (it, installer, short, long) = widths;
        assert!(it >= installer + 4.0 && it > 170.0, "{it} for {installer}");
        assert_eq!((short, long), (140.0, 240.0));
    }

    #[test]
    fn the_shortcuts_are_the_help_windows_and_the_counts_say_one() {
        use egui::os::OperatingSystem;
        let rows: Vec<_> = shortcut_rows(Language::It, OperatingSystem::Windows).collect();
        assert_eq!(rows.len(), crate::SHORTCUTS.len());
        for ((key, it), (en_key, en)) in rows.iter().zip(crate::SHORTCUTS) {
            assert_eq!(key, en_key);
            assert_ne!(it, en, "untranslated shortcut: {en}");
        }
        assert!(rows.iter().any(|(k, a)| k == "Ctrl+," && *a == "Impostazioni"), "{rows:?}");
        // On a Mac the shortcuts listen for Cmd: the table says so (the plain keys stay).
        let mac: Vec<_> = shortcut_rows(Language::It, OperatingSystem::Mac).collect();
        for key in ["Cmd+,", "Cmd+R", "Cmd+Enter", "/", "?", "Esc"] {
            assert!(mac.iter().any(|(k, _)| k == key), "{key} in {mac:?}");
        }
        assert!(!mac.iter().any(|(k, _)| k.contains("Ctrl")), "{mac:?}");
        assert!(shortcut_rows(Language::En, OperatingSystem::IOS).any(|(k, a)| k == "Cmd+," && a == "Settings"));
        let mut app = DeskApp::demo();
        app.language = Language::It;
        app.go(Screen::Settings);
        let texts: Vec<String> = texts_at(&mut app, 1100.0, 3200.0).into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t == "Ctrl+,") && texts.iter().any(|t| t == "?"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("Comma")), "{texts:?}");
        let texts: Vec<String> = texts_on(&mut app, 1100.0, 3200.0, OperatingSystem::Mac).into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t == "Cmd+,") && texts.iter().any(|t| t == "Cmd+Enter"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.starts_with("Ctrl+")), "{texts:?}");
        // The counts of the status card.
        assert_eq!(counts_line(Language::It, 1, 1, 1), "1 cliente · 1 PC · 1 intervento");
        assert_eq!(counts_line(Language::It, 3, 0, 12), "3 clienti · 0 PC · 12 interventi");
        assert_eq!(counts_line(Language::En, 1, 2, 1), "1 customer · 2 devices · 1 intervention");
        assert_eq!(counts_line(Language::En, 0, 1, 5), "0 customers · 1 device · 5 interventions");
    }

    #[test]
    fn a_done_step_number_reads_in_both_themes() {
        for t in [Tokens::LIGHT, Tokens::DARK] {
            let (fill, digit) = badge_colors(&t, true);
            assert!(widgets::contrast(fill, digit) >= 4.5, "dark {}: {:.2}", t.dark, widgets::contrast(fill, digit));
            assert_eq!(badge_colors(&t, false), (t.accent, t.on_accent));
        }
    }
}
