//! The devices screen (to be implemented on the shell's API).

use egui::RichText;

use crate::theme::Tokens;
use crate::{Completed, DeskApp};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `device.connect` answered: open the link.
    Connect,
}

/// This screen's own state (forms, loaded results).
#[derive(Default)]
pub struct State {}

pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    crate::central_panel(ui, t, |ui| {
        ui.label(RichText::new(app.tr("Devices")).color(t.muted));
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

/// Opens the new-PC form for a customer (picker when `None`) with a RustDesk ID prefilled.
pub fn open_new(app: &mut DeskApp, customer: Option<u64>, rustdesk_id: &str) {
    let _ = (app, customer, rustdesk_id);
}

/// Opens the edit form of a PC.
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let _ = (app, id);
}

/// A PC row (in the list and in customer cards); `with_customer` names the customer too.
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, with_customer: bool) -> egui::Response {
    let _ = (app, t, id, with_customer);
    ui.label("")
}
