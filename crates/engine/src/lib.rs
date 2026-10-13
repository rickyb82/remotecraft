//! RemoteCraft's engine (layer L5): the [`Desk`] session on the desk file, the command table
//! (`cmd.rs`) the CLI, the HTTP API and the MCP server all call, and the audit log.
//!
//! Every command runs on a copy of the data's state: if it fails, or the file cannot be
//! written, the desk is left exactly as it was. Every change is appended to `audit.jsonl`
//! (who, when, which command, which records — never the personal data itself).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cmd;
pub mod params;
mod session;
mod views;

use std::path::{Path, PathBuf};

pub use cmd::{COMMANDS, Command};
pub use remotecraft_core as core;
pub use session::{Completed, DeskSession, Session};
use remotecraft_core::chrono::{DateTime, Utc};
use remotecraft_core::{Data, storage, time};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Errors of the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnknownCommand(String),
    Core(remotecraft_core::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(c) => write!(f, "unknown command `{c}` (see `commands`)"),
            Self::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<remotecraft_core::Error> for Error {
    fn from(e: remotecraft_core::Error) -> Self {
        Self::Core(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Where a desk keeps its files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub dir: PathBuf,
    pub desk: PathBuf,
    pub audit: PathBuf,
}

impl Paths {
    pub fn under(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf(), desk: dir.join("desk.json"), audit: dir.join("audit.jsonl") }
    }

    /// `$REMOTECRAFT_HOME` or the platform data directory.
    pub fn default_dirs() -> Self {
        Self::under(&storage::home_dir())
    }
}

/// One line of the audit log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditEntry {
    pub at: String,
    pub actor: String,
    pub command: String,
    /// The records touched: `customer:3`, `device:7`, `intervention:12`…
    pub targets: Vec<String>,
    /// A short reason when the command carries one (an erasure, a consent override).
    pub detail: String,
}

/// The desk as a remote interface sees it (`GET /api/snapshot`): everything, by revision, so
/// clients fetch it only when it changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub revision: u64,
    pub data: Data,
    /// The technician the client works as (from its token).
    pub actor: String,
    pub problems: Vec<String>,
    /// The RustDesk server next to the desk: name and key, when the container knows them.
    pub rustdesk_host: String,
    pub rustdesk_key: String,
}

/// Audit entries kept in memory when the desk has no files (tests, demo).
const MAX_MEMORY_AUDIT: usize = 10_000;

/// What a running command knows and collects.
pub(crate) struct Ctx {
    pub actor: String,
    pub now: DateTime<Utc>,
    pub targets: Vec<String>,
    pub detail: String,
}

impl Ctx {
    pub fn touch(&mut self, kind: &str, id: u64) {
        let t = format!("{kind}:{id}");
        if !self.targets.contains(&t) && self.targets.len() < 1_000 {
            self.targets.push(t);
        }
    }
}

/// A support desk: the data, where it is stored, and who is working.
pub struct Desk {
    data: Data,
    paths: Option<Paths>,
    /// The desk file's modification time when it was last read or written: another process
    /// (the CLI next to the server) may have changed it since.
    stamp: Option<std::time::SystemTime>,
    clock: Option<DateTime<Utc>>,
    actor: String,
    memory_audit: Vec<AuditEntry>,
    problems: Vec<String>,
    /// What the host knows about the RustDesk server next to it.
    rustdesk: RustDeskEnv,
    /// Bumped on every change (and every reload), for remote interfaces.
    revision: u64,
}

/// Opens a TCP connection to `host:port` and answers how long it took (ms), or why it failed.
/// Injected by the app: the engine itself never touches the network.
pub type Probe = Box<dyn Fn(&str, u16) -> std::result::Result<u64, String> + Send>;

/// The RustDesk server running next to the desk (same container or machine).
#[derive(Default)]
pub(crate) struct RustDeskEnv {
    /// Its public key (`id_ed25519.pub`), shown when the preference is empty.
    pub key: String,
    /// The name clients use, when the preference is empty.
    pub host: String,
    /// Where `rustdesk.check` connects by default (`127.0.0.1` inside the container).
    pub check_host: String,
    pub probe: Option<Probe>,
}

impl Desk {
    /// Opens (or starts) the desk in `paths`.
    pub fn open(paths: Paths) -> Result<Self> {
        let mut data = storage::load(&paths.desk)?;
        let problems = data.repair();
        let stamp = file_stamp(&paths.desk);
        Ok(Self {
            data,
            paths: Some(paths),
            stamp,
            clock: None,
            actor: default_actor(),
            memory_audit: Vec::new(),
            problems,
            rustdesk: RustDeskEnv::default(),
            revision: 1,
        })
    }

    /// A desk that lives only in memory.
    pub fn in_memory(data: Data) -> Self {
        let mut data = data;
        let problems = data.repair();
        Self {
            data,
            paths: None,
            stamp: None,
            clock: None,
            actor: default_actor(),
            memory_audit: Vec::new(),
            problems,
            rustdesk: RustDeskEnv::default(),
            revision: 1,
        }
    }

    /// The demo desk, in memory.
    pub fn demo() -> Self {
        let mut d = Self::in_memory(remotecraft_core::mock::demo(time::now()));
        d.actor = "riccardo".into();
        d
    }

    /// Fixes the clock (tests).
    pub fn set_clock(&mut self, now: DateTime<Utc>) {
        self.clock = Some(now);
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.clock.unwrap_or_else(time::now)
    }

    /// Who works when a command names nobody (the CLI's user, `--as`).
    pub fn set_actor(&mut self, actor: &str) {
        let a = remotecraft_core::line(actor);
        if !a.is_empty() {
            self.actor = a;
        }
    }

    pub fn actor(&self) -> &str {
        &self.actor
    }

    /// The RustDesk server's public key shown by `rustdesk.setup` when the preference is empty.
    pub fn set_default_rustdesk_key(&mut self, key: &str) {
        self.rustdesk.key = remotecraft_core::line(key);
    }

    /// The RustDesk server's public name, when the preference is empty.
    pub fn set_default_rustdesk_host(&mut self, host: &str) {
        self.rustdesk.host = remotecraft_core::line(host);
    }

    /// How `rustdesk.check` reaches the server: where, and with what.
    pub fn set_rustdesk_probe(&mut self, check_host: &str, probe: Probe) {
        self.rustdesk.check_host = remotecraft_core::line(check_host);
        self.rustdesk.probe = Some(probe);
    }

    pub fn data(&self) -> &Data {
        &self.data
    }

    pub fn paths(&self) -> Option<&Paths> {
        self.paths.as_ref()
    }

    /// Runs a command as the desk's actor.
    pub fn execute(&mut self, command: &str, params: &Value) -> Result<Value> {
        let actor = self.actor.clone();
        self.execute_as(&actor, command, params)
    }

    /// Runs a command for `actor` (the technician the framework or the token names).
    pub fn execute_as(&mut self, actor: &str, command: &str, params: &Value) -> Result<Value> {
        let Some(spec) = COMMANDS.iter().find(|c| c.id == command) else {
            return Err(Error::UnknownCommand(remotecraft_core::clip(command, 80).to_string()));
        };
        let empty = json!({});
        let params = match params {
            Value::Object(_) => params,
            Value::Null => &empty,
            _ => return Err(remotecraft_core::Error::Invalid("params must be an object".into()).into()),
        };
        self.reload_if_changed()?;
        let actor = remotecraft_core::line(actor);
        let mut ctx =
            Ctx { actor: if actor.is_empty() { self.actor.clone() } else { actor }, now: self.now(), targets: Vec::new(), detail: String::new() };
        if !spec.writes {
            let out = cmd::dispatch(self, &mut ctx, command, params)?;
            if spec.audited {
                self.audit(&ctx, command);
            }
            return Ok(out);
        }
        let before = self.data.clone();
        let out = match cmd::dispatch(self, &mut ctx, command, params) {
            Ok(v) => v,
            Err(e) => {
                self.data = before;
                return Err(e);
            }
        };
        if let Err(e) = self.save() {
            self.data = before;
            return Err(e);
        }
        self.audit(&ctx, command);
        Ok(out)
    }

    pub(crate) fn data_mut(&mut self) -> &mut Data {
        &mut self.data
    }

    pub(crate) fn rustdesk(&self) -> &RustDeskEnv {
        &self.rustdesk
    }

    pub(crate) fn problems(&self) -> &[String] {
        &self.problems
    }

    fn save(&mut self) -> Result<()> {
        if let Some(p) = &self.paths {
            storage::save(&p.desk, &self.data)?;
            self.stamp = file_stamp(&p.desk);
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// The current revision: changes whenever the data does.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The whole desk for a remote interface working as `actor`, or `None` when nothing
    /// changed since `since`. Reads the file again first, so another process's changes show.
    pub fn snapshot(&mut self, since: u64, actor: &str) -> Result<Option<Snapshot>> {
        self.reload_if_changed()?;
        if since == self.revision {
            return Ok(None);
        }
        Ok(Some(Snapshot {
            revision: self.revision,
            data: self.data.clone(),
            actor: remotecraft_core::line(actor),
            problems: self.problems.clone(),
            rustdesk_host: self.rustdesk.host.clone(),
            rustdesk_key: self.rustdesk.key.clone(),
        }))
    }

    /// Reads the file again when another process wrote it.
    fn reload_if_changed(&mut self) -> Result<()> {
        let Some(p) = &self.paths else { return Ok(()) };
        let now = file_stamp(&p.desk);
        if now.is_some() && now != self.stamp {
            let mut data = storage::load(&p.desk)?;
            self.problems = data.repair();
            self.data = data;
            self.stamp = now;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }

    fn audit(&mut self, ctx: &Ctx, command: &str) {
        let entry = AuditEntry {
            at: time::stamp(ctx.now),
            actor: ctx.actor.clone(),
            command: command.into(),
            targets: ctx.targets.clone(),
            detail: ctx.detail.clone(),
        };
        match &self.paths {
            Some(p) => {
                if let Ok(line) = serde_json::to_string(&entry) {
                    let _ = storage::append_private(&p.audit, &line);
                }
            }
            None => {
                if self.memory_audit.len() >= MAX_MEMORY_AUDIT {
                    self.memory_audit.remove(0);
                }
                self.memory_audit.push(entry);
            }
        }
    }

    /// The audit log, oldest first (the newest `max` entries).
    pub fn audit_entries(&self, max: usize) -> Vec<AuditEntry> {
        let all: Vec<AuditEntry> = match &self.paths {
            None => self.memory_audit.clone(),
            Some(p) => {
                let text = read_tail(&p.audit, 64 * 1024 * 1024);
                text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
            }
        };
        let skip = all.len().saturating_sub(max);
        all.into_iter().skip(skip).collect()
    }
}

/// The last `max` bytes of a file, from a line start.
fn read_tail(path: &Path, max: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = f.take(max).read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text }
}

fn file_stamp(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn default_actor() -> String {
    let user = std::env::var("REMOTECRAFT_USER").or_else(|_| std::env::var("USER")).or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let user = remotecraft_core::line(&user);
    if user.is_empty() { "tecnico".into() } else { user }
}

#[cfg(test)]
mod tests;
