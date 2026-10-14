//! The customers screen: the list (search, a tag filter, the archived toggle and, when the search
//! is a RustDesk ID, the "Who is {id}?" group of the PCs that carry it), the customer card in the
//! right pane (the privacy strip first, warnings, details, contacts, PCs, interventions, notes)
//! and the new / edit customer form that replaces the card. It acts only through commands
//! (`customer.add`, `customer.update`, `customer.archive`, `customer.remove`, `gdpr.export`,
//! `device.update`); erasure is `erase.rs`, the privacy strip and forms are `privacy.rs`.

use egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use remotecraft_engine::core::privacy::PurposeState;
use remotecraft_engine::core::{Customer, CustomerKind, Data, clip, fold, limits, rustdesk, valid_email};
use serde_json::{Value, json};

use crate::derived::{self, CustomerDerived};
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Tone};
use crate::{Completed, Confirm, DeskApp, Screen, chrome, devices, erase, format, forms, interventions, privacy, timer};

/// The labels this screen shows: `(English, Italian)`.
pub const LABELS_IT: &[(&str, &str)] = &[
    ("New customer", "Nuovo cliente"),
    ("Edit customer", "Modifica cliente"),
    ("Customer saved", "Cliente salvato"),
    ("No customers yet", "Ancora nessun cliente"),
    (
        "Add the first customer, or ask the assistant (MCP) to create one from a description.",
        "Aggiungi il primo cliente, oppure chiedi all'assistente (MCP) di crearlo da una descrizione.",
    ),
    ("No customer matches \"{query}\"", "Nessun cliente corrisponde a \"{query}\""),
    ("Create customer \"{query}\"", "Crea il cliente \"{query}\""),
    ("No customer has the tag {tag}", "Nessun cliente ha il tag {tag}"),
    ("Tag", "Tag"),
    ("Tag: {tag}", "Tag: {tag}"),
    ("Any tag", "Qualsiasi tag"),
    ("Show archived", "Mostra archiviati"),
    ("1 customer", "1 cliente"),
    ("1 PC", "1 PC"),
    ("{n} PCs", "{n} PC"),
    ("Last intervention {date}", "Ultimo intervento {date}"),
    ("Export personal data", "Esporta i dati personali"),
    ("More actions", "Altre azioni"),
    ("Archive", "Archivia"),
    ("Unarchive", "Ripristina"),
    ("Archive {name}?", "Archiviare {name}?"),
    ("The customer leaves the lists; nothing is deleted.", "Il cliente esce dagli elenchi; non viene eliminato niente."),
    ("Customer archived", "Cliente archiviato"),
    ("Customer restored", "Cliente ripristinato"),
    ("Delete customer", "Elimina cliente"),
    ("Delete {name}?", "Eliminare {name}?"),
    ("This cannot be undone.", "Non si può annullare."),
    ("Customer deleted", "Cliente eliminato"),
    (
        "Only a customer without history can be deleted; archive or erase instead",
        "Si può eliminare solo un cliente senza storico: archivialo o cancellane i dati personali",
    ),
    ("Erase personal data…", "Cancella i dati personali…"),
    ("Audit entries", "Voci di audit"),
    (
        "Personal data erased on {date}. Times and the privacy record are kept without names.",
        "Dati personali cancellati il {date}. Tempi e registro privacy restano, senza nomi.",
    ),
    ("1 PC anonymised", "1 PC anonimizzato"),
    ("{n} PCs anonymised", "{n} PC anonimizzati"),
    ("1 intervention kept without text", "1 intervento conservato senza testo"),
    ("{n} interventions kept without text", "{n} interventi conservati senza testo"),
    ("1 privacy event kept", "1 evento privacy conservato"),
    ("{n} privacy events kept", "{n} eventi privacy conservati"),
    ("Details", "Dettagli"),
    ("Contacts", "Contatti"),
    ("Add contact", "Aggiungi contatto"),
    ("Remove contact", "Togli contatto"),
    ("Click to copy", "Clicca per copiare"),
    ("PCs", "PC"),
    ("New PC", "Nuovo PC"),
    ("Show all PCs", "Mostra tutti i PC"),
    ("Notes", "Note"),
    ("Name", "Nome"),
    ("The name is required", "Il nome è obbligatorio"),
    ("Tax ID", "P. IVA / C.F."),
    ("VAT number or tax code", "Partita IVA o codice fiscale"),
    ("Address", "Indirizzo"),
    ("Tags", "Tag"),
    ("Tags, comma-separated", "Tag separati da virgole"),
    ("Role", "Ruolo"),
    ("Created", "Creato"),
    ("Updated", "Aggiornato"),
    ("Not an e-mail address", "Non è un indirizzo e-mail"),
    ("Allow a duplicate (same tax ID or name)", "Consenti un doppione (stessa P. IVA o stesso nome)"),
    ("{name} already has this name or tax ID", "{name} ha già questo nome o questa P. IVA"),
    ("1 intervention · {hours} h · {running} running", "1 intervento · {hours} h · {running} in corso"),
    ("{n} interventions · {hours} h · {running} running", "{n} interventi · {hours} h · {running} in corso"),
    ("Show all", "Mostra tutti"),
    ("No interventions yet", "Ancora nessun intervento"),
    ("Next steps", "Prossimi passi"),
    ("Add a PC", "Aggiungi un PC"),
    ("Record notice and consent", "Registra informativa e consenso"),
    ("No activity since {date}: past the {years}-year retention", "Nessuna attività dal {date}: oltre la conservazione di {years} anni"),
    ("Export and erase…", "Esporta e cancella…"),
    (
        "{device} has unattended access but the unattended-access consent is {state}",
        "{device} ha l'accesso non presidiato ma il consenso all'accesso non presidiato è {state}",
    ),
    ("Not unattended any more", "Rimuovi l'accesso non presidiato"),
    ("Record consent", "Registra consenso"),
    ("Remove the permanent password from the RustDesk client of: {devices}", "Togli la password permanente dal client RustDesk di: {devices}"),
    ("Untick now", "Togli la spunta ora"),
    ("Company", "Azienda"),
    ("Person", "Persona"),
    ("Archived", "Archiviato"),
    ("Erased", "Cancellato"),
    ("Edit", "Modifica"),
    ("Kind", "Tipo"),
    ("Who is {id}?", "Di chi è {id}?"),
    ("No PC has the RustDesk ID {id}", "Nessun PC ha l'ID RustDesk {id}"),
    ("Add a PC with this ID", "Aggiungi un PC con questo ID"),
    ("Notice delivered, remote assistance consent given", "Informativa consegnata, consenso all'assistenza remota dato"),
];

/// The height of a customer row.
const ROW_H: f32 = 56.0;
/// The right column of a row: the date, or the hover actions (four 26 px squares, 30 px apart).
const ACT_W: f32 = 124.0;
/// How many PCs the "Who is {id}?" group lists.
const WHO_MAX: usize = 6;
/// How many interventions the card lists.
const RECENT: usize = 20;
/// How many tag chips a row shows before `+n`.
const ROW_TAGS: usize = 3;
/// Below this window width the card replaces the list: the nav (235 px), the pane (520 px) and
/// a list of about 480 px, the least that shows a row's name, its PCs and its date.
const NARROW: f32 = 1240.0;
/// The width of the form's label column.
const LABEL_W: f32 = 104.0;

/// A contact being edited in the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub name: String,
    pub role: String,
    pub email: String,
    pub phone: String,
}

impl Contact {
    fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.role.trim().is_empty() && self.email.trim().is_empty() && self.phone.trim().is_empty()
    }
}

/// The new / edit customer form, drawn in the right pane in place of the card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    /// The customer edited; `None` for a new one.
    pub id: Option<u64>,
    pub kind: CustomerKind,
    pub name: String,
    pub tax_id: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    /// Comma-separated, as typed.
    pub tags: String,
    pub contacts: Vec<Contact>,
    pub notes: String,
    /// The engine refused a duplicate: the "allow a duplicate" checkbox shows.
    pub duplicate: bool,
    pub allow_duplicate: bool,
    pub error: String,
}

impl Form {
    /// The form for editing a customer.
    pub fn of(c: &Customer) -> Self {
        Self {
            id: Some(c.id),
            kind: c.kind,
            name: c.name.clone(),
            tax_id: c.tax_id.clone(),
            email: c.email.clone(),
            phone: c.phone.clone(),
            address: c.address.clone(),
            tags: c.tags.join(", "),
            contacts: c
                .contacts
                .iter()
                .map(|k| Contact { name: k.name.clone(), role: k.role.clone(), email: k.email.clone(), phone: k.phone.clone() })
                .collect(),
            notes: c.notes.clone(),
            ..Self::default()
        }
    }

    /// The tags typed, lower case, without blanks or repeats.
    fn tag_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for tag in self.tags.split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()) {
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
        out
    }

    /// Whether the form can be sent: a name, and e-mail addresses that look like ones.
    pub fn valid(&self) -> bool {
        !self.name.trim().is_empty() && valid_email(self.email.trim()) && self.contacts.iter().all(|k| valid_email(k.email.trim()))
    }

    /// The parameters of `customer.add` / `customer.update`: every field, contacts and tags
    /// whole (the engine replaces them).
    pub fn params(&self) -> Value {
        let contacts: Vec<Value> = self
            .contacts
            .iter()
            .filter(|k| !k.is_empty())
            .map(|k| json!({"name": k.name.trim(), "role": k.role.trim(), "email": k.email.trim(), "phone": k.phone.trim()}))
            .collect();
        let mut p = json!({
            "name": self.name.trim(),
            "kind": kind_id(self.kind),
            "tax_id": self.tax_id.trim(),
            "email": self.email.trim(),
            "phone": self.phone.trim(),
            "address": self.address.trim(),
            "contacts": contacts,
            "tags": self.tag_list(),
            "notes": self.notes,
        });
        if let Some(o) = p.as_object_mut() {
            if let Some(id) = self.id {
                o.insert("customer".into(), json!(id));
            }
            if self.duplicate && self.allow_duplicate {
                o.insert("allow_duplicate".into(), json!(true));
            }
        }
        p
    }
}

/// What a command queued by this screen was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// `customer.add` / `customer.update` from the form.
    Saved,
    /// `customer.archive`: archived (`true`) or back in the lists.
    Archived(bool),
    /// `customer.remove`.
    Removed,
    /// `device.update {unattended: false}` from a warning on the card.
    Unticked,
}

/// This screen's own state.
#[derive(Default)]
pub struct State {
    pub form: Option<Form>,
    /// The form's command in flight.
    pub busy: Option<u64>,
    pub show_archived: bool,
    /// Only the customers with this tag.
    pub tag: Option<String>,
    /// The ids the list shows, in order (what `j` / `k` walk).
    pub rows: Vec<u64>,
    /// `(revision, folded query, show_archived, tag)` the rows were computed for.
    pub rows_key: Option<(u64, String, bool, Option<String>)>,
    /// After a withdrawal (set by the privacy screen): the customer and the PCs whose permanent
    /// RustDesk password must go. Cleared once none of them is unattended.
    pub unattended_banner: Option<(u64, Vec<u64>)>,
    /// Scroll the list to the selection on the next frame (after `j` / `k`).
    scroll_to: bool,
    /// The list's scroll offset and visible height last frame.
    view: (f32, f32),
}

/// The engine's name of a customer kind.
pub fn kind_id(kind: CustomerKind) -> &'static str {
    match kind {
        CustomerKind::Company => "company",
        CustomerKind::Person => "person",
    }
}

/// The label of a customer kind, to translate.
pub fn kind_label(kind: CustomerKind) -> &'static str {
    match kind {
        CustomerKind::Company => "Company",
        CustomerKind::Person => "Person",
    }
}

fn kind_icon(kind: CustomerKind) -> Icon {
    match kind {
        CustomerKind::Company => Icon::Company,
        CustomerKind::Person => Icon::Customer,
    }
}

/// The label of a consent state, to translate.
fn state_label(state: PurposeState) -> &'static str {
    match state {
        PurposeState::Given => "Given",
        PurposeState::Expired => "Expired",
        PurposeState::Withdrawn => "Withdrawn",
        PurposeState::Missing => "Missing",
    }
}

/// Minutes as hours with one decimal (`5.3`, `5,3` in Italian).
fn hours(minutes: u64, italian: bool) -> String {
    let h = format!("{:.1}", minutes as f64 / 60.0);
    if italian { h.replace('.', ",") } else { h }
}

/// `{n} PCs`, singular for one.
fn pcs_text(app: &DeskApp, n: usize) -> String {
    if n == 1 { app.tr("1 PC").to_string() } else { app.language.fill("{n} PCs", &[("n", &n.to_string())]) }
}

/// The second line of a row: tax id and phone (or e-mail).
fn subtitle(c: &Customer) -> String {
    let second = if c.phone.trim().is_empty() { c.email.trim() } else { c.phone.trim() };
    match (c.tax_id.trim(), second) {
        ("", s) => s.to_string(),
        (tax, "") => tax.to_string(),
        (tax, s) => format!("{tax} · {s}"),
    }
}

/// The ids of the customers to list, sorted by folded name.
fn filtered(data: &Data, fq: &str, rid: Option<&str>, archived: bool, tag: Option<&str>) -> Vec<u64> {
    let mut list: Vec<(String, u64)> = data
        .customers
        .iter()
        .filter(|c| archived || (!c.archived && c.erased_at.is_none()))
        .filter(|c| tag.is_none_or(|t| c.tags.iter().any(|x| x == t)))
        .filter(|c| derived::customer_filter(data, c, fq, rid))
        .map(|c| (fold(&c.name), c.id))
        .collect();
    list.sort();
    list.into_iter().map(|(_, id)| id).collect()
}

/// Recomputes the rows when the data, the search or a filter changed.
fn refresh_rows(app: &mut DeskApp) {
    let key = (app.session.revision(), app.query(), app.customers.show_archived, app.customers.tag.clone());
    if app.customers.rows_key.as_ref() == Some(&key) {
        return;
    }
    let rid = derived::rustdesk_query(&app.search);
    app.customers.rows = filtered(app.data(), &key.1, rid.as_deref(), key.2, key.3.as_deref());
    app.customers.rows_key = Some(key);
}

/// Drops the withdrawal banner once none of its PCs is unattended any more.
fn clear_banner(app: &mut DeskApp) {
    let Some((customer, list)) = app.customers.unattended_banner.clone() else { return };
    let d = app.data();
    let left: Vec<u64> =
        list.into_iter().filter(|id| d.device(*id).is_some_and(|x| x.unattended && !x.retired && x.customer_id == customer)).collect();
    app.customers.unattended_banner = if left.is_empty() { None } else { Some((customer, left)) };
}

// ---------------------------------------------------------------------------------------------
// The shell's entry points

/// Draws the screen: the card (or the form) in the right pane when something is open, then the
/// list. Below [`NARROW`] px of window width the pane replaces the list.
pub fn show(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    refresh_rows(app);
    if app.erase.customer.is_some() && app.erase.customer != app.selected_customer {
        erase::close(app);
    }
    clear_banner(app);
    // A privacy form (opened from a card, the timer card or the review) takes the pane; its
    // customer is the one behind it when nothing else is open.
    let privacy_form = privacy::form_customer(app);
    if app.selected_customer.is_none() && privacy_form.is_some() {
        app.selected_customer = privacy_form;
    }
    let selected = app.selected_customer.is_some_and(|id| app.data().customer(id).is_some());
    let pending = app.pending_open.as_ref().is_some_and(|p| p.screen == Screen::Customers);
    let pane_open = app.customers.form.is_some() || privacy_form.is_some() || selected || pending;
    if pane_open && narrow(ui) {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.card)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("customer-pane").auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::NONE.inner_margin(egui::Margin::symmetric(20, 14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    pane(app, ui, t);
                });
            });
        });
        return;
    }
    if pane_open {
        crate::detail_panel(ui, t, |ui| pane(app, ui, t));
    }
    crate::central_panel(ui, t, |ui| list(app, ui, t));
}

fn narrow(ui: &egui::Ui) -> bool {
    narrow_width(ui.ctx().content_rect().width())
}

/// Whether a window this wide shows the pane alone.
fn narrow_width(width: f32) -> bool {
    width < NARROW
}

/// Applies a finished command of this screen.
pub fn on_result(app: &mut DeskApp, intent: Intent, c: &Completed) {
    match intent {
        Intent::Saved => {
            // Only the form that sent it: on a remote desk the answer may come after the form
            // was closed and another one opened, which a late answer must leave alone.
            let mine = app.customers.busy == Some(c.job);
            if mine {
                app.customers.busy = None;
            }
            match &c.result {
                Ok(v) => {
                    app.toast(app.tr("Customer saved").to_string());
                    if mine {
                        app.customers.form = None;
                        if let Some(id) = forms::new_id(v, "customer") {
                            app.open_when_ready(Screen::Customers, id);
                        }
                    }
                }
                Err(_) if !mine => {}
                Err(e) => {
                    let text = e.to_string();
                    let duplicate = text.contains("already has this name or tax id");
                    let message = if duplicate {
                        let name = text.split('`').nth(1).unwrap_or_default();
                        app.language.fill("{name} already has this name or tax ID", &[("name", name)])
                    } else {
                        app.language.tr_message(&text)
                    };
                    if let Some(f) = app.customers.form.as_mut() {
                        f.duplicate |= duplicate;
                        f.error = message;
                    }
                }
            }
        }
        Intent::Archived(archived) => {
            if c.result.is_ok() {
                app.toast(app.tr(if archived { "Customer archived" } else { "Customer restored" }).to_string());
            }
        }
        Intent::Removed => {
            if let Ok(v) = &c.result {
                if v["removed"].as_u64().is_some_and(|id| app.selected_customer == Some(id)) {
                    app.selected_customer = None;
                }
                app.toast(app.tr("Customer deleted").to_string());
            }
        }
        Intent::Unticked => {
            if c.result.is_ok() {
                app.toast(app.tr("Saved").to_string());
            }
        }
    }
}

/// Closes what is open in the pane: the form, else the erase card, else a privacy form drawn
/// in the pane. Answers whether something was open.
pub fn close(app: &mut DeskApp) -> bool {
    if app.customers.form.take().is_some() {
        app.customers.busy = None;
        return true;
    }
    if erase::close(app) {
        return true;
    }
    privacy::form_customer(app).is_some() && privacy::close(app)
}

/// Opens the new-customer form (`n`).
pub fn new(app: &mut DeskApp) {
    open_new(app, "");
}

/// Opens the new-customer form with `name` prefilled (from the search, the setup guide).
pub fn open_new(app: &mut DeskApp, name: &str) {
    app.customers.form = Some(Form { name: clip(name.trim(), limits::MAX_FIELD).to_string(), ..Form::default() });
    app.customers.busy = None;
    if app.screen != Screen::Customers {
        app.go(Screen::Customers);
    }
}

/// Opens the edit form of a customer (nothing for an erased one).
pub fn open_edit(app: &mut DeskApp, id: u64) {
    let Some(form) = app.data().customer(id).filter(|c| c.erased_at.is_none()).map(Form::of) else { return };
    app.customers.form = Some(form);
    app.customers.busy = None;
    app.selected_customer = Some(id);
    if app.screen != Screen::Customers {
        app.go(Screen::Customers);
    }
}

/// Moves the selection by `delta` rows of the list (the card follows).
pub fn step(app: &mut DeskApp, delta: i32) {
    refresh_rows(app);
    let rows = &app.customers.rows;
    let Some(last) = rows.len().checked_sub(1) else { return };
    let by = delta.unsigned_abs() as usize;
    let i = match app.selected_customer.and_then(|s| rows.iter().position(|r| *r == s)) {
        Some(i) if delta >= 0 => i.saturating_add(by).min(last),
        Some(i) => i.saturating_sub(by),
        None if delta >= 0 => 0,
        None => last,
    };
    app.selected_customer = rows.get(i).copied();
    app.customers.scroll_to = true;
}

// ---------------------------------------------------------------------------------------------
// Actions shared by rows, the card and the header

/// Starts an intervention for a customer: straight away on their only PC with a RustDesk ID
/// (`timer::start_now`), else the start card prefilled with the customer.
fn start(app: &mut DeskApp, customer: u64) {
    let pcs: Vec<u64> =
        app.data().devices.iter().filter(|d| d.customer_id == customer && !d.retired && !d.rustdesk_id.is_empty()).map(|d| d.id).collect();
    match pcs.as_slice() {
        [one] => timer::start_now(app, *one),
        _ => timer::open_start(app, Some(customer), None),
    }
}

/// Opens the new-PC form (`devices::open_new` goes to the Devices screen, where it lives).
fn new_pc(app: &mut DeskApp, customer: Option<u64>, rustdesk_id: &str) {
    devices::open_new(app, customer, rustdesk_id);
}

/// Asks before archiving a customer.
fn ask_archive(app: &mut DeskApp, id: u64) {
    let name = crate::customer_name(app.data(), id);
    app.ask(Confirm {
        title: app.language.fill("Archive {name}?", &[("name", &name)]),
        text: app.tr("The customer leaves the lists; nothing is deleted.").to_string(),
        ok_label: app.tr("Archive").to_string(),
        danger: true,
        command: "customer.archive".into(),
        params: json!({"customer": id, "archived": true}),
        intent: crate::Intent::Customers(Intent::Archived(true)),
    });
}

/// Puts an archived customer back in the lists (nothing to confirm).
fn unarchive(app: &mut DeskApp, id: u64) {
    app.run_as("customer.archive", json!({"customer": id, "archived": false}), crate::Intent::Customers(Intent::Archived(false)));
}

/// Asks before deleting a customer without history.
fn ask_delete(app: &mut DeskApp, id: u64) {
    let name = crate::customer_name(app.data(), id);
    app.ask(Confirm {
        title: app.language.fill("Delete {name}?", &[("name", &name)]),
        text: app.tr("This cannot be undone.").to_string(),
        ok_label: app.tr("Delete").to_string(),
        danger: true,
        command: "customer.remove".into(),
        params: json!({"customer": id}),
        intent: crate::Intent::Customers(Intent::Removed),
    });
}

/// Clears a PC's unattended flag (after the permanent password was removed from its client).
fn untick(app: &mut DeskApp, device: u64) {
    app.run_as("device.update", json!({"device": device, "unattended": false}), crate::Intent::Customers(Intent::Unticked));
}

/// Exports everything the desk holds about a customer; the shell saves the file.
fn export(app: &mut DeskApp, id: u64) {
    let name = erase::export_name(&crate::customer_name(app.data(), id), app.now());
    app.run_as("gdpr.export", json!({"customer": id}), crate::Intent::Export(name));
}

/// Sends the form.
fn submit(app: &mut DeskApp, mut f: Form) {
    let command = if f.id.is_some() { "customer.update" } else { "customer.add" };
    f.error.clear();
    let job = app.run_as(command, f.params(), crate::Intent::Customers(Intent::Saved));
    app.customers.busy = Some(job);
    app.customers.form = Some(f);
}

/// A warning across the pane, like `widgets::banner` (tone fill, a bar in the tone's colour, an
/// icon, wrapped text, small actions at the right; answers the action clicked) but with the bar
/// bounded to the banner and the text wrapped before the actions, so it never widens the pane.
pub(crate) fn banner(ui: &mut egui::Ui, t: &Tokens, tone: Tone, icon: Icon, text: &str, actions: &[&str]) -> Option<usize> {
    widgets::banner(ui, t, tone, icon, text, actions)
}

// ---------------------------------------------------------------------------------------------
// The list

fn list(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    header(app, ui, t);
    who_is(app, ui, t);
    if app.customers.rows.is_empty() {
        empty(app, ui, t);
        return;
    }
    let mut area = egui::ScrollArea::vertical().id_salt("customer-list").auto_shrink([false, false]);
    if std::mem::take(&mut app.customers.scroll_to)
        && let Some(i) = app.selected_customer.and_then(|s| app.customers.rows.iter().position(|r| *r == s))
    {
        let top = i as f32 * ROW_H;
        let (offset, height) = app.customers.view;
        if top < offset {
            area = area.vertical_scroll_offset(top);
        } else if height > ROW_H && top + ROW_H > offset + height {
            area = area.vertical_scroll_offset((top + ROW_H - height).max(0.0));
        }
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let rows = std::mem::take(&mut app.customers.rows);
    let out = area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
        for id in rows.get(range).unwrap_or_default() {
            if row(app, ui, t, *id, true).clicked() {
                app.selected_customer = Some(*id);
            }
        }
    });
    app.customers.rows = rows;
    app.customers.view = (out.state.offset.y, out.inner_rect.height());
}

/// A pill in the header: an optional icon, a text, accent when `on`; `closable` adds a `×`
/// that answers `true` in the second value when clicked.
fn pill(ui: &mut egui::Ui, t: &Tokens, icon: Option<Icon>, text: &str, on: bool, closable: bool) -> (egui::Response, bool) {
    let color = if on { t.accent } else { t.ui_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), color);
    let lead = if icon.is_some() { 22.0 } else { 0.0 };
    let tail = if closable { 18.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 22.0 + lead + tail, 28.0), Sense::click());
    let p = ui.painter();
    let fill = if on {
        t.active
    } else if resp.hovered() {
        t.hover
    } else {
        t.panel
    };
    p.rect_filled(rect, 14.0, fill);
    if !on {
        p.rect_stroke(rect, 14.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    let mut x = rect.left() + 11.0;
    if let Some(i) = icon {
        icons::paint(p, Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(15.0, 15.0)), i, color);
        x += lead;
    }
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    let mut cleared = false;
    if closable {
        let x_rect = Rect::from_center_size(pos2(rect.right() - 15.0, rect.center().y), vec2(18.0, 18.0));
        let xr = ui.interact(x_rect, ui.id().with(("pill-x", text)), Sense::click());
        if xr.hovered() {
            ui.painter().circle_filled(x_rect.center(), 9.0, t.hover);
        }
        icons::paint(ui.painter(), x_rect.shrink(4.0), Icon::Close, color);
        cleared = xr.clicked();
    }
    (resp, cleared)
}

/// The main action at the right end of a wrapped header row (New customer, New notice
/// version): the accent button when the rest of the row fits it, else its icon, on a row of its
/// own when even that does not fit. Answers whether it was clicked.
pub(crate) fn header_action(ui: &mut egui::Ui, t: &Tokens, icon: Icon, label: &str, tip: &str) -> bool {
    let gap = ui.spacing().item_spacing.x;
    // `widgets::primary_button`'s width: the text, the padding and the icon.
    let wide = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(14.0), t.on_accent).size().x + 54.0 + gap;
    // What is left of the row (`available_width` is the whole row's in a wrapping layout).
    if ui.available_size_before_wrap().x < 32.0 + gap {
        ui.end_row();
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.available_width() >= wide {
            widgets::primary_button(ui, t, Some(icon), label).on_hover_text(tip).clicked()
        } else {
            widgets::icon_button(ui, t, icon, label, true, false).clicked()
        }
    })
    .inner
}

/// The header strip: title and count, the tag filter, the archived toggle, New customer.
fn header(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let mut tag_pick: Option<Option<String>> = None;
    let mut toggle = false;
    let mut create = false;
    egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Wrapped: when the pane narrows the list, what does not fit goes to a second line
        // instead of under the pane.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let q = app.search.trim();
            let title =
                if q.is_empty() { app.tr("Customers").to_string() } else { format!("{} · \"{}\"", app.tr("Customers"), format::clipped(q, 32)) };
            ui.add(egui::Label::new(RichText::new(title).strong().color(t.ui_text)).truncate());
            let n = app.customers.rows.len();
            let count = if n == 1 { app.tr("1 customer").to_string() } else { app.language.fill("{n} customers", &[("n", &n.to_string())]) };
            ui.add(egui::Label::new(RichText::new(count).small().color(t.muted)).extend());
            widgets::divider(ui, t);
            let current = app.customers.tag.clone();
            let (resp, cleared) = match &current {
                Some(tag) => pill(ui, t, Some(Icon::Tag), &app.language.fill("Tag: {tag}", &[("tag", tag)]), true, true),
                None => pill(ui, t, Some(Icon::Tag), app.tr("Tag"), false, false),
            };
            if cleared {
                tag_pick = Some(None);
            }
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(180.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    if ui.selectable_label(current.is_none(), app.tr("Any tag")).clicked() {
                        tag_pick = Some(None);
                        ui.close();
                    }
                    for tag in &app.derived.tags {
                        if ui.selectable_label(current.as_ref() == Some(tag), tag).clicked() {
                            tag_pick = Some(Some(tag.clone()));
                            ui.close();
                        }
                    }
                });
            });
            let (resp, _) = pill(ui, t, Some(Icon::Archive), app.tr("Show archived"), app.customers.show_archived, false);
            toggle = resp.clicked();
            create = header_action(ui, t, Icon::Plus, app.tr("New customer"), &app.tip("customer.add"));
        });
    });
    if let Some(tag) = tag_pick {
        app.customers.tag = tag;
    }
    if toggle {
        app.customers.show_archived = !app.customers.show_archived;
    }
    if create {
        open_new(app, "");
    }
}

/// The "Who is {id}?" group: the PCs whose RustDesk ID is the search (retired last).
fn who_is(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    let Some(rid) = derived::rustdesk_query(&app.search) else { return };
    let mut found: Vec<(bool, String, u64)> =
        app.data().devices.iter().filter(|d| d.rustdesk_id == rid).map(|d| (d.retired, fold(&d.name), d.id)).collect();
    found.sort();
    let found: Vec<u64> = found.into_iter().take(WHO_MAX).map(|f| f.2).collect();
    let shown = rustdesk::display(&rid);
    let quick = found.is_empty() && app.search_quick_offer().is_some();
    let mut add = false;
    let mut connect = false;
    let mut open = None;
    egui::Frame::NONE.fill(t.card).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::NONE.inner_margin(egui::Margin { left: 14, right: 12, top: 8, bottom: 6 }).show(ui, |ui| {
            ui.label(RichText::new(app.language.fill("Who is {id}?", &[("id", &shown)])).small().strong().color(t.muted));
        });
        if found.is_empty() {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 14, right: 12, top: 0, bottom: 10 }).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Device, t.muted);
                    ui.label(RichText::new(app.language.fill("No PC has the RustDesk ID {id}", &[("id", &shown)])).color(t.ui_text));
                    if quick && widgets::primary_button(ui, t, Some(Icon::Remote), app.tr("Connect now")).clicked() {
                        connect = true;
                    }
                    if ui.button(app.tr("Add a PC with this ID")).clicked() {
                        add = true;
                    }
                });
            });
        }
        for id in &found {
            if devices::row(app, ui, t, *id, true).clicked() {
                open = Some(*id);
            }
        }
        let r = ui.min_rect();
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
    });
    if connect {
        app.quick_connect(&rid);
    }
    if add {
        new_pc(app, None, &rid);
    }
    if let Some(id) = open {
        app.open_device(id);
    }
}

/// The list's empty state: connecting, an empty desk, no match, no customer with the tag.
fn empty(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if app.session.problem().as_deref() == Some("connecting") && app.session.revision() == 0 {
        widgets::empty_state(ui, t, Icon::Sync, app.tr("Connecting…"), "");
        return;
    }
    let q = app.search.trim().to_string();
    let mut create: Option<String> = None;
    if app.data().customers.is_empty() {
        widgets::empty_state(
            ui,
            t,
            Icon::Customers,
            app.tr("No customers yet"),
            app.tr("Add the first customer, or ask the assistant (MCP) to create one from a description."),
        );
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if widgets::primary_button(ui, t, Some(Icon::Plus), app.tr("New customer")).clicked() {
                create = Some(String::new());
            }
        });
    } else if !q.is_empty() {
        let shown = format::clipped(&q, 60);
        widgets::empty_state(ui, t, Icon::Search, &app.language.fill("No customer matches \"{query}\"", &[("query", &shown)]), "");
        if derived::rustdesk_query(&q).is_none() {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                if widgets::primary_button(ui, t, Some(Icon::Plus), &app.language.fill("Create customer \"{query}\"", &[("query", &shown)])).clicked()
                {
                    create = Some(q.clone());
                }
            });
        }
    } else if let Some(tag) = app.customers.tag.clone() {
        widgets::empty_state(ui, t, Icon::Tag, &app.language.fill("No customer has the tag {tag}", &[("tag", &tag)]), "");
    } else {
        widgets::empty_state(ui, t, Icon::Customers, app.tr("Nothing here yet"), "");
    }
    if let Some(name) = create {
        open_new(app, &name);
    }
}

/// What a row's hover action asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAct {
    Start,
    NewPc,
    Edit,
    Archive,
    Unarchive,
}

/// A customer row (in the list): avatar, name with the kind glyph and tag chips, tax id and
/// phone, the Running chip, the privacy dot (reasons in its tooltip), the PC count and the last
/// intervention. `show_hover` replaces the date with the actions Start intervention, New PC,
/// Edit and Archive while the pointer is over the row. Archived and erased rows are drawn at
/// 70 % alpha. The caller selects on `clicked()`.
pub fn row(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, show_hover: bool) -> egui::Response {
    let selected = app.selected_customer == Some(id);
    let (rect, resp) = widgets::row(ui, t, ROW_H, selected);
    let Some(c) = app.data().customer(id) else { return resp };
    let name = c.name.clone();
    let kind = c.kind;
    let sub = subtitle(c);
    let tags = c.tags.clone();
    let archived = c.archived;
    let erased = c.erased_at.is_some();
    let info = app.derived.customer(id);
    let pcs = info.map_or(0, |i| i.devices);
    let running = info.map_or(0, |i| i.running);
    let last = info.map(|i| i.last_intervention.clone()).unwrap_or_default();
    let dot = info.and_then(|i| i.dot);
    let reasons: Vec<&'static str> = info.map(|i| i.reasons.clone()).unwrap_or_default();
    let hot = show_hover && !erased && ui.rect_contains_pointer(rect);
    if hot && !selected && !resp.hovered() {
        ui.painter().rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - 1.0)), 0.0, t.hover);
    }
    let fade = if archived || erased { 0.7 } else { 1.0 };
    let (ink, muted) = (t.ui_text.gamma_multiply(fade), t.muted.gamma_multiply(fade));
    let p = ui.painter().clone();
    let cy = rect.center().y;
    // Avatar.
    let av = pos2(rect.left() + 28.0, cy);
    p.circle_filled(av, 14.0, if erased { t.muted.gamma_multiply(0.6) } else { chrome::color_for(&name).gamma_multiply(fade) });
    p.text(av, Align2::CENTER_CENTER, format::initials(&name), FontId::proportional(11.0), t.on_accent);
    // The right side, from the edge inwards: date or actions, PCs, privacy dot, Running.
    let right = rect.right() - 12.0;
    let mut edge = right - ACT_W;
    if pcs > 0 {
        p.text(pos2(edge - 10.0, cy), Align2::RIGHT_CENTER, pcs_text(app, pcs), FontId::proportional(12.5), muted);
    }
    edge -= 66.0;
    if let Some((icon, tone)) = dot {
        let r = Rect::from_center_size(pos2(edge - 9.0, cy), vec2(15.0, 15.0));
        icons::paint(&p, r, icon, widgets::tone_colors(t, tone).1.gamma_multiply(fade));
        let tip = if reasons.is_empty() {
            app.tr("Notice delivered, remote assistance consent given").to_string()
        } else {
            reasons.iter().map(|r| app.tr(r)).collect::<Vec<_>>().join("\n")
        };
        ui.interact(r.expand(3.0), ui.id().with(("customer-dot", id)), Sense::hover()).on_hover_text(tip);
    }
    edge -= 26.0;
    if running > 0 {
        let (fill, color) = widgets::tone_colors(t, Tone::Ok);
        let g = p.layout_no_wrap(app.tr("Running").to_string(), FontId::proportional(12.0), color);
        let w = g.size().x + 26.0;
        let r = Rect::from_min_size(pos2(edge - w, cy - 10.0), vec2(w, 20.0));
        p.rect_filled(r, 10.0, fill);
        p.circle_filled(pos2(r.left() + 10.0, cy), 3.0, color);
        p.galley(pos2(r.left() + 18.0, cy - g.size().y / 2.0), g, color);
        edge -= w + 8.0;
    }
    let text_right = edge - 8.0;
    // Name line: kind glyph, name (faux bold), state and tag chips.
    let x0 = rect.left() + 54.0;
    let (y1, y2) = (cy - 9.0, cy + 10.0);
    icons::paint(&p, Rect::from_center_size(pos2(x0 + 7.0, y1), vec2(14.0, 14.0)), kind_icon(kind), muted);
    let nx = x0 + 20.0;
    let avail = (text_right - nx).max(20.0);
    let chips_wanted = archived || erased || !tags.is_empty();
    let g = widgets::truncate(&p, &name, FontId::proportional(14.0), ink, if chips_wanted { avail * 0.62 } else { avail });
    let (gw, gh) = (g.size().x, g.size().y);
    p.galley(pos2(nx, y1 - gh / 2.0), g.clone(), ink);
    p.galley(pos2(nx + 0.5, y1 - gh / 2.0), g, ink);
    let mut chips: Vec<String> = Vec::new();
    if erased {
        chips.push(app.tr("Erased").to_string());
    } else if archived {
        chips.push(app.tr("Archived").to_string());
    }
    chips.extend(tags.iter().take(ROW_TAGS).cloned());
    let mut extra = tags.len().saturating_sub(ROW_TAGS);
    let mut cx = nx + gw + 8.0;
    let (fill, color) = widgets::tone_colors(t, Tone::Neutral);
    for (i, text) in chips.iter().enumerate() {
        let g = p.layout_no_wrap(text.clone(), FontId::proportional(11.5), color.gamma_multiply(fade));
        let w = g.size().x + 14.0;
        if cx + w > text_right {
            extra += chips.len().saturating_sub(i);
            break;
        }
        let r = Rect::from_min_size(pos2(cx, y1 - 9.0), vec2(w, 18.0));
        p.rect_filled(r, 9.0, fill);
        p.rect_stroke(r, 9.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        p.galley(pos2(r.left() + 7.0, y1 - g.size().y / 2.0), g, color);
        cx += w + 4.0;
    }
    if extra > 0 && cx + 26.0 < text_right {
        p.text(pos2(cx + 2.0, y1), Align2::LEFT_CENTER, format!("+{extra}"), FontId::proportional(11.5), muted);
    }
    // Second line.
    if !sub.is_empty() {
        let g = widgets::truncate(&p, &sub, FontId::proportional(12.5), muted, (text_right - x0).max(20.0));
        p.galley(pos2(x0, y2 - g.size().y / 2.0), g, muted);
    }
    // The right column: the hover actions, or the last intervention.
    let mut act = None;
    if hot {
        let mut hx = right;
        let archive = if archived { (Icon::Archive, RowAct::Unarchive, "Unarchive") } else { (Icon::Archive, RowAct::Archive, "Archive") };
        for (icon, a, tip) in
            [archive, (Icon::Edit, RowAct::Edit, "Edit"), (Icon::Plus, RowAct::NewPc, "New PC"), (Icon::Play, RowAct::Start, "Start intervention")]
        {
            let r = Rect::from_center_size(pos2(hx - 13.0, cy), vec2(26.0, 26.0));
            let hr = ui.interact(r, ui.id().with(("customer-act", id, tip)), Sense::click());
            if hr.hovered() {
                ui.painter().rect_filled(r, 6.0, t.active);
            }
            icons::paint(ui.painter(), r.shrink(4.0), icon, t.ui_text);
            if hr.on_hover_text(app.tr(tip)).clicked() {
                act = Some(a);
            }
            hx -= 30.0;
        }
    } else if !last.is_empty() {
        let date = format::short(&last, app.now(), app.italian());
        let r = p.text(pos2(right, cy), Align2::RIGHT_CENTER, date, FontId::proportional(12.5), muted);
        let long = format::long(&last, app.italian());
        ui.interact(r, ui.id().with(("customer-last", id)), Sense::hover())
            .on_hover_text(app.language.fill("Last intervention {date}", &[("date", &long)]));
    }
    match act {
        Some(RowAct::Start) => start(app, id),
        Some(RowAct::NewPc) => new_pc(app, Some(id), ""),
        Some(RowAct::Edit) => open_edit(app, id),
        Some(RowAct::Archive) => ask_archive(app, id),
        Some(RowAct::Unarchive) => unarchive(app, id),
        None => {}
    }
    resp
}

// ---------------------------------------------------------------------------------------------
// The right pane

/// The pane: the customer form, else an open privacy form, else the erase card of the open
/// customer, else the customer card ("Loading…" while a new record is on its way).
fn pane(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) {
    if form_ui(app, ui, t) {
        return;
    }
    if privacy::form_customer(app).is_some() && privacy::form_ui(app, ui, t) {
        return;
    }
    let Some(id) = app.selected_customer.filter(|id| app.data().customer(*id).is_some()) else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(18.0).color(t.accent));
            ui.label(RichText::new(app.tr("Loading…")).color(t.muted));
        });
        return;
    };
    if app.erase.customer == Some(id) && erase::ui(app, ui, t) {
        return;
    }
    card(app, ui, t, id);
}

/// What the card's action bar asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardAct {
    Start,
    Log,
    NewPc,
    Edit,
    Export,
    Archive,
    Unarchive,
    Delete,
    Erase,
    Audit,
    Close,
}

/// The card's action bar: Back (narrow), Start intervention, Log past work, New PC, Edit, Export,
/// More (archive, delete, erase, audit), Close.
fn action_bar(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &Customer, info: Option<&CustomerDerived>) {
    let live = c.erased_at.is_none();
    let can_delete = info.is_some_and(|i| i.interventions == 0 && i.privacy_events == 0);
    let mut act = None;
    let back = narrow(ui);
    ui.horizontal(|ui| {
        if back {
            if widgets::icon_button(ui, t, Icon::Back, app.tr("Back"), true, false).clicked() {
                act = Some(CardAct::Close);
            }
            widgets::divider(ui, t);
        }
        let buttons = [
            (Icon::Play, app.tr("Start intervention").to_string(), CardAct::Start),
            (Icon::Interventions, app.tr("Log past work").to_string(), CardAct::Log),
            (Icon::Plus, app.tr("New PC").to_string(), CardAct::NewPc),
            (Icon::Edit, app.tip("customer.update"), CardAct::Edit),
            (Icon::Export, app.tr("Export personal data").to_string(), CardAct::Export),
        ];
        for (icon, tip, a) in buttons {
            if widgets::icon_button(ui, t, icon, &tip, live, false).clicked() {
                act = Some(a);
            }
        }
        let more = widgets::icon_button(ui, t, Icon::More, app.tr("More actions"), true, false);
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(230.0);
            if live {
                let (label, a) = if c.archived { ("Unarchive", CardAct::Unarchive) } else { ("Archive", CardAct::Archive) };
                if ui.button(app.tr(label)).clicked() {
                    act = Some(a);
                    ui.close();
                }
            }
            let delete = ui
                .add_enabled(can_delete, egui::Button::new(app.tr("Delete customer")))
                .on_disabled_hover_text(app.tr("Only a customer without history can be deleted; archive or erase instead"));
            if delete.clicked() {
                act = Some(CardAct::Delete);
                ui.close();
            }
            if live && ui.button(RichText::new(app.tr("Erase personal data…")).color(widgets::danger(t))).clicked() {
                act = Some(CardAct::Erase);
                ui.close();
            }
            ui.separator();
            if ui.button(app.tr("Audit entries")).clicked() {
                act = Some(CardAct::Audit);
                ui.close();
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !back && widgets::icon_button(ui, t, Icon::Close, app.tr("Close"), true, false).clicked() {
                act = Some(CardAct::Close);
            }
        });
    });
    let id = c.id;
    match act {
        Some(CardAct::Start) => start(app, id),
        Some(CardAct::Log) => interventions::open_log(app, Some(id), None),
        Some(CardAct::NewPc) => new_pc(app, Some(id), ""),
        Some(CardAct::Edit) => open_edit(app, id),
        Some(CardAct::Export) => export(app, id),
        Some(CardAct::Archive) => ask_archive(app, id),
        Some(CardAct::Unarchive) => unarchive(app, id),
        Some(CardAct::Delete) => ask_delete(app, id),
        Some(CardAct::Erase) => erase::open(app, id),
        Some(CardAct::Audit) => privacy::open_audit(app, Some(id), None, None),
        Some(CardAct::Close) => app.selected_customer = None,
        None => {}
    }
}

/// A two-column grid of keys and values (empty values skipped).
fn details_grid(ui: &mut egui::Ui, t: &Tokens, salt: &str, rows: &[(&str, String)]) {
    egui::Grid::new(salt).num_columns(2).spacing([14.0, 6.0]).min_col_width(80.0).show(ui, |ui| {
        for (key, value) in rows.iter().filter(|(_, v)| !v.trim().is_empty()) {
            ui.label(RichText::new(*key).small().color(t.muted));
            ui.add(egui::Label::new(RichText::new(value).color(t.ui_text)).wrap().selectable(true));
            ui.end_row();
        }
    });
}

/// A value that copies itself when clicked; answers whether it was.
fn copy_value(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let r = ui.add(egui::Label::new(RichText::new(text).color(t.accent)).sense(Sense::click())).on_hover_text(app.tr("Click to copy"));
    if r.clicked() {
        ui.ctx().copy_text(text.to_string());
        return true;
    }
    false
}

/// The customer card.
fn card(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64) {
    let Some(c) = app.data().customer(id).cloned() else { return };
    let info = app.derived.customer(id).cloned();
    let live = c.erased_at.is_none();
    action_bar(app, ui, t, &c, info.as_ref());
    ui.add_space(8.0);
    // Title and subtitle.
    ui.add(egui::Label::new(RichText::new(&c.name).size(20.0).strong().color(if live { t.ui_text } else { t.muted })).wrap());
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        widgets::tone_chip(ui, t, app.tr(kind_label(c.kind)), Tone::Neutral);
        if !live {
            widgets::tone_chip(ui, t, app.tr("Erased"), Tone::Neutral);
        } else if c.archived {
            widgets::tone_chip(ui, t, app.tr("Archived"), Tone::Neutral);
        }
        if info.as_ref().is_some_and(|i| i.running > 0) {
            widgets::tone_chip(ui, t, app.tr("Running"), Tone::Ok);
        }
        for tag in c.tags.iter().take(12) {
            widgets::tone_chip(ui, t, tag, Tone::Accent);
        }
        ui.label(RichText::new(format!("#{}", c.id)).small().color(t.muted));
    });
    ui.add_space(10.0);
    if let Some(at) = &c.erased_at {
        erased_banner(app, ui, t, &c, at, info.as_ref());
        privacy::strip(app, ui, t, id);
        interventions_section(app, ui, t, id, info.as_ref());
        return;
    }
    next_steps(app, ui, t, id, info.as_ref());
    warnings(app, ui, t, id, info.as_ref());
    privacy::strip(app, ui, t, id);
    // Details.
    widgets::section(ui, t, app.tr("Details"));
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let italian = app.italian();
        details_grid(
            ui,
            t,
            "customer-details",
            &[
                (app.tr("Tax ID"), c.tax_id.clone()),
                (app.tr("E-mail"), c.email.clone()),
                (app.tr("Phone"), c.phone.clone()),
                (app.tr("Address"), c.address.clone()),
                (app.tr("Created"), format::long(&c.created_at, italian)),
                (app.tr("Updated"), format::long(&c.updated_at, italian)),
            ],
        );
    });
    // Contacts.
    if !c.contacts.is_empty() {
        widgets::section(ui, t, app.tr("Contacts"));
        let mut copied = false;
        widgets::card(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, k) in c.contacts.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&k.name).strong().color(t.ui_text));
                    if !k.role.is_empty() {
                        ui.label(RichText::new(&k.role).color(t.muted));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    copied |= copy_value(app, ui, t, &k.email);
                    copied |= copy_value(app, ui, t, &k.phone);
                });
            }
        });
        if copied {
            app.toast(app.tr("Copied").to_string());
        }
    }
    pcs_section(app, ui, t, id);
    interventions_section(app, ui, t, id, info.as_ref());
    if !c.notes.trim().is_empty() {
        widgets::section(ui, t, app.tr("Notes"));
        widgets::card(t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&c.notes).color(t.ui_text)).wrap().selectable(true));
        });
    }
    ui.add_space(12.0);
}

/// The erased state: the neutral banner and what was kept.
fn erased_banner(app: &DeskApp, ui: &mut egui::Ui, t: &Tokens, c: &Customer, at: &str, info: Option<&CustomerDerived>) {
    let date = format::date(at, app.italian());
    banner(
        ui,
        t,
        Tone::Neutral,
        Icon::Erase,
        &app.language.fill("Personal data erased on {date}. Times and the privacy record are kept without names.", &[("date", &date)]),
        &[],
    );
    let devices = app.data().devices.iter().filter(|d| d.customer_id == c.id).count();
    let (work, events) = info.map_or((0, 0), |i| (i.interventions, i.privacy_events));
    let kept = [
        (devices, "1 PC anonymised", "{n} PCs anonymised"),
        (work, "1 intervention kept without text", "{n} interventions kept without text"),
        (events, "1 privacy event kept", "{n} privacy events kept"),
    ]
    .iter()
    .map(|(n, one, many)| if *n == 1 { app.tr(one).to_string() } else { app.language.fill(many, &[("n", &n.to_string())]) })
    .collect::<Vec<_>>()
    .join(" · ");
    ui.label(RichText::new(kept).small().color(t.muted));
    ui.add_space(6.0);
}

/// For a customer with nothing yet: Add a PC, Record notice and consent.
fn next_steps(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, info: Option<&CustomerDerived>) {
    let Some(i) = info else { return };
    let any_device = app.data().devices.iter().any(|d| d.customer_id == id);
    if any_device || i.privacy_events > 0 || i.interventions > 0 {
        return;
    }
    let mut pc = false;
    let mut consent = false;
    widgets::section(ui, t, app.tr("Next steps"));
    ui.horizontal_wrapped(|ui| {
        pc = widgets::primary_button(ui, t, Some(Icon::Device), app.tr("Add a PC")).clicked();
        consent = widgets::icon_text_button(ui, t, Icon::Signature, app.tr("Record notice and consent"), true).clicked();
    });
    ui.add_space(4.0);
    if pc {
        new_pc(app, Some(id), "");
    }
    if consent {
        privacy::open_consent(app, id, &["remote_assistance"]);
    }
}

/// The warnings above the privacy strip: the PCs to untick after a withdrawal, unattended PCs
/// without the unattended-access consent, the retention period passed.
fn warnings(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, info: Option<&CustomerDerived>) {
    let names = |app: &DeskApp, list: &[u64]| list.iter().map(|d| crate::device_name(app.data(), Some(*d))).collect::<Vec<_>>().join(", ");
    let withdrawn: Vec<u64> = app.customers.unattended_banner.as_ref().filter(|(c, _)| *c == id).map(|(_, l)| l.clone()).unwrap_or_default();
    if !withdrawn.is_empty() {
        let text = app.language.fill("Remove the permanent password from the RustDesk client of: {devices}", &[("devices", &names(app, &withdrawn))]);
        if banner(ui, t, Tone::Bad, Icon::Lock, &text, &[app.tr("Untick now")]) == Some(0) {
            for d in &withdrawn {
                untick(app, *d);
            }
        }
    }
    let state = app.derived.purpose(id, "unattended_access");
    if state != PurposeState::Given {
        let unattended: Vec<u64> = info.map(|i| i.unattended_devices.clone()).unwrap_or_default();
        let word = app.tr(state_label(state)).to_lowercase();
        for d in unattended.iter().filter(|d| !withdrawn.contains(d)).take(5) {
            let text = app.language.fill(
                "{device} has unattended access but the unattended-access consent is {state}",
                &[("device", &crate::device_name(app.data(), Some(*d))), ("state", &word)],
            );
            match banner(ui, t, Tone::Warn, Icon::Lock, &text, &[app.tr("Record consent"), app.tr("Not unattended any more")]) {
                Some(0) => privacy::open_consent(app, id, &["unattended_access"]),
                Some(1) => untick(app, *d),
                _ => {}
            }
        }
    }
    if let Some(since) = info.and_then(|i| i.retention_due.clone()) {
        let years = app.data().preferences.retention_years.to_string();
        let text = app
            .language
            .fill("No activity since {date}: past the {years}-year retention", &[("date", &format::date(&since, app.italian())), ("years", &years)]);
        if banner(ui, t, Tone::Neutral, Icon::History, &text, &[app.tr("Export and erase…")]) == Some(0) {
            erase::open(app, id);
        }
    }
}

/// The customer's PCs (active ones, by name), New PC.
fn pcs_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64) {
    let mut pcs: Vec<(String, u64)> =
        app.data().devices.iter().filter(|d| d.customer_id == id && !d.retired).map(|d| (fold(&d.name), d.id)).collect();
    pcs.sort();
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.tr("PCs")).small().strong().color(t.muted));
        ui.label(RichText::new(pcs.len().to_string()).small().color(t.muted));
    });
    ui.add_space(2.0);
    let mut open = None;
    let mut add = false;
    let mut all = false;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (_, d) in &pcs {
            if devices::row(app, ui, t, *d, false).clicked() {
                open = Some(*d);
            }
        }
        if pcs.is_empty() {
            ui.label(RichText::new(app.tr("Nothing here yet")).color(t.muted));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            add = widgets::icon_text_button(ui, t, Icon::Plus, app.tr("New PC"), true).clicked();
            if !pcs.is_empty() {
                all = widgets::icon_text_button(ui, t, Icon::Device, app.tr("Show all PCs"), true).clicked();
            }
        });
    });
    if let Some(d) = open {
        app.open_device(d);
    }
    if add {
        new_pc(app, Some(id), "");
    }
    if all {
        app.devices.customer = Some(id);
        app.go(Screen::Devices);
    }
}

/// The customer's interventions: totals and the most recent ones; Show all opens the
/// Interventions screen filtered on the customer.
fn interventions_section(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens, id: u64, info: Option<&CustomerDerived>) {
    let mut work: Vec<(&str, u64)> = app.data().interventions.iter().filter(|i| i.customer_id == id).map(|i| (i.started_at.as_str(), i.id)).collect();
    work.sort_by(|a, b| b.cmp(a));
    let recent: Vec<u64> = work.iter().take(RECENT).map(|w| w.1).collect();
    let total = work.len();
    let (minutes, running) = info.map_or((0, 0), |i| (i.minutes, i.running));
    widgets::section(ui, t, app.tr("Interventions"));
    let mut open = None;
    let mut all = false;
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if total == 0 {
            ui.label(RichText::new(app.tr("No interventions yet")).color(t.muted));
            return;
        }
        let text =
            if total == 1 { "1 intervention · {hours} h · {running} running" } else { "{n} interventions · {hours} h · {running} running" };
        let totals =
            app.language.fill(text, &[("n", &total.to_string()), ("hours", &hours(minutes, app.italian())), ("running", &running.to_string())]);
        ui.label(RichText::new(totals).small().color(t.muted));
        ui.add_space(4.0);
        for i in &recent {
            if interventions::row(app, ui, t, *i, false).clicked() {
                open = Some(*i);
            }
        }
        ui.add_space(4.0);
        all = widgets::icon_text_button(ui, t, Icon::Interventions, app.tr("Show all"), true).clicked();
    });
    if let Some(i) = open {
        app.open_intervention(i);
    }
    if all {
        app.work_filter = crate::WorkFilter { customer: Some(id), ..crate::WorkFilter::default() };
        app.go(Screen::Interventions);
    }
}

// ---------------------------------------------------------------------------------------------
// The form

/// A form row like `widgets::field` with a small validation note under the field when `check`
/// has one for the value (ends the row).
fn field_checked(
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &str,
    value: &mut String,
    hint: &str,
    check: impl Fn(&str) -> Option<(String, egui::Color32)>,
) -> egui::Response {
    ui.label(RichText::new(label).color(t.muted));
    let edit = |ui: &mut egui::Ui, value: &mut String| ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(f32::INFINITY));
    let r = match check(value) {
        // Without a note the field sits in the cell like any other row.
        None => edit(ui, value),
        Some((note, color)) => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let r = edit(ui, value);
                ui.label(RichText::new(note).small().color(color));
                r
            })
            .inner
        }
    };
    ui.end_row();
    r
}

/// Draws the new / edit customer form in the pane; answers whether a form is open.
pub fn form_ui(app: &mut DeskApp, ui: &mut egui::Ui, t: &Tokens) -> bool {
    let Some(mut f) = app.customers.form.clone() else { return false };
    let busy = app.customers.busy.is_some();
    let mut action = forms::FormAction::None;
    ui.horizontal(|ui| {
        let title = if f.id.is_some() { "Edit customer" } else { "New customer" };
        ui.label(RichText::new(app.tr(title)).size(20.0).strong().color(t.ui_text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, t, Icon::Close, app.tr("Cancel"), true, false).clicked() {
                action = forms::FormAction::Cancel;
            }
        });
    });
    if let Some(c) = f.id.and_then(|id| app.data().customer(id)) {
        let updated = format::long(&c.updated_at, app.italian());
        ui.label(RichText::new(format!("#{} · {} {}", c.id, app.tr("Updated"), updated)).small().color(t.muted));
    }
    ui.add_space(8.0);
    widgets::card(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("customer-grid").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            let kinds = [(kind_id(CustomerKind::Company), app.tr("Company")), (kind_id(CustomerKind::Person), app.tr("Person"))];
            if let Some(k) = widgets::combo(ui, t, "customer-kind", app.tr("Kind"), kind_id(f.kind), &kinds) {
                f.kind = CustomerKind::parse(&k).unwrap_or_default();
            }
            // "Required" once there is something to say: an edit emptied it, or a save failed.
            let insist = f.id.is_some() || !f.error.is_empty();
            let required = app.tr("The name is required").to_string();
            field_checked(ui, t, &format!("{} *", app.tr("Name")), &mut f.name, "", |v| {
                (insist && v.trim().is_empty()).then(|| (required.clone(), widgets::danger(t)))
            });
            if widgets::field(ui, t, app.tr("Tax ID"), &mut f.tax_id, app.tr("VAT number or tax code")).changed() {
                f.tax_id = f.tax_id.to_uppercase();
            }
            let not_email = app.tr("Not an e-mail address").to_string();
            field_checked(ui, t, app.tr("E-mail"), &mut f.email, "", |v| (!valid_email(v.trim())).then(|| (not_email.clone(), widgets::danger(t))));
            widgets::field(ui, t, app.tr("Phone"), &mut f.phone, "");
            widgets::field(ui, t, app.tr("Address"), &mut f.address, "");
            widgets::field(ui, t, app.tr("Tags"), &mut f.tags, app.tr("Tags, comma-separated"));
        });
        widgets::section(ui, t, app.tr("Contacts"));
        let mut remove = None;
        for (i, k) in f.contacts.iter_mut().enumerate() {
            egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let half = ((ui.available_width() - 48.0) / 2.0).max(60.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut k.name).id_salt(("contact-name", i)).hint_text(app.tr("Name")).desired_width(half));
                    ui.add(egui::TextEdit::singleline(&mut k.role).id_salt(("contact-role", i)).hint_text(app.tr("Role")).desired_width(half));
                    if widgets::icon_button(ui, t, Icon::Close, app.tr("Remove contact"), true, false).clicked() {
                        remove = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut k.email).id_salt(("contact-email", i)).hint_text(app.tr("E-mail")).desired_width(half));
                    ui.add(egui::TextEdit::singleline(&mut k.phone).id_salt(("contact-phone", i)).hint_text(app.tr("Phone")).desired_width(half));
                });
                if !valid_email(k.email.trim()) {
                    ui.label(RichText::new(app.tr("Not an e-mail address")).small().color(widgets::danger(t)));
                }
            });
            ui.add_space(4.0);
        }
        if let Some(i) = remove
            && i < f.contacts.len()
        {
            f.contacts.remove(i);
        }
        if f.contacts.len() < limits::MAX_CONTACTS && widgets::icon_text_button(ui, t, Icon::Plus, app.tr("Add contact"), true).clicked() {
            f.contacts.push(Contact::default());
        }
        ui.add_space(6.0);
        egui::Grid::new("customer-notes").num_columns(2).spacing([12.0, 8.0]).min_col_width(LABEL_W).show(ui, |ui| {
            widgets::multiline(ui, t, app.tr("Notes"), &mut f.notes, "", 6);
        });
        if f.duplicate {
            ui.add_space(4.0);
            ui.checkbox(&mut f.allow_duplicate, app.tr("Allow a duplicate (same tax ID or name)"));
        }
        ui.add_space(4.0);
        widgets::error_line(ui, t, &f.error);
        let submitted = forms::submit_row(app, ui, t, app.tr("Save"), busy, f.valid());
        if submitted != forms::FormAction::None {
            action = submitted;
        }
    });
    match action {
        forms::FormAction::Save => submit(app, f),
        forms::FormAction::Cancel => {
            app.customers.form = None;
            app.customers.busy = None;
        }
        forms::FormAction::None => app.customers.form = Some(f),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    /// The demo app in English (the demo desk's language is Italian).
    fn demo() -> DeskApp {
        let mut app = DeskApp::demo();
        app.language = Language::En;
        app
    }

    /// Renders one frame of the app at `w`×900 (no window, no GPU).
    fn frame_at(app: &mut DeskApp, w: f32) {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(w, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }

    fn frame(app: &mut DeskApp) {
        frame_at(app, 1440.0);
    }

    #[test]
    fn the_screen_renders_with_a_card_a_form_and_every_filter() {
        for lang in Language::ALL {
            for dark in [false, true] {
                let mut app = demo();
                app.language = lang;
                app.dark = dark;
                app.go(Screen::Customers);
                frame(&mut app);
                for id in [1, 2, 3] {
                    app.selected_customer = Some(id);
                    frame(&mut app);
                }
                app.customers.unattended_banner = Some((1, vec![1, 2]));
                app.selected_customer = Some(1);
                frame(&mut app);
                open_edit(&mut app, 1);
                frame(&mut app);
                assert!(app.customers.form.as_ref().is_some_and(|f| f.id == Some(1) && f.contacts.len() == 1));
                close(&mut app);
                open_new(&mut app, "Nuovo");
                frame(&mut app);
                close(&mut app);
                erase::open(&mut app, 3);
                frame(&mut app);
                close(&mut app);
                for search in ["100 200 300", "999 888 777", "nessuno", "lunaria"] {
                    app.search = search.into();
                    frame(&mut app);
                }
                app.search.clear();
                app.customers.tag = Some("negozio".into());
                app.customers.show_archived = true;
                frame(&mut app);
                app.customers.tag = Some("nobody-has-it".into());
                frame(&mut app);
                // Narrow: the card replaces the list.
                app.customers.tag = None;
                frame_at(&mut app, 900.0);
            }
        }
    }

    #[test]
    fn rows_follow_the_search_the_tag_and_the_archived_toggle() {
        let mut app = demo();
        refresh_rows(&mut app);
        assert_eq!(app.customers.rows, vec![3, 2, 1], "sorted by folded name");
        app.search = "100 200 300".into();
        refresh_rows(&mut app);
        assert_eq!(app.customers.rows, vec![1], "a RustDesk ID finds the owner");
        app.search.clear();
        app.customers.tag = Some("negozio".into());
        refresh_rows(&mut app);
        assert_eq!(app.customers.rows, vec![2]);
        app.customers.tag = None;
        app.run("customer.archive", json!({"customer": 2}));
        frame(&mut app);
        assert_eq!(app.customers.rows, vec![3, 1], "archived customers leave the list");
        app.customers.show_archived = true;
        refresh_rows(&mut app);
        assert_eq!(app.customers.rows, vec![3, 2, 1]);
    }

    #[test]
    fn j_and_k_walk_the_visible_rows() {
        let mut app = demo();
        step(&mut app, 1);
        assert_eq!(app.selected_customer, Some(3));
        step(&mut app, 1);
        assert_eq!(app.selected_customer, Some(2));
        step(&mut app, 5);
        assert_eq!(app.selected_customer, Some(1), "clamped at the end");
        step(&mut app, -1);
        assert_eq!(app.selected_customer, Some(2));
        app.selected_customer = None;
        step(&mut app, -1);
        assert_eq!(app.selected_customer, Some(1), "up from nothing starts at the end");
        frame(&mut app);
        app.search = "zzz".into();
        app.selected_customer = None;
        step(&mut app, 1);
        assert_eq!(app.selected_customer, None, "nothing to walk");
    }

    #[test]
    fn the_form_saves_a_new_customer_and_opens_it() {
        let mut app = demo();
        frame(&mut app);
        open_new(&mut app, "  Officina Rapida ");
        let mut f = app.customers.form.clone().unwrap();
        assert_eq!(f.name, "Officina Rapida");
        f.kind = CustomerKind::Person;
        f.tax_id = "rpdmra80a01h501u".to_uppercase();
        f.tags = "Officina, nuovo, officina".into();
        f.contacts = vec![
            Contact { name: "Mara".into(), role: "Titolare".into(), email: "mara@rapida.example".into(), phone: String::new() },
            Contact::default(),
        ];
        assert!(f.valid());
        let p = f.params();
        assert_eq!(p["tags"], json!(["officina", "nuovo"]));
        assert_eq!(p["contacts"].as_array().map(Vec::len), Some(1), "empty contact rows are dropped");
        assert_eq!(p["kind"], "person");
        assert!(p.get("customer").is_none() && p.get("allow_duplicate").is_none());
        submit(&mut app, f);
        assert!(app.customers.busy.is_some());
        frame(&mut app);
        assert!(app.customers.form.is_none(), "closed on success");
        assert!(app.customers.busy.is_none());
        assert!(app.toast.as_ref().is_some_and(|(text, _)| text == "Customer saved"));
        let id = app.selected_customer.unwrap();
        let c = app.data().customer(id).unwrap();
        assert_eq!(c.name, "Officina Rapida");
        assert_eq!(c.kind, CustomerKind::Person);
        assert_eq!(c.contacts.len(), 1);
        assert_eq!(c.tags, vec!["officina", "nuovo"]);
    }

    #[test]
    fn a_duplicate_is_refused_then_allowed() {
        let mut app = demo();
        app.language = Language::It;
        frame(&mut app);
        open_new(&mut app, "Corvo bis");
        let mut f = app.customers.form.clone().unwrap();
        f.tax_id = "IT09876543210".into();
        submit(&mut app, f);
        frame(&mut app);
        let f = app.customers.form.clone().unwrap();
        assert!(f.duplicate, "the checkbox shows");
        assert!(f.error.contains("Ferramenta Corvo"), "{}", f.error);
        assert!(f.error.contains("P. IVA"), "translated: {}", f.error);
        let mut f = f;
        f.allow_duplicate = true;
        assert_eq!(f.params()["allow_duplicate"], true);
        submit(&mut app, f);
        frame(&mut app);
        assert!(app.customers.form.is_none());
        assert_eq!(app.data().customers.iter().filter(|c| c.tax_id == "IT09876543210").count(), 2);
    }

    #[test]
    fn editing_sends_every_field_and_errors_keep_the_form_open() {
        let mut app = demo();
        frame(&mut app);
        open_edit(&mut app, 1);
        let mut f = app.customers.form.clone().unwrap();
        f.phone = "+39 0471 999999".into();
        f.contacts.clear();
        assert_eq!(f.params()["customer"], 1);
        submit(&mut app, f);
        frame(&mut app);
        assert!(app.customers.form.is_none());
        let c = app.data().customer(1).unwrap();
        assert_eq!(c.phone, "+39 0471 999999");
        assert!(c.contacts.is_empty(), "contacts are sent whole");
        assert_eq!(c.tags, vec!["contratto", "studio"], "tags kept");
        // A refusal: the form stays open with the message.
        open_edit(&mut app, 2);
        let mut f = app.customers.form.clone().unwrap();
        f.email = "not an address".into();
        assert!(!f.valid());
        submit(&mut app, f);
        frame(&mut app);
        let f = app.customers.form.clone().unwrap();
        assert!(f.error.contains("e-mail"), "{}", f.error);
        assert!(!f.duplicate);
        assert!(close(&mut app));
        assert!(!close(&mut app));
    }

    #[test]
    fn archive_asks_first_and_delete_needs_an_empty_history() {
        let mut app = demo();
        frame(&mut app);
        ask_archive(&mut app, 2);
        let c = app.confirm.clone().unwrap();
        assert_eq!(c.command, "customer.archive");
        assert!(c.danger);
        app.confirm = None;
        app.run_as(&c.command, c.params, c.intent);
        frame(&mut app);
        assert!(app.data().customer(2).unwrap().archived);
        assert!(app.toast.as_ref().is_some_and(|(text, _)| text == "Customer archived"));
        unarchive(&mut app, 2);
        frame(&mut app);
        assert!(!app.data().customer(2).unwrap().archived);
        // A customer without history can be deleted.
        app.run("customer.add", json!({"name": "Prova"}));
        frame(&mut app);
        let id = app.data().customers.iter().find(|c| c.name == "Prova").unwrap().id;
        app.selected_customer = Some(id);
        frame(&mut app);
        let info = app.derived.customer(id).unwrap();
        assert_eq!((info.interventions, info.privacy_events), (0, 0));
        ask_delete(&mut app, id);
        let c = app.confirm.take().unwrap();
        app.run_as(&c.command, c.params, c.intent);
        frame(&mut app);
        assert!(app.data().customer(id).is_none());
        assert_eq!(app.selected_customer, None);
    }

    #[test]
    fn the_withdrawal_banner_unticks_and_clears() {
        let mut app = demo();
        app.selected_customer = Some(1);
        app.customers.unattended_banner = Some((1, vec![1, 2, 99]));
        frame(&mut app);
        assert_eq!(app.customers.unattended_banner, Some((1, vec![1, 2])), "unknown PCs dropped");
        untick(&mut app, 1);
        untick(&mut app, 2);
        frame(&mut app);
        frame(&mut app);
        assert!(!app.data().device(1).unwrap().unattended);
        assert_eq!(app.customers.unattended_banner, None, "cleared once nothing is unattended");
    }

    #[test]
    fn start_uses_the_only_pc_with_an_id_or_opens_the_card() {
        let mut app = demo();
        frame(&mut app);
        start(&mut app, 2);
        assert!(app.requests.iter().any(|r| matches!(r, crate::Request::OpenUrl(u) if u.contains("200300400"))), "{:?}", app.requests);
        frame(&mut app);
        assert!(app.data().interventions.iter().any(|i| i.running() && i.customer_id == 2));
        let mut app = demo();
        start(&mut app, 1);
        assert!(app.timer.card.is_some(), "two PCs with an ID: the card opens");
    }

    #[test]
    fn export_from_the_card_saves_a_file() {
        let mut app = demo();
        frame(&mut app);
        export(&mut app, 2);
        frame(&mut app);
        assert!(app.requests.iter().any(|r| matches!(r, crate::Request::SaveFile { name, .. } if name.starts_with("export-ferramenta-corvo-"))));
    }

    #[test]
    fn a_late_answer_leaves_the_next_form_alone() {
        // On a remote desk the answer may come after the form was closed and another opened.
        let mut app = demo();
        frame(&mut app);
        open_new(&mut app, "Officina A");
        let f = app.customers.form.clone().unwrap();
        submit(&mut app, f);
        assert!(close(&mut app), "Esc before the answer");
        open_new(&mut app, "Officina B");
        app.selected_customer = Some(2);
        frame(&mut app);
        assert!(app.data().customers.iter().any(|c| c.name == "Officina A"), "A was saved");
        assert!(app.toast.as_ref().is_some_and(|(text, _)| text == "Customer saved"));
        let f = app.customers.form.clone().expect("B's form stays open");
        assert_eq!(f.name, "Officina B");
        assert!(f.error.is_empty());
        assert_eq!(app.selected_customer, Some(2), "no jump to A");
        assert!(app.pending_open.is_none());
        // A late refusal does not land in B's form either.
        open_edit(&mut app, 2);
        let mut f = app.customers.form.clone().unwrap();
        f.email = "not an address".into();
        submit(&mut app, f);
        open_new(&mut app, "Officina C");
        frame(&mut app);
        let f = app.customers.form.clone().expect("C's form stays open");
        assert_eq!(f.name, "Officina C");
        assert!(f.error.is_empty() && !f.duplicate, "{}", f.error);
        // The form that sent it still gets its answer.
        let f = Form { email: "not an address".into(), ..f };
        submit(&mut app, f);
        frame(&mut app);
        assert!(app.customers.form.as_ref().is_some_and(|f| f.error.contains("e-mail")));
    }

    /// The header's texts at `w` (the title, the count, the pills, New customer), each drawn
    /// whole and none over another.
    fn header_is_whole(app: &mut DeskApp, w: f32) {
        use crate::privacy::tests::{texts_at, whole};
        let texts = texts_at(app, w);
        let title = app.tr("Customers").to_string();
        let mut labels: Vec<String> = [app.tr("Tag"), app.tr("Show archived"), app.tr("New customer")].iter().map(|s| s.to_string()).collect();
        labels.extend(
            (0..=3).map(|n| if n == 1 { app.tr("1 customer").to_string() } else { app.language.fill("{n} customers", &[("n", &n.to_string())]) }),
        );
        let header: Vec<&(String, Rect, Rect)> = texts.iter().filter(|(s, _, _)| s.starts_with(&title) || labels.contains(s)).collect();
        for label in [app.tr("Tag"), app.tr("Show archived")] {
            assert!(header.iter().any(|(s, _, _)| s == label), "{w}: {label} is drawn: {texts:?}");
        }
        for (i, a) in header.iter().enumerate() {
            assert!(whole(a), "{w}: {a:?} is cut off");
            for b in header.iter().skip(i + 1) {
                assert!(!a.1.shrink(1.0).intersects(b.1.shrink(1.0)), "{w}: {a:?} over {b:?}");
            }
        }
    }

    #[test]
    fn the_header_fits_next_to_the_card() {
        for lang in Language::ALL {
            let mut app = demo();
            app.language = lang;
            app.selected_customer = Some(1);
            for search in ["", "studio", "studio tecnico lunaria e altro testo"] {
                app.search = search.into();
                for w in [1240.0, 1300.0, 1440.0, 1800.0] {
                    header_is_whole(&mut app, w);
                }
            }
            // Narrower, the card replaces the list (whose names, PCs and dates would not fit).
            app.search.clear();
            for w in [1000.0, 1200.0] {
                let texts = crate::privacy::tests::texts_at(&mut app, w);
                assert!(crate::privacy::tests::find(&texts, app.tr("Show archived")).is_none(), "{lang:?} at {w}: no list");
                assert!(crate::privacy::tests::find(&texts, "Studio Tecnico Lunaria").is_some_and(crate::privacy::tests::whole), "{lang:?} at {w}");
            }
            assert!(narrow_width(1239.0) && !narrow_width(1240.0));
        }
    }

    #[test]
    fn counts_of_one_are_singular() {
        use crate::privacy::tests::{find, texts_in};
        let texts_at = |app: &mut DeskApp, w: f32| texts_in(app, w, 1800.0);
        let mut app = demo();
        app.selected_customer = Some(3);
        let texts = texts_at(&mut app, 1440.0);
        assert!(find(&texts, "1 intervention · 1.5 h · 0 running").is_some(), "{texts:?}");
        app.language = Language::It;
        let texts = texts_at(&mut app, 1440.0);
        assert!(find(&texts, "1 intervento · 1,5 h · 0 in corso").is_some(), "{texts:?}");
        // The erased card: what was kept, each count with its own noun.
        app.run("gdpr.erase", json!({"customer": 3, "confirm": true, "reason": "richiesta"}));
        frame(&mut app);
        app.language = Language::En;
        assert_eq!(app.derived.customer(3).map(|i| i.privacy_events), Some(1), "Elio has one privacy event");
        let texts = texts_at(&mut app, 1440.0);
        let kept = "1 PC anonymised · 1 intervention kept without text · 1 privacy event kept";
        assert!(find(&texts, kept).is_some(), "{texts:?}");
        app.language = Language::It;
        let texts = texts_at(&mut app, 1440.0);
        let kept = "1 PC anonimizzato · 1 intervento conservato senza testo · 1 evento privacy conservato";
        assert!(find(&texts, kept).is_some(), "{texts:?}");
        // And the plural, for a customer with more of each.
        app.run("gdpr.erase", json!({"customer": 1, "confirm": true, "reason": "richiesta"}));
        frame(&mut app);
        app.selected_customer = Some(1);
        let texts = texts_at(&mut app, 1440.0);
        assert!(
            texts.iter().any(|(s, _, _)| s.starts_with("3 PC anonimizzati · ") && s.contains(" interventi conservati senza testo · ")),
            "{texts:?}"
        );
    }

    #[test]
    fn labels_are_unique_and_translated() {
        let mut seen = Vec::new();
        for (en, it) in LABELS_IT {
            assert!(!en.is_empty() && !it.is_empty(), "{en}");
            assert!(!seen.contains(en), "duplicate {en}");
            assert!(!crate::i18n::LABELS.contains(en), "{en} is already shared");
            seen.push(*en);
        }
        assert_eq!(Language::It.tr("New customer"), "Nuovo cliente");
        assert_eq!(subtitle(&Customer { tax_id: "IT1".into(), phone: "333".into(), ..Customer::default() }), "IT1 · 333");
        assert_eq!(subtitle(&Customer { email: "a@b.it".into(), ..Customer::default() }), "a@b.it");
        assert_eq!(hours(90, true), "1,5");
        assert_eq!(hours(90, false), "1.5");
    }
}
