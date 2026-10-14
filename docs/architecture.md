# Architecture

> **Last updated:** 2026-10-14

| Crate | Layer | What it holds |
|---|---|---|
| `crates/core` (`remotecraft-core`) | L0 | The model: customers, contacts, devices, interventions, preferences (`model.rs`); privacy notices, the append-only GDPR event record and purpose states (`privacy.rs`); RustDesk IDs, connect links, the import config string and the self-configuring installer name (`rustdesk.rs`); times typed by people (`time.rs`); user-only atomic files (`storage.rs`); the demo desk (`mock.rs`); `limits`. No network. |
| `crates/engine` (`remotecraft-engine`) | L5 | `Desk`: the data, its file, the actor, the clock, the audit log; the command table and handlers (`cmd.rs`); hostile-parameter readers (`params.rs`); reference resolution and JSON views (`views.rs`). The network probe for `rustdesk.check` is injected by the app. |
| `crates/mcp` (`remotecraft-mcp`) | L6 | The MCP server over stdio: tools, prompts, resources, each tool an engine command run through a `Runner` (the local `Desk` or a server). |
| `crates/remote` (`remotecraft-remote`) | L6 | `RemoteDesk`: a `DeskSession` that mirrors a `remotecraft-cli serve` server over HTTP (`ehttp`, native and wasm): `POST /api/cmd`, `GET /api/snapshot?since=N`. |
| `crates/ui-egui` (`remotecraft-ui-egui`) | L6 | `DeskApp`: the MailCraft-style interface (top bar with search, navigation, lists with a detail pane, the docked intervention card, settings, toasts, shortcuts, i18n, themes, icons). The `snapshot` example renders it offscreen. |
| `apps/remotecraft` | L7 | The desktop app (eframe; opens `rustdesk://` links and saves exports through the system). |
| `apps/remotecraft-cli` | L7 | The CLI, `serve` (HTTP API, tokens, the web app's files) and `mcp` (local or `--server URL --token T` over `ureq`); the TCP probe. |
| `apps/remotecraft-web` | L7 | The web app (wasm32, trunk): the same `DeskApp` on a `RemoteDesk`. |
| `docker/`, `Dockerfile` | — | One image: the official hbbs/hbbr binaries plus `remotecraft-cli serve`, supervised by `entrypoint.sh`. |
| `xtask` | — | `cargo xtask ci`, `cargo xtask layers`. |

## Sessions

The interface is written against the `DeskSession` trait (`crates/engine/src/session.rs`):
the data to show, the actor, `execute(command, params) -> job id`, `pump() -> Vec<Completed>`.
Locally it is `Session` (a `Desk` in the process: commands run at once and are reported by the
next `pump`); in the browser it is `RemoteDesk`, which posts commands to the server and polls
`GET /api/snapshot?since=N` (every 2 s idle, 0.4 s while commands are in flight) for the whole
desk when its revision moved. The `Desk` bumps its revision on every save and on every reload
of a file another process wrote. Results are routed back to the screen that asked by an
`Intent` the interface stores per job, so desktop and web behave the same.

- **Revisions** start at a value unique to the process and the desk (the time mixed with the
  process's random hash seed, below 2^53 so JSON clients read it exactly), so a client that
  kept revision N across a server restart is never told `unchanged` by a new server that
  happens to be at N. A snapshot names the actor commands run as: with no user named, the
  desk's own.
- **Local session.** `rustdesk.check` waits on the network (3 s per address when a firewall
  drops the packets), so `Session` prepares it on the caller's thread
  (`Desk::prepare_rustdesk_check`: parameters and host) and probes on another thread, the three
  ports in parallel (`RustDeskCheck::run`); the report arrives through `pump` and counts in
  `jobs_pending`. Wasm has no threads and no probe. `serve` likewise probes after releasing the
  desk's lock; the CLI and the MCP server simply wait. Every `pump` also calls `Desk::refresh`
  (a stat of the desk file), and the desktop app repaints at least every 2 s, so what the MCP
  server or the CLI writes shows in an idle window.
- **Remote session.** Polls are timed on a monotonic clock (`web_time::Instant`). A poll still
  unanswered after its window (30 s; the browser's fetch has no timeout) is given up and
  another goes out with the window doubled, up to 60 s, back to 30 s on the next answer; the
  late answer of a given-up poll still counts unless a newer one was already applied (each
  carries its poll number), so a slow download of a large desk is not thrown away. Answers are
  streamed and cut off past 64 MiB (an early `Content-Length` check first), so a hostile
  server cannot fill the client's memory. `problem()` is `connecting` until the first answer,
  then `None` or a stable prefix and the detail: `unauthorized: ` (401/403, a revoked token, or
  a token withheld from plain `http://` to another machine), `incompatible: ` (an answer the
  client cannot read: not a RemoteCraft server, another version, too large) or `unreachable: `
  (no connection, no answer, 5xx from a proxy). Only a poll that succeeds clears it; command
  answers do not.
- **Hosts.** The interface asks the apps to open only `rustdesk://connection/new/<ID>` built
  from an ID `rustdesk::normalize` accepts (`connect_link`), never a link from an answer; the
  apps check it again (the desktop opens it without a shell: `explorer.exe` on Windows, never
  `cmd /C start`). The web app reads `token`, `server` and `lang` from the URL fragment first
  (`#token=…` never reaches the server or a proxy's logs), removes the token from the address
  bar, keeps the server and the token as one pair (a saved token goes only to its own server;
  the page's own origin comes before a saved server) and can forget them from Settings.

## Server

`remotecraft-cli serve` is a small thread-per-connection HTTP server; its defences are in
`apps/remotecraft-cli/src/serve.rs`. At most 64 connections live at once (more get 503), the
request head must arrive whole within 10 s and 16 KiB (100 headers), and the caller is
authenticated from the head before any body is read (bodies up to 1 MiB, `POST /api/cmd` only,
`Content-Type: application/json` or 415). A failed token waits 250 ms; tokens are compared in
constant time and the service token needs 32 characters. Without a service token the server
binds only to loopback and answers only to a loopback `Host` (no DNS rebinding) and to loopback
origins or those given with `--allow-origin`; with one, the API is bearer-only and any origin
may call it. Static files (`--static`, the web app) are served only from inside their
directory: the decoded path must be plain components, the canonical file must lie under the
canonical directory, and only regular files up to 64 MiB are streamed (hashed bundles cached as
immutable, `index.html` never). Every answer carries `Referrer-Policy: no-referrer` and
`X-Content-Type-Options: nosniff`.

## Commands

Every user-visible action is a `Command` in `crates/engine/src/cmd.rs` (id, label, menu,
params, `writes`, `audited`). `Desk::execute_as(actor, id, params)`:

1. rejects unknown commands and non-object params;
2. reloads the desk file if another process wrote it;
3. runs the handler on the live data, keeping a copy of the state before it;
4. for writing commands, saves atomically (keeping `desk.json.bak`); if the handler fails or the
   file cannot be written, restores the copy, so the desk is never half-changed;
5. appends an audit entry (time, actor, command, touched record ids, a short reason for erasures
   and consent overrides) for writing and audited commands (`device.connect`, `gdpr.export`).

`every_command_survives_empty_params` and `every_command_survives_hostile_params` run every
command with empty and junk parameters and check the desk is unchanged on failure.

## Interface

One `DeskApp` on top of the session: `Screen` (customers, devices, interventions, privacy,
reports, settings), the top bar (search that jumps to a PC when the query is a RustDesk ID),
the navigation with counts, each screen's list in the centre and a resizable detail pane on
the right, the intervention card docked bottom right (start, running timer, stop with notes),
a confirmation for actions that cannot be undone, toasts, and host `Request`s (open a link,
save a file) served by the apps. Lists and privacy states are computed from the mirror with
the engine's own helpers (`engine::views`), never by the interface alone. The per-customer
states are rebuilt when the revision moves (and once a minute, for expiries) in one pass:
`views::ByCustomer` groups devices, interventions and privacy events by customer, and
`views::review_grouped` reads the groups, so the cost grows with the records, not with
customers × records (`views::review` is the same review for the CLI and MCP). Every label is
English in code and translated in `i18n/it.rs` or the screen's `LABELS_IT`; a test checks the
tables are complete and that a label repeated across screens has one Italian.

## Storage

`desk.json` holds everything (`Data`: counters, customers, devices, interventions, notices,
privacy events, preferences), every struct `#[serde(default)]` so older and newer files load;
`Data::repair` moves the counters past the highest ids and drops duplicate ids from a file
edited by hand. A file that does not parse is an error, never replaced by an empty desk. Times
are UTC RFC 3339 stamps, which sort as text.

## The GDPR record

Privacy events are only ever appended: `notice_delivered`, `consent_given`,
`consent_withdrawn`, each with the notice version, method, time, signer, evidence and who
recorded it. A purpose's state is computed from its latest event, the event's expiry and the
desk's validity preference. Erasure anonymises in place and keeps the record without names.

## RustDesk

The desk never talks the RustDesk protocol. It stores IDs, builds `rustdesk://` links and the
client configuration (host, relay, key, the reversed-base64 import string, the installer name)
from the preferences or from what the container passes (`RUSTDESK_HOST`, the key file), and
checks the server's TCP ports through the probe the app injects.
