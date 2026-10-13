# Architecture

> **Last updated:** 2026-10-13

| Crate | Layer | What it holds |
|---|---|---|
| `crates/core` (`remotecraft-core`) | L0 | The model: customers, contacts, devices, interventions, preferences (`model.rs`); privacy notices, the append-only GDPR event record and purpose states (`privacy.rs`); RustDesk IDs, connect links, the import config string and the self-configuring installer name (`rustdesk.rs`); times typed by people (`time.rs`); user-only atomic files (`storage.rs`); the demo desk (`mock.rs`); `limits`. No network. |
| `crates/engine` (`remotecraft-engine`) | L5 | `Desk`: the data, its file, the actor, the clock, the audit log; the command table and handlers (`cmd.rs`); hostile-parameter readers (`params.rs`); reference resolution and JSON views (`views.rs`). The network probe for `rustdesk.check` is injected by the app. |
| `crates/mcp` (`remotecraft-mcp`) | L6 | The MCP server over stdio: tools, prompts, resources, each tool an engine command run through a `Runner` (the local `Desk` or a server). |
| `apps/remotecraft-cli` | L7 | The CLI, `serve` (HTTP API, tokens) and `mcp` (local or `--server URL --token T` over `ureq`); the TCP probe. |
| `docker/`, `Dockerfile` | — | One image: the official hbbs/hbbr binaries plus `remotecraft-cli serve`, supervised by `entrypoint.sh`. |
| `xtask` | — | `cargo xtask ci`, `cargo xtask layers`. |

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
