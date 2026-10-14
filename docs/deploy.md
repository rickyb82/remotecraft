# Installazione e integrazione con il framework

> **Ultimo aggiornamento:** 2026-10-14

## Il container

Il container `remotecraft` contiene tre processi, avviati da `docker/entrypoint.sh` con due
utenti non privilegiati dagli id fissi:

| Processo | Utente | Porte | Cosa fa |
|---|---|---|---|
| `hbbs -k _ [-r $RUSTDESK_HOST]` | `rustdesk` (10002) | 21115/tcp, 21116/tcp+udp | ID e avvio delle connessioni RustDesk; accetta solo client con la chiave del server |
| `hbbr -k _` | `rustdesk` (10002) | 21117/tcp | relay RustDesk |
| `remotecraft-cli serve` | `remotecraft` (10001) | 7979/tcp | API del gestionale e app web |

hbbs e hbbr sono esposti a internet: girano senza `REMOTECRAFT_TOKEN` nell'ambiente e possono
scrivere solo in `/data/rustdesk`. Solo `remotecraft` può leggere e scrivere `/data/desk`; di
`/data/rustdesk` legge soltanto la chiave pubblica (`id_ed25519.pub`). Sull'host i file di
`./desk` appartengono all'uid 10001 e quelli di `./data` all'uid 10002: l'entrypoint sistema
proprietario e permessi a ogni avvio, anche per le cartelle delle versioni precedenti.

Se uno dei tre si ferma, si ferma il container e la policy `restart: unless-stopped` lo
riavvia. L'`HEALTHCHECK` verifica le porte RustDesk ogni minuto.

Variabili d'ambiente:

| Variabile | Obbligatoria | Significato |
|---|---|---|
| `REMOTECRAFT_TOKEN` | sì | token di servizio del framework per l'API: almeno 32 caratteri casuali, generali con `openssl rand -hex 32` (più corto, il container non parte) |
| `RUSTDESK_HOST` | consigliata | nome pubblico del server (es. `rustdesk.siriustec.it`): hbbs lo annuncia come relay, il gestionale lo mostra ai client |
| `TZ` | no (`Europe/Rome`) | fuso orario per gli orari scritti a mano (`9:30`) e per i report mensili |
| `RUSTDESK_TAG` (build) | no (`1.1.16`) | versione dell'immagine ufficiale da cui copiare hbbs/hbbr |

Volumi: `/data/rustdesk` (coppia di chiavi e database di hbbs) e `/data/desk` (gestionale).

## Migrazione dai due container di prima

1. `git pull` nella cartella `remotecraft` (la cartella `data/` con le chiavi resta dov'è).
2. Crea `.env` con un token nuovo e il nome pubblico:
   `echo "REMOTECRAFT_TOKEN=$(openssl rand -hex 32)" >> .env` e
   `echo "RUSTDESK_HOST=rustdesk.siriustec.it" >> .env`.
3. `docker compose up -d --build --remove-orphans`: i vecchi `rustdesk-hbbs` e `rustdesk-hbbr`
   vengono rimossi, il nuovo container riusa `data/` (l'entrypoint ne prende la proprietà).
4. `docker compose exec -u remotecraft remotecraft remotecraft-cli rustdesk`: la chiave
   mostrata deve essere quella di sempre.

## Identità: come il framework indica il tecnico

| Token | Chi lo usa | Il tecnico è |
|---|---|---|
| **di servizio** (`REMOTECRAFT_TOKEN`) | il backend del framework | l'header `X-Remotecraft-User: <id utente>` (opzionale: senza, vale il tecnico del server, `REMOTECRAFT_USER` o altrimenti «tecnico», e lo stesso nome torna nello snapshot dell'app web) |
| **utente** | il processo MCP di un tecnico | il token stesso |

Gli id vengono normalizzati (`Anna Rossi` → `anna-rossi`). Il nome finisce negli interventi
(`technician`) e nel registro di audit.

```http
POST /api/users/<utente>/token        → {"user": "marco", "token": "64 caratteri hex"}
POST /api/users/<utente>/revoke       → {"user": "marco", "revoked": 1}
GET  /api/users                       → {"users": ["marco"]}
```

Solo con il token di servizio: un server avviato senza token risponde 403 (i token utente
servono solo quando l'API è protetta). I token utente sono in `/data/desk/tokens.json` (0600).

## L'app web nel framework

Il container serve l'app web (WebAssembly) su `/`. Quando un tecnico apre la sezione
assistenza del framework, crea (o riusa) il suo token e apri la pagina con il token dopo `#`:

```
https://assistenza.example.it/#token=<token del tecnico>&lang=it
```

Quello che segue `#` resta nel browser: non arriva al server né al reverse proxy, quindi il token
non finisce nei loro log. Il vecchio link con `?token=` funziona ancora, ma il token compare nei
log del proxy: usa `#token=`.

La pagina toglie subito il token dalla barra degli indirizzi e lo tiene nello storage del
browser insieme al suo server: non lo manda mai a un altro server, né in chiaro (`http://`) a
un'altra macchina. Su un PC condiviso o smarrito, *Impostazioni → Stato → Dimentica il token su
questo browser* lo cancella e ricarica la pagina senza; il link del framework riporta dentro.
La pagina parla con la propria origine (il server che l'ha servita). Può anche essere ospitata altrove (GitHub Pages) e puntata al server con
`?server=https://assistenza.example.it`: con il token di servizio il server risponde al CORS per
qualunque origine, perché è il token (che il browser non aggiunge mai da sé) a proteggere i dati.
Il link `rustdesk://` apre il client RustDesk installato sul PC del tecnico; gli export GDPR
arrivano come download.

Senza token si scaricano solo i file della cartella dell'app (`--static`, `REMOTECRAFT_STATIC`):
file normali fino a 64 MiB che stanno davvero dentro la cartella, inviati direttamente dal disco.
Percorsi assoluti anche codificati (`/%2Fetc%2F…`), `..` e collegamenti simbolici che escono
dalla cartella ricevono 400 o 404. Nella cartella va messo solo l'output di `trunk build`: i file
con l'impronta nel nome (`remotecraft-web-<hash>.js`, `…_bg.wasm`) restano in cache per un anno,
`index.html` viene ricontrollato a ogni apertura, così dopo un aggiornamento i browser caricano
subito la versione nuova. Le pagine vanno con `Referrer-Policy: no-referrer`.

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

`Content-Type: application/json` è obbligatorio (altrimenti 415): così una pagina di un altro
sito non può spedire comandi con un semplice modulo. Le rotte di sola lettura, l'elenco completo
dei comandi e i limiti delle richieste sono in [api.md](api.md).

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
openssl rand -hex 32 > ~/.remotecraft-token && chmod 600 ~/.remotecraft-token
REMOTECRAFT_TOKEN="$(cat ~/.remotecraft-token)" \
REMOTECRAFT_RUSTDESK_KEY_FILE=/percorso/data/id_ed25519.pub \
REMOTECRAFT_RUSTDESK_HOST=rustdesk.siriustec.it \
remotecraft-cli serve --bind 127.0.0.1:7979
```

Passa il token con `REMOTECRAFT_TOKEN` e non con `--token`: gli argomenti dei processi sono
visibili a tutti gli utenti della macchina (`ps`). Il token deve avere almeno 32 caratteri.
hbbs e hbbr vanno allora avviati a parte (il vecchio `docker-compose.yml` o i binari ufficiali).
`REMOTECRAFT_RUSTDESK_CHECK_HOST` dice a `rustdesk.check` dove controllarli.

### Senza token, solo su questa macchina

Senza token `serve` ascolta solo su `127.0.0.1`/`localhost` ed è pensato per l'uso personale
(l'app web con `trunk serve`, un MCP locale). Per non diventare una porta aperta verso il
gestionale per qualunque sito aperto nel browser:

- risponde solo alle richieste indirizzate a questa macchina (`Host: 127.0.0.1:<porta>`,
  `localhost:<porta>` o `[::1]:<porta>`), mai a un altro nome che punta qui (DNS rebinding);
- dal browser accetta solo le pagine di questa macchina (`http://127.0.0.1:*`,
  `http://localhost:*`, `http://[::1]:*`); un'app ospitata altrove va indicata con
  `--allow-origin https://app.example.it` (o `REMOTECRAFT_ALLOW_ORIGIN`, più origini separate da
  virgole), le altre ricevono 403;
- non crea token utente (`/api/users` risponde 403).

## Limiti (0.1)

- Un file JSON per tutto il gestionale, riscritto in modo atomico a ogni modifica: adatto a
  migliaia di clienti e decine di migliaia di interventi, non a milioni.
- Le scritture passano da un solo processo alla volta (il server). La CLI nello stesso container
  vede le modifiche del server e viceversa, ma due scritture simultanee da processi diversi
  possono sovrascriversi: per lavorare usa l'API o l'MCP, la CLI per consultare.
- L'app web mostra l'intero gestionale al tecnico (è pensata per una piccola azienda di
  assistenza): i token dei tecnici vanno dati solo a chi deve vedere tutti i clienti.
