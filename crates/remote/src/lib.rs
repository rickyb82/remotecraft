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

/// What arrived from the server, to be applied on the next `pump`. Poll results carry the
/// number of the poll they answer: one given up for silence is ignored when it arrives late.
enum Incoming {
    Snapshot {
        poll: u64,
        snapshot: Box<Snapshot>,
    },
    Unchanged {
        poll: u64,
    },
    /// The poll failed: the problem to show ([`UNAUTHORIZED`], [`INCOMPATIBLE`] or
    /// [`UNREACHABLE`] and the detail).
    PollFailed {
        poll: u64,
        problem: String,
    },
    /// A command's answer.
    Answer {
        job: u64,
        command: String,
        result: Result<Value, String>,
    },
    /// The command's answer never came, or was nonsense: its error and the problem to show.
    Failed {
        job: u64,
        command: String,
        error: String,
        problem: String,
    },
}

type Inbox = Arc<Mutex<Vec<Incoming>>>;

/// How often the desk is polled when nothing is happening (seconds).
const IDLE_POLL: f64 = 2.0;
/// How often while commands are in flight.
const BUSY_POLL: f64 = 0.4;
/// A poll still unanswered after this long is given up and another one goes out (the
/// browser's fetch has no timeout: a connection lost while the PC slept never settles).
const POLL_TIMEOUT: f64 = 30.0;
/// Largest answer accepted from the server.
const MAX_ANSWER: usize = 256 * 1024 * 1024;

/// [`DeskSession::problem`] starts with one of these stable prefixes (then the detail), so the
/// interface can tell the cases apart: the token was refused (401/403)…
pub const UNAUTHORIZED: &str = "unauthorized: ";
/// …the server answered something this client does not understand (not a RemoteCraft server,
/// another version, an answer too large)…
pub const INCOMPATIBLE: &str = "incompatible: ";
/// …or the server cannot be reached (no connection, no answer, a proxy saying it is down).
/// The only other problem is `connecting`, until the first answer.
pub const UNREACHABLE: &str = "unreachable: ";

/// Seconds since the session started, on a monotonic clock: a wall clock stepping back (a time
/// sync) would stop the polls for as long as the step.
fn monotonic_clock() -> Box<dyn Fn() -> f64 + Send> {
    let origin = web_time::Instant::now();
    Box::new(move || origin.elapsed().as_secs_f64())
}

/// The problem a non-2xx answer means.
fn status_problem(r: &ehttp::Response) -> String {
    let detail = answer_error(r);
    match r.status {
        401 | 403 => format!("{UNAUTHORIZED}{detail}"),
        408 | 429 | 500..=599 => format!("{UNREACHABLE}{detail}"),
        _ => format!("{INCOMPATIBLE}{detail}"),
    }
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
    /// A poll is out (since `last_poll`); `poll_seq` is its number.
    polling: bool,
    poll_seq: u64,
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
            poll_seq: 0,
            problem: Some("connecting".into()),
            in_flight: 0,
            next_job: 1,
            poll_now: true,
            last_poll: 0.0,
            clock: monotonic_clock(),
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
        self.poll_seq = self.poll_seq.wrapping_add(1);
        let poll = self.poll_seq;
        let inbox = self.inbox.clone();
        let req = self.request(ehttp::Request::get(format!("{}/api/snapshot?since={}", self.base, self.revision)));
        ehttp::fetch(req, move |result| {
            let failed = |problem: String| Incoming::PollFailed { poll, problem };
            let incoming = match result {
                Ok(r) if r.ok && r.bytes.len() <= MAX_ANSWER => match serde_json::from_slice::<Value>(&r.bytes) {
                    Ok(v) if v.get("unchanged").is_some() => Incoming::Unchanged { poll },
                    Ok(v) => match serde_json::from_value::<Snapshot>(v) {
                        Ok(s) => Incoming::Snapshot { poll, snapshot: Box::new(s) },
                        Err(e) => failed(format!("{INCOMPATIBLE}bad snapshot: {e}")),
                    },
                    Err(e) => failed(format!("{INCOMPATIBLE}bad snapshot: {e}")),
                },
                Ok(r) if r.ok => failed(format!("{INCOMPATIBLE}the snapshot is too large")),
                Ok(r) => failed(status_problem(&r)),
                Err(e) => failed(format!("{UNREACHABLE}{e}")),
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
            let failed = |error: String, problem: String| Incoming::Failed { job, command: command.clone(), error, problem };
            let incoming = match result {
                Ok(r) if r.bytes.len() <= MAX_ANSWER => match serde_json::from_slice::<Value>(&r.bytes) {
                    Ok(v) if v.get("error").is_some() => {
                        Incoming::Answer { job, command: command.clone(), result: Err(v["error"].as_str().unwrap_or("error").to_string()) }
                    }
                    Ok(v) if r.ok => Incoming::Answer { job, command: command.clone(), result: Ok(v.get("result").cloned().unwrap_or(Value::Null)) },
                    Ok(_) => failed(answer_error(&r), status_problem(&r)),
                    Err(e) if r.ok => failed(format!("bad answer: {e}"), format!("{INCOMPATIBLE}bad answer: {e}")),
                    Err(_) => failed(answer_error(&r), status_problem(&r)),
                },
                Ok(_) => failed("the answer is too large".into(), format!("{INCOMPATIBLE}the answer is too large")),
                Err(e) => failed(e.clone(), format!("{UNREACHABLE}{e}")),
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
                // Only the poll out now counts: one given up for silence may still answer.
                Incoming::Snapshot { poll, .. } | Incoming::Unchanged { poll } | Incoming::PollFailed { poll, .. }
                    if !self.polling || poll != self.poll_seq => {}
                Incoming::Snapshot { snapshot, .. } => {
                    self.polling = false;
                    self.apply(*snapshot);
                }
                Incoming::Unchanged { .. } => {
                    self.polling = false;
                    self.problem = None;
                }
                Incoming::PollFailed { problem, .. } => {
                    self.polling = false;
                    self.problem = Some(problem);
                }
                // A command's answer says nothing about the mirror: the problem stays until a
                // poll succeeds (the next one goes out at once).
                Incoming::Answer { job, command, result } => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    self.poll_now = true;
                    out.push(Completed { job, command, result: result.map_err(|e| Error::Core(remotecraft_engine::core::Error::Invalid(e))) });
                }
                Incoming::Failed { job, command, error, problem } => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    self.problem = Some(problem);
                    out.push(Completed { job, command, result: Err(Error::Core(remotecraft_engine::core::Error::Io(error))) });
                }
            }
        }
        let now = (self.clock)();
        if self.polling && now - self.last_poll > POLL_TIMEOUT {
            self.polling = false;
            self.problem = Some(format!("{UNREACHABLE}the server does not answer"));
        }
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
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;

    /// A server answering every request with `answer(method and path)`: (status, JSON body).
    fn fake(answer: fn(&str) -> (u16, &'static str)) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let mut length = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let target: Vec<&str> = first.split_whitespace().take(2).collect();
                let (status, text) = answer(&target.join(" "));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        addr
    }

    /// Pumps until `until` holds (or 4 s pass), keeping every completion.
    fn pump_until(r: &mut RemoteDesk, done: &mut Vec<Completed>, until: &dyn Fn(&RemoteDesk, &[Completed]) -> bool) -> bool {
        for _ in 0..200 {
            done.extend(r.pump());
            if until(r, done) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn has(prefix: &'static str) -> impl Fn(&RemoteDesk, &[Completed]) -> bool {
        move |r, _| r.problem().is_some_and(|p| p.starts_with(prefix))
    }

    #[test]
    fn a_refused_token_is_unauthorized_and_command_answers_keep_the_problem() {
        let base = fake(|target| match target {
            t if t.starts_with("GET /api/snapshot") => (401, r#"{"error": "unknown token"}"#),
            _ => (200, r#"{"result": {"ok": true}}"#),
        });
        let mut r = RemoteDesk::new(&base, "revoked");
        let mut done = Vec::new();
        assert!(pump_until(&mut r, &mut done, &has(UNAUTHORIZED)), "{:?}", r.problem());
        assert_eq!(r.problem().as_deref(), Some("unauthorized: unknown token"));
        // A command that succeeds does not hide the problem: the mirror is still frozen.
        let job = r.execute("app.status", &json!({}));
        for _ in 0..200 {
            let now = r.pump();
            if now.iter().any(|c| c.job == job) {
                assert!(now.iter().any(|c| c.job == job && c.result.is_ok()));
                assert!(r.problem().is_some_and(|p| p.starts_with(UNAUTHORIZED)), "{:?}", r.problem());
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(r.jobs_pending(), 0);
    }

    #[test]
    fn answers_the_client_cannot_read_are_incompatible() {
        let base = fake(|_| (200, "<html>not RemoteCraft</html>"));
        let mut r = RemoteDesk::new(&base, "t");
        let mut done = Vec::new();
        assert!(pump_until(&mut r, &mut done, &has(INCOMPATIBLE)), "{:?}", r.problem());
        assert!(r.problem().is_some_and(|p| p.contains("bad snapshot")));
        let job = r.execute("app.status", &json!({}));
        assert!(pump_until(&mut r, &mut done, &|_, d| d.iter().any(|c| c.job == job)));
        assert!(done.iter().any(|c| c.job == job && c.result.is_err()));
        assert!(r.problem().is_some_and(|p| p.starts_with(INCOMPATIBLE)));
        let base = fake(|_| (404, r#"{"error": "unknown endpoint"}"#));
        let mut r = RemoteDesk::new(&base, "t");
        assert!(pump_until(&mut r, &mut done, &has(INCOMPATIBLE)), "{:?}", r.problem());
        // A proxy saying the server behind it is down: unreachable.
        let base = fake(|_| (502, "Bad Gateway"));
        let mut r = RemoteDesk::new(&base, "t");
        assert!(pump_until(&mut r, &mut done, &has(UNREACHABLE)), "{:?}", r.problem());
        // A good answer clears the problem.
        let base = fake(|_| (200, r#"{"unchanged": true}"#));
        let mut r = RemoteDesk::new(&base, "t");
        assert!(pump_until(&mut r, &mut done, &|r, _| r.problem().is_none()), "{:?}", r.problem());
    }

    #[test]
    fn a_silent_poll_is_given_up_and_its_late_answer_ignored() {
        // A server that accepts and never answers (a connection lost while the PC slept).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for s in listener.incoming() {
                held.push(s);
            }
        });
        let mut r = RemoteDesk::new(&base, "t");
        // The default clock counts from the session's start, not the wall clock's epoch.
        assert!((r.clock)() < 60.0);
        let now = Arc::new(Mutex::new(0.0f64));
        let t = now.clone();
        r.clock = Box::new(move || t.lock().map(|t| *t).unwrap_or(0.0));
        r.last_poll = 0.0;
        let first = r.poll_seq;
        *now.lock().unwrap() = 10.0;
        r.pump();
        assert!(r.polling && r.poll_seq == first, "still waiting");
        assert_eq!(r.problem().as_deref(), Some("connecting"));
        *now.lock().unwrap() = 31.0;
        r.pump();
        assert!(r.problem().is_some_and(|p| p.starts_with(UNREACHABLE)), "{:?}", r.problem());
        assert!(r.polling && r.poll_seq == first + 1, "another poll went out");
        // The given-up poll answering late changes nothing.
        let mut late = Snapshot { revision: 7, ..Snapshot::default() };
        late.data.customers.push(remotecraft_engine::core::Customer { id: 1, name: "Vecchio".into(), ..Default::default() });
        r.inbox.lock().unwrap().push(Incoming::Snapshot { poll: first, snapshot: Box::new(late) });
        r.inbox.lock().unwrap().push(Incoming::PollFailed { poll: first, problem: "unreachable: late".into() });
        r.pump();
        assert!(r.data().customers.is_empty());
        assert_eq!(r.revision(), 0);
        assert!(r.polling);
        assert_ne!(r.problem().as_deref(), Some("unreachable: late"));
        // The current one is applied.
        r.inbox.lock().unwrap().push(Incoming::Unchanged { poll: first + 1 });
        r.pump();
        assert!(!r.polling);
        assert_eq!(r.problem(), None);
    }

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
        assert!(s.problem().is_some_and(|p| p.starts_with(UNREACHABLE)), "{:?}", s.problem());
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].job, job);
        assert!(done[0].result.is_err());
        assert_eq!(s.jobs_pending(), 0);
        assert_eq!(s.server(), "http://127.0.0.1:1");
        assert!(s.data().customers.is_empty());
    }
}
