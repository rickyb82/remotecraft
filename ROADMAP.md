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
