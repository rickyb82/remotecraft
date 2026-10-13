//! A [`DeskSession`] that lives in a browser (or any client) and mirrors a `remotecraft-cli
//! serve` server: commands go to `POST /api/cmd`, the desk comes back from
//! `GET /api/snapshot?since=N` (only when it changed). Works natively and on wasm32 through
//! `ehttp`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, Mutex};

use remotecraft_engine::core::Data;
use remotecraft_engine::core::chrono::{DateTime, Utc};
use remotecraft_engine::{Completed, DeskSession, Error, Snapshot};
use serde_json::{Value, json};

/// What arrived from the server, to be applied on the next `pump`.
enum Incoming {
    Snapshot(Box<Snapshot>),
    Unchanged,
    /// A command's answer.
    Answer { job: u64, command: String, result: Result<Value, String> },
    /// The server could not be reached, or answered nonsense.
    Failed { job: Option<(u64, String)>, error: String },
}

type Inbox = Arc<Mutex<Vec<Incoming>>>;

/// How often the desk is polled when nothing is happening (seconds).
const IDLE_POLL: f64 = 2.0;
/// How often while commands are in flight.
const BUSY_POLL: f64 = 0.4;
/// Largest answer accepted from the server.
const MAX_ANSWER: usize = 256 * 1024 * 1024;

fn now_seconds() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// The remote session.
pub struct RemoteDesk {
    base: String,
    token: String,
    data: Data,
    actor: String,
    problems: Vec<String>,
    rustdesk_host: String,
    rustdesk_key: String,
    revision: u64,
    inbox: Inbox,
    polling: bool,
    problem: Option<String>,
    /// Commands sent whose answers have not arrived yet.
    in_flight: usize,
    next_job: u64,
    /// Set when the next poll should go out as soon as possible.
    poll_now: bool,
    last_poll: f64,
    clock: Box<dyn Fn() -> f64 + Send>,
}

impl RemoteDesk {
    /// Connects to `base` (`http://127.0.0.1:7979`) with a bearer token (the technician's).
    pub fn new(base: &str, token: &str) -> Self {
        let mut s = Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
            data: Data::default(),
            actor: String::new(),
            problems: Vec::new(),
            rustdesk_host: String::new(),
            rustdesk_key: String::new(),
            revision: 0,
            inbox: Arc::default(),
            polling: false,
            problem: Some("connecting".into()),
            in_flight: 0,
            next_job: 1,
            poll_now: true,
            last_poll: 0.0,
            clock: Box::new(now_seconds),
        };
        s.poll();
        s
    }

    fn request(&self, mut req: ehttp::Request) -> ehttp::Request {
        if !self.token.is_empty() {
            req.headers.insert("Authorization", format!("Bearer {}", self.token));
        }
        req
    }

    fn poll(&mut self) {
        if self.polling {
            return;
        }
        self.polling = true;
        self.poll_now = false;
        self.last_poll = (self.clock)();
        let inbox = self.inbox.clone();
        let req = self.request(ehttp::Request::get(format!("{}/api/snapshot?since={}", self.base, self.revision)));
        ehttp::fetch(req, move |result| {
            let incoming = match result {
                Ok(r) if r.ok && r.bytes.len() <= MAX_ANSWER => match serde_json::from_slice::<Value>(&r.bytes) {
                    Ok(v) if v.get("unchanged").is_some() => Incoming::Unchanged,
                    Ok(v) => match serde_json::from_value::<Snapshot>(v) {
                        Ok(s) => Incoming::Snapshot(Box::new(s)),
                        Err(e) => Incoming::Failed { job: None, error: format!("bad snapshot: {e}") },
                    },
                    Err(e) => Incoming::Failed { job: None, error: format!("bad snapshot: {e}") },
                },
                Ok(r) if r.ok => Incoming::Failed { job: None, error: "the snapshot is too large".into() },
                Ok(r) => Incoming::Failed { job: None, error: answer_error(&r) },
                Err(e) => Incoming::Failed { job: None, error: e },
            };
            if let Ok(mut q) = inbox.lock() {
                q.push(incoming);
            }
        });
    }

    fn apply(&mut self, s: Snapshot) {
        self.revision = s.revision;
        self.data = s.data;
        self.actor = s.actor;
        self.problems = s.problems;
        self.rustdesk_host = s.rustdesk_host;
        self.rustdesk_key = s.rustdesk_key;
        self.problem = None;
    }
}

/// The error of a non-2xx answer: the JSON `error`, else the status line.
fn answer_error(r: &ehttp::Response) -> String {
    serde_json::from_slice::<Value>(&r.bytes)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("server answered {} {}", r.status, r.status_text))
}

impl DeskSession for RemoteDesk {
    fn data(&self) -> &Data {
        &self.data
    }

    fn actor(&self) -> &str {
        &self.actor
    }

    fn now(&self) -> DateTime<Utc> {
        remotecraft_engine::core::time::now()
    }

    fn execute(&mut self, command: &str, params: &Value) -> u64 {
        let job = self.next_job;
        self.next_job = self.next_job.wrapping_add(1).max(1);
        let body = json!({"command": command, "params": params}).to_string().into_bytes();
        let mut req = ehttp::Request::post(format!("{}/api/cmd", self.base), body);
        req.headers = ehttp::Headers::new(&[("Accept", "application/json"), ("Content-Type", "application/json")]);
        let req = self.request(req);
        let inbox = self.inbox.clone();
        let command = command.to_string();
        self.in_flight += 1;
        ehttp::fetch(req, move |result| {
            let incoming = match result {
                Ok(r) if r.bytes.len() <= MAX_ANSWER => match serde_json::from_slice::<Value>(&r.bytes) {
                    Ok(v) if v.get("error").is_some() => {
                        Incoming::Answer { job, command, result: Err(v["error"].as_str().unwrap_or("error").to_string()) }
                    }
                    Ok(v) if r.ok => Incoming::Answer { job, command, result: Ok(v.get("result").cloned().unwrap_or(Value::Null)) },
                    Ok(_) => Incoming::Failed { job: Some((job, command)), error: answer_error(&r) },
                    Err(e) => Incoming::Failed { job: Some((job, command)), error: format!("bad answer: {e}") },
                },
                Ok(_) => Incoming::Failed { job: Some((job, command)), error: "the answer is too large".into() },
                Err(e) => Incoming::Failed { job: Some((job, command)), error: e },
            };
            if let Ok(mut q) = inbox.lock() {
                q.push(incoming);
            }
        });
        job
    }

    fn pump(&mut self) -> Vec<Completed> {
        let incoming: Vec<Incoming> = self.inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        let mut out = Vec::new();
        for i in incoming {
            match i {
                Incoming::Snapshot(s) => {
                    self.polling = false;
                    self.apply(*s);
                }
                Incoming::Unchanged => {
                    self.polling = false;
                    self.problem = None;
                }
                Incoming::Answer { job, command, result } => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    self.poll_now = true;
                    self.problem = None;
                    out.push(Completed { job, command, result: result.map_err(|e| Error::Core(remotecraft_engine::core::Error::Invalid(e))) });
                }
                Incoming::Failed { job, error } => match job {
                    Some((job, command)) => {
                        self.in_flight = self.in_flight.saturating_sub(1);
                        self.problem = Some(error.clone());
                        out.push(Completed { job, command, result: Err(Error::Core(remotecraft_engine::core::Error::Io(error))) });
                    }
                    None => {
                        self.polling = false;
                        self.problem = Some(error);
                    }
                },
            }
        }
        let now = (self.clock)();
        let interval = if self.in_flight > 0 { BUSY_POLL } else { IDLE_POLL };
        if self.poll_now || now - self.last_poll >= interval {
            self.poll();
        }
        out
    }

    fn jobs_pending(&self) -> usize {
        self.in_flight
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn problem(&self) -> Option<String> {
        self.problem.clone()
    }

    fn server(&self) -> String {
        self.base.clone()
    }

    fn rustdesk_defaults(&self) -> (String, String) {
        (self.rustdesk_host.clone(), self.rustdesk_key.clone())
    }

    fn problems(&self) -> Vec<String> {
        self.problems.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_server_is_a_problem_not_a_panic() {
        let mut s = RemoteDesk::new("http://127.0.0.1:1", "t");
        assert_eq!(s.problem(), Some("connecting".into()));
        let job = s.execute("app.status", &json!({}));
        let mut done = Vec::new();
        for _ in 0..100 {
            done.extend(s.pump());
            std::thread::sleep(std::time::Duration::from_millis(20));
            if !done.is_empty() && s.problem().is_some_and(|p| p != "connecting") {
                break;
            }
        }
        assert!(s.problem().is_some_and(|p| p != "connecting"), "{:?}", s.problem());
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].job, job);
        assert!(done[0].result.is_err());
        assert_eq!(s.jobs_pending(), 0);
        assert_eq!(s.server(), "http://127.0.0.1:1");
        assert!(s.data().customers.is_empty());
    }
}
