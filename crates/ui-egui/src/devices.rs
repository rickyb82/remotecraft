//! The devices screen (to be implemented on the shell's API).

use egui::RichText;

use crate::theme::Tokens;
use crate::{Completed, DeskApp};

/// Labels this screen translates.
pub const LABELS: &[&str] = &[];

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
