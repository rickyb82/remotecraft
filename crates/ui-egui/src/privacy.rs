//! The privacy screen (to be implemented on the shell's API).

use egui::RichText;

use crate::theme::Tokens;
use crate::{Completed, DeskApp};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Placeholder,
}

/// This screen's own state (forms, loaded results).
#[derive(Default)]
pub struct State {
    pub tab: Tab,
}

pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    crate::central_panel(ui, t, |ui| {
        ui.label(RichText::new(app.tr("Privacy")).color(t.muted));
    });
}

pub fn on_result(_app: &mut DeskApp, _intent: Intent, _c: &Completed) {}

/// Closes an open form; answers whether one was open.
pub fn close(_app: &mut DeskApp) -> bool {
    false
}

/// Opens the new-record form.
pub fn new(_app: &mut DeskApp) {}

/// The tabs of the Privacy screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Review,
    Notices,
    Audit,
}

/// The privacy strip at the top of a customer card: the notice line and the purposes.
pub fn strip(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, customer: u64) {
    let _ = (app, ui, t, customer);
}

/// Opens the consent form for a customer with purposes prechecked.
pub fn open_consent(app: &mut DeskApp, customer: u64, purposes: &[&str]) {
    let _ = (app, customer, purposes);
}

/// Opens the notice-delivery form for a customer (the current version when `None`).
pub fn open_delivery(app: &mut DeskApp, customer: u64, version: Option<String>) {
    let _ = (app, customer, version);
}

/// Opens the withdrawal form for a customer with a purpose prechecked.
pub fn open_withdraw(app: &mut DeskApp, customer: u64, purpose: &str) {
    let _ = (app, customer, purpose);
}

/// Opens the new-notice-version form.
pub fn open_notice(app: &mut DeskApp) {
    let _ = app;
}

/// Draws the open privacy form (in a card's pane); answers whether one is open.
pub fn form_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let _ = (app, ui, t);
    false
}

/// The customer whose privacy form is open, if any.
pub fn form_customer(app: &DeskApp) -> Option<u64> {
    let _ = app;
    None
}
