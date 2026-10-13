# RemoteCraft

Assistenza remota con **RustDesk Server** ospitato sul tuo server, accanto a Emily. Questo
repository contiene la configurazione Docker e la guida per metterlo in funzione.

RustDesk è composto da due parti:

- **Il server** (`hbbs` per gli ID e l'avvio delle connessioni, `hbbr` per il relay): gira qui,
  in Docker. Non vede il contenuto delle sessioni, che sono cifrate tra i due client.
- **Il client RustDesk**, da installare sia sul tuo PC sia su quello da assistere, scaricato
  dal sito ufficiale e configurato per usare il tuo server invece di quelli pubblici.

## Cosa offre la versione gratuita

La versione open source del server (OSS) offre solo il servizio ID e il relay. Non include:

- la console web di amministrazione (dispositivi, utenti, gruppi, log);
- la rubrica condivisa tra tecnici e l'accesso con utenti, 2FA, LDAP o OIDC;
- il client web ospitato da te (controllo remoto da una scheda del browser);
- il generatore di client personalizzati con il tuo server già impostato.

Queste funzioni sono di **RustDesk Server Pro**, a pagamento. Si può partire con la versione
gratuita e passare a Pro più avanti tenendo la stessa chiave del server.

## 1. Preparare la rete

RustDesk non passa dal proxy HTTPS di Emily: usa porte TCP e UDP proprie.

1. Crea un record DNS, per esempio `rustdesk.siriustec.it`, che punta all'IP pubblico
   dell'ufficio.
2. Sul router inoltra queste porte verso il server (per esempio `192.168.10.80`):

   | Porta | Protocollo | A cosa serve |
   |---|---|---|
   | 21115 | TCP | test del tipo di NAT |
   | 21116 | TCP e UDP | registrazione degli ID e avvio delle connessioni |
   | 21117 | TCP | relay, quando la connessione diretta non riesce |

3. Se il server ha un firewall attivo, apri le stesse porte. Con `ufw`:

   ```sh
   sudo ufw allow 21115:21117/tcp
   sudo ufw allow 21116/udp
   ```

Per usarlo solo dentro la LAN bastano l'IP interno del server e nessun inoltro sul router.

## 2. Avviare il server

Sul server, nella cartella dove tieni i servizi Docker:

```sh
git clone https://github.com/rickyb82/remotecraft
cd remotecraft
docker compose up -d
docker compose ps
```

Al primo avvio il server genera la sua coppia di chiavi nella cartella `data/`. La chiave
pubblica serve a tutti i client:

```sh
cat data/id_ed25519.pub
```

La cartella `data/` contiene anche la chiave privata. Non va mai nel repository (è già in
`.gitignore`) e va inclusa nei backup: se la perdi, devi riconfigurare tutti i client.

## 3. Configurare i client

Su ogni PC, il tuo e quelli da assistere:

1. Scarica RustDesk dal sito ufficiale, `rustdesk.com`, oppure dalla pagina delle release del
   progetto su GitHub.
2. Apri **Impostazioni → Rete → Server ID/Relay** e compila:

   | Campo | Valore |
   |---|---|
   | Server ID | `rustdesk.siriustec.it` |
   | Server relay | `rustdesk.siriustec.it` |
   | Server API | vuoto (serve solo con Pro) |
   | Chiave | il contenuto di `data/id_ed25519.pub` |

3. In basso nella finestra principale deve comparire **Pronto**. Se compare un errore di
   connessione, controlla DNS, inoltro delle porte e chiave.

## 4. Fare un'assistenza

1. La persona da assistere apre RustDesk e ti comunica il suo **ID** e la **password monouso**
   mostrati nella finestra principale.
2. Tu inserisci l'ID, ti colleghi e inserisci la password.
3. Sul suo PC compare la finestra della connessione attiva: vede che sei collegato e può
   chiudere la sessione in qualsiasi momento.

Per i PC che gestisci stabilmente, RustDesk permette di impostare una password permanente
dalle impostazioni di sicurezza del client. Usala solo sulle macchine di cui hai la
responsabilità, con il consenso di chi le usa.

## 5. Manutenzione

- **Aggiornare:** `docker compose pull && docker compose up -d`. Dopo il primo avvio fissa
  la versione in un file `.env` (`RUSTDESK_TAG=…`, il numero mostrato da
  `docker compose images`), così gli aggiornamenti avvengono quando lo decidi tu.
- **Log:** `docker compose logs -f hbbs hbbr`.
- **Verificare le porte da fuori:** da una rete esterna, `nc -vz rustdesk.siriustec.it 21116`.
- **Backup:** la cartella `data/`.

## Licenze

RustDesk Server OSS è distribuito sotto licenza AGPL-3.0. Usarlo così com'è, anche per
assistere clienti, è consentito. Se ne modifichi il codice e lo offri come servizio, devi
pubblicare le modifiche. Questo repository contiene solo configurazione e documentazione.
