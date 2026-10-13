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
    fn italian(&self) -> bool {
        self.data().preferences.language != "en"
    }
}

/// A desk in this process, as a session: commands run at once and are reported by the next
/// [`DeskSession::pump`].
pub struct Session {
    desk: Desk,
    next_job: u64,
    done: Vec<Completed>,
}

impl Session {
    pub fn new(desk: Desk) -> Self {
        Self { desk, next_job: 1, done: Vec::new() }
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
        let result = self.desk.execute(command, params);
        if self.done.len() < 10_000 {
            self.done.push(Completed { job, command: command.to_string(), result });
        }
        job
    }

    fn pump(&mut self) -> Vec<Completed> {
        std::mem::take(&mut self.done)
    }

    fn jobs_pending(&self) -> usize {
        0
    }

    fn revision(&self) -> u64 {
        self.desk.revision()
    }

    fn rustdesk_defaults(&self) -> (String, String) {
        let env = self.desk.rustdesk();
        (env.host.clone(), env.key.clone())
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
