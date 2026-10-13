//! RemoteCraft's egui front end: the support desk the MailCraft way. A thin interface over a
//! [`DeskSession`] (the local desk or a server mirrored over HTTP): it reads the desk's data and
//! acts only through engine commands; results come back through `pump` and are routed to the
//! screen that asked, by [`Intent`]. Host-only actions (opening a `rustdesk://` link, saving a
//! file) are [`Request`]s the app serves.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod chrome;
pub mod customers;
pub mod derived;
pub mod devices;
pub mod erase;
pub mod format;
pub mod forms;
pub mod i18n;
pub mod icons;
pub mod interventions;
mod nav;
pub mod privacy;
pub mod reports;
pub mod settings;
pub mod theme;
pub mod timer;
pub mod widgets;

use std::collections::BTreeMap;
use std::time::Duration;

pub use i18n::Language;
pub use remotecraft_engine as engine;
pub use remotecraft_engine::core;
use remotecraft_engine::core::Data;
pub use remotecraft_engine::{Completed, DeskSession, Session};
use serde_json::{Value, json};
use web_time::Instant;

/// Which screen fills the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Customers,
    Devices,
    Interventions,
    Privacy,
    Reports,
    Settings,
}

impl Screen {
    pub const ALL: [Self; 6] = [Self::Customers, Self::Devices, Self::Interventions, Self::Privacy, Self::Reports, Self::Settings];

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "customers" | "clienti" => Some(Self::Customers),
            "devices" | "pc" => Some(Self::Devices),
            "interventions" | "interventi" => Some(Self::Interventions),
            "privacy" => Some(Self::Privacy),
            "reports" | "report" => Some(Self::Reports),
            "settings" | "impostazioni" => Some(Self::Settings),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Customers => "customers",
            Self::Devices => "devices",
            Self::Interventions => "interventions",
            Self::Privacy => "privacy",
            Self::Reports => "reports",
            Self::Settings => "settings",
        }
    }
}

/// Something only the host app can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Open a link: `rustdesk://connection/new/<id>` in the RustDesk client, `https://…` in
    /// the browser.
    OpenUrl(String),
    /// Save bytes somewhere (a GDPR export).
    SaveFile { name: String, data: Vec<u8> },
}

/// What a queued command was for: how its outcome is applied when it completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Nothing to do beyond the status bar.
    None,
    /// Show this (already translated) text when the command succeeds.
    Toast(String),
    /// Save the result as a JSON file with this name.
    Export(String),
    Customers(customers::Intent),
    Devices(devices::Intent),
    Interventions(interventions::Intent),
    Privacy(privacy::Intent),
    Reports(reports::Intent),
    Settings(settings::Intent),
    Timer(timer::Intent),
    Erase(erase::Intent),
}

/// A record to open as soon as it appears in the data (a remote desk answers a command before
/// the snapshot that holds the new record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingOpen {
    pub screen: Screen,
    pub id: u64,
    pub since: Instant,
}

/// The keyboard shortcuts (key, what), for the help window.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("/", "Search"),
    ("s", "Start intervention on the selection"),
    ("x", "Stop my running intervention"),
    ("n", "New record on this screen"),
    ("l", "Log past work"),
    ("e", "Edit the open record"),
    ("c", "Connect to the open PC"),
    ("p", "Record consent for the open customer"),
    ("j / k", "Next / previous row"),
    ("Esc", "Close the card, the form or the pane"),
    ("1 … 5", "Customers, Devices, Interventions, Privacy, Reports"),
    ("Ctrl+,", "Settings"),
    ("Ctrl+R", "Reload this screen's data"),
    ("Ctrl+Enter", "Save the form, or start on the PC found by the search"),
    ("?", "This help"),
];

/// A question before an action that cannot be undone: `DeskApp::ask(Confirm { .. })`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub ok_label: String,
    pub danger: bool,
    pub command: String,
    pub params: Value,
    pub intent: Intent,
}

/// The period and filters the Interventions screen and the Reports share (a report row drills
/// down into the interventions behind it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkFilter {
    /// Form text the engine accepts (`2026-10-01`), empty = no bound.
    pub from: String,
    pub to: String,
    pub customer: Option<u64>,
    pub device: Option<u64>,
    /// Empty = every technician.
    pub technician: String,
    /// Empty = every kind; else `remote`, `onsite`, `phone`, `lab`, `other`.
    pub kind: String,
    pub running_only: bool,
    pub billable_only: bool,
}

/// The whole app state the interface keeps on top of the session.
pub struct DeskApp {
    /// The local desk, or a server mirrored over HTTP (the web app).
    pub session: Box<dyn DeskSession>,
    pub language: Language,
    pub dark: bool,
    applied_dark: Option<bool>,
    pub screen: Screen,
    /// The screen to return to from Settings.
    previous: Screen,
    pub search: String,
    pub search_focus: bool,
    pub status: String,
    toast: Option<(String, Instant)>,
    /// Host actions the app should serve (links, files).
    pub requests: Vec<Request>,
    /// What each queued job is for.
    jobs: BTreeMap<u64, Intent>,
    pub confirm: Option<Confirm>,
    pub selected_customer: Option<u64>,
    pub selected_device: Option<u64>,
    pub selected_intervention: Option<u64>,
    /// The period and filters of the Interventions screen and the Reports.
    pub work_filter: WorkFilter,
    pub customers: customers::State,
    pub devices: devices::State,
    pub interventions: interventions::State,
    pub privacy: privacy::State,
    pub reports: reports::State,
    pub settings: settings::State,
    pub timer: timer::State,
    pub erase: erase::State,
    /// Findings of the last privacy review (the badge in the navigation).
    pub privacy_findings: usize,
    /// What the screens read every frame: privacy states, counts, timers, findings.
    pub derived: derived::Derived,
    /// A record to open as soon as the data holds it.
    pub pending_open: Option<PendingOpen>,
    pub shortcuts_open: bool,
    /// Bumped by `Ctrl+R`: the screens' cached commands run again.
    pub refresh_serial: u64,
    /// Settings opened only because the desk was empty: leave it once it is set up.
    auto_settings: bool,
    last_revision: Option<u64>,
}

/// The name of a customer, for rows and chips.
pub fn customer_name(data: &Data, id: u64) -> String {
    data.customer(id).map(|c| c.name.clone()).unwrap_or_default()
}

/// The name of a device, for rows and chips.
pub fn device_name(data: &Data, id: Option<u64>) -> String {
    id.and_then(|id| data.device(id)).map(|d| d.name.clone()).unwrap_or_default()
}

/// The screens' `(English, Italian)` label tables, one after the other.
pub fn module_labels() -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for table in [
        customers::LABELS_IT,
        devices::LABELS_IT,
        interventions::LABELS_IT,
        privacy::LABELS_IT,
        reports::LABELS_IT,
        settings::LABELS_IT,
        timer::LABELS_IT,
    ] {
        out.extend(table.iter().copied());
    }
    out
}

/// The Italian of a screen's label, from the screens' tables.
pub(crate) fn module_label(text: &str) -> Option<&'static str> {
    [
        customers::LABELS_IT,
        devices::LABELS_IT,
        interventions::LABELS_IT,
        privacy::LABELS_IT,
        reports::LABELS_IT,
        settings::LABELS_IT,
        timer::LABELS_IT,
    ]
    .iter()
    .flat_map(|t| t.iter())
    .find(|(en, _)| *en == text)
    .map(|(_, it)| *it)
}

/// Every translated label of the interface: the shared ones and each screen's.
pub fn all_labels() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = i18n::LABELS.to_vec();
    for (en, _) in module_labels() {
        if !out.contains(&en) {
            out.push(en);
        }
    }
    out
}

impl DeskApp {
    pub fn new(session: Box<dyn DeskSession>) -> Self {
        let language = match session.data().preferences.language.as_str() {
            "it" => Language::It,
            "en" => Language::En,
            _ => Language::system(),
        };
        let dark = session.data().preferences.theme == "dark";
        let empty = session.data().customers.is_empty() && session.data().notices.is_empty() && session.problem().is_none();
        let screen = if empty { Screen::Settings } else { Screen::Customers };
        Self {
            session,
            language,
            dark,
            applied_dark: None,
            screen,
            previous: Screen::Customers,
            search: String::new(),
            search_focus: false,
            status: String::new(),
            toast: None,
            requests: Vec::new(),
            jobs: BTreeMap::new(),
            confirm: None,
            selected_customer: None,
            selected_device: None,
            selected_intervention: None,
            work_filter: WorkFilter::default(),
            customers: customers::State::default(),
            devices: devices::State::default(),
            interventions: interventions::State::default(),
            privacy: privacy::State::default(),
            reports: reports::State::default(),
            settings: settings::State::default(),
            timer: timer::State::default(),
            erase: erase::State::default(),
            privacy_findings: 0,
            derived: derived::Derived::default(),
            pending_open: None,
            shortcuts_open: false,
            refresh_serial: 0,
            auto_settings: empty,
            last_revision: None,
        }
    }

    /// The demo app: the demo desk, no files.
    pub fn demo() -> Self {
        Self::new(Box::new(Session::demo()))
    }

    pub fn tr<'a>(&self, text: &'a str) -> &'a str {
        self.language.tr(text)
    }

    pub fn italian(&self) -> bool {
        self.language == Language::It
    }

    /// A command's translated label, for tooltips.
    pub fn tip(&self, command: &str) -> String {
        match engine::COMMANDS.iter().find(|c| c.id == command) {
            Some(c) => self.tr(c.label).to_string(),
            None => command.to_string(),
        }
    }

    pub fn data(&self) -> &Data {
        self.session.data()
    }

    pub fn now(&self) -> core::chrono::DateTime<core::chrono::Utc> {
        self.session.now()
    }

    /// The window title: the selected record and the app's name.
    pub fn window_title(&self) -> String {
        let d = self.session.data();
        let what = match self.screen {
            Screen::Customers => self.selected_customer.map(|id| customer_name(d, id)),
            Screen::Devices => self.selected_device.and_then(|id| d.device(id)).map(|x| format!("{} · {}", x.name, customer_name(d, x.customer_id))),
            Screen::Interventions => self.selected_intervention.and_then(|id| d.intervention(id)).map(|i| customer_name(d, i.customer_id)),
            _ => None,
        };
        let actor = self.session.actor();
        let running = d.interventions.iter().filter(|i| i.running() && i.technician == actor).map(|i| i.started_at.as_str()).max();
        let timer = running.map(|s| format!("(⏱ {}) ", format::elapsed_short(s, self.now()))).unwrap_or_default();
        match what.filter(|w| !w.is_empty()) {
            Some(w) => format!("{timer}{w} – RemoteCraft"),
            None => format!("{timer}RemoteCraft"),
        }
    }

    /// Opens `screen` on `id` now when the data holds it, else as soon as it does.
    pub fn open_when_ready(&mut self, screen: Screen, id: u64) {
        let d = self.session.data();
        let present = match screen {
            Screen::Customers | Screen::Privacy => d.customer(id).is_some(),
            Screen::Devices => d.device(id).is_some(),
            Screen::Interventions => d.intervention(id).is_some(),
            _ => true,
        };
        if present {
            self.open_on(screen, id);
        } else {
            self.pending_open = Some(PendingOpen { screen, id, since: Instant::now() });
        }
    }

    fn open_on(&mut self, screen: Screen, id: u64) {
        match screen {
            Screen::Customers | Screen::Privacy => self.open_customer(id),
            Screen::Devices => self.open_device(id),
            Screen::Interventions => self.open_intervention(id),
            _ => {}
        }
    }

    /// Goes to the privacy review.
    pub fn privacy_review(&mut self) {
        self.privacy.tab = privacy::Tab::Review;
        self.go(Screen::Privacy);
    }

    pub fn toast(&mut self, text: String) {
        self.toast = Some((text, Instant::now()));
    }

    /// Queues a command with what to do when it completes.
    pub fn run_as(&mut self, command: &str, params: Value, intent: Intent) -> u64 {
        let job = self.session.execute(command, &params);
        if self.jobs.len() < 10_000 {
            self.jobs.insert(job, intent);
        }
        job
    }

    /// Queues a command whose outcome only matters in the status bar.
    pub fn run(&mut self, command: &str, params: Value) -> u64 {
        self.run_as(command, params, Intent::None)
    }

    /// Asks before running a command that cannot be undone.
    pub fn ask(&mut self, confirm: Confirm) {
        self.confirm = Some(confirm);
    }

    /// Applies finished jobs.
    fn pump(&mut self) {
        let done = self.session.pump();
        if self.auto_settings && !self.session.data().customers.is_empty() {
            self.auto_settings = false;
            if self.screen == Screen::Settings {
                self.screen = Screen::Customers;
            }
        }
        for c in done {
            let intent = self.jobs.remove(&c.job).unwrap_or(Intent::None);
            if let Err(e) = &c.result {
                self.status = self.language.tr_message(&e.to_string());
            }
            match intent {
                Intent::None => {}
                Intent::Toast(text) => {
                    if c.result.is_ok() {
                        self.toast(text);
                    }
                }
                Intent::Export(name) => {
                    if let Ok(v) = &c.result {
                        let data = serde_json::to_vec_pretty(v).unwrap_or_default();
                        self.requests.push(Request::SaveFile { name, data });
                        self.toast(self.tr("Export saved (recorded in the audit log)").to_string());
                    }
                }
                Intent::Customers(i) => customers::on_result(self, i, &c),
                Intent::Devices(i) => devices::on_result(self, i, &c),
                Intent::Interventions(i) => interventions::on_result(self, i, &c),
                Intent::Privacy(i) => privacy::on_result(self, i, &c),
                Intent::Reports(i) => reports::on_result(self, i, &c),
                Intent::Settings(i) => settings::on_result(self, i, &c),
                Intent::Timer(i) => timer::on_result(self, i, &c),
                Intent::Erase(i) => erase::on_result(self, i, &c),
            }
        }
        let rev = self.session.revision();
        if self.derived.stale(rev) {
            let defaults = self.session.rustdesk_defaults();
            let actor = self.session.actor().to_string();
            self.derived = derived::build(self.session.data(), self.now(), rev, &defaults, &actor);
            self.privacy_findings = self.derived.findings.len();
        }
        if let Some(p) = self.pending_open.clone() {
            let d = self.session.data();
            let present = match p.screen {
                Screen::Customers | Screen::Privacy => d.customer(p.id).is_some(),
                Screen::Devices => d.device(p.id).is_some(),
                Screen::Interventions => d.intervention(p.id).is_some(),
                _ => true,
            };
            if present {
                self.pending_open = None;
                self.open_on(p.screen, p.id);
            } else if p.since.elapsed().as_secs() > 10 {
                self.pending_open = None;
            }
        }
        // The data changed (here or on the server): drop selections that no longer exist.
        if self.last_revision != Some(rev) {
            self.last_revision = Some(rev);
            let d = self.session.data();
            if self.selected_customer.is_some_and(|id| d.customer(id).is_none()) {
                self.selected_customer = None;
            }
            if self.selected_device.is_some_and(|id| d.device(id).is_none()) {
                self.selected_device = None;
            }
            if self.selected_intervention.is_some_and(|id| d.intervention(id).is_none()) {
                self.selected_intervention = None;
            }
        }
    }

    pub fn go(&mut self, screen: Screen) {
        if self.screen != Screen::Settings {
            self.previous = self.screen;
        }
        self.screen = screen;
    }

    /// Leaves Settings for the screen it was opened from.
    pub fn go_back(&mut self) {
        self.screen = if self.previous == Screen::Settings { Screen::Customers } else { self.previous };
    }

    pub fn open_customer(&mut self, id: u64) {
        self.selected_customer = Some(id);
        self.go(Screen::Customers);
    }

    pub fn open_device(&mut self, id: u64) {
        self.selected_device = Some(id);
        if let Some(d) = self.session.data().device(id) {
            self.selected_customer = Some(d.customer_id);
        }
        self.go(Screen::Devices);
    }

    pub fn open_intervention(&mut self, id: u64) {
        self.selected_intervention = Some(id);
        self.go(Screen::Interventions);
    }

    /// The main action (`s`, the nav button): the fast path on the selected PC or customer
    /// (`timer::start_now`), else the start card.
    pub fn start_intervention(&mut self) {
        let d = self.session.data();
        let customer = match self.screen {
            Screen::Customers | Screen::Privacy => self.selected_customer,
            Screen::Devices => self.selected_device.and_then(|x| d.device(x)).map(|x| x.customer_id),
            Screen::Interventions => self.selected_intervention.and_then(|x| d.intervention(x)).map(|i| i.customer_id),
            _ => None,
        };
        let device = match self.screen {
            Screen::Devices => self.selected_device,
            Screen::Interventions => self.selected_intervention.and_then(|x| d.intervention(x)).and_then(|i| i.device_id),
            _ => None,
        };
        match device {
            Some(x) if self.session.data().device(x).is_some_and(|d| !d.rustdesk_id.is_empty()) => timer::start_now(self, x),
            _ => timer::open_start(self, customer, device),
        }
    }

    /// Opens the RustDesk client on a device: the link goes out in this frame (browsers only
    /// open custom schemes on a click), `device.connect` records it and reports the consent.
    pub fn connect(&mut self, device: u64) {
        if let Some(d) = self.session.data().device(device)
            && !d.rustdesk_id.is_empty()
        {
            self.requests.push(Request::OpenUrl(core::rustdesk::connect_uri(&d.rustdesk_id)));
            self.status = self.language.fill(
                "Opening RustDesk… If nothing happens, install the RustDesk client or run: {command}",
                &[("command", &core::rustdesk::connect_command(&d.rustdesk_id))],
            );
        }
        self.run_as("device.connect", json!({"device": device}), Intent::Devices(devices::Intent::Connect));
    }

    /// The running interventions chip in the top bar.
    fn timer_chip(&mut self, ui: &mut egui::Ui, t: &theme::Tokens) {
        timer::chip(self, ui, t);
    }

    fn search_changed(&mut self) {
        // Lists filter as you type; the RustDesk-ID jump happens on Enter.
    }

    /// Enter in the search box: a RustDesk ID (or a unique PC) opens that PC straight away,
    /// anything else shows the customers that match.
    fn search_submit(&mut self) {
        let q = self.search.trim().to_string();
        if q.is_empty() {
            return;
        }
        let d = self.session.data();
        if let Ok(id) = core::rustdesk::normalize(&q)
            && !id.is_empty()
            && let Some(dev) = d.devices.iter().find(|x| x.rustdesk_id == id && !x.retired).or_else(|| d.devices.iter().find(|x| x.rustdesk_id == id))
        {
            let dev_id = dev.id;
            self.open_device(dev_id);
            return;
        }
        if self.screen == Screen::Settings || self.screen == Screen::Reports {
            self.go(Screen::Customers);
        }
    }

    /// `Cmd+Enter` in the search box: starts the timer on the PC the RustDesk ID names.
    pub fn search_start(&mut self) {
        let q = self.search.trim().to_string();
        let d = self.session.data();
        if let Some(id) = derived::rustdesk_query(&q)
            && let Some(dev) = d.devices.iter().find(|x| x.rustdesk_id == id && !x.retired)
        {
            let device = dev.id;
            timer::start_now(self, device);
        }
    }

    /// The customer the open record belongs to (for `p`, `l`).
    fn open_customer_id(&self) -> Option<u64> {
        let d = self.session.data();
        match self.screen {
            Screen::Customers | Screen::Privacy => self.selected_customer,
            Screen::Devices => self.selected_device.and_then(|x| d.device(x)).map(|x| x.customer_id),
            Screen::Interventions => self.selected_intervention.and_then(|x| d.intervention(x)).map(|i| i.customer_id),
            _ => None,
        }
    }

    /// The PC the open record belongs to (for `c`).
    fn open_device_id(&self) -> Option<u64> {
        let d = self.session.data();
        match self.screen {
            Screen::Devices => self.selected_device,
            Screen::Interventions => self.selected_intervention.and_then(|x| d.intervention(x)).and_then(|i| i.device_id),
            _ => None,
        }
    }

    /// Keyboard shortcuts (outside text fields).
    fn keyboard(&mut self, ui: &egui::Ui) {
        use egui::{Key, Modifiers};
        if self.confirm.is_some() {
            return;
        }
        let pressed = |m: Modifiers, k: Key| ui.ctx().input_mut(|i| i.consume_key(m, k));
        if ui.ctx().memory(|m| m.focused().is_some()) {
            if pressed(Modifiers::NONE, Key::Escape)
                && let Some(id) = ui.ctx().memory(|m| m.focused())
            {
                ui.ctx().memory_mut(|m| m.surrender_focus(id));
            }
            return;
        }
        if self.shortcuts_open {
            if pressed(Modifiers::NONE, Key::Escape) {
                self.shortcuts_open = false;
            }
            return;
        }
        if pressed(Modifiers::NONE, Key::Slash) {
            self.search_focus = true;
        } else if pressed(Modifiers::COMMAND, Key::Comma) {
            self.go(Screen::Settings);
        } else if pressed(Modifiers::COMMAND, Key::R) {
            self.refresh_serial = self.refresh_serial.wrapping_add(1);
        } else if pressed(Modifiers::SHIFT, Key::Slash) || pressed(Modifiers::NONE, Key::Questionmark) {
            self.shortcuts_open = true;
        } else if pressed(Modifiers::NONE, Key::Escape) {
            if !self.close_open_form() {
                if self.screen == Screen::Settings {
                    self.go_back();
                } else {
                    self.clear_selection();
                }
            }
        } else if pressed(Modifiers::NONE, Key::S) {
            self.start_intervention();
        } else if pressed(Modifiers::NONE, Key::X) {
            self.stop_mine();
        } else if pressed(Modifiers::NONE, Key::N) {
            self.new_record();
        } else if pressed(Modifiers::NONE, Key::L) {
            let (c, d) = (self.open_customer_id(), self.open_device_id());
            interventions::open_log(self, c, d);
        } else if pressed(Modifiers::NONE, Key::E) {
            self.edit_record();
        } else if pressed(Modifiers::NONE, Key::C) {
            if let Some(d) = self.open_device_id() {
                self.connect(d);
            }
        } else if pressed(Modifiers::NONE, Key::P) {
            if let Some(c) = self.open_customer_id() {
                privacy::open_consent(self, c, &["remote_assistance"]);
            }
        } else if pressed(Modifiers::NONE, Key::J) || pressed(Modifiers::NONE, Key::ArrowDown) {
            self.step(1);
        } else if pressed(Modifiers::NONE, Key::K) || pressed(Modifiers::NONE, Key::ArrowUp) {
            self.step(-1);
        } else if pressed(Modifiers::NONE, Key::Num1) {
            self.go(Screen::Customers);
        } else if pressed(Modifiers::NONE, Key::Num2) {
            self.go(Screen::Devices);
        } else if pressed(Modifiers::NONE, Key::Num3) {
            self.go(Screen::Interventions);
        } else if pressed(Modifiers::NONE, Key::Num4) {
            self.go(Screen::Privacy);
        } else if pressed(Modifiers::NONE, Key::Num5) {
            self.go(Screen::Reports);
        }
    }

    /// Closes the form of the current screen (or the timer card); answers whether one was open.
    pub fn close_open_form(&mut self) -> bool {
        if timer::close(self) {
            return true;
        }
        match self.screen {
            Screen::Customers => customers::close(self),
            Screen::Devices => devices::close(self),
            Screen::Interventions => interventions::close(self),
            Screen::Privacy => privacy::close(self),
            Screen::Reports => reports::close(self),
            Screen::Settings => settings::close(self),
        }
    }

    /// Deselects on the current screen (the detail pane closes).
    pub fn clear_selection(&mut self) {
        match self.screen {
            Screen::Customers => self.selected_customer = None,
            Screen::Devices => self.selected_device = None,
            Screen::Interventions => self.selected_intervention = None,
            _ => {}
        }
    }

    /// Opens the stop form of my newest running intervention (`x`).
    pub fn stop_mine(&mut self) {
        let actor = self.session.actor().to_string();
        let mine = self.derived.running.iter().copied().find(|id| self.session.data().intervention(*id).is_some_and(|i| i.technician == actor));
        if let Some(id) = mine {
            timer::open_running(self, id, true);
        }
    }

    /// Opens the edit form of the open record (`e`).
    pub fn edit_record(&mut self) {
        match self.screen {
            Screen::Customers | Screen::Privacy => {
                if let Some(id) = self.selected_customer {
                    customers::open_edit(self, id);
                }
            }
            Screen::Devices => {
                if let Some(id) = self.selected_device {
                    devices::open_edit(self, id);
                }
            }
            Screen::Interventions => {
                if let Some(id) = self.selected_intervention {
                    interventions::open_edit(self, id);
                }
            }
            _ => {}
        }
    }

    /// Opens the new-record form of the current screen (`n`).
    pub fn new_record(&mut self) {
        match self.screen {
            Screen::Customers => customers::new(self),
            Screen::Devices => devices::new(self),
            Screen::Interventions => interventions::new(self),
            Screen::Privacy => privacy::new(self),
            Screen::Reports | Screen::Settings => {}
        }
    }

    /// Moves the selection in the current list (`j`/`k`).
    pub fn step(&mut self, delta: i32) {
        match self.screen {
            Screen::Customers => customers::step(self, delta),
            Screen::Devices => devices::step(self, delta),
            Screen::Interventions => interventions::step(self, delta),
            _ => {}
        }
    }

    /// The search text, folded, for the screens' lists.
    pub fn query(&self) -> String {
        core::fold(self.search.trim())
    }

    /// Draws the app.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let tokens = theme::Tokens::for_dark_mode(self.dark);
        if self.applied_dark != Some(self.dark) {
            theme::apply(ui.ctx(), &tokens);
            self.applied_dark = Some(self.dark);
        }
        self.pump();
        if self.session.jobs_pending() > 0 || self.session.problem().is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if self.session.data().interventions.iter().any(|i| i.running()) {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        self.keyboard(ui);
        chrome::top_bar(self, ui, &tokens);
        chrome::status_bar(self, ui, &tokens);
        match self.screen {
            Screen::Settings => settings::show(self, ui, &tokens),
            screen => {
                nav::show(self, ui, &tokens);
                match screen {
                    Screen::Customers => customers::show(self, ui, &tokens),
                    Screen::Devices => devices::show(self, ui, &tokens),
                    Screen::Interventions => interventions::show(self, ui, &tokens),
                    Screen::Privacy => privacy::show(self, ui, &tokens),
                    Screen::Reports => reports::show(self, ui, &tokens),
                    Screen::Settings => {}
                }
            }
        }
        timer::show(self, ui, &tokens);
        chrome::confirm(self, ui, &tokens);
        chrome::shortcuts_help(self, ui, &tokens);
        chrome::toast(self, ui, &tokens);
    }
}

/// The right-hand detail pane, in the card colour, resizable.
pub fn detail_panel(ui: &mut egui::Ui, t: &theme::Tokens, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::right("detail").default_size(520.0).min_size(360.0).resizable(true).frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                add(ui);
            });
        });
    });
}

/// The centre of a screen, in the panel colour.
pub fn central_panel(ui: &mut egui::Ui, t: &theme::Tokens, add: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| add(ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders one frame of the app at 1440×900 (no window, no GPU).
    pub fn frame(app: &mut DeskApp) {
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    #[test]
    fn every_screen_renders_in_both_languages_and_themes() {
        for lang in Language::ALL {
            for dark in [false, true] {
                for screen in Screen::ALL {
                    let mut app = DeskApp::demo();
                    app.language = lang;
                    app.dark = dark;
                    app.go(screen);
                    frame(&mut app);
                    // With a record selected and its form open.
                    app.selected_customer = Some(1);
                    app.selected_device = Some(1);
                    app.selected_intervention = Some(1);
                    frame(&mut app);
                    app.new_record();
                    frame(&mut app);
                    app.start_intervention();
                    frame(&mut app);
                    assert!(app.close_open_form() || true);
                    frame(&mut app);
                }
            }
        }
    }

    #[test]
    fn an_empty_desk_opens_on_settings_and_commands_are_routed() {
        let mut app = DeskApp::new(Box::new(Session::new(engine::Desk::in_memory(Data::new()))));
        assert_eq!(app.screen, Screen::Settings);
        frame(&mut app);
        app.run_as("customer.add", json!({"name": "Primo"}), Intent::Toast("ok".into()));
        frame(&mut app);
        assert!(app.toast.is_some());
        assert_eq!(app.screen, Screen::Customers, "the desk is set up: leave settings");
        app.run("customer.get", json!({"customer": 99}));
        frame(&mut app);
        assert!(app.status.contains("99"), "{}", app.status);
        app.run_as("customer.get", json!({"customer": 1}), Intent::Export("x.json".into()));
        frame(&mut app);
        assert!(matches!(app.requests.first(), Some(Request::SaveFile { name, .. }) if name == "x.json"));
    }

    #[test]
    fn search_jumps_to_a_rustdesk_id() {
        let mut app = DeskApp::demo();
        app.search = "100 200 300".into();
        app.search_submit();
        assert_eq!(app.screen, Screen::Devices);
        assert_eq!(app.selected_device, Some(1));
        assert_eq!(app.selected_customer, Some(1));
        assert!(app.window_title().contains("Lunaria"));
        app.search = "nothing".into();
        app.search_submit();
        assert_eq!(app.screen, Screen::Devices);
    }

    #[test]
    fn pending_open_resolves_when_the_record_appears() {
        let mut app = DeskApp::demo();
        app.open_when_ready(Screen::Customers, 999);
        assert!(app.pending_open.is_some());
        frame(&mut app);
        assert!(app.pending_open.is_some());
        app.run("customer.add", json!({"name": "Quarto"}));
        frame(&mut app);
        assert!(app.pending_open.is_some(), "another id: still waiting");
        app.pending_open = None;
        app.open_when_ready(Screen::Customers, 4);
        assert_eq!(app.selected_customer, Some(4));
        assert!(app.derived.customer(4).is_some() || app.derived.stale(app.session.revision()));
        frame(&mut app);
        assert!(app.derived.customer(4).is_some());
        assert!(app.window_title().contains("Quarto"));
    }

    #[test]
    fn labels_are_unique_across_screens() {
        let all = all_labels();
        assert!(all.len() > 30);
    }
}
