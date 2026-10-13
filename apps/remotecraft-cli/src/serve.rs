//! `remotecraft-cli serve`: the HTTP API in front of the desk, for the framework and for the
//! per-technician MCP processes.
//!
//! - `POST /api/cmd {"command", "params"}` runs any engine command;
//! - read-only REST routes for the common lookups (`GET /api/customers?query=…`,
//!   `GET /api/rustdesk/123456789`, `GET /api/interventions?from=…`, `GET /api/rustdesk/check`…);
//! - `GET /api/snapshot?since=N` for the web app: the whole desk when its revision moved;
//! - `GET /api/health` (no token), `GET /api/commands`;
//! - `POST /api/users/<user>/token`, `/revoke`, `GET /api/users` (service token only);
//! - the web app's files from a directory (`--static DIR`), when given.
//!
//! The service token is the framework's: it names the technician with `X-Remotecraft-User`.
//! A user token is a technician's (the MCP process): the token is the technician.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use remotecraft_engine::core::{slug, storage};
use remotecraft_engine::{Desk, Error};
use serde_json::{Value, json};

pub struct Options {
    pub bind: String,
    /// The service token: required beyond localhost.
    pub token: String,
    /// Where `tokens.json` lives (the desk's directory); `None` keeps tokens in memory.
    pub tokens_dir: Option<PathBuf>,
    /// The web app's files (`trunk build` output), served at `/`.
    pub static_dir: Option<PathBuf>,
}

struct Hub {
    desk: Mutex<Desk>,
    token: String,
    static_dir: Option<PathBuf>,
    tokens_path: Option<PathBuf>,
    /// User tokens: token → technician.
    tokens: Mutex<HashMap<String, String>>,
}

impl Hub {
    fn load_tokens(&self) {
        if let Some(p) = &self.tokens_path
            && let Ok(text) = std::fs::read_to_string(p)
            && let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&text)
            && let Ok(mut t) = self.tokens.lock()
        {
            *t = map;
        }
    }

    fn save_tokens(&self) -> Result<(), String> {
        let Some(p) = &self.tokens_path else { return Ok(()) };
        let t = self.tokens.lock().map_err(|_| "tokens unavailable".to_string())?;
        let text = serde_json::to_string_pretty(&*t).map_err(|e| e.to_string())?;
        storage::write_private(p, text.as_bytes()).map_err(|e| e.to_string())
    }

    fn mint(&self, user: &str) -> Result<String, String> {
        let token = random_token()?;
        self.tokens.lock().map_err(|_| "tokens unavailable".to_string())?.insert(token.clone(), user.to_string());
        self.save_tokens()?;
        Ok(token)
    }

    fn revoke(&self, user: &str) -> Result<usize, String> {
        let n = {
            let mut t = self.tokens.lock().map_err(|_| "tokens unavailable".to_string())?;
            let before = t.len();
            t.retain(|_, u| u != user);
            before - t.len()
        };
        self.save_tokens()?;
        Ok(n)
    }

    fn users(&self) -> Vec<String> {
        let mut out: Vec<String> = self.tokens.lock().map(|t| t.values().cloned().collect()).unwrap_or_default();
        out.sort();
        out.dedup();
        out
    }

    fn user_of(&self, token: &str) -> Option<String> {
        self.tokens.lock().ok()?.iter().find(|(t, _)| constant_eq(t, token)).map(|(_, u)| u.clone())
    }
}

/// Compares secrets in time independent of where they differ.
fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes from the OS, as hex. No randomness, no token.
fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).map_err(|e| format!("no random source for tokens: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_HEADERS: usize = 100;
const MAX_LINE: u64 = 16 * 1024;

pub(crate) struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_line(reader: &mut impl BufRead) -> Option<String> {
    let mut line = String::new();
    let n = reader.by_ref().take(MAX_LINE).read_line(&mut line).ok()?;
    (n > 0).then_some(line)
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let line = read_line(&mut reader)?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let mut headers = Vec::new();
    let mut length = 0usize;
    loop {
        let h = read_line(&mut reader)?;
        if h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
            if k == "content-length" {
                length = v.parse().ok()?;
            }
            headers.push((k, v));
        }
        if headers.len() > MAX_HEADERS {
            return None;
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request { method, path, query, headers, body })
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Decodes `%XX` escapes and `+`.
fn url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'%' => match bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 3;
                }
                None => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The query string as command parameters (every value a string; the engine converts).
fn query_params(query: &str) -> Value {
    let mut out = serde_json::Map::new();
    for kv in query.split('&').filter(|s| !s.is_empty()).take(50) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, "true"));
        let k = url_decode(k);
        if k != "token" && k != "user" {
            out.insert(k, Value::String(url_decode(v)));
        }
    }
    Value::Object(out)
}

fn respond_bytes(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8], origin: Option<&str>) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\
Access-Control-Allow-Origin: {}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, X-Remotecraft-User\r\nAccess-Control-Max-Age: 600\r\n\r\n",
        body.len(),
        origin.unwrap_or("*")
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn respond(stream: &mut TcpStream, status: &str, body: &Value, origin: Option<&str>) {
    respond_bytes(stream, status, "application/json; charset=utf-8", body.to_string().as_bytes(), origin);
}

fn mime_of(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// A file of the web app (`index.html` for `/`); paths never leave the directory.
fn serve_static(stream: &mut TcpStream, dir: &std::path::Path, path: &str, origin: Option<&str>) {
    let rel = url_decode(path.trim_start_matches('/'));
    let rel = if rel.is_empty() { "index.html".to_string() } else { rel };
    if rel.contains("..") || rel.contains('\\') || rel.contains('\0') {
        respond_bytes(stream, "400 Bad Request", "text/plain", b"bad path", origin);
        return;
    }
    let file = dir.join(&rel);
    match std::fs::read(&file) {
        Ok(bytes) => respond_bytes(stream, "200 OK", mime_of(&file), &bytes, origin),
        Err(_) => respond_bytes(stream, "404 Not Found", "text/plain; charset=utf-8", b"not found", origin),
    }
}

const WELCOME: &str = "RemoteCraft server is running.\n\nAPI: POST /api/cmd {\"command\", \"params\"}  ·  GET /api/snapshot?since=N  ·  GET /api/health\nStart the server with --static DIR to serve the web app's files (apps/remotecraft-web, built with trunk).\n";

/// The HTTP status of an engine error.
fn status_of(e: &Error) -> &'static str {
    use remotecraft_engine::core::Error as E;
    match e {
        Error::UnknownCommand(_) | Error::Core(E::NotFound(_)) => "404 Not Found",
        Error::Core(E::Invalid(_)) => "400 Bad Request",
        Error::Core(E::Refused(_)) => "409 Conflict",
        Error::Core(E::Io(_)) => "500 Internal Server Error",
    }
}

/// Who is calling.
enum Caller {
    /// The framework (service token) or anyone on an open local server: the technician comes
    /// from `X-Remotecraft-User`.
    Service(Option<String>),
    /// A technician's token.
    User(String),
}

fn authenticate(hub: &Hub, req: &Request) -> Result<Caller, &'static str> {
    let given = header(req, "authorization").and_then(|a| a.strip_prefix("Bearer ")).map(str::trim).filter(|t| !t.is_empty());
    let named = header(req, "x-remotecraft-user").map(slug).filter(|u| !u.is_empty());
    match given {
        None if hub.token.is_empty() => Ok(Caller::Service(named)),
        None => Err("missing token (Authorization: Bearer …)"),
        Some(t) if !hub.token.is_empty() && constant_eq(t, &hub.token) => Ok(Caller::Service(named)),
        Some(t) => hub.user_of(t).map(Caller::User).ok_or("unknown token"),
    }
}

/// A read-only REST route → the command and its parameters.
fn route(path: &str, query: &str) -> Option<(&'static str, Value)> {
    let parts: Vec<String> = path.trim_start_matches("/api/").split('/').filter(|p| !p.is_empty()).map(url_decode).collect();
    let mut p = query_params(query);
    let mut put = |k: &str, v: &str| {
        if let Some(o) = p.as_object_mut() {
            o.insert(k.to_string(), Value::String(v.to_string()));
        }
    };
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let command = match parts.as_slice() {
        ["customers"] => "customer.list",
        ["customers", c] => {
            put("customer", c);
            "customer.get"
        }
        ["customers", c, "privacy"] => {
            put("customer", c);
            "privacy.status"
        }
        ["customers", c, "export"] => {
            put("customer", c);
            "gdpr.export"
        }
        ["devices"] => "device.list",
        ["devices", d] => {
            put("device", d);
            "device.get"
        }
        ["rustdesk"] => "rustdesk.setup",
        ["rustdesk", "check"] => "rustdesk.check",
        ["rustdesk", id] => {
            put("rustdesk_id", id);
            "device.find"
        }
        ["interventions"] => "intervention.list",
        ["interventions", "running"] => "intervention.running",
        ["interventions", i] => {
            put("id", i);
            "intervention.get"
        }
        ["report", "time"] => "report.time",
        ["notices"] => "notice.list",
        ["review"] => "gdpr.review",
        ["status"] => "app.status",
        ["commands"] => "app.commands",
        _ => return None,
    };
    Some((command, p))
}

fn run(hub: &Hub, actor: &str, command: &str, params: &Value) -> (&'static str, Value) {
    let result = match hub.desk.lock() {
        Ok(mut d) => d.execute_as(actor, command, params),
        Err(_) => return ("500 Internal Server Error", json!({"error": "the desk is unavailable"})),
    };
    match result {
        Ok(v) => ("200 OK", json!({"result": v})),
        Err(e) => (status_of(&e), json!({"error": e.to_string()})),
    }
}

fn handle(mut stream: TcpStream, hub: &Hub) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Some(req) = read_request(&stream) else {
        respond(&mut stream, "400 Bad Request", &json!({"error": "bad request"}), None);
        return;
    };
    let origin = header(&req, "origin").map(str::to_string);
    let origin = origin.as_deref();
    if req.method == "OPTIONS" {
        respond(&mut stream, "204 No Content", &json!({}), origin);
        return;
    }
    if req.method == "GET" && req.path == "/api/health" {
        respond(&mut stream, "200 OK", &json!({"ok": true, "name": "remotecraft", "version": env!("CARGO_PKG_VERSION")}), origin);
        return;
    }
    if !req.path.starts_with("/api/") {
        match (req.method.as_str(), &hub.static_dir) {
            ("GET", Some(dir)) => serve_static(&mut stream, dir, &req.path, origin),
            ("GET", None) if req.path == "/" => respond_bytes(&mut stream, "200 OK", "text/plain; charset=utf-8", WELCOME.as_bytes(), origin),
            ("GET", None) => {
                respond(&mut stream, "404 Not Found", &json!({"error": "RemoteCraft API: see /api/health, POST /api/cmd, GET /api/commands"}), origin)
            }
            _ => respond(&mut stream, "405 Method Not Allowed", &json!({"error": "method not allowed"}), origin),
        }
        return;
    }
    let caller = match authenticate(hub, &req) {
        Ok(c) => c,
        Err(e) => {
            respond(&mut stream, "401 Unauthorized", &json!({"error": e}), origin);
            return;
        }
    };
    if let Some(rest) = req.path.strip_prefix("/api/users") {
        let Caller::Service(_) = caller else {
            respond(&mut stream, "403 Forbidden", &json!({"error": "service token required"}), origin);
            return;
        };
        let parts: Vec<String> = rest.split('/').filter(|p| !p.is_empty()).map(|p| slug(&url_decode(p))).collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let (status, body) = match (req.method.as_str(), parts.as_slice()) {
            ("GET", []) => ("200 OK", json!({"users": hub.users()})),
            ("POST", [user, "token"]) if !user.is_empty() => match hub.mint(user) {
                Ok(t) => ("200 OK", json!({"user": user, "token": t})),
                Err(e) => ("500 Internal Server Error", json!({"error": e})),
            },
            ("POST", [user, "revoke"]) if !user.is_empty() => match hub.revoke(user) {
                Ok(n) => ("200 OK", json!({"user": user, "revoked": n})),
                Err(e) => ("500 Internal Server Error", json!({"error": e})),
            },
            _ => ("404 Not Found", json!({"error": "unknown users endpoint"})),
        };
        respond(&mut stream, status, &body, origin);
        return;
    }
    let actor = match &caller {
        Caller::User(u) => u.clone(),
        Caller::Service(Some(u)) => u.clone(),
        Caller::Service(None) => String::new(),
    };
    let (status, body) = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/snapshot") => {
            let since: u64 = query_params(&req.query)["since"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            match hub.desk.lock() {
                Ok(mut d) => match d.snapshot(since, &actor) {
                    Ok(Some(s)) => ("200 OK", serde_json::to_value(s).unwrap_or(Value::Null)),
                    Ok(None) => ("200 OK", json!({"unchanged": true})),
                    Err(e) => (status_of(&e), json!({"error": e.to_string()})),
                },
                Err(_) => ("500 Internal Server Error", json!({"error": "the desk is unavailable"})),
            }
        }
        ("POST", "/api/cmd") => match serde_json::from_slice::<Value>(&req.body) {
            Ok(body) => {
                let command = body.get("command").and_then(Value::as_str).unwrap_or_default().to_string();
                let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
                run(hub, &actor, &command, &params)
            }
            Err(e) => ("400 Bad Request", json!({"error": format!("invalid JSON: {e}")})),
        },
        ("GET", path) => match route(path, &req.query) {
            Some((command, params)) => run(hub, &actor, command, &params),
            None => ("404 Not Found", json!({"error": "unknown endpoint"})),
        },
        _ => ("405 Method Not Allowed", json!({"error": "method not allowed"})),
    };
    respond(&mut stream, status, &body, origin);
}

fn is_loopback(bind: &str) -> bool {
    bind.starts_with("127.") || bind.starts_with("localhost") || bind.starts_with("[::1]")
}

/// Binds and serves until the process ends.
pub fn run_server(desk: Desk, opts: Options) -> Result<(), String> {
    let listener = bind(&opts)?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    eprintln!("RemoteCraft API on http://{addr}/api/ ({})", if opts.token.is_empty() { "no token: local use only" } else { "token required" });
    match &opts.static_dir {
        Some(d) => eprintln!("serving the web app from {}", d.display()),
        None => eprintln!("no --static DIR: the web app is not served (see docs/deploy.md)"),
    }
    serve(listener, desk, opts);
    Ok(())
}

pub(crate) fn bind(opts: &Options) -> Result<TcpListener, String> {
    if !is_loopback(&opts.bind) && opts.token.is_empty() {
        return Err("binding beyond localhost needs --token SECRET (and HTTPS in front: a reverse proxy or the framework)".into());
    }
    TcpListener::bind(&opts.bind).map_err(|e| format!("cannot listen on {}: {e}", opts.bind))
}

pub(crate) fn serve(listener: TcpListener, desk: Desk, opts: Options) {
    let hub = Arc::new(Hub {
        desk: Mutex::new(desk),
        token: opts.token,
        static_dir: opts.static_dir,
        tokens_path: opts.tokens_dir.map(|d| d.join("tokens.json")),
        tokens: Mutex::new(HashMap::new()),
    });
    hub.load_tokens();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let h = hub.clone();
        let _ = std::thread::Builder::new().name("remotecraft-http".into()).spawn(move || handle(stream, &h));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(token: &str) -> String {
        let opts = Options { bind: "127.0.0.1:0".into(), token: token.into(), tokens_dir: None, static_dir: None };
        let listener = bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut desk = Desk::demo();
        desk.set_actor("server");
        std::thread::spawn(move || serve(listener, desk, opts));
        addr
    }

    fn http(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect(addr).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(body);
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let status: u16 = out.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = out.split_once("\r\n\r\n").unwrap().1;
        (status, serde_json::from_str(body).unwrap())
    }

    #[test]
    fn tokens_users_and_commands() {
        let addr = start("service-secret");
        let auth = ("Authorization", "Bearer service-secret");
        assert_eq!(http(&addr, "GET", "/api/health", &[], "").0, 200);
        assert_eq!(http(&addr, "GET", "/api/customers", &[], "").0, 401);
        assert_eq!(http(&addr, "GET", "/api/customers", &[("Authorization", "Bearer wrong")], "").0, 401);
        let (st, v) = http(&addr, "GET", "/api/customers?query=corvo", &[auth], "");
        assert_eq!(st, 200);
        assert_eq!(v["result"]["customers"][0]["name"], "Ferramenta Corvo");
        let (_, v) = http(&addr, "GET", "/api/rustdesk/100%20200%20300", &[auth], "");
        assert_eq!(v["result"]["devices"][0]["customer"]["id"], 1);
        // The framework names the technician; the work is recorded under that name.
        let body = r#"{"command": "intervention.start", "params": {"customer": 2, "kind": "phone"}}"#;
        let (st, v) = http(&addr, "POST", "/api/cmd", &[auth, ("X-Remotecraft-User", "Anna Rossi")], body);
        assert_eq!(st, 200, "{v}");
        assert_eq!(v["result"]["intervention"]["technician"], "anna-rossi");
        // A user token for the MCP process of a technician.
        let (_, t) = http(&addr, "POST", "/api/users/Marco/token", &[auth], "");
        let token = format!("Bearer {}", t["token"].as_str().unwrap());
        let (st, v) = http(&addr, "POST", "/api/cmd", &[("Authorization", &token)], r#"{"command": "app.status"}"#);
        assert_eq!(st, 200);
        assert_eq!(v["result"]["actor"], "marco");
        assert_eq!(http(&addr, "GET", "/api/users", &[("Authorization", &token)], "").0, 403);
        assert_eq!(http(&addr, "GET", "/api/users", &[auth], "").1["users"], json!(["marco"]));
        assert_eq!(http(&addr, "POST", "/api/users/marco/revoke", &[auth], "").1["revoked"], 1);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[("Authorization", &token)], r#"{"command": "app.status"}"#).0, 401);
        // Errors carry meaningful statuses.
        assert_eq!(http(&addr, "POST", "/api/cmd", &[auth], r#"{"command": "nope"}"#).0, 404);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[auth], r#"{"command": "customer.add", "params": {}}"#).0, 400);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[auth], r#"{"command": "intervention.start", "params": {"device": "300400500"}}"#).0, 409);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[auth], "{not json").0, 400);
        assert_eq!(http(&addr, "GET", "/api/customers/999", &[auth], "").0, 404);
        assert_eq!(http(&addr, "DELETE", "/api/customers/1", &[auth], "").0, 405);
        assert_eq!(http(&addr, "GET", "/api/nothing", &[auth], "").0, 404);
    }

    #[test]
    fn open_local_server_and_hostile_requests() {
        let addr = start("");
        assert_eq!(http(&addr, "GET", "/api/status", &[], "").0, 200);
        assert_eq!(http(&addr, "GET", "/api/report/time?group_by=month", &[], "").0, 200);
        assert_eq!(http(&addr, "GET", "/api/interventions?limit=-1", &[], "").0, 400);
        // Garbage on the socket gets an answer (or a closed connection), never a crash.
        for junk in [
            "\r\n\r\n",
            "GET\r\n\r\n",
            "POST /api/cmd HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n",
            "GET /api/status HTTP/1.1\r\nContent-Length: x\r\n\r\n",
        ] {
            let mut s = TcpStream::connect(&addr).unwrap();
            s.write_all(junk.as_bytes()).unwrap();
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
        }
        assert_eq!(http(&addr, "GET", "/api/health", &[], "").0, 200);
        assert!(bind(&Options { bind: "0.0.0.0:0".into(), token: String::new(), tokens_dir: None, static_dir: None }).is_err());
    }

    #[test]
    fn snapshots_and_static_files() {
        let dir = std::env::temp_dir().join(format!("remotecraft-static-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<html>app</html>").unwrap();
        std::fs::write(dir.join("app.wasm"), [0u8, 1, 2]).unwrap();
        let opts = Options { bind: "127.0.0.1:0".into(), token: "svc".into(), tokens_dir: None, static_dir: Some(dir.clone()) };
        let listener = bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || serve(listener, Desk::demo(), opts));
        let auth = ("Authorization", "Bearer svc");
        let (st, v) = http(&addr, "GET", "/api/snapshot?since=0", &[auth, ("X-Remotecraft-User", "Anna")], "");
        assert_eq!(st, 200);
        let rev = v["revision"].as_u64().unwrap();
        assert_eq!(v["actor"], "anna");
        assert_eq!(v["data"]["customers"].as_array().unwrap().len(), 3);
        assert_eq!(http(&addr, "GET", &format!("/api/snapshot?since={rev}"), &[auth], "").1, json!({"unchanged": true}));
        http(&addr, "POST", "/api/cmd", &[auth], r#"{"command": "customer.add", "params": {"name": "Nuovo"}}"#);
        let (_, v) = http(&addr, "GET", &format!("/api/snapshot?since={rev}"), &[auth], "");
        assert!(v["revision"].as_u64().unwrap() > rev);
        assert_eq!(http(&addr, "GET", "/api/snapshot", &[], "").0, 401);
        // Static files, with the index at the root, and nothing above the directory.
        let mut s = TcpStream::connect(&addr).unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200") && out.contains("text/html") && out.ends_with("<html>app</html>"), "{out}");
        let mut s = TcpStream::connect(&addr).unwrap();
        s.write_all(b"GET /app.wasm HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        assert!(String::from_utf8_lossy(&out).contains("application/wasm"));
        for bad in ["/../Cargo.toml", "/%2e%2e/Cargo.toml", "/missing.js"] {
            let mut s = TcpStream::connect(&addr).unwrap();
            s.write_all(format!("GET {bad} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            assert!(out.starts_with("HTTP/1.1 4"), "{bad}: {out}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_strings_decode() {
        assert_eq!(url_decode("a%20b+c%zz%"), "a b c%zz%");
        assert_eq!(query_params("query=Ros%C3%A0&token=x&flag"), json!({"query": "Rosà", "flag": "true"}));
        assert!(route("/api/customers/1/export", "").is_some_and(|(c, p)| c == "gdpr.export" && p["customer"] == "1"));
        assert!(route("/api/elsewhere", "").is_none());
    }
}
