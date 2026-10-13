# RemoteCraft — instructions for agents

RemoteCraft is the RustDesk Server OSS plus an open-source, Rust-native support desk (customers,
their PCs with the RustDesk ID, interventions with times and notes, GDPR notice and consent
record) with an HTTP API and an MCP server, part of the *craft* family (`mailcraft`, …) and
following the same conventions.

## Never crash
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!` or `unsafe` in
  non-test code (crates carry `#![deny(clippy::unwrap_used, …)]`, `unsafe_code = "forbid"`).
- Errors are `Result` through `core::Error` / `engine::Error`; the HTTP API maps them to
  400/404/409/500.
- Input is hostile: files, HTTP requests, command and MCP parameters. Bounded parsing
  (`core::limits`), typed parameter readers (`engine::params`), `get()` over indexing, checked
  arithmetic, char-boundary clipping (`core::clip`).
- A failing command (or a failing save) leaves the desk exactly as it was (`Desk::execute_as`
  restores the copy). `every_command_survives_empty_params` and
  `every_command_survives_hostile_params` guard every command; every crash fix lands with a test.

## Privacy and security
- The GDPR record is append-only: never edit or delete privacy events except in `gdpr.erase`,
  which anonymises. The audit log holds ids, never personal data.
- Never store RustDesk passwords. Remote work checks the `remote_assistance` consent.
- Tools that delete or erase are described so an agent asks first; keep it that way.
- Tokens are compared in constant time; `serve` refuses to bind beyond localhost without one.

## Architecture
- Layers (checked by `cargo xtask layers`): `core` (L0) → `engine` (L5) → `mcp` / `remote` /
  `ui-egui` (L6) → `apps/remotecraft-cli`, `apps/remotecraft`, `apps/remotecraft-web` (L7).
  Only apps and `remote` touch the network (`ureq`, `ehttp`, the TCP probe injected into the
  engine for `rustdesk.check`); only apps use `eframe`/`rfd`.
- **The UI is thin**: it reads a `DeskSession` (the local `Session` or `RemoteDesk`), acts only
  through `DeskApp::run_as(command, params, Intent)` and applies results in `on_result`; it never
  mutates `Data`. Colours from `theme::Tokens`, icons from `icons.rs`, derived per-customer state
  from `derived.rs`; host-only actions (links, files) are `Request`s served by the apps.
- Interface strings are English in code; Italian in `i18n/it.rs` (shared) or the screen's own
  `LABELS_IT` table; engine messages are translated by prefix in `it.rs` `MESSAGES`.
- **Never break wasm**: `apps/remotecraft-web` must build for `wasm32-unknown-unknown`; use
  `web_time` instead of `std::time::{Instant, SystemTime}` in the UI.
- For UI work, look at the result:
  `cargo run -p remotecraft-ui-egui --example snapshot -- out.png --lang it --screen customers --open 0`.
- **Everything is a command** (`crates/engine/src/cmd.rs`). The CLI, the HTTP API and the MCP
  server call the same commands; a new feature is a command first, then a route or a tool.
- RustDesk binaries come unmodified from the official image (AGPL-3.0); never copy RustDesk code
  into the repository. No vendor assets; any asset file needs a row in `ATTRIBUTION.md`.

## Working
- Quality gate before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, layers, wasm).
- Rust is pinned in `rust-toolchain.toml`.
- Code, comments and engine messages in English; user docs (`README.md`, `docs/`) in Italian
  except `docs/architecture.md`.
- Update `ROADMAP.md` (progress log) and the docs in `docs/` when work lands.
