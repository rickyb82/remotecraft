//! The demo desk: invented customers, PCs and interventions for tests, `--demo` and
//! screenshots. Every name, ID and note here is made up.

use chrono::{DateTime, Duration, Utc};

use crate::model::{Contact, Customer, CustomerKind, Data, Device, Intervention, InterventionKind};
use crate::privacy::{Method, Notice, PrivacyEvent, PrivacyKind};
use crate::time::stamp;

const NOTICE: &str = "Informativa di esempio per la demo di RemoteCraft. Sostituiscila con il testo preparato per la tua attività: \
chi è il titolare, quali dati tratti (anagrafica, dispositivi, interventi), per quali finalità e con quale base giuridica, \
per quanto tempo li conservi, a chi li comunichi e come si esercitano i diritti degli articoli 15-22 del GDPR.";

/// A desk with three customers, five devices, a dozen interventions and their privacy records,
/// dated relative to `now`.
pub fn demo(now: DateTime<Utc>) -> Data {
    let ago = |days: i64, hour: i64| stamp(now - Duration::days(days) - Duration::hours(hour));
    let mut d = Data::new();
    d.preferences.company_name = "Assistenza Demo".into();
    d.preferences.rustdesk_host = "rustdesk.example.net".into();

    let customer = |id, kind, name: &str, tax: &str, email: &str, phone: &str, address: &str, tags: &[&str]| Customer {
        id,
        kind,
        name: name.into(),
        tax_id: tax.into(),
        email: email.into(),
        phone: phone.into(),
        address: address.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        created_at: stamp(now - Duration::days(400)),
        updated_at: stamp(now - Duration::days(30)),
        ..Customer::default()
    };
    let mut studio = customer(
        1,
        CustomerKind::Company,
        "Studio Tecnico Lunaria",
        "IT01234567890",
        "segreteria@lunaria.example",
        "+39 0471 000001",
        "Via dei Mulini 4, Borgo Nuovo",
        &["contratto", "studio"],
    );
    studio.contacts = vec![Contact {
        name: "Marta Selva".into(),
        role: "Segreteria".into(),
        email: "marta@lunaria.example".into(),
        phone: "+39 333 0000001".into(),
    }];
    studio.notes = "Backup su NAS in archivio, controllo il primo lunedì del mese.".into();
    let ferri = customer(
        2,
        CustomerKind::Company,
        "Ferramenta Corvo",
        "IT09876543210",
        "info@corvo.example",
        "+39 0471 000002",
        "Piazza Grande 12, Borgo Nuovo",
        &["negozio"],
    );
    let privato = customer(3, CustomerKind::Person, "Elio Brina", "", "elio.brina@example.net", "+39 347 0000003", "", &["privato"]);
    d.customers = vec![studio, ferri, privato];

    let device = |id, customer_id, name: &str, rustdesk: &str, host: &str, os: &str, who: &str, unattended| Device {
        id,
        customer_id,
        name: name.into(),
        rustdesk_id: rustdesk.into(),
        hostname: host.into(),
        os: os.into(),
        assigned_to: who.into(),
        unattended,
        created_at: stamp(now - Duration::days(390)),
        updated_at: stamp(now - Duration::days(60)),
        ..Device::default()
    };
    d.devices = vec![
        device(1, 1, "PC segreteria", "100200300", "LUN-SEG01", "Windows 11 Pro", "Marta Selva", true),
        device(2, 1, "Workstation CAD", "100200301", "LUN-CAD01", "Windows 11 Pro", "Ing. Paolo Rame", true),
        device(3, 1, "NAS archivio", "", "lun-nas", "Linux", "", false),
        device(4, 2, "PC cassa", "200300400", "CORVO-CASSA", "Windows 10 Pro", "", false),
        device(5, 3, "Notebook", "300400500", "elio-nb", "Windows 11 Home", "Elio Brina", false),
    ];

    let work = |id, customer_id, device_id, kind, days, hour, minutes: u32, summary: &str, notes: &str| {
        let start = now - Duration::days(days) - Duration::hours(hour);
        Intervention {
            id,
            customer_id,
            device_id,
            kind,
            technician: "riccardo".into(),
            summary: summary.into(),
            notes: notes.into(),
            started_at: stamp(start),
            ended_at: Some(stamp(start + Duration::minutes(i64::from(minutes)))),
            minutes,
            billable: true,
            created_at: stamp(start),
            updated_at: stamp(start + Duration::minutes(i64::from(minutes))),
            ..Intervention::default()
        }
    };
    use InterventionKind::{Lab, Onsite, Phone, Remote};
    d.interventions = vec![
        work(1, 1, Some(1), Remote, 62, 3, 25, "Stampante non stampa", "Coda di stampa bloccata, riavviato lo spooler e aggiornato il driver."),
        work(2, 1, Some(2), Remote, 48, 5, 70, "Aggiornamento software CAD", "Installata la nuova versione, licenza riattivata."),
        work(
            3,
            1,
            Some(3),
            Onsite,
            35,
            2,
            120,
            "Controllo backup e dischi NAS",
            "Un disco con settori riallocati: da sostituire. Backup verificato.",
        ),
        work(4, 1, Some(3), Onsite, 28, 4, 45, "Sostituzione disco NAS", "Disco sostituito, ricostruzione RAID avviata."),
        work(5, 2, Some(4), Remote, 20, 6, 15, "Lettore codici a barre", "Driver reinstallato."),
        work(6, 2, None, Phone, 14, 1, 10, "Domanda su fatturazione elettronica", "Spiegata la procedura di invio."),
        work(7, 3, Some(5), Lab, 10, 3, 90, "Notebook lento", "Pulizia, SSD al posto del disco meccanico, sistema migrato."),
        work(8, 1, Some(1), Remote, 6, 2, 20, "Posta non sincronizza", "Password della casella cambiata, aggiornata nel client."),
        work(9, 2, Some(4), Remote, 3, 5, 35, "Aggiornamenti Windows", "Installati gli aggiornamenti in sospeso, riavvio programmato."),
        work(10, 1, Some(2), Remote, 1, 4, 40, "Plotter offline", "Indirizzo IP del plotter cambiato dal DHCP: impostata una prenotazione."),
    ];
    if let Some(i) = d.interventions.get_mut(5) {
        i.billable = false;
    }

    d.notices = vec![Notice {
        id: 1,
        version: "2026-01".into(),
        title: "Informativa assistenza tecnica".into(),
        text: NOTICE.into(),
        effective_from: stamp(now - Duration::days(300)),
        created_at: stamp(now - Duration::days(300)),
        created_by: "riccardo".into(),
    }];
    let event = |id, customer_id, kind, purpose: &str, days, method, signer: &str, evidence: &str| PrivacyEvent {
        id,
        customer_id,
        kind,
        purpose: purpose.into(),
        notice_version: "2026-01".into(),
        method,
        at: ago(days, 0),
        signer: signer.into(),
        evidence: evidence.into(),
        recorded_by: "riccardo".into(),
        recorded_at: ago(days, 0),
        ..PrivacyEvent::default()
    };
    use PrivacyKind::{ConsentGiven, NoticeDelivered};
    d.privacy = vec![
        event(1, 1, NoticeDelivered, "", 290, Method::Paper, "Marta Selva", "Modulo firmato, archivio 2026/01"),
        event(2, 1, ConsentGiven, "remote_assistance", 290, Method::Paper, "Marta Selva", "Modulo firmato, archivio 2026/01"),
        event(3, 1, ConsentGiven, "unattended_access", 290, Method::Paper, "Marta Selva", "Modulo firmato, archivio 2026/01"),
        event(4, 2, NoticeDelivered, "", 200, Method::Email, "Gino Corvo", "Mail del titolare con il modulo firmato"),
        event(5, 2, ConsentGiven, "remote_assistance", 200, Method::Email, "Gino Corvo", "Mail del titolare con il modulo firmato"),
        event(6, 3, NoticeDelivered, "", 10, Method::Paper, "Elio Brina", "Consegnata al ritiro del notebook"),
    ];
    d.repair();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_is_consistent() {
        let mut d = demo(crate::time::now());
        assert!(d.repair().is_empty());
        assert_eq!(d.counters.intervention, 10);
        for dev in &d.devices {
            assert_eq!(crate::rustdesk::normalize(&dev.rustdesk_id).unwrap(), dev.rustdesk_id);
        }
    }
}
