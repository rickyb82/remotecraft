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
- Layers (checked by `cargo xtask layers`): `core` (L0) → `engine` (L5) → `mcp` (L6) →
  `apps/remotecraft-cli` (L7). Only the app touches the network (`ureq`, the TCP probe injected
  into the engine for `rustdesk.check`).
- **Everything is a command** (`crates/engine/src/cmd.rs`). The CLI, the HTTP API and the MCP
  server call the same commands; a new feature is a command first, then a route or a tool.
- RustDesk binaries come unmodified from the official image (AGPL-3.0); never copy RustDesk code
  into the repository. No vendor assets; any asset file needs a row in `ATTRIBUTION.md`.

## Working
- Quality gate before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, layers).
- Rust is pinned in `rust-toolchain.toml`.
- Code, comments and engine messages in English; user docs (`README.md`, `docs/`) in Italian
  except `docs/architecture.md`.
- Update `ROADMAP.md` (progress log) and the docs in `docs/` when work lands.
