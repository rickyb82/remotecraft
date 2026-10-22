//! The operator's intranet as the engine's [`IntranetCall`]: `POST /api/cmd` of
//! `intranet-cli serve`, with its service token and the technician's name in the headers
//! (`X-Intranet-User`, `X-Intranet-Role`), as its framework sends them. Native only: the web
//! app's commands run on the server, which holds the connection.

use std::time::{Duration, Instant};

use remotecraft_engine::IntranetCall;
use serde_json::{Value, json};

/// How long one call may take, the wait for the intranet's answer included: a search while
/// the technician waits.
const TIMEOUT: Duration = Duration::from_secs(15);
/// How often the intranet is asked whether the job finished.
const POLL: Duration = Duration::from_millis(150);
/// Largest answer read to a command (`{"result": {"job": N}}`, or an error).
const MAX_ANSWER: u64 = 64 * 1024;
/// Largest snapshot read: the session's state and its last 200 answers.
const MAX_SNAPSHOT: u64 = 16 * 1024 * 1024;
/// Longest error text kept from the intranet.
const MAX_ERROR: usize = 300;

/// A connection to the intranet at `base` (`http://intranet:7978`) with its service `token`
/// (sent as `Authorization: Bearer`), calling as the technician with `role`
/// (`X-Intranet-Role`, `super_admin` unless the intranet admits another). An empty `base`
/// gives `None`: the desk then has no intranet.
pub fn caller(base: &str, token: &str, role: &str) -> Option<IntranetCall> {
    let base = base.trim().trim_end_matches('/').to_string();
    if base.is_empty() || !(base.starts_with("http://") || base.starts_with("https://")) {
        return None;
    }
    let token = token.trim().to_string();
    let role = remotecraft_engine::core::line(role);
    let role = if role.is_empty() { "super_admin".to_string() } else { role };
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(TIMEOUT)).build().into();
    let link = Link { agent, base, token, role };
    Some(Box::new(move |actor: &str, command: &str, params: &Value| link.call(actor, command, params)))
}

/// The intranet as one technician's requests reach it.
struct Link {
    agent: ureq::Agent,
    base: String,
    token: String,
    role: String,
}

impl Link {
    /// The intranet runs commands on a worker and answers `{"job": N}` at once; the result
    /// comes with the session's snapshot (`GET /api/snapshot`), in `completed`, where this
    /// technician's session (named by `X-Intranet-User`) keeps it. A command that answers
    /// without a job is answered as is.
    fn call(&self, actor: &str, command: &str, params: &Value) -> Result<Value, String> {
        let started = Instant::now();
        let body = json!({"command": command, "params": params}).to_string();
        let actor = header_value(actor, "remotecraft");
        let resp = self.request(self.agent.post(format!("{}/api/cmd", self.base)), &actor).header("Content-Type", "application/json").send(body);
        let v = answer(resp, MAX_ANSWER)?;
        let Some(job) = v.get("job").and_then(Value::as_u64) else { return Ok(v) };
        loop {
            if started.elapsed() >= TIMEOUT {
                return Err(format!("the intranet did not answer job {job} within {} s", TIMEOUT.as_secs()));
            }
            std::thread::sleep(POLL);
            let resp = self.request(self.agent.get(format!("{}/api/snapshot?since=0", self.base)), &actor).call();
            let snapshot = answer_raw(resp, MAX_SNAPSHOT)?;
            if let Some(c) = snapshot["completed"].as_array().into_iter().flatten().find(|c| c["job"].as_u64() == Some(job)) {
                let error = c["error"].as_str().unwrap_or_default();
                return if error.is_empty() { Ok(c["result"].clone()) } else { Err(remotecraft_engine::core::clip(error, MAX_ERROR).to_string()) };
            }
        }
    }

    /// Header values are one line of printable text: the actor comes from a token file or a
    /// framework header, the role from the environment.
    fn request<B>(&self, req: ureq::RequestBuilder<B>, actor: &str) -> ureq::RequestBuilder<B> {
        let req = req.header("X-Intranet-User", actor).header("X-Intranet-Role", &header_value(&self.role, "super_admin"));
        if self.token.is_empty() { req } else { req.header("Authorization", &format!("Bearer {}", self.token)) }
    }
}

/// The JSON of a response, within `limit` bytes.
fn answer_raw(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>, limit: u64) -> Result<Value, String> {
    let mut resp = resp.map_err(|e| format!("the intranet is unreachable ({e})"))?;
    let status = resp.status();
    let text = resp.body_mut().with_config().limit(limit).read_to_string().map_err(|e| format!("bad answer from the intranet: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("bad answer from the intranet ({status})"))?;
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Err(remotecraft_engine::core::clip(e, MAX_ERROR).to_string());
    }
    if !status.is_success() {
        return Err(format!("bad answer from the intranet ({status})"));
    }
    Ok(v)
}

/// The `result` of a command's response.
fn answer(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>, limit: u64) -> Result<Value, String> {
    let v = answer_raw(resp, limit)?;
    v.get("result").cloned().ok_or_else(|| "bad answer from the intranet (no result)".to_string())
}

/// `text` as a header value: printable ASCII only, clipped; `fallback` when nothing is left.
fn header_value(text: &str, fallback: &str) -> String {
    let clean: String = text.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(80).collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() { fallback.to_string() } else { clean }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// The requests a fake intranet got: `(headers, body)`.
    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    /// A fake intranet recording the requests it gets: a POST is answered with `job` (as the
    /// intranet does: the command runs on its worker), a GET of the snapshot with
    /// `snapshot`; `status` for both. `job` empty: the POST is answered with it verbatim.
    fn fake(job: &'static str, snapshot: &'static str, status: u16) -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut length = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&h);
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let answer = if head.starts_with("GET /api/snapshot") { snapshot } else { job };
                log.lock().unwrap().push((head, String::from_utf8_lossy(&body).to_string()));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (addr, seen)
    }

    #[test]
    fn calls_the_intranet_as_the_technician_and_waits_for_the_job() {
        let (base, seen) = fake(
            r#"{"result": {"job": 7}}"#,
            r#"{"revision": 3, "user": "riccardo", "completed": [{"job": 6, "command": "x", "result": {"other": true}, "error": ""}, {"job": 7, "command": "customers.search", "result": {"customers": [{"id": 5, "name": "Rossi"}], "count": 1}, "error": ""}]}"#,
            200,
        );
        let call = caller(&format!("{base}/"), " secret-token ", "").unwrap();
        let v = call("riccardo", "customers.search", &json!({"query": "rossi"})).unwrap();
        assert_eq!(v["count"], 1, "{v}");
        let requests = seen.lock().unwrap().clone();
        let (head, body) = requests.first().cloned().unwrap();
        assert!(head.starts_with("POST /api/cmd "), "{head}");
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer secret-token\r\n"), "{head}");
        assert!(lower.contains("x-intranet-user: riccardo\r\n") && lower.contains("x-intranet-role: super_admin\r\n"), "{head}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"command": "customers.search", "params": {"query": "rossi"}}));
        // The snapshot is asked as the same technician (the job lives in their session).
        let (head, _) = requests.get(1).cloned().unwrap();
        assert!(head.starts_with("GET /api/snapshot?since=0 "), "{head}");
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer secret-token\r\n") && lower.contains("x-intranet-user: riccardo\r\n"), "{head}");
        // A hostile actor name cannot smuggle a header; the role from the environment is kept.
        let call = caller(&base, "", "ops_admin").unwrap();
        call("ric\r\nX-Intranet-Role: super_admin\r\n", "customer.get", &json!({"id": 5})).unwrap();
        let (head, _) = seen.lock().unwrap().get(2).cloned().unwrap();
        let lower = head.to_ascii_lowercase();
        assert!(!lower.contains("\r\nx-intranet-role: super_admin"), "{head}");
        assert!(lower.contains("x-intranet-role: ops_admin\r\n"), "{head}");
        assert!(!lower.contains("authorization:"), "no token, no header");
    }

    #[test]
    fn a_job_that_failed_or_never_finished_is_an_error() {
        let (base, _) = fake(r#"{"result": {"job": 7}}"#, r#"{"completed": [{"job": 7, "result": null, "error": "no customer 5"}]}"#, 200);
        let call = caller(&base, "t", "x").unwrap();
        assert_eq!(call("a", "customer.get", &json!({"id": 5})).unwrap_err(), "no customer 5");
        // A command answered without a job (an older intranet) is answered as is.
        let (base, _) = fake(r#"{"result": {"customers": []}}"#, "{}", 200);
        let call = caller(&base, "t", "x").unwrap();
        assert_eq!(call("a", "customers.search", &json!({})).unwrap()["customers"], json!([]));
    }

    #[test]
    fn refusals_and_nonsense_are_errors_not_panics() {
        let (base, _) = fake(r#"{"error": "the intranet is reserved to: super_admin"}"#, "{}", 403);
        let call = caller(&base, "t", "x").unwrap();
        assert_eq!(call("a", "customer.get", &json!({})).unwrap_err(), "the intranet is reserved to: super_admin");
        let (base, _) = fake("<html>", "<html>", 502);
        let call = caller(&base, "t", "x").unwrap();
        assert!(call("a", "customer.get", &json!({})).unwrap_err().contains("502"));
        let (base, _) = fake(r#"{"result": {"job": 1}}"#, r#"{"completed": "nonsense"}"#, 200);
        let call = caller(&base, "t", "x").unwrap();
        // Never answered: the wait ends (the test's timeout is the real one, kept short enough).
        let t = std::thread::spawn(move || call("a", "customer.get", &json!({})));
        let r = t.join().unwrap();
        assert!(r.unwrap_err().contains("did not answer job 1"));
        let call = caller("http://127.0.0.1:1", "t", "x").unwrap();
        assert!(call("a", "customer.get", &json!({})).unwrap_err().contains("unreachable"));
        assert!(caller("", "t", "x").is_none() && caller("intranet:7978", "t", "x").is_none());
    }
}
