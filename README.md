# RemoteCraft

Assistenza remota per la tua attività: il **server RustDesk** e un **gestionale dell'assistenza**
nello stesso container. Il gestionale tiene l'anagrafica dei clienti, i loro PC con l'ID RustDesk,
lo storico degli interventi con tempi e note e la registrazione di informativa e consensi GDPR.
Ha un'**interfaccia grafica** come quella di MailCraft (web e desktop), un'**API HTTP** per il tuo
framework e un **server MCP**, così un assistente AI può lavorarci in linguaggio naturale. Fa parte della famiglia *craft* (`mailcraft`, `gridcraft`, …) e ne segue
le convenzioni: Rust, «tutto è un comando», mai crash.

```
┌──────────────────────────── container remotecraft ────────────────────────────┐
│  hbbs  :21115 :21116 tcp/udp   ID e avvio delle connessioni   ┐ RustDesk       │
│  hbbr  :21117                  relay                          ┘ Server OSS     │
│  remotecraft-cli serve :7979   app web + API ──► browser, framework, MCP        │
│  /data/rustdesk  chiavi del server        /data/desk  gestionale + audit       │
└───────────────────────────────────────────────────────────────────────────────┘
```

## L'interfaccia

```
┌──────────────┬────────────────────────────────────┬───────────────────────────────┐
│ ▶ Avvia      │ Clienti · 3        ⏷Etichetta  + Nuovo│ Studio Tecnico Lunaria   ▶ ✎ ⋯│
│ ▸ Clienti  3 │ SL Studio Tecnico Lunaria  🛡 3 PC   │ ▌Informativa 2026-01 consegnata│
│   PC       5 │ FC Ferramenta Corvo        ⚠ 1 PC   │ ▌Assistenza remota   Dato      │
│   Interventi │ EB Elio Brina              ⛔ 1 PC   │ PC · Interventi · Note         │
│   Privacy  1 │                                    │                 ┌ Avvia timer ┐│
│   Report     │                                    │                 │ ID o nome…  ││
└──────────────┴────────────────────────────────────┴─────────────────┴─────────────┘┘
```

La stessa interfaccia gira **nel browser** (servita dal container su `/`, o da GitHub Pages) e
come **app desktop**: barra in alto con la ricerca (un ID RustDesk porta dritto al PC), la
navigazione a sinistra, l'elenco al centro e la scheda a destra, la scheda dell'intervento
agganciata in basso a destra con il **cronometro**, tema chiaro e scuro, italiano e inglese,
scorciatoie da tastiera (`/` cerca, `s` avvia, `x` chiude, `n` nuovo, `?` aiuto).

- **Telefonata in arrivo**: digiti l'ID RustDesk che il cliente legge, `Ctrl+Invio` nella
  ricerca avvia il cronometro e apre il client RustDesk sul suo PC; a fine lavoro scrivi le note
  e chiudi. Se l'ID non è nel gestionale, **Collegati subito** (o `Ctrl+Invio`) apre RustDesk su
  quell'ID senza registrare niente: la password te la detta il cliente e la scrivi in RustDesk. Se chiudi o riduci la scheda di un intervento in corso, oggetto e fatturabilità
  vengono salvati subito; le note restano nella scheda fino allo stop. Nei moduli `Ctrl+Invio`
  salva solo se il cursore è in uno dei loro campi.
- **Privacy a colpo d'occhio**: ogni cliente ha un indicatore (consenso dato, da sistemare,
  mancante); la scheda mostra per prima l'informativa e i consensi, con i pulsanti per
  registrarli; un avvio remoto senza consenso chiede di registrarlo o di indicare il motivo.
- **Report**: ore e ore fatturabili del mese per cliente, tecnico, PC o tipo, con il dettaglio
  degli interventi, esportabili in CSV. Il periodo comprende entrambi i giorni indicati (*Dal* e
  *Al*), come il filtro *Periodo* degli interventi.
- **Sempre aggiornata**: l'app desktop mostra entro un paio di secondi quello che un assistente
  AI (MCP) o la riga di comando cambiano sullo stesso archivio; nel browser i cambiamenti degli
  altri tecnici arrivano da soli. La barra di stato distingue server irraggiungibile, token
  rifiutato e versioni diverse di server e app. La lingua scelta in alto resta anche alla
  riapertura.
- **Impostazioni**: guida iniziale (azienda, informativa, server RustDesk), regole di
  fatturazione e privacy, verifica del server RustDesk e configurazione dei client da copiare.

## Cosa fa il gestionale

- **Clienti**: aziende o privati, P.IVA/codice fiscale, recapiti, referenti, etichette, note;
  ricerca per nome, P.IVA, e-mail, telefono, referente, etichetta o ID RustDesk di un loro PC.
  I doppioni (stessa P.IVA) vengono segnalati.
- **PC**: nome, **ID RustDesk** (scritto come vuoi, `123 456 789` o `123-456-789`, salvato in
  forma unica e mai duplicato tra PC attivi), hostname, sistema, seriale, ubicazione, chi lo usa,
  accesso non presidiato. Da un PC si ottiene il link `rustdesk://connection/new/<ID>` che apre
  la connessione nel client. **Le password RustDesk non vengono mai salvate.**
- **Interventi**: con il cronometro (`start`/`stop`) o inseriti dopo (inizio e fine, oppure
  minuti), di tipo remoto, in sede, telefonico, in laboratorio. Ogni intervento ha tecnico,
  oggetto, note, riferimento a un ticket e indicazione se è fatturabile. Il **report ore** è per
  cliente, tecnico, mese, PC o tipo, con le ore fatturabili arrotondate per intervento (es. a
  15 minuti, con un minimo).
- **GDPR**:
  - le versioni dell'**informativa** (art. 13);
  - per ogni cliente il registro, mai sovrascritto, di consegna dell'informativa, **consensi** e
    **revoche** per finalità (`remote_assistance`, `unattended_access`, `marketing`, …), con
    data, modalità (carta, e-mail, online, verbale), firmatario e prova;
  - un intervento remoto senza consenso all'assistenza remota viene **bloccato**, salvo motivo
    esplicito, che resta registrato;
  - **export** dei dati di un cliente (art. 15 e 20) e **cancellazione** (art. 17: dati
    anonimizzati, ore e prove di consenso conservate senza nomi);
  - una **revisione** che segnala informative da consegnare, consensi mancanti o scaduti, dati
    oltre il periodo di conservazione e cronometri dimenticati;
  - un **registro di audit** di chi ha fatto cosa: solo identificativi, nessun dato personale.
- **RustDesk integrato**:
  - il gestionale legge da solo la chiave del server;
  - per ogni client fornisce i valori da inserire, la **stringa da importare** e il **nome da
    dare all'installer Windows** perché si configuri da solo;
  - `rustdesk.check` controlla che hbbs e hbbr rispondano.
- **Clienti dalla intranet**: se sulla stessa macchina gira la intranet (il gestionale di
  fatturazione), un cliente nuovo si prende da lì invece di riscriverlo: nel modulo *Nuovo
  cliente* e nel campo *Cliente* di *Nuovo PC* compare *Cerca nella intranet*; una riga scelta
  importa anagrafica, partita IVA, contatti e indirizzo, e resta legata al cliente della intranet
  (una seconda importazione lo aggiorna, non lo duplica). Si importa solo chi serve, uno alla
  volta; PC, interventi e registro privacy restano nel gestionale. In MCP: `search_intranet` e
  `import_customer`.
- **API e MCP**: il framework chiama `POST /api/cmd` (o le rotte REST di sola lettura) con il
  suo token di servizio e indica il tecnico. Ogni tecnico può avere il suo assistente MCP con un
  token personale. Le regole sono le stesse dappertutto, perché interfaccia, CLI, API e MCP
  chiamano gli stessi comandi.

## Avvio rapido (Docker)

Sul server, al posto dei due container hbbs/hbbr di prima:

```sh
git clone https://github.com/rickyb82/remotecraft
cd remotecraft
cat > .env <<EOF
REMOTECRAFT_TOKEN=$(openssl rand -hex 32)
RUSTDESK_HOST=rustdesk.siriustec.it
EOF
docker compose up -d --build
docker compose ps                       # deve diventare "healthy"
```

- `./data` resta la cartella delle chiavi RustDesk di prima: **i client già configurati
  continuano a funzionare**. Al primo avvio su un server nuovo la coppia di chiavi viene generata
  lì.
- `./desk` contiene il gestionale (`desk.json`, `audit.jsonl`, `tokens.json`, file leggibili
  solo dal servizio).
- Le porte RustDesk sono pubbliche (vedi sotto). L'API del gestionale ascolta solo su
  `127.0.0.1:7979`: davanti mettici il framework o un reverse proxy HTTPS.
- RustDesk non dipende dal gestionale: se l'API non parte, hbbs e hbbr continuano a servire i
  client e `docker compose logs` dice perché.
- **Con la intranet sulla stessa macchina**: aggiungi a `.env` il suo token
  (`REMOTECRAFT_INTRANET_TOKEN=…`, lo stesso `INTRANET_TOKEN` del suo `.env`) e avvia con
  `docker compose -f docker-compose.yml -f docker-compose.intranet.yml up -d --build`: il
  container entra nella rete Docker della intranet e i clienti si importano da lì
  ([docs/deploy.md](docs/deploy.md#la-intranet)).

### Aggiornamento

`git pull && docker compose up -d --build`. Il token di servizio ora deve avere almeno 32
caratteri: con quello degli esempi di prima (`un-segreto-lungo-e-casuale`) RustDesk continua a
funzionare, ma l'API del gestionale non parte (`REMOTECRAFT_TOKEN is too short` nei log).
Rigeneralo e aggiorna la copia nel framework:

```sh
sed -i "s/^REMOTECRAFT_TOKEN=.*/REMOTECRAFT_TOKEN=$(openssl rand -hex 32)/" .env
docker compose up -d
```

I dettagli sono in [docs/deploy.md](docs/deploy.md#aggiornare).

Prime impostazioni e verifica:

```sh
docker compose exec -u remotecraft remotecraft remotecraft-cli rustdesk
docker compose exec -u remotecraft remotecraft remotecraft-cli run prefs.set \
  '{"company_name": "Sirius Tecnologie", "billing_unit_minutes": 15}'
```

Il primo comando mostra server, chiave, stringa di configurazione e nome dell'installer, e
verifica le porte. Usa sempre `-u remotecraft` con `exec`, così i file restano del servizio.

L'**app web** è su `http://127.0.0.1:7979/` (dietro il proxy HTTPS o il framework): la si apre
con il token del tecnico, `https://assistenza.example.it/#token=<token>&lang=it` (vedi
[docs/deploy.md](docs/deploy.md)). L'**app desktop** lavora sul gestionale locale:
`cargo install --path apps/remotecraft` e poi `remotecraft --as riccardo`, oppure
`remotecraft --demo` per provarla.

## La rete per RustDesk

RustDesk non passa dal proxy HTTPS: usa porte TCP e UDP proprie.

1. Crea un record DNS, per esempio `rustdesk.siriustec.it`, che punta all'IP pubblico
   dell'ufficio, e mettilo in `RUSTDESK_HOST`.
2. Sul router inoltra verso il server (per esempio `192.168.10.80`):

   | Porta | Protocollo | A cosa serve |
   |---|---|---|
   | 21115 | TCP | test del tipo di NAT |
   | 21116 | TCP e UDP | registrazione degli ID e avvio delle connessioni |
   | 21117 | TCP | relay, quando la connessione diretta non riesce |

3. Se il server ha un firewall attivo: `sudo ufw allow 21115:21117/tcp && sudo ufw allow 21116/udp`.

Da una rete esterna: `remotecraft-cli rustdesk rustdesk.siriustec.it` (o `nc -vz … 21116`).
La UDP non si può verificare con una connessione: se i client restano offline con le TCP
aperte, controlla l'inoltro UDP.

## Configurare i client RustDesk

Scarica RustDesk dal sito ufficiale (`rustdesk.com`) o dalle release su GitHub, poi scegli uno
di questi modi (i valori li dà `remotecraft-cli rustdesk` o `GET /api/rustdesk`):

- **Installer già configurato (Windows)**: rinomina l'installer con il nome indicato
  (`rustdesk-host=…,key=…,.exe`) e installalo.
- **Stringa di configurazione**: *Impostazioni → Rete → Importa configurazione server* e incolla
  la stringa.
- **A mano**: *Impostazioni → Rete → Server ID/Relay*, con Server ID e Server relay uguali a
  `RUSTDESK_HOST`, Server API vuoto e la Chiave mostrata.

In basso nella finestra principale deve comparire **Pronto**.

## Fare un'assistenza

1. Il cliente apre RustDesk e ti comunica **ID** e **password monouso**.
2. Nell'interfaccia premi `/`, scrivi l'ID e `Ctrl+Invio`: il cronometro parte e si apre il
   client RustDesk sul suo PC (dalla riga di comando: `remotecraft-cli who 123 456 789` e
   `remotecraft-cli start --device "123 456 789"`; in MCP: `find_device`, `start_intervention`).
   Se manca il consenso all'assistenza remota, l'avvio viene rifiutato finché non lo registri o
   indichi il motivo.

   **Connessione rapida**, per un PC che il gestionale non conosce (un cliente occasionale): la
   ricerca dice *Nessun PC ha l'ID RustDesk …* e propone **Collegati subito**; `Ctrl+Invio` fa
   lo stesso. Vale solo per gli ID di sole cifre che nessun PC ha (un PC dismesso si collega
   dalla sua scheda, con il consenso del suo cliente) e che non sono il telefono o la partita IVA
   di un cliente. Si apre RustDesk su quell'ID, la password la scrivi in RustDesk
   e nel gestionale non viene salvato niente: nessun cliente, PC o intervento, e nessun
   consenso viene controllato (la barra di stato lo ricorda). Dalla riga di comando:
   `remotecraft-cli run device.connect '{"rustdesk_id": "987 654 321"}'`; in MCP:
   `connect_device` con `rustdesk_id`. Se poi vuoi tenerne traccia, *Aggiungi un PC con questo
   ID*: nel campo *Cliente* scegli dall'elenco, oppure **Crea il cliente "…"** con il nome
   scritto; finché il cliente non è scelto, sotto il modulo compare cosa manca per salvare.
3. A fine lavoro scrivi le note nella scheda dell'intervento e chiudi (`remotecraft-cli stop
   --notes "cosa ho fatto"`).

Sul PC del cliente compare la finestra della connessione attiva: vede che sei collegato e può
chiudere quando vuole. Per i PC che gestisci stabilmente RustDesk permette una password
permanente, da usare solo con il consenso `unattended_access`. Nel gestionale si segna
`unattended: true` (la password resta solo nel client).

## Il gestionale da riga di comando

```sh
remotecraft-cli customers rossi                # cerca clienti
remotecraft-cli customer "Studio Rossi"        # scheda: PC, interventi, privacy
remotecraft-cli who 123456789                  # di chi è questo PC?
remotecraft-cli start --device 123456789 --summary "Stampante"
remotecraft-cli stop --notes "Driver aggiornato"
remotecraft-cli running
remotecraft-cli report --from 2026-10-01 --to 2026-11-01 --by customer
remotecraft-cli review                          # cosa manca nel registro GDPR
remotecraft-cli export "Studio Rossi" --out rossi.json
remotecraft-cli run customer.add '{"name": "Bar Aurora", "tax_id": "IT00112233445"}'
remotecraft-cli commands                        # tutti i comandi e i parametri
```

`--demo` usa un gestionale dimostrativo in memoria, `--home DIR` sceglie la cartella dei dati e
`--as NOME` il tecnico.

## Documentazione

- [docs/deploy.md](docs/deploy.md): installazione, integrazione con il framework (token, utenti,
  API), backup, aggiornamenti.
- [docs/api.md](docs/api.md): API HTTP e comandi.
- [docs/mcp.md](docs/mcp.md): server MCP (Claude Code, Claude Desktop, MCP per tecnico).
- [docs/gdpr.md](docs/gdpr.md): come il gestionale aiuta con informativa, consensi, diritti e
  conservazione.
- [docs/architecture.md](docs/architecture.md): crate, livelli, comandi, archiviazione.

## Sviluppo

```sh
cargo xtask ci        # fmt, clippy -D warnings, test, controllo dei livelli, build wasm
cargo run -p remotecraft -- --demo                     # l'app desktop sul gestionale demo
cd apps/remotecraft-web && trunk serve                 # l'app web su http://127.0.0.1:8783/ (con remotecraft-cli serve --demo su 127.0.0.1:7979; un altro server: ?server=URL, poi ricordato)
cargo run -p remotecraft-ui-egui --example snapshot -- out.png --lang it --screen customers --open 0
```

L'ultimo comando disegna l'interfaccia in un PNG senza finestra né GPU, per guardarla mentre
la si modifica.

## Licenze

Il codice di RemoteCraft è distribuito sotto licenza MIT OR Apache-2.0.

L'immagine Docker contiene i binari **non modificati** di RustDesk Server OSS (`hbbs`, `hbbr`),
copiati dall'immagine ufficiale `rustdesk/rustdesk-server`. Sono distribuiti sotto licenza
AGPL-3.0 e il sorgente è su https://github.com/rustdesk/rustdesk-server. Usarli così come sono,
anche per assistere clienti, è consentito. RemoteCraft non ne include né ne modifica il codice:
dialoga con loro solo via rete e legge la chiave pubblica. Vedi [ATTRIBUTION.md](ATTRIBUTION.md).

La versione OSS del server non ha console web, rubrica condivisa, client web né generatore di
client personalizzati, che sono funzioni di RustDesk Server Pro. Si può passare a Pro più
avanti tenendo la stessa chiave.
