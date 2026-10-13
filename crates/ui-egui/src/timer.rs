//! The docked intervention card and the running-timer chip (to be implemented).

use crate::theme::Tokens;
use crate::{Completed, DeskApp};

/// Labels this screen translates.
pub const LABELS: &[&str] = &[];

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Placeholder,
}

/// This screen's own state (forms, loaded results).
#[derive(Default)]
pub struct State {}

/// The docked card (drawn after the screens, like MailCraft's compose card).
pub fn show(_app: &mut DeskApp, _ui: &mut egui::Ui, _t: &Tokens) {}

pub fn on_result(_app: &mut DeskApp, _intent: Intent, _c: &Completed) {}

/// Closes an open form; answers whether one was open.
pub fn close(_app: &mut DeskApp) -> bool {
    false
}

/// Opens the start card for a customer and/or a device.
pub fn open_start(_app: &mut DeskApp, _customer: Option<u64>, _device: Option<u64>) {}

/// The running-interventions chip in the top bar.
pub fn chip(_app: &mut DeskApp, _ui: &mut egui::Ui, _t: &Tokens) {}
