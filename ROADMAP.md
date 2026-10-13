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

## Next (0.2)

- An egui interface (web and desktop) on the same commands, as in MailCraft: customer card,
  devices with a *Connect* button, the running timer, the privacy panel.
- Printable consent form generated from the current notice, and attachments (scans of signed
  forms) stored with the privacy event.
- Monthly report export (CSV) and invoice lines.
- Contracts and prepaid hour packages per customer.
- Import of customers and devices from CSV.
- RustDesk Server Pro support (API address, address book sync) when the desk upgrades.

## Progress log

- 2026-10-13: RemoteCraft 0.1: the desk (core, engine, MCP, CLI and HTTP API) and the
  all-in-one container with RustDesk Server OSS 1.1.16.
