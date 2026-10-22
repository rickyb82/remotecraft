//! What an interface needs from a desk: the [`DeskSession`] trait, implemented directly by the
//! local [`Session`] (a [`Desk`] in this process) and by `remotecraft-remote` for a desk on a
//! server. Commands are queued and their results arrive through [`DeskSession::pump`], so the
//! interface has one code path whether the desk is here or on the server.

use remotecraft_core::Data;
use remotecraft_core::chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{Desk, Error, Result};

/// A finished command, as [`DeskSession::pump`] reports it.
#[derive(Clone, Debug)]
pub struct Completed {
    pub job: u64,
    pub command: String,
    pub result: Result<Value>,
}

/// What an interface needs from a desk: the data to show, who works, a way to run commands
/// and to learn their outcome.
pub trait DeskSession {
    /// The desk's records (a mirror, for a remote session).
    fn data(&self) -> &Data;
    /// The technician the session works as.
    fn actor(&self) -> &str;
    fn now(&self) -> DateTime<Utc>;
    /// Queues a command and answers its job id; the outcome comes from [`DeskSession::pump`]
    /// (at once for a local desk, when the server answers for a remote one).
    fn execute(&mut self, command: &str, params: &Value) -> u64;
    /// The commands finished since the last call, and, for a remote session, the latest state.
    fn pump(&mut self) -> Vec<Completed>;
    /// Commands queued whose outcome has not arrived yet.
    fn jobs_pending(&self) -> usize;
    /// Changes whenever the data does.
    fn revision(&self) -> u64;
    /// Why the session cannot work right now (a server out of reach), if so.
    fn problem(&self) -> Option<String> {
        None
    }
    /// The server a remote session talks to (empty for a local one).
    fn server(&self) -> String {
        String::new()
    }
    /// The RustDesk server next to the desk, when known: `(host, key)`.
    fn rustdesk_defaults(&self) -> (String, String) {
        (String::new(), String::new())
    }
    /// Problems found loading the desk file.
    fn problems(&self) -> Vec<String> {
        Vec::new()
    }
    /// Whether the operator's intranet is reachable (customers can be imported from it).
    fn intranet(&self) -> bool {
        false
    }
    fn italian(&self) -> bool {
        self.data().preferences.language != "en"
    }
}

/// A desk in this process, as a session: commands run at once and are reported by the next
/// [`DeskSession::pump`]. `rustdesk.check`, which waits on the network for seconds, probes on
/// other threads (natively) and is reported when it finishes, so the interface never freezes.
/// Every `pump` also reads the desk file again if another process (the MCP server, the CLI)
/// wrote it.
pub struct Session {
    desk: Desk,
    next_job: u64,
    done: Vec<Completed>,
    #[cfg(not(target_arch = "wasm32"))]
    background: Background,
}

/// Commands finishing on other threads.
#[cfg(not(target_arch = "wasm32"))]
struct Background {
    tx: std::sync::mpsc::Sender<Done>,
    rx: std::sync::mpsc::Receiver<Done>,
    pending: usize,
}

/// What a background thread hands back: a finished command, or the intranet's answer for a
/// command that `pump` then runs on the desk.
#[cfg(not(target_arch = "wasm32"))]
enum Done {
    Completed(Completed),
    Fetched { job: u64, fetch: Box<crate::IntranetFetch>, answer: std::result::Result<Value, String> },
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Background {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { tx, rx, pending: 0 }
    }
}

/// At most this many results wait for `pump`.
const MAX_DONE: usize = 10_000;

impl Session {
    pub fn new(desk: Desk) -> Self {
        Self {
            desk,
            next_job: 1,
            done: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            background: Background::default(),
        }
    }

    /// The demo desk, in memory.
    pub fn demo() -> Self {
        Self::new(Desk::demo())
    }

    pub fn desk(&self) -> &Desk {
        &self.desk
    }

    pub fn desk_mut(&mut self) -> &mut Desk {
        &mut self.desk
    }

    /// Runs a command now and answers its result (the CLI's way); it is not reported by `pump`.
    pub fn run(&mut self, command: &str, params: &Value) -> Result<Value> {
        self.desk.execute(command, params)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn background_pending(&self) -> usize {
        self.background.pending
    }

    #[cfg(target_arch = "wasm32")]
    fn background_pending(&self) -> usize {
        0
    }

    fn finish(&mut self, c: Completed) {
        if self.done.len() < MAX_DONE {
            self.done.push(c);
        }
    }

    /// Probes on another thread; the report arrives through `pump`. Where no thread can be
    /// started, probes here.
    #[cfg(not(target_arch = "wasm32"))]
    fn check_in_background(&mut self, job: u64, check: crate::RustDeskCheck) {
        let command = "rustdesk.check".to_string();
        let tx = self.background.tx.clone();
        let theirs = check.clone();
        let started = std::thread::Builder::new().name("remotecraft-check".into()).spawn(move || {
            // A probe that panics still answers, so the job never stays pending.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| theirs.run()))
                .map_err(|_| Error::Core(remotecraft_core::Error::Io("the RustDesk check failed".into())));
            let _ = tx.send(Done::Completed(Completed { job, command, result }));
        });
        match started {
            Ok(_) => self.background.pending = self.background.pending.saturating_add(1),
            Err(_) => self.finish(Completed { job, command: "rustdesk.check".into(), result: Ok(check.run()) }),
        }
    }

    /// Asks the intranet on another thread; `pump` runs the command with the answer. Where no
    /// thread can be started, asks here.
    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_in_background(&mut self, job: u64, fetch: crate::IntranetFetch) {
        let tx = self.background.tx.clone();
        let theirs = fetch.clone();
        let started = std::thread::Builder::new().name("remotecraft-intranet".into()).spawn(move || {
            let answer =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| theirs.run())).unwrap_or_else(|_| Err("the intranet call failed".into()));
            let _ = tx.send(Done::Fetched { job, fetch: Box::new(theirs), answer });
        });
        match started {
            Ok(_) => self.background.pending = self.background.pending.saturating_add(1),
            Err(_) => {
                let answer = fetch.run();
                let result = self.desk.execute_fetched(&fetch, answer);
                self.finish(Completed { job, command: fetch.command_id().to_string(), result });
            }
        }
    }
}

impl DeskSession for Session {
    fn data(&self) -> &Data {
        self.desk.data()
    }

    fn actor(&self) -> &str {
        self.desk.actor()
    }

    fn now(&self) -> DateTime<Utc> {
        self.desk.now()
    }

    fn execute(&mut self, command: &str, params: &Value) -> u64 {
        let job = self.next_job;
        self.next_job = self.next_job.wrapping_add(1).max(1);
        #[cfg(not(target_arch = "wasm32"))]
        if command == "rustdesk.check" {
            match self.desk.prepare_rustdesk_check(params) {
                Ok(check) => self.check_in_background(job, check),
                Err(e) => self.finish(Completed { job, command: command.to_string(), result: Err(e) }),
            }
            return job;
        }
        // The intranet answers in seconds (and not at all when it is down): asked off this
        // thread, the command runs with the answer on a later pump.
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(command, "intranet.search" | "customer.import") {
            let actor = self.desk.actor().to_string();
            match self.desk.prepare_intranet(&actor, command, params) {
                Ok(fetch) => self.fetch_in_background(job, fetch),
                Err(e) => self.finish(Completed { job, command: command.to_string(), result: Err(e) }),
            }
            return job;
        }
        let result = self.desk.execute(command, params);
        self.finish(Completed { job, command: command.to_string(), result });
        job
    }

    fn pump(&mut self) -> Vec<Completed> {
        // Another process's changes show without waiting for a command of this one; a file
        // that cannot be read now is read on a later pump (and reported by the next command).
        let _ = self.desk.refresh();
        #[cfg(not(target_arch = "wasm32"))]
        while let Ok(done) = self.background.rx.try_recv() {
            self.background.pending = self.background.pending.saturating_sub(1);
            match done {
                Done::Completed(c) => self.finish(c),
                Done::Fetched { job, fetch, answer } => {
                    let result = self.desk.execute_fetched(&fetch, answer);
                    self.finish(Completed { job, command: fetch.command_id().to_string(), result });
                }
            }
        }
        std::mem::take(&mut self.done)
    }

    fn jobs_pending(&self) -> usize {
        self.background_pending()
    }

    fn revision(&self) -> u64 {
        self.desk.revision()
    }

    fn rustdesk_defaults(&self) -> (String, String) {
        let env = self.desk.rustdesk();
        (env.host.clone(), env.key.clone())
    }

    fn intranet(&self) -> bool {
        self.desk.has_intranet()
    }

    fn problems(&self) -> Vec<String> {
        self.desk.problems().to_vec()
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_session_reports_results_through_pump() {
        let mut s = Session::demo();
        assert!(s.pump().is_empty());
        let before = s.revision();
        let a = s.execute("customer.add", &json!({"name": "Nuovo"}));
        let b = s.execute("customer.get", &json!({"customer": 999}));
        assert_ne!(a, b);
        let done = s.pump();
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].job, a);
        assert!(done[0].result.is_ok());
        assert!(done[1].result.is_err());
        assert!(s.pump().is_empty());
        assert_eq!(s.jobs_pending(), 0);
        assert!(s.revision() > before);
        let same = s.revision();
        s.execute("customer.get", &json!({"customer": 1}));
        assert_eq!(s.revision(), same, "reads do not bump the revision");
        assert!(s.italian());
        assert_eq!(s.actor(), "riccardo");
    }

    #[test]
    fn the_intranet_is_asked_off_the_calling_thread_and_the_command_runs_on_pump() {
        use std::sync::{Mutex, mpsc};
        use std::time::Duration;
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let mut desk = Desk::demo();
        desk.set_intranet(Box::new(move |actor, command, params| {
            assert_eq!(actor, "riccardo");
            let released = wait.lock().map(|rx| rx.recv_timeout(Duration::from_secs(10)).is_ok()).unwrap_or(false);
            if !released {
                return Err("not released".into());
            }
            match command {
                "customer.get" if params["id"] == 9 => {
                    Ok(json!({"customer": {"id": 9, "name": "Dalla Intranet S.r.l.", "vat": "IT09999999999", "private": false}}))
                }
                _ => Err("no".into()),
            }
        }));
        let mut s = Session::new(desk);
        let count = s.data().customers.len();
        let job = s.execute("customer.import", &json!({"intranet_id": 9}));
        assert_eq!(s.jobs_pending(), 1, "asked on another thread");
        assert!(s.pump().is_empty());
        // Meanwhile other commands go on, and the desk is unchanged.
        s.execute("customer.get", &json!({"customer": 1}));
        assert_eq!(s.pump().len(), 1);
        assert_eq!(s.data().customers.len(), count);
        go.send(()).unwrap();
        let mut done = Vec::new();
        for _ in 0..1000 {
            done.extend(s.pump());
            if !done.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!((done.len(), done[0].job, done[0].command.as_str()), (1, job, "customer.import"));
        let v = done[0].result.as_ref().unwrap();
        assert_eq!(v["created"], true);
        assert_eq!(s.data().customers.len(), count + 1, "applied on pump");
        assert!(s.data().customers.iter().any(|c| c.intranet_id == Some(9)));
        assert_eq!(s.jobs_pending(), 0);
        // What is refused without the intranet is refused at once.
        let bad = s.execute("customer.import", &json!({}));
        let done = s.pump();
        assert_eq!((done[0].job, done[0].result.is_err()), (bad, true));
        let mut s = Session::demo();
        let none = s.execute("intranet.search", &json!({"query": "x"}));
        let done = s.pump();
        assert!(done[0].job == none && done[0].result.as_ref().unwrap_err().to_string().contains("not reachable"));
    }

    #[test]
    fn rustdesk_check_runs_off_the_calling_thread() {
        use std::sync::{Mutex, mpsc};
        use std::time::Duration;
        // The probes wait until the test lets them go (10 s at most, so a regression fails
        // instead of hanging): `execute` must come back before that.
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let mut desk = Desk::demo();
        desk.set_rustdesk_probe(
            "127.0.0.1",
            Box::new(move |_: &str, port: u16| {
                let released = wait.lock().map(|rx| rx.recv_timeout(Duration::from_secs(10)).is_ok()).unwrap_or(false);
                if released { Ok(u64::from(port)) } else { Err("not released".into()) }
            }),
        );
        let mut s = Session::new(desk);
        let job = s.execute("rustdesk.check", &json!({}));
        assert_eq!(s.jobs_pending(), 1);
        assert!(s.pump().is_empty());
        // Other commands go on meanwhile.
        s.execute("customer.get", &json!({"customer": 1}));
        let other = s.pump();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].command, "customer.get");
        for _ in 0..3 {
            go.send(()).unwrap();
        }
        let mut done = Vec::new();
        for _ in 0..1000 {
            done.extend(s.pump());
            if !done.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].job, job);
        assert_eq!(done[0].command, "rustdesk.check");
        let report = done[0].result.as_ref().unwrap();
        assert!(report["ports"].as_array().unwrap().iter().all(|p| p["ok"] == true), "{report}");
        assert_eq!(s.jobs_pending(), 0);
        // What can be refused without the network is refused at once.
        let bad = s.execute("rustdesk.check", &json!({"host": "a b"}));
        assert_eq!(s.jobs_pending(), 0);
        let done = s.pump();
        assert_eq!(done[0].job, bad);
        assert!(done[0].result.as_ref().is_err_and(|e| e.to_string().contains("host")));
        let mut none = Session::demo();
        none.execute("rustdesk.check", &json!({}));
        assert!(none.pump()[0].result.as_ref().is_err_and(|e| e.to_string().contains("probe")));
        // The CLI's way still waits for the answer.
        let mut cli = Desk::demo();
        cli.set_rustdesk_probe("127.0.0.1", Box::new(|_: &str, _: u16| Ok(1)));
        assert_eq!(Session::new(cli).run("rustdesk.check", &json!({})).unwrap()["ports"][0]["ok"], true);
    }

    #[test]
    fn pump_shows_what_another_process_wrote() {
        let dir = std::env::temp_dir().join(format!("remotecraft-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = crate::Paths::under(&dir);
        let mut s = Session::new(Desk::open(paths.clone()).unwrap());
        s.execute("customer.add", &json!({"name": "Dal desktop"}));
        assert!(s.pump()[0].result.is_ok());
        let rev = s.revision();
        assert!(s.pump().is_empty());
        assert_eq!(s.revision(), rev, "nothing changed, nothing reloaded");
        // The MCP server next to the desktop app records something: the next pump shows it.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut mcp = Desk::open(paths).unwrap();
        mcp.execute("customer.add", &json!({"name": "Dall'agente"})).unwrap();
        assert!(s.pump().is_empty());
        assert_eq!(s.data().customers.len(), 2);
        assert_ne!(s.revision(), rev);
        // A file that cannot be read keeps what was shown (it is not parsed again every
        // frame); a command reports it, and a good file is read again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let good = std::fs::read(&mcp.paths().unwrap().desk).unwrap();
        std::fs::write(&mcp.paths().unwrap().desk, "{broken").unwrap();
        assert!(s.desk_mut().refresh().is_err());
        assert!(s.desk_mut().refresh().is_ok(), "the same broken file is not read twice");
        assert!(s.pump().is_empty());
        assert_eq!(s.data().customers.len(), 2);
        s.execute("customer.list", &json!({}));
        assert!(s.pump()[0].result.is_err());
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&mcp.paths().unwrap().desk, good).unwrap();
        assert!(s.desk_mut().refresh().is_ok());
        assert_eq!(s.data().customers.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_without_an_actor_names_the_desk_actor() {
        let mut d = Desk::demo();
        let s = d.snapshot(0, "  ").unwrap().unwrap();
        assert_eq!(s.actor, "riccardo");
        // ...the technician commands then run as, so "my" timers are found.
        let v = d.execute_as("", "intervention.start", &json!({"customer": 2, "kind": "phone"})).unwrap();
        assert_eq!(v["intervention"]["technician"], s.actor.as_str());
    }

    #[test]
    fn snapshot_by_revision() {
        let mut d = Desk::demo();
        let first = d.snapshot(0, "Anna Rossi").unwrap().unwrap();
        assert_eq!(first.actor, "Anna Rossi");
        assert_eq!(first.data.customers.len(), 3);
        assert!(d.snapshot(first.revision, "x").unwrap().is_none());
        d.execute("customer.add", &json!({"name": "Nuovo"})).unwrap();
        let second = d.snapshot(first.revision, "x").unwrap().unwrap();
        assert!(second.revision > first.revision);
        assert_eq!(second.data.customers.len(), 4);
    }
}
