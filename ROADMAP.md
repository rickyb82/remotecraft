# RemoteCraft roadmap

> **Stage:** 0.1: the RustDesk Server and the support desk in one container, with API, MCP
> and CLI, tested with in-memory and on-disk desks, a local HTTP server and the container itself.

## Now (0.1, done)

- Workspace in the craft layout: `core` → `engine` → `mcp` → `remotecraft-cli`.
- Customers (companies and people, tax id, contacts, tags, duplicate check, archive), devices
  with the RustDesk ID (normalized, unique among active devices, connect link), interventions
  (timer or logged afterwards, kinds, billable, notes appended on stop), time report with
  per-intervention rounding and minimum.
- GDPR record: notice versions, append-only deliveries, consents and withdrawals by purpose
  with method, signer, evidence and expiry; remote work blocked without consent (override with
  an audited reason); export, erasure by anonymisation, review, audit log without personal data.
- RustDesk: hbbs + hbbr in the same image as the desk, supervised; key read from the server's
  files; client setup values, import config string and self-configuring Windows installer name;
  port check (`rustdesk.check`, container health check).
- HTTP API: `POST /api/cmd`, read-only REST routes, service token naming the technician,
  per-technician tokens; MCP server (37 tools, 4 prompts, 2 resources), local or against the
  server; CLI.

- The MailCraft-style egui interface, web (wasm, served by the container) and desktop:
  customers with the privacy strip and the GDPR erasure, devices with *Connect*, interventions
  with the shared period filter, the docked intervention card and the timer chip, the privacy
  review, notices and audit log, the time report with CSV export, settings with the setup guide
  and the RustDesk client setup; light/dark, Italian/English, keyboard shortcuts. It reads the
  desk through `DeskSession` (local, or mirrored from `serve` over HTTP) and acts only through
  engine commands.

## Next (0.2)

- Printable consent form generated from the current notice, and attachments (scans of signed
  forms) stored with the privacy event.
- Monthly report export (CSV) and invoice lines.
- Contracts and prepaid hour packages per customer.
- Import of customers and devices from CSV.
- RustDesk Server Pro support (API address, address book sync) when the desk upgrades.

## Progress log

- 2026-10-13: RemoteCraft 0.1: the desk (core, engine, MCP, CLI and HTTP API) and the
  all-in-one container with RustDesk Server OSS 1.1.16.
- 2026-10-14: the graphical interface (egui, web and desktop), `DeskSession`, `RemoteDesk`,
  `GET /api/snapshot`, the web app served by the container.
- 2026-10-14: adversarial review of the interface, the remote session and the server; 37
  confirmed defects fixed, each with a regression test. Among them: static files confined to
  their directory (no encoded absolute paths, symlinks or special files; size cap), revisions
  unique per process, the snapshot naming the desk's actor, `rustdesk.check` probing off the
  UI thread and in parallel, the desktop picking up changes from MCP/CLI, `RemoteDesk` with a
  monotonic clock, a poll timeout and problem classes, consent checks when an intervention
  becomes remote (`intervention.update` with `consent_override`), put-backs that would
  duplicate a RustDesk ID refused, Ctrl+Enter saving a form only from its own fields, the
  running card saving its summary when closed, inclusive report periods, singular counts,
  narrow layouts from 1240 px, dark-theme contrast, and `views::review` grouped by customer
  (one pass instead of customers × records).
- 2026-10-14: verification of those fixes (all 37 confirmed) and a second security review of the
  server, the remote session and the apps; 6 regressions and 13 findings fixed. A server
  without a service token answers only on a loopback `Host` and origin (`--allow-origin` for
  others); the server requires JSON for commands, authenticates before reading a body (1 MiB),
  caps connections (64) and the request head (10 s, 16 KiB), streams static files with cache
  headers, sends `Referrer-Policy: no-referrer` and needs a 32-character service token; `Data::repair` clears RustDesk IDs that are not IDs; the Docker relays run
  as their own user without the desk's token; private files are created 0600. The web app takes
  the token from `#token=`, strips it from the address bar, pairs it with its server, never
  sends it over plain http to another machine, and can forget it from Settings; hosts open only
  valid `rustdesk://` links (no shell on Windows); answers are streamed with a 64 MiB cap and a
  slow snapshot is no longer thrown away. The running card keeps its text when the mirror lags,
  and a start from a row no longer discards an unsent form.
- 2026-10-14: a third pass over those fixes. A wrong token no longer holds a connection slot
  (no fixed wait: 32-character tokens cannot be guessed); the RustDesk relays keep running when
  the desk's token is missing or too short (the desk API stays off and says why) and the desk
  is restarted on its own if it stops; the entrypoint never follows a symlink as root and the
  key file is read only when it is a small regular file; `mcp --server` works against a
  tokenless local server; the 503 carries the security headers; the server marks the
  `index.html` it serves, so the web app uses its own origin only there (static hosts and
  `trunk serve` go back to the saved server); forgetting the token holds in every open tab;
  answers that would expand too much in memory are refused before parsing; Ctrl+R reloads the
  screen's data in the browser instead of the page; Cmd is shown only in the macOS desktop
  app; starts keep their warnings, and a start from a row on the customer of a typed Start
  form takes over the form instead of starting a second timer; Ctrl+R works from text fields
  too; the running card waits for its own command by job, so a stop answered after it moved
  on no longer leaves it stuck.
