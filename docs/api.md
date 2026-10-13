# API HTTP e comandi

> **Ultimo aggiornamento:** 2026-10-13

Tutto passa da `POST /api/cmd {"command", "params"}`. Le rotte `GET` sono scorciatoie di sola
lettura sugli stessi comandi: i parametri della query string diventano `params`.

| Rotta | Comando |
|---|---|
| `GET /api/health` (senza token) | — |
| `GET /api/customers?query=&tag=&include_archived=` | `customer.list` |
| `GET /api/customers/<id o nome>` | `customer.get` |
| `GET /api/customers/<id>/privacy` | `privacy.status` |
| `GET /api/customers/<id>/export` | `gdpr.export` (registrato nell'audit) |
| `GET /api/devices?customer=&query=` | `device.list` |
| `GET /api/devices/<id, ID RustDesk o nome>` | `device.get` |
| `GET /api/rustdesk/<ID RustDesk>` | `device.find` |
| `GET /api/rustdesk` | `rustdesk.setup` |
| `GET /api/rustdesk/check` | `rustdesk.check` |
| `GET /api/interventions?customer=&device=&technician=&kind=&from=&to=&running=&billable=&query=` | `intervention.list` |
| `GET /api/interventions/running` | `intervention.running` |
| `GET /api/interventions/<id>` | `intervention.get` |
| `GET /api/report/time?from=&to=&group_by=` | `report.time` |
| `GET /api/notices` | `notice.list` |
| `GET /api/review` | `gdpr.review` |
| `GET /api/status`, `GET /api/commands` | `app.status`, `app.commands` |

## Riferimenti

Dove un comando chiede `customer`, vanno bene l'id (`12`, `"#12"`), il nome, l'e-mail, la P.IVA
o il nome di un referente. Se più clienti corrispondono, l'errore li elenca. Per `device` vanno
bene l'id, l'ID RustDesk (in qualunque formato) o il nome/hostname. Gli orari si scrivono come
`2026-10-13 09:30`, `13/10/2026 09:30`, `09:30` (oggi) o RFC 3339; vengono salvati in UTC
(`2026-10-13T07:30:00Z`).

## Comandi

L'elenco sempre aggiornato è `remotecraft-cli commands` (o `GET /api/commands`). In sintesi:

| Area | Comandi |
|---|---|
| Clienti | `customer.add`, `customer.update`, `customer.get`, `customer.list`, `customer.archive`, `customer.remove` (solo senza storico) |
| PC | `device.add`, `device.update`, `device.get`, `device.list`, `device.find`, `device.retire`, `device.remove` (solo senza storico), `device.connect` |
| Interventi | `intervention.start`, `intervention.stop`, `intervention.add`, `intervention.update`, `intervention.remove`, `intervention.get`, `intervention.list`, `intervention.running` |
| Report | `report.time` |
| Informativa | `notice.add`, `notice.list`, `notice.get` |
| Registro GDPR | `privacy.purposes`, `privacy.status`, `privacy.history`, `privacy.notice`, `privacy.consent`, `privacy.withdraw` |
| Diritti | `gdpr.export`, `gdpr.erase`, `gdpr.review`, `audit.list` |
| RustDesk | `rustdesk.setup`, `rustdesk.check` |
| Impostazioni | `prefs.get`, `prefs.set`, `app.status`, `app.commands` |

### Esempi

```json
{"command": "customer.add", "params": {"name": "Studio Rossi", "kind": "company", "tax_id": "IT01234567890",
  "email": "info@rossi.example", "contacts": [{"name": "Mario Rossi", "role": "titolare", "phone": "+39 333 1"}], "tags": ["contratto"]}}

{"command": "device.add", "params": {"customer": "Studio Rossi", "name": "PC reception", "rustdesk_id": "123 456 789", "os": "Windows 11"}}

{"command": "privacy.consent", "params": {"customer": "Studio Rossi", "purposes": ["remote_assistance"], "with_notice": true,
  "method": "paper", "signer": "Mario Rossi", "evidence": "Modulo firmato, archivio 2026/12"}}

{"command": "intervention.start", "params": {"device": "123456789", "summary": "Stampante non stampa"}}
→ {"intervention": {...}, "connect": {"uri": "rustdesk://connection/new/123456789", "command": "rustdesk --connect 123456789"}, "warnings": []}

{"command": "intervention.stop", "params": {"notes": "Spooler riavviato, driver aggiornato"}}

{"command": "intervention.add", "params": {"customer": 2, "kind": "onsite", "summary": "Cablaggio", "started_at": "2026-10-12 14:00", "ended_at": "2026-10-12 16:30"}}

{"command": "report.time", "params": {"from": "2026-10-01", "to": "2026-11-01", "group_by": "customer"}}
```

## Preferenze

| Chiave | Predefinito | Significato |
|---|---|---|
| `language` | `it` | lingua dei testi per le persone (descrizioni delle finalità) |
| `company_name` | — | titolare del trattamento, riportato negli export |
| `billing_unit_minutes` | 15 | arrotondamento per eccesso delle ore fatturabili, per intervento (0/1 = esatto) |
| `minimum_minutes` | 0 | durata minima fatturabile di un intervento |
| `require_consent` | `true` | un intervento remoto senza consenso `remote_assistance` viene rifiutato (salvo `consent_override`) |
| `consent_validity_months` | 0 | dopo quanti mesi un consenso conta come scaduto (0 = mai) |
| `retention_years` | 10 | dopo quanti anni senza interventi `gdpr.review` propone la cancellazione (0 = mai) |
| `rustdesk_host`, `rustdesk_key` | dal container | sostituiscono `RUSTDESK_HOST` e la chiave letta da `id_ed25519.pub` |
