# GDPR: cosa fa il gestionale

> **Ultimo aggiornamento:** 2026-10-13

RemoteCraft aiuta a **tenere traccia** degli adempimenti e a dimostrarli. Non sostituisce le
scelte del titolare né il parere di un consulente: i testi dell'informativa, le basi giuridiche
e i tempi di conservazione li decidi tu.

## Informativa (art. 13)

- `notice.add {version, title, text, effective_from}` salva una versione. Le versioni non si
  modificano: un testo nuovo è una versione nuova. È in vigore l'ultima già efficace.
- `privacy.notice {customer, method, signer, evidence}` registra la consegna al cliente della
  versione in vigore.
- `privacy.status` dice se il cliente ha la versione corrente. `gdpr.review` elenca chi non l'ha
  ricevuta o ne ha una vecchia, e gli interventi remoti lo segnalano.

## Consensi (art. 6-7)

L'assistenza in sé si basa di solito sul contratto. Il consenso serve per le finalità che lo
richiedono o per cui scegli di chiederlo. Il gestionale propone:

| Finalità | Uso |
|---|---|
| `remote_assistance` | controllo del PC con RustDesk, con la persona presente, vedendo i dati sullo schermo |
| `unattended_access` | collegamento con password permanente senza nessuno al PC |
| `marketing`, `newsletter` | comunicazioni commerciali |
| altre | qualunque finalità tua (`[a-z0-9_]`) |

- `privacy.consent {customer, purposes, method, signer, evidence, at, expires_at, with_notice}`
  e `privacy.withdraw {customer, purposes}` aggiungono eventi a un registro **che non viene mai
  riscritto**. Lo stato di una finalità è l'ultimo evento: dato, revocato, scaduto o mancante.
- **Consenso informato:** se l'informativa non risulta consegnata, il consenso viene registrato
  con un avviso. `with_notice: true` registra insieme consegna e consenso (un unico modulo
  firmato).
- **Prova:** `evidence` dice dove sta l'originale (numero d'archivio, e-mail, id del modulo
  online). `method: verbal` è una prova debole: annota chi l'ha raccolto.
- **Scadenza:** `expires_at` per il singolo consenso, `consent_validity_months` per tutti.
- **Regola operativa:** con `require_consent` (predefinito) un intervento remoto senza
  `remote_assistance` valido viene rifiutato. Si può procedere solo con `consent_override` e un
  motivo, che resta nell'intervento e nell'audit. Se il consenso `unattended_access` manca o è
  revocato e un PC ha l'accesso non presidiato, il gestionale lo segnala: rimuovi la password
  permanente dal client.

## Diritti dell'interessato

- **Accesso e portabilità (art. 15, 20):** `gdpr.export` (o `remotecraft-cli export CLIENTE
  --out file.json`) produce in JSON anagrafica, PC, interventi, registro privacy, informative
  ricevute e accessi registrati nell'audit. L'export stesso viene registrato.
- **Rettifica (art. 16):** `customer.update`, `device.update`, `intervention.update`.
- **Cancellazione (art. 17):** `gdpr.erase {customer, confirm: true, reason}`:
  - anonimizza il cliente («Cliente cancellato #id») e i suoi PC (dismessi, senza ID RustDesk,
    hostname, seriale, utente e note);
  - cancella oggetto e note degli interventi, conservando date, durate e tecnico per gli
    obblighi contabili;
  - conserva gli eventi del registro privacy come prova, senza firmatario e note;
  - l'audit, che contiene solo identificativi, registra la cancellazione e il motivo.

  Proponi prima l'export, se l'interessato vuole una copia. L'operazione non si annulla. Il file
  `desk.json.bak` conserva la versione precedente fino alla modifica successiva: tienine conto
  nei backup.

## Conservazione e revisione

`retention_years` (predefinito 10) è il periodo dopo il quale `gdpr.review` propone la
cancellazione dei clienti senza attività. La revisione segnala anche:

- l'assenza dell'informativa nel gestionale;
- informative non consegnate o vecchie;
- assistenza remota o accesso non presidiato senza consenso valido;
- cronometri attivi da più di 12 ore.

## Sicurezza e accountability (art. 5, 24, 32)

- File del gestionale leggibili solo dal servizio (0600), scritti in modo atomico.
- Accesso all'API solo con token; token separati per tecnico e revocabili; nessun dato
  personale nei log di audit (solo id, comando, chi e quando).
- Le password RustDesk non vengono mai salvate. Le sessioni RustDesk sono cifrate tra i due
  client e il server non ne vede il contenuto.
- `audit.list` mostra chi ha creato, modificato, collegato, esportato o cancellato cosa.
