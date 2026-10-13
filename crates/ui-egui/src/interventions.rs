//! The interventions screen (to be implemented on the shell's API).

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
pub struct State {}

pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    crate::central_panel(ui, t, |ui| {
        ui.label(RichText::new(app.tr("Interventions")).color(t.muted));
    });
}

pub fn on_result(_app: &mut DeskApp, _intent: Intent, _c: &Completed) {}

/// Closes an open form; answers whether one was open.
pub fn close(_app: &mut DeskApp) -> bool {
    false
}

/// Opens the new-record form.
pub fn new(_app: &mut DeskApp) {}

/// Moves the selection by `delta` rows.
pub fn step(_app: &mut DeskApp, _delta: i32) {}

/// Opens the log-past-work form, prefilled with a customer and/or a PC.
pub fn open_log(app: &mut DeskApp, customer: Option<u64>, device: Option<u64>) {
    let _ = (app, customer, device);
}

/// Opens the edit form of an intervention.
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let _ = (app, id);
}

/// An intervention row (in the list and in cards); `with_customer` names the customer too.
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, with_customer: bool) -> egui::Response {
    let _ = (app, t, id, with_customer);
    ui.label("")
}
