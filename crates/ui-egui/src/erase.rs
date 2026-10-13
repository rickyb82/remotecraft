//! Erasing a customer's personal data (GDPR art. 17): the only caller of `gdpr.erase`. To be
//! implemented: the red-bordered card replacing the customer card (export first, reason, the
//! typed name, the running-timer block).

use crate::theme::Tokens;
use crate::{Completed, DeskApp};

/// The labels this card shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[];

/// What a command queued by this card was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Exported,
    Erased,
}

/// The card's state.
#[derive(Default)]
pub struct State {
    pub customer: Option<u64>,
}

/// Opens the erase card for a customer.
pub fn open(app: &mut DeskApp, customer: u64) {
    app.erase.customer = Some(customer);
}

/// Draws the card in a customer's pane; answers whether it is open for that customer.
pub fn ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let _ = (ui, t);
    app.erase.customer.is_some()
}

pub fn on_result(_app: &mut DeskApp, _intent: Intent, _c: &Completed) {}

/// Closes the card; answers whether it was open.
pub fn close(app: &mut DeskApp) -> bool {
    app.erase.customer.take().is_some()
}
