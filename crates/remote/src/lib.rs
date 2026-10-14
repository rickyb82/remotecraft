//! A [`DeskSession`] that lives in a browser (or any client) and mirrors a `remotecraft-cli
//! serve` server: commands go to `POST /api/cmd`, the desk comes back from
//! `GET /api/snapshot?since=N` (only when it changed). Works natively and on wasm32 through
//! `ehttp`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use remotecraft_engine::core::Data;
use remotecraft_engine::core::chrono::{DateTime, Utc};
use remotecraft_engine::{Completed, DeskSession, Error, Snapshot};
use serde_json::{Value, json};

/// What arrived from the server, to be applied on the next `pump`. Poll results carry the
/// number of the poll they answer: a late one counts only when no newer answer was applied.
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
/// A poll still unanswered after this long (seconds) is given up and another one goes out (the
/// browser's fetch has no timeout: a connection lost while the PC slept never settles). The
/// answer of a given-up poll still counts if it arrives before a newer one.
const POLL_TIMEOUT: f64 = 30.0;
/// A give-up doubles the next poll's window up to this, so a large desk on a slow link is not
/// downloaded again every 30 s; an answer brings it back to [`POLL_TIMEOUT`]. Kept short: a poll
/// that hangs (the PC slept) is given up only after its window, and while the session answers
/// the mirror may lag behind the server by about as much.
const MAX_POLL_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest answer accepted from the server (a snapshot of the whole desk, an export): a larger
/// one is cut off while it arrives, so a hostile server cannot fill the client's memory.
const MAX_ANSWER: usize = 64 * 1024 * 1024;

/// [`DeskSession::problem`] starts with one of these stable prefixes (then the detail), so the
/// interface can tell the cases apart: the token was refused (401/403)…
pub const UNAUTHORIZED: &str = "unauthorized: ";
/// …the server answered something this client does not understand (not a RemoteCraft server,
/// another version, an answer too large)…
pub const INCOMPATIBLE: &str = "incompatible: ";
/// …or the server cannot be reached (no connection, no answer, a proxy saying it is down).
/// The only other problem is `connecting`, until the first answer.
pub const UNREACHABLE: &str = "unreachable: ";
/// The detail of [`UNAUTHORIZED`] when the server refused a client that kept its token back
/// because the address is plain `http://` to another machine (see [`sends_token_to`]).
pub const TOKEN_WITHHELD: &str = "the token is sent only to an https:// address or to this computer";

/// Whether a bearer token may travel to `base`: over HTTPS, or over plain HTTP only to this
/// machine (`127.0.0.1`, `localhost`, `[::1]`); elsewhere plain HTTP shows it to the network.
/// Anything unusual in the address (spaces, control characters, another scheme) says no.
pub fn sends_token_to(base: &str) -> bool {
    if base.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let lower = base.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else { return false };
    // The authority ends where a browser ends it (`\` included); the host follows any `user@`.
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    match host_port.strip_prefix('[') {
        Some(inner) => inner.split_once(']').is_some_and(|(ip, port)| ip == "::1" && (port.is_empty() || port.starts_with(':'))),
        None => matches!(host_port.split(':').next().unwrap_or_default(), "127.0.0.1" | "localhost"),
    }
}

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

/// How an exchange with the server ended.
enum Fetched {
    /// The whole answer (any status), at most the limit long.
    Answer(ehttp::Response),
    /// The answer is larger than the limit: cut off as soon as that was known.
    TooLarge,
    /// No answer: the error.
    Failed(String),
}

/// An exchange in progress: what to call when it ends, and what arrived so far.
struct Receiving {
    done: Option<Box<dyn FnOnce(Fetched) + Send>>,
    head: Option<ehttp::PartialResponse>,
    body: Vec<u8>,
}

/// Sends `req` and calls `done` once with how it ended, holding at most `limit` bytes of the
/// answer: one that says (`Content-Length`) or turns out to be larger is cut off right away.
fn fetch_bounded(req: ehttp::Request, limit: usize, done: impl FnOnce(Fetched) + Send + 'static) {
    let state = Mutex::new(Receiving { done: Some(Box::new(done)), head: None, body: Vec::new() });
    let limit_u64 = u64::try_from(limit).unwrap_or(u64::MAX);
    ehttp::streaming::fetch(req, move |part| {
        let Ok(mut s) = state.lock() else { return ControlFlow::Break(()) };
        if s.done.is_none() {
            return ControlFlow::Break(());
        }
        let ended = match part {
            Err(e) => Some(Fetched::Failed(e)),
            Ok(ehttp::streaming::Part::Response(head)) => {
                let declared = head.headers.get("content-length").and_then(|v| v.trim().parse::<u64>().ok());
                if declared.is_some_and(|n| n > limit_u64) {
                    Some(Fetched::TooLarge)
                } else {
                    s.head = Some(head);
                    None
                }
            }
            // An empty chunk ends the answer.
            Ok(ehttp::streaming::Part::Chunk(chunk)) if chunk.is_empty() => {
                let body = std::mem::take(&mut s.body);
                Some(match s.head.take() {
                    Some(head) => Fetched::Answer(head.complete(body)),
                    None => Fetched::Failed("the server sent no answer".into()),
                })
            }
            Ok(ehttp::streaming::Part::Chunk(chunk)) => {
                if s.body.len().saturating_add(chunk.len()) > limit {
                    Some(Fetched::TooLarge)
                } else {
                    s.body.extend_from_slice(&chunk);
                    None
                }
            }
        };
        let Some(ended) = ended else { return ControlFlow::Continue(()) };
        let done = s.done.take();
        s.head = None;
        s.body = Vec::new();
        drop(s);
        if let Some(done) = done {
            done(ended);
        }
        ControlFlow::Break(())
    });
}

/// A poll's answer: `None` when the desk is unchanged (an object with `unchanged`), else the
/// snapshot. Read straight from the bytes, with no intermediate JSON tree of the whole desk.
fn read_poll(bytes: &[u8]) -> serde_json::Result<Option<Snapshot>> {
    #[derive(serde::Deserialize)]
    struct Unchanged {
        #[serde(rename = "unchanged")]
        _unchanged: serde::de::IgnoredAny,
    }
    if serde_json::from_slice::<Unchanged>(bytes).is_ok() {
        return Ok(None);
    }
    serde_json::from_slice::<Snapshot>(bytes).map(Some)
}

/// The remote session.
pub struct RemoteDesk {
    base: String,
    token: String,
    /// A token was given but kept back: the address is plain `http://` to another machine.
    withheld: bool,
    data: Data,
    actor: String,
    problems: Vec<String>,
    rustdesk_host: String,
    rustdesk_key: String,
    revision: u64,
    inbox: Inbox,
    /// A poll is out (since `last_poll`, given up after `poll_window` seconds); `poll_seq` is
    /// its number.
    polling: bool,
    poll_seq: u64,
    poll_window: f64,
    /// The window of the next poll: doubled by a give-up, reset by an answer.
    next_window: f64,
    /// The number of the last poll whose answer was applied: older answers are stale.
    applied_poll: u64,
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
    /// Connects to `base` (`http://127.0.0.1:7979`) with a bearer token (the technician's),
    /// which travels only where [`sends_token_to`] allows.
    pub fn new(base: &str, token: &str) -> Self {
        let base = base.trim().trim_end_matches('/').to_string();
        let token = token.trim();
        let withheld = !token.is_empty() && !sends_token_to(&base);
        let mut s = Self {
            token: if withheld { String::new() } else { token.to_string() },
            base,
            withheld,
            data: Data::default(),
            actor: String::new(),
            problems: Vec::new(),
            rustdesk_host: String::new(),
            rustdesk_key: String::new(),
            revision: 0,
            inbox: Arc::default(),
            polling: false,
            poll_seq: 0,
            poll_window: POLL_TIMEOUT,
            next_window: POLL_TIMEOUT,
            applied_poll: 0,
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

    /// The problem a refused or failed answer means, naming a token kept back when the server
    /// asks for one.
    fn refused(withheld: bool, r: &ehttp::Response) -> String {
        if withheld && matches!(r.status, 401 | 403) { format!("{UNAUTHORIZED}{TOKEN_WITHHELD}") } else { status_problem(r) }
    }

    fn poll(&mut self) {
        if self.polling {
            return;
        }
        self.polling = true;
        self.poll_now = false;
        self.last_poll = (self.clock)();
        self.poll_window = self.next_window;
        self.poll_seq = self.poll_seq.wrapping_add(1);
        let poll = self.poll_seq;
        let withheld = self.withheld;
        let inbox = self.inbox.clone();
        let mut req = self.request(ehttp::Request::get(format!("{}/api/snapshot?since={}", self.base, self.revision)));
        // Natively ehttp cuts a body off after its timeout (30 s by default): a slow but
        // progressing snapshot gets as long as the longest window.
        req.timeout = Some(MAX_POLL_TIMEOUT);
        fetch_bounded(req, MAX_ANSWER, move |fetched| {
            let failed = |problem: String| Incoming::PollFailed { poll, problem };
            let incoming = match fetched {
                Fetched::Answer(r) if r.ok => match read_poll(&r.bytes) {
                    Ok(None) => Incoming::Unchanged { poll },
                    Ok(Some(s)) => Incoming::Snapshot { poll, snapshot: Box::new(s) },
                    Err(e) => failed(format!("{INCOMPATIBLE}bad snapshot: {e}")),
                },
                Fetched::Answer(r) => failed(Self::refused(withheld, &r)),
                Fetched::TooLarge => failed(format!("{INCOMPATIBLE}the snapshot is too large")),
                Fetched::Failed(e) => failed(format!("{UNREACHABLE}{e}")),
            };
            if let Ok(mut q) = inbox.lock() {
                q.push(incoming);
            }
        });
    }

    /// Poll `poll` answered and its answer is being applied.
    fn answered(&mut self, poll: u64) {
        self.applied_poll = poll;
        self.next_window = POLL_TIMEOUT;
        if poll == self.poll_seq {
            self.polling = false;
        }
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
        let withheld = self.withheld;
        self.in_flight += 1;
        fetch_bounded(req, MAX_ANSWER, move |fetched| {
            let failed = |error: String, problem: String| Incoming::Failed { job, command: command.clone(), error, problem };
            let incoming = match fetched {
                Fetched::Answer(r) => match serde_json::from_slice::<Value>(&r.bytes) {
                    Ok(v) if v.get("error").is_some() => Incoming::Answer {
                        job,
                        command: command.clone(),
                        result: Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string()),
                    },
                    Ok(v) if r.ok => Incoming::Answer { job, command: command.clone(), result: Ok(v.get("result").cloned().unwrap_or(Value::Null)) },
                    Ok(_) => failed(answer_error(&r), Self::refused(withheld, &r)),
                    Err(e) if r.ok => failed(format!("bad answer: {e}"), format!("{INCOMPATIBLE}bad answer: {e}")),
                    Err(_) => failed(answer_error(&r), Self::refused(withheld, &r)),
                },
                Fetched::TooLarge => failed("the answer is too large".into(), format!("{INCOMPATIBLE}the answer is too large")),
                Fetched::Failed(e) => failed(e.clone(), format!("{UNREACHABLE}{e}")),
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
                // A poll's answer counts unless a newer one was applied already: a given-up
                // poll that was only slow (a large desk on a slow link) still brings the desk,
                // and the newer poll stays out.
                Incoming::Snapshot { poll, snapshot } if poll > self.applied_poll => {
                    self.answered(poll);
                    self.apply(*snapshot);
                }
                Incoming::Unchanged { poll } if poll > self.applied_poll => {
                    self.answered(poll);
                    self.problem = None;
                }
                Incoming::Snapshot { .. } | Incoming::Unchanged { .. } => {}
                // A failure says something only about the poll out now.
                Incoming::PollFailed { poll, problem } if self.polling && poll == self.poll_seq => {
                    self.polling = false;
                    self.problem = Some(problem);
                }
                Incoming::PollFailed { .. } => {}
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
        if self.polling && now - self.last_poll > self.poll_window {
            self.polling = false;
            self.problem = Some(format!("{UNREACHABLE}the server does not answer"));
            self.next_window = (self.poll_window * 2.0).min(MAX_POLL_TIMEOUT.as_secs_f64());
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

    /// A server that accepts connections and never answers (a connection lost while the PC
    /// slept, or a download still under way), and a session on it driven by a clock the test
    /// sets.
    fn silent_session() -> (RemoteDesk, Arc<Mutex<f64>>) {
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
        (r, now)
    }

    fn desk_with(revision: u64, name: &str) -> Box<Snapshot> {
        let mut s = Snapshot { revision, ..Snapshot::default() };
        s.data.customers.push(remotecraft_engine::core::Customer { id: 1, name: name.into(), ..Default::default() });
        Box::new(s)
    }

    #[test]
    fn a_slow_poll_given_up_still_brings_the_desk_when_it_answers() {
        let (mut r, now) = silent_session();
        let first = r.poll_seq;
        *now.lock().unwrap() = 10.0;
        r.pump();
        assert!(r.polling && r.poll_seq == first, "still waiting");
        assert_eq!(r.problem().as_deref(), Some("connecting"));
        *now.lock().unwrap() = 31.0;
        r.pump();
        assert!(r.problem().is_some_and(|p| p.starts_with(UNREACHABLE)), "{:?}", r.problem());
        assert!(r.polling && r.poll_seq == first + 1, "another poll went out");
        // The given-up poll was only slow (a large desk on a slow link): its answer, complete,
        // arrives while the newer poll is still out, and the desk loads.
        r.inbox.lock().unwrap().push(Incoming::Snapshot { poll: first, snapshot: desk_with(7, "Rossi") });
        // A failure of a given-up poll says nothing new.
        r.inbox.lock().unwrap().push(Incoming::PollFailed { poll: first, problem: "unreachable: late".into() });
        r.pump();
        assert_eq!(r.revision(), 7);
        assert_eq!(r.data().customers.first().map(|c| c.name.as_str()), Some("Rossi"));
        assert_eq!(r.problem(), None);
        assert!(r.polling && r.poll_seq == first + 1, "the newer poll stays out");
        // The newer poll answers: applied, and the session polls again later.
        r.inbox.lock().unwrap().push(Incoming::Snapshot { poll: first + 1, snapshot: desk_with(8, "Bianchi") });
        r.pump();
        assert!(!r.polling);
        assert_eq!(r.revision(), 8);
        // An answer older than one applied is stale, whatever it says.
        r.inbox.lock().unwrap().push(Incoming::Snapshot { poll: first, snapshot: desk_with(6, "Vecchio") });
        r.inbox.lock().unwrap().push(Incoming::Unchanged { poll: first });
        r.pump();
        assert_eq!(r.revision(), 8);
        assert_eq!(r.data().customers.first().map(|c| c.name.as_str()), Some("Bianchi"));
        // The current poll's failure is shown.
        *now.lock().unwrap() = 40.0;
        r.pump();
        let current = r.poll_seq;
        assert!(r.polling && current == first + 2);
        r.inbox.lock().unwrap().push(Incoming::PollFailed { poll: current, problem: "unreachable: gone".into() });
        r.pump();
        assert_eq!(r.problem().as_deref(), Some("unreachable: gone"));
        assert_eq!(r.revision(), 8, "the desk shown stays");
    }

    #[test]
    fn the_poll_window_backs_off_after_a_give_up_and_resets_on_an_answer() {
        let (mut r, now) = silent_session();
        let set = |t: f64| *now.lock().unwrap() = t;
        let first = r.poll_seq;
        set(31.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 1, "given up after 30 s");
        // The next one gets 60 s: not given up at 31 s.
        set(31.0 + 59.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 1, "the doubled window holds");
        set(31.0 + 61.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 2, "given up after 60 s");
        // And no more than 60 s after that.
        set(92.0 + 59.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 2);
        set(92.0 + 61.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 3, "capped at 60 s");
        // An answer brings the window back to 30 s for the polls after it.
        r.inbox.lock().unwrap().push(Incoming::Unchanged { poll: first + 3 });
        r.pump();
        assert!(!r.polling);
        assert_eq!(r.problem(), None);
        set(153.0 + 3.0);
        r.pump();
        assert!(r.polling && r.poll_seq == first + 4, "the next poll is out");
        set(156.0 + 31.0);
        r.pump();
        assert_eq!(r.poll_seq, first + 5, "given up after 30 s again");
        assert!(r.problem().is_some_and(|p| p.starts_with(UNREACHABLE)), "{:?}", r.problem());
    }

    /// A server answering every request with the raw bytes `answer` (status line, headers and
    /// whatever body), then closing.
    fn raw(answer: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                }
                let _ = stream.write_all(answer);
                // Keep the connection open a little: a client waiting for the rest would hang.
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        addr
    }

    fn fetch_now(url: &str, limit: usize) -> Fetched {
        let (tx, rx) = std::sync::mpsc::channel();
        fetch_bounded(ehttp::Request::get(url), limit, move |f| {
            let _ = tx.send(f);
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    #[test]
    fn answers_larger_than_the_limit_are_cut_off_while_they_arrive() {
        // Said by the header: refused before the body (which never comes).
        let base = raw(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100000000000\r\n\r\n{");
        assert!(matches!(fetch_now(&base, 100), Fetched::TooLarge));
        // No length: counted while it arrives (close-delimited, then chunked).
        static BIG: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let big = BIG.get_or_init(|| {
            let mut v = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
            v.extend(std::iter::repeat_n(b'x', 5000));
            v
        });
        assert!(matches!(fetch_now(&raw(big), 1000), Fetched::TooLarge));
        static CHUNKED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let chunked = CHUNKED.get_or_init(|| {
            let mut v = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
            for _ in 0..10 {
                v.extend(b"200\r\n");
                v.extend(std::iter::repeat_n(b'y', 0x200));
                v.extend(b"\r\n");
            }
            v.extend(b"0\r\n\r\n");
            v
        });
        assert!(matches!(fetch_now(&raw(chunked), 1000), Fetched::TooLarge));
        // Within the limit: the whole answer, status included.
        let small = raw(b"HTTP/1.1 404 Not Found\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"error\":\"x\"}");
        match fetch_now(&small, 1000) {
            Fetched::Answer(r) => {
                assert_eq!(r.status, 404);
                assert_eq!(r.bytes, br#"{"error":"x"}"#);
            }
            _ => panic!("expected the answer"),
        }
        assert!(matches!(fetch_now(&raw(chunked), 10_000), Fetched::Answer(r) if r.bytes.len() == 10 * 0x200));
        assert!(matches!(fetch_now("http://127.0.0.1:1", 1000), Fetched::Failed(_)));
    }

    #[test]
    fn a_snapshot_announced_too_large_is_refused_at_once() {
        // A hostile server announcing more than the client accepts, and sending it slowly: the
        // poll ends on the header, without holding (or waiting for) the body.
        let base = raw(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000000000\r\n\r\n{\"revision\":");
        let mut r = RemoteDesk::new(&base, "t");
        let mut done = Vec::new();
        assert!(pump_until(&mut r, &mut done, &has(INCOMPATIBLE)), "{:?}", r.problem());
        assert_eq!(r.problem().as_deref(), Some("incompatible: the snapshot is too large"));
        const { assert!(MAX_ANSWER <= 64 * 1024 * 1024) };
    }

    #[test]
    fn polls_read_unchanged_and_snapshots_without_mixing_them_up() {
        assert!(matches!(read_poll(br#"{"unchanged": true}"#), Ok(None)));
        assert!(matches!(read_poll(br#"{"unchanged": true, "revision": 4}"#), Ok(None)));
        let s = read_poll(br#"{"revision": 3, "actor": "Anna"}"#).unwrap().unwrap();
        assert_eq!((s.revision, s.actor.as_str()), (3, "Anna"));
        for bad in [&b""[..], b"[1,2]", b"<html>", b"{\"revision\": -1}", b"{\"unchanged\": true"] {
            assert!(read_poll(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn the_token_travels_only_over_https_or_to_this_computer() {
        for ok in [
            "https://assistenza.example.it",
            "HTTPS://Assistenza.example.it/",
            "http://127.0.0.1:7979",
            "http://localhost:7979/",
            "http://[::1]:7979",
            "http://LOCALHOST",
            "http://evil.example@127.0.0.1:7979",
        ] {
            assert!(sends_token_to(ok), "{ok}");
        }
        for no in [
            "http://192.168.1.10:7979",
            "http://assistenza.example.it",
            "http://127.0.0.1.evil.example",
            "http://localhost.evil.example:7979",
            "http://127.0.0.1@evil.example",
            "http://evil.example\\@127.0.0.1",
            "http://evil.example/@127.0.0.1",
            "http://evil.example?@127.0.0.1",
            "http://evil.example#@127.0.0.1",
            "http://[::1].evil.example",
            "http://127.0.0.1\t.evil.example",
            " https://x",
            "ftp://127.0.0.1",
            "javascript:alert(1)",
            "127.0.0.1:7979",
            "",
        ] {
            assert!(!sends_token_to(no), "{no:?}");
        }
    }

    #[test]
    fn a_token_for_plain_http_elsewhere_is_kept_back_and_the_refusal_says_why() {
        // `localhost.` is this computer for the resolver but not for the rule: the token stays.
        let base = fake(|_| (401, r#"{"error": "missing token"}"#)).replace("127.0.0.1", "localhost.");
        let mut r = RemoteDesk::new(&base, "secret");
        assert!(r.withheld && r.token.is_empty());
        let mut done = Vec::new();
        assert!(pump_until(&mut r, &mut done, &has(UNAUTHORIZED)), "{:?}", r.problem());
        assert_eq!(r.problem(), Some(format!("{UNAUTHORIZED}{TOKEN_WITHHELD}")));
        // To this computer it travels.
        let r = RemoteDesk::new("http://127.0.0.1:1/", " secret ");
        assert!(!r.withheld);
        assert_eq!(r.token, "secret");
        assert_eq!(r.server(), "http://127.0.0.1:1");
        // No token, nothing kept back.
        assert!(!RemoteDesk::new("http://192.168.1.10:1", "").withheld);
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
