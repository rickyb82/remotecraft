# Installazione e integrazione con il framework

> **Ultimo aggiornamento:** 2026-10-13

## Il container

Il container `remotecraft` contiene tre processi, avviati da `docker/entrypoint.sh` come
utente non privilegiato `remotecraft`:

| Processo | Porte | Cosa fa |
|---|---|---|
| `hbbs -k _ [-r $RUSTDESK_HOST]` | 21115/tcp, 21116/tcp+udp | ID e avvio delle connessioni RustDesk; accetta solo client con la chiave del server |
| `hbbr -k _` | 21117/tcp | relay RustDesk |
| `remotecraft-cli serve` | 7979/tcp | API del gestionale e app web |

Se uno dei tre si ferma, si ferma il container e la policy `restart: unless-stopped` lo
riavvia. L'`HEALTHCHECK` verifica le porte RustDesk ogni minuto.

Variabili d'ambiente:

| Variabile | Obbligatoria | Significato |
|---|---|---|
| `REMOTECRAFT_TOKEN` | sì | token di servizio del framework per l'API |
| `RUSTDESK_HOST` | consigliata | nome pubblico del server (es. `rustdesk.siriustec.it`): hbbs lo annuncia come relay, il gestionale lo mostra ai client |
| `TZ` | no (`Europe/Rome`) | fuso orario per gli orari scritti a mano (`9:30`) e per i report mensili |
| `RUSTDESK_TAG` (build) | no (`1.1.16`) | versione dell'immagine ufficiale da cui copiare hbbs/hbbr |

Volumi: `/data/rustdesk` (coppia di chiavi e database di hbbs) e `/data/desk` (gestionale).

## Migrazione dai due container di prima

1. `git pull` nella cartella `remotecraft` (la cartella `data/` con le chiavi resta dov'è).
2. Crea `.env` con `REMOTECRAFT_TOKEN` e `RUSTDESK_HOST`.
3. `docker compose up -d --build --remove-orphans`: i vecchi `rustdesk-hbbs` e `rustdesk-hbbr`
   vengono rimossi, il nuovo container riusa `data/` (l'entrypoint ne prende la proprietà).
4. `docker compose exec -u remotecraft remotecraft remotecraft-cli rustdesk`: la chiave
   mostrata deve essere quella di sempre.

## Identità: come il framework indica il tecnico

| Token | Chi lo usa | Il tecnico è |
|---|---|---|
| **di servizio** (`REMOTECRAFT_TOKEN`) | il backend del framework | l'header `X-Remotecraft-User: <id utente>` (opzionale: senza, l'azione è registrata come «tecnico») |
| **utente** | il processo MCP di un tecnico | il token stesso |

Gli id vengono normalizzati (`Anna Rossi` → `anna-rossi`). Il nome finisce negli interventi
(`technician`) e nel registro di audit.

```http
POST /api/users/<utente>/token        → {"user": "marco", "token": "64 caratteri hex"}
POST /api/users/<utente>/revoke       → {"user": "marco", "revoked": 1}
GET  /api/users                       → {"users": ["marco"]}
```

Solo con il token di servizio. I token utente sono in `/data/desk/tokens.json` (0600).

## L'app web nel framework

Il container serve l'app web (WebAssembly) su `/`. Quando un tecnico apre la sezione
assistenza del framework, crea (o riusa) il suo token e apri la pagina con il token:

```
https://assistenza.example.it/?token=<token del tecnico>&lang=it
```

La pagina tiene il token nello storage del browser e parla con la propria origine (il server
che l'ha servita). Può anche essere ospitata altrove (GitHub Pages) e puntata al server con
`?server=https://assistenza.example.it`: il server risponde al CORS per qualunque origine, è il
token a proteggere i dati. Il link `rustdesk://` apre il client RustDesk installato sul PC del
tecnico; gli export GDPR arrivano come download.

L'app desktop (`cargo install --path apps/remotecraft`, oppure `remotecraft --demo`) lavora
sul gestionale locale nella cartella dei dati, senza server.

## Chiamare l'API

```http
POST /api/cmd
Authorization: Bearer <token>
X-Remotecraft-User: riccardo
Content-Type: application/json

{"command": "intervention.start", "params": {"device": "123 456 789", "summary": "Stampante"}}
→ 200 {"result": {...}}
→ 400 {"error": "..."}   parametro mancante o non valido
→ 404 {"error": "..."}   cliente, PC, intervento o comando inesistente
→ 409 {"error": "..."}   rifiutato da una regola (consenso, doppione, cancellazione…)
```

Le rotte di sola lettura e l'elenco completo dei comandi sono in [api.md](api.md).

## HTTPS

`serve` parla HTTP. Il compose lo espone solo su `127.0.0.1:7979`. Se il framework gira su
un'altra macchina, mettici davanti un reverse proxy con TLS: i token viaggiano negli header.

## Backup

```
data/id_ed25519, data/id_ed25519.pub   chiavi RustDesk (perderle = riconfigurare tutti i client)
desk/desk.json                         il gestionale (desk.json.bak: la versione precedente)
desk/audit.jsonl                       registro di audit, solo in aggiunta
desk/tokens.json                       token dei tecnici
```

Fai il backup di entrambe le cartelle, cifrato: contengono dati personali dei clienti.

## Aggiornare

- RemoteCraft: `git pull && docker compose up -d --build`.
- RustDesk: cambia `RUSTDESK_TAG` in `.env` (la versione in uso la mostra `docker compose logs`
  all'avvio di hbbs) e ricostruisci.

## Senza Docker

```sh
cargo install --path apps/remotecraft-cli
REMOTECRAFT_RUSTDESK_KEY_FILE=/percorso/data/id_ed25519.pub \
REMOTECRAFT_RUSTDESK_HOST=rustdesk.siriustec.it \
remotecraft-cli serve --bind 127.0.0.1:7979 --token SEGRETO
```

hbbs e hbbr vanno allora avviati a parte (il vecchio `docker-compose.yml` o i binari ufficiali).
`REMOTECRAFT_RUSTDESK_CHECK_HOST` dice a `rustdesk.check` dove controllarli.

## Limiti (0.1)

- Un file JSON per tutto il gestionale, riscritto in modo atomico a ogni modifica: adatto a
  migliaia di clienti e decine di migliaia di interventi, non a milioni.
- Le scritture passano da un solo processo alla volta (il server). La CLI nello stesso container
  vede le modifiche del server e viceversa, ma due scritture simultanee da processi diversi
  possono sovrascriversi: per lavorare usa l'API o l'MCP, la CLI per consultare.
- L'app web mostra l'intero gestionale al tecnico (è pensata per una piccola azienda di
  assistenza): i token dei tecnici vanno dati solo a chi deve vedere tutti i clienti.
