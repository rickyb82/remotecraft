//! The operator's intranet as the engine's [`IntranetCall`]: `POST /api/cmd` of
//! `intranet-cli serve`, with its service token and the technician's name in the headers
//! (`X-Intranet-User`, `X-Intranet-Role`), as its framework sends them. Native only: the web
//! app's commands run on the server, which holds the connection.

use std::time::Duration;

use remotecraft_engine::IntranetCall;
use serde_json::{Value, json};

/// How long one call may take: a search while the technician waits.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Largest answer read (a customer with its sites and contracts).
const MAX_ANSWER: u64 = 4 * 1024 * 1024;
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
    let url = format!("{base}/api/cmd");
    let token = token.trim().to_string();
    let role = remotecraft_engine::core::line(role);
    let role = if role.is_empty() { "super_admin".to_string() } else { role };
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(TIMEOUT)).build().into();
    Some(Box::new(move |actor: &str, command: &str, params: &Value| call(&agent, &url, &token, &role, actor, command, params)))
}

fn call(agent: &ureq::Agent, url: &str, token: &str, role: &str, actor: &str, command: &str, params: &Value) -> Result<Value, String> {
    let body = json!({"command": command, "params": params}).to_string();
    // Header values are one line of printable text: the actor comes from a token file or a
    // framework header, the role from the environment.
    let actor = header_value(actor, "remotecraft");
    let mut req = agent
        .post(url)
        .header("Content-Type", "application/json")
        .header("X-Intranet-User", &actor)
        .header("X-Intranet-Role", &header_value(role, "super_admin"));
    if !token.is_empty() {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let mut resp = req.send(body).map_err(|e| format!("the intranet is unreachable ({e})"))?;
    let status = resp.status();
    let text = resp.body_mut().with_config().limit(MAX_ANSWER).read_to_string().map_err(|e| format!("bad answer from the intranet: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("bad answer from the intranet ({status})"))?;
    match (v.get("result"), v.get("error").and_then(Value::as_str)) {
        (_, Some(e)) => Err(remotecraft_engine::core::clip(e, MAX_ERROR).to_string()),
        (Some(r), None) => Ok(r.clone()),
        (None, None) => Err(format!("bad answer from the intranet ({status})")),
    }
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

    /// A fake intranet recording the requests it gets, answering `answer`.
    fn fake(answer: &'static str, status: u16) -> (String, Seen) {
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
    fn calls_the_intranet_as_the_technician_with_the_token() {
        let (base, seen) = fake(r#"{"result": {"customers": [{"id": 5, "name": "Rossi"}], "count": 1}}"#, 200);
        let call = caller(&format!("{base}/"), " secret-token ", "").unwrap();
        let v = call("riccardo", "customers.search", &json!({"query": "rossi"})).unwrap();
        assert_eq!(v["count"], 1);
        let (head, body) = seen.lock().unwrap().first().cloned().unwrap();
        assert!(head.starts_with("POST /api/cmd "), "{head}");
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer secret-token\r\n"), "{head}");
        assert!(lower.contains("x-intranet-user: riccardo\r\n") && lower.contains("x-intranet-role: super_admin\r\n"), "{head}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"command": "customers.search", "params": {"query": "rossi"}}));
        // A hostile actor name cannot smuggle a header; the role from the environment is kept.
        let call = caller(&base, "", "ops_admin").unwrap();
        call("ric\r\nX-Intranet-Role: super_admin\r\n", "customer.get", &json!({"id": 5})).unwrap();
        let (head, _) = seen.lock().unwrap().get(1).cloned().unwrap();
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("x-intranet-user: ricx-intranet-role: super_admin\r\n") || !lower.contains("x-intranet-role: super_admin"), "{head}");
        assert!(lower.contains("x-intranet-role: ops_admin\r\n"), "{head}");
        assert!(!lower.contains("authorization:"), "no token, no header");
    }

    #[test]
    fn refusals_and_nonsense_are_errors_not_panics() {
        let (base, _) = fake(r#"{"error": "the intranet is reserved to: super_admin"}"#, 403);
        let call = caller(&base, "t", "x").unwrap();
        assert_eq!(call("a", "customer.get", &json!({})).unwrap_err(), "the intranet is reserved to: super_admin");
        let (base, _) = fake("<html>", 502);
        let call = caller(&base, "t", "x").unwrap();
        assert!(call("a", "customer.get", &json!({})).unwrap_err().contains("502"));
        let call = caller("http://127.0.0.1:1", "t", "x").unwrap();
        assert!(call("a", "customer.get", &json!({})).unwrap_err().contains("unreachable"));
        assert!(caller("", "t", "x").is_none() && caller("intranet:7978", "t", "x").is_none());
    }
}
