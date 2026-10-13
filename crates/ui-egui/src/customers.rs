//! The customers screen (to be implemented on the shell's API).

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
        ui.label(RichText::new(app.tr("Customers")).color(t.muted));
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

/// Opens the new-customer form with `name` prefilled.
pub fn open_new(app: &mut DeskApp, name: &str) {
    let _ = (app, name);
}

/// Opens the edit form of a customer.
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let _ = (app, id);
}

/// A customer row (in lists and pickers); `show_hover` adds the hover actions.
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, show_hover: bool) -> egui::Response {
    let _ = (app, t, id, show_hover);
    ui.label("")
}
