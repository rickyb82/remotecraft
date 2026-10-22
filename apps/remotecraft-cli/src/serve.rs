//! `remotecraft-cli serve`: the HTTP API in front of the desk, for the framework and for the
//! per-technician MCP processes.
//!
//! - `POST /api/cmd {"command", "params"}` (`Content-Type: application/json`) runs any engine
//!   command;
//! - read-only REST routes for the common lookups (`GET /api/customers?query=…`,
//!   `GET /api/rustdesk/123456789`, `GET /api/interventions?from=…`, `GET /api/rustdesk/check`…);
//! - `GET /api/snapshot?since=N` for the web app: the whole desk when its revision moved;
//! - `GET /api/health` (no token), `GET /api/commands`;
//! - `POST /api/users/<user>/token`, `/revoke`, `GET /api/users` (service token only);
//! - the web app's files from a directory (`--static DIR`), when given.
//!
//! The service token is the framework's: it names the technician with `X-Remotecraft-User`.
//! A user token is a technician's (the MCP process): the token is the technician.
//!
//! Without a service token the server listens only on this machine, and only for this
//! machine: requests must name it in `Host` (a page elsewhere cannot point a name of its own at
//! 127.0.0.1 and pass for the same site), and only its own pages (or `--allow-origin`) may
//! call it from a browser.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use remotecraft_engine::core::{slug, storage};
use remotecraft_engine::{Desk, Error};
use serde_json::{Value, json};

pub struct Options {
    pub bind: String,
    /// The service token: required beyond localhost, at least [`MIN_TOKEN`] characters.
    pub token: String,
    /// Where `tokens.json` lives (the desk's directory); `None` keeps tokens in memory.
    pub tokens_dir: Option<PathBuf>,
    /// The web app's files (`trunk build` output), served at `/`.
    pub static_dir: Option<PathBuf>,
    /// Web origins (`https://app.example.it`) that may call a server without a token from a
    /// browser, besides this machine's own pages (`http://127.0.0.1:*`, `http://localhost:*`).
    pub allow_origins: Vec<String>,
}

/// Shortest service token accepted.
pub const MIN_TOKEN: usize = 32;

/// The origins of `--allow-origin` / `REMOTECRAFT_ALLOW_ORIGIN` (comma separated), in the form
/// browsers send them (`https://host[:port]`, no path, lower case).
pub fn parse_origins(text: &str) -> Vec<String> {
    text.split(',').map(|o| o.trim().trim_end_matches('/').to_ascii_lowercase()).filter(|o| !o.is_empty()).take(50).collect()
}

/// How long and how many: the defaults, smaller in the tests.
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    /// Time to send the request line and the headers, all of them.
    pub head: Duration,
    /// Connections served at once; the next ones get 503 until one ends.
    pub connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { head: Duration::from_secs(10), connections: 64 }
    }
}

struct Hub {
    desk: Mutex<Desk>,
    token: String,
    static_dir: Option<PathBuf>,
    tokens_path: Option<PathBuf>,
    /// User tokens: token → technician.
    tokens: Mutex<HashMap<String, String>>,
    allow_origins: Vec<String>,
    /// The port the server listens on (a request without a token must name it in `Host`).
    port: u16,
    limits: Limits,
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
            before.saturating_sub(t.len())
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

    /// The technician of a user token. Every stored token is compared (no early stop).
    fn user_of(&self, given: &str) -> Option<String> {
        let tokens = self.tokens.lock().ok()?;
        let mut found = None;
        for (t, u) in tokens.iter() {
            if constant_eq(given, t) {
                found = Some(u.clone());
            }
        }
        found
    }
}

/// Compares a token with a secret in a time that depends only on the token given: not on
/// where they differ, nor on the secret's length.
fn constant_eq(given: &str, secret: &str) -> bool {
    let (g, s) = (given.as_bytes(), secret.as_bytes());
    let mut diff = u8::from(g.len() != s.len());
    for (i, x) in g.iter().enumerate() {
        let y = i.checked_rem(s.len()).and_then(|j| s.get(j)).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// 32 random bytes from the OS, as hex. No randomness, no token.
fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).map_err(|e| format!("no random source for tokens: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Largest request body: a command's parameters are a few KB.
const MAX_BODY: usize = 1024 * 1024;
const MAX_HEADERS: usize = 100;
/// Largest request head: the request line and every header together.
const MAX_HEAD: u64 = 16 * 1024;
/// Time to send an accepted body (read only for an authenticated caller).
const BODY_DEADLINE: Duration = Duration::from_secs(30);
/// Time for each write of an answer.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// After an early answer: how long, and how much, of what the client still sends is read
/// and dropped.
const LINGER: Duration = Duration::from_secs(2);
const LINGER_BYTES: usize = 2 * MAX_BODY;

const BAD_REQUEST: &str = "400 Bad Request";
const TIMEOUT: &str = "408 Request Timeout";
const HEAD_TOO_LARGE: &str = "431 Request Header Fields Too Large";

/// `Instant::now() + d`, never overflowing (an impossible overflow gives "now": time is up).
fn deadline_in(d: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(d).unwrap_or(now)
}

/// A socket whose reads end at a deadline. The socket's own timeout starts again with every
/// byte, so a client sending one byte every few seconds would hold its thread for ever.
struct Timed {
    stream: TcpStream,
    deadline: Instant,
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

/// The request line and the headers. The body is read later, and only when the caller is
/// known.
pub(crate) struct Head {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    /// `Content-Length`.
    length: usize,
}

/// A line of the head, paid from what is left of [`MAX_HEAD`].
fn read_line(reader: &mut impl BufRead, budget: &mut u64) -> Result<String, &'static str> {
    let mut line = Vec::new();
    let n = reader.by_ref().take(*budget).read_until(b'\n', &mut line).map_err(|e| match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => TIMEOUT,
        _ => BAD_REQUEST,
    })?;
    *budget = budget.saturating_sub(u64::try_from(n).unwrap_or(u64::MAX));
    if !line.ends_with(b"\n") {
        return Err(if *budget == 0 { HEAD_TOO_LARGE } else { BAD_REQUEST });
    }
    String::from_utf8(line).map_err(|_| BAD_REQUEST)
}

/// Reads the head; the error is the status to answer.
fn read_head(reader: &mut impl BufRead) -> Result<Head, &'static str> {
    let mut budget = MAX_HEAD;
    let line = read_line(reader, &mut budget)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(BAD_REQUEST)?.to_string();
    let target = parts.next().ok_or(BAD_REQUEST)?.to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let mut headers = Vec::new();
    let mut length: Option<usize> = None;
    loop {
        let h = read_line(reader, &mut budget)?;
        if h.trim().is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(HEAD_TOO_LARGE);
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
            if k == "content-length" {
                let n: usize = v.parse().map_err(|_| BAD_REQUEST)?;
                // Two different lengths: which body is meant is anybody's guess.
                if length.is_some_and(|l| l != n) {
                    return Err(BAD_REQUEST);
                }
                length = Some(n);
            }
            headers.push((k, v));
        }
    }
    Ok(Head { method, path, query, headers, length: length.unwrap_or(0) })
}

/// The body announced by the head, at most [`MAX_BODY`] (checked by the caller).
fn read_body(reader: &mut impl Read, length: usize) -> Option<Vec<u8>> {
    let mut body = Vec::with_capacity(length.min(64 * 1024));
    reader.take(u64::try_from(length).ok()?).read_to_end(&mut body).ok()?;
    (body.len() == length).then_some(body)
}

fn header<'a>(head: &'a Head, name: &str) -> Option<&'a str> {
    head.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Decodes `%XX` escapes, and `+` as a space when `plus_is_space` (query strings only).
fn decode(text: &str, plus_is_space: bool) -> String {
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
            b'+' if plus_is_space => {
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

/// A query-string key or value: `%XX` escapes and `+` as a space.
fn url_decode(text: &str) -> String {
    decode(text, true)
}

/// A path segment: `%XX` escapes only (`/a+b.js` is the file `a+b.js`).
fn url_decode_path(text: &str) -> String {
    decode(text, false)
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

/// Headers every answer carries, the 503 of [`busy`] too: no guessing a type other than the
/// one sent, and no address (a `#token=` link's) passed on to another site.
const SAFETY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n";

/// Writes the status line and the headers. `cors` is an origin already allowed.
fn write_head(stream: &mut impl Write, status: &str, content_type: &str, length: u64, cache: &str, cors: Option<&str>) -> std::io::Result<()> {
    let cors = match cors {
        Some(origin) => format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type, Authorization, X-Remotecraft-User\r\nAccess-Control-Max-Age: 600\r\n"
        ),
        None => String::new(),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nCache-Control: {cache}\r\nConnection: close\r\n\
{SAFETY_HEADERS}Vary: Origin\r\n{cors}\r\n"
    );
    stream.write_all(head.as_bytes())
}

fn respond_bytes(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8], cors: Option<&str>) {
    let length = u64::try_from(body.len()).unwrap_or(u64::MAX);
    if write_head(stream, status, content_type, length, "no-store", cors).is_ok() {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

fn respond(stream: &mut TcpStream, status: &str, body: &Value, cors: Option<&str>) {
    respond_bytes(stream, status, "application/json; charset=utf-8", body.to_string().as_bytes(), cors);
}

/// After an answer sent before the whole request was read: stops sending, then reads and drops
/// what the client is still sending (a little, for a moment), so the connection ends cleanly
/// instead of being reset before the client has read the answer.
fn linger(stream: &mut TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let deadline = deadline_in(LINGER);
    let mut buf = [0u8; 8192];
    let mut left = LINGER_BYTES;
    while left > 0 {
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() || stream.set_read_timeout(Some(wait)).is_err() {
            break;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

fn mime_of(path: &Path) -> &'static str {
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

/// How long browsers and proxies keep a file of the web app. trunk names the script, the
/// module and the styles after their content (`remotecraft-web-1f2e3d4c5b6a7980_bg.wasm`):
/// those never change and are kept a year. Everything else (`index.html`, which names the
/// current ones) is checked again every time.
fn cache_control(rel: &str) -> &'static str {
    let path = Path::new(rel);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let stem = stem.strip_suffix("_bg").unwrap_or(stem);
    let hashed = stem
        .rsplit_once('-')
        .is_some_and(|(name, hash)| !name.is_empty() && (8..=32).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    if hashed && matches!(ext, "js" | "wasm" | "css") { "public, max-age=31536000, immutable" } else { "no-cache" }
}

/// Largest file of the web app served (the wasm module is a few MB).
const MAX_STATIC: u64 = 64 * 1024 * 1024;

/// A file of the web app (`index.html` for `/`). The decoded path must be plain names (no
/// root, `..`, `.`, drive or backslash: `/%2Fetc%2Fpasswd` must not become `/etc/passwd`),
/// the file it resolves to (symbolic links followed) must lie inside the directory, and only
/// regular files up to [`MAX_STATIC`] are sent, streamed from the disk (never whole in memory:
/// many slow readers of the wasm module would add up).
fn serve_static(stream: &mut TcpStream, dir: &Path, path: &str, cors: Option<&str>) {
    use std::path::Component;
    let rel = url_decode_path(path.trim_start_matches('/'));
    let rel = if rel.is_empty() { "index.html".to_string() } else { rel };
    let plain = Path::new(&rel).components().all(|c| matches!(c, Component::Normal(_)));
    if !plain || rel.contains(['\\', '\0', ':']) {
        respond_bytes(stream, BAD_REQUEST, "text/plain", b"bad path", cors);
        return;
    }
    match open_static(dir, &rel) {
        // The root page says that this server serves it: the web app talks to its own origin
        // only then (a page copied to another site does not).
        Some((file, len)) if rel == INDEX => match read_index(file, len) {
            Some(page) => {
                let page = with_server_mark(&page);
                let length = u64::try_from(page.len()).unwrap_or(u64::MAX);
                if write_head(stream, "200 OK", mime_of(Path::new(INDEX)), length, cache_control(INDEX), cors).is_ok() {
                    let _ = stream.write_all(&page);
                }
                let _ = stream.flush();
            }
            None => respond_bytes(stream, "500 Internal Server Error", "text/plain; charset=utf-8", b"index.html is too large or unreadable", cors),
        },
        Some((file, len)) => {
            if write_head(stream, "200 OK", mime_of(Path::new(&rel)), len, cache_control(&rel), cors).is_ok() {
                let _ = std::io::copy(&mut file.take(len), stream);
            }
            let _ = stream.flush();
        }
        None => respond_bytes(stream, "404 Not Found", "text/plain; charset=utf-8", b"not found", cors),
    }
}

/// The web app's root page.
const INDEX: &str = "index.html";
/// Largest root page read whole to mark it (trunk's is a couple of KB).
const MAX_INDEX: u64 = 1024 * 1024;
/// Added to the root page: the web app treats its own origin as the server only with it.
const SERVER_MARK: &str = r#"<meta name="remotecraft-server" content="same-origin">"#;

/// The whole root page, when it is at most [`MAX_INDEX`] bytes.
fn read_index(file: std::fs::File, len: u64) -> Option<Vec<u8>> {
    if len > MAX_INDEX {
        return None;
    }
    let mut page = Vec::with_capacity(usize::try_from(len).ok()?);
    file.take(MAX_INDEX.checked_add(1)?).read_to_end(&mut page).ok()?;
    (u64::try_from(page.len()).ok()? <= MAX_INDEX).then_some(page)
}

/// The page with [`SERVER_MARK`] right after its opening `<head>` tag (any case, with or
/// without attributes; `<header>` is not one), or before everything when it has none.
fn with_server_mark(page: &[u8]) -> Vec<u8> {
    let at = page
        .windows(5)
        .enumerate()
        .filter(|(_, w)| w.eq_ignore_ascii_case(b"<head"))
        .find_map(|(i, _)| {
            let rest = page.get(i.checked_add(5)?..)?;
            let next = *rest.first()?;
            if next != b'>' && next != b'/' && !next.is_ascii_whitespace() {
                return None;
            }
            let close = rest.iter().position(|&b| b == b'>')?;
            i.checked_add(5)?.checked_add(close)?.checked_add(1)
        })
        .unwrap_or(0);
    let (before, after) = page.split_at(at.min(page.len()));
    [before, SERVER_MARK.as_bytes(), after].concat()
}

/// `dir/rel` and its length, when it is a regular file inside `dir`, at most [`MAX_STATIC`].
fn open_static(dir: &Path, rel: &str) -> Option<(std::fs::File, u64)> {
    let root = dir.canonicalize().ok()?;
    let file = root.join(rel).canonicalize().ok()?;
    if !file.starts_with(&root) {
        return None;
    }
    // Checked before opening: opening a FIFO would wait for a writer.
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > MAX_STATIC {
        return None;
    }
    let f = std::fs::File::open(&file).ok()?;
    // And again on what was opened: the length sent is the length of this file.
    let meta = f.metadata().ok()?;
    (meta.is_file() && meta.len() <= MAX_STATIC).then_some((f, meta.len()))
}

const WELCOME: &str = "RemoteCraft server is running.\n\nAPI: POST /api/cmd {\"command\", \"params\"}  ·  GET /api/snapshot?since=N  ·  GET /api/health\nStart the server with --static DIR to serve the web app's files (apps/remotecraft-web, built with trunk).\n";

/// The HTTP status of an engine error.
fn status_of(e: &Error) -> &'static str {
    use remotecraft_engine::core::Error as E;
    match e {
        Error::UnknownCommand(_) | Error::Core(E::NotFound(_)) => "404 Not Found",
        Error::Core(E::Invalid(_)) => BAD_REQUEST,
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

fn authenticate(hub: &Hub, head: &Head) -> Result<Caller, &'static str> {
    let given = header(head, "authorization").and_then(|a| a.strip_prefix("Bearer ")).map(str::trim).filter(|t| !t.is_empty());
    let named = header(head, "x-remotecraft-user").map(slug).filter(|u| !u.is_empty());
    match given {
        None if hub.token.is_empty() => Ok(Caller::Service(named)),
        None => Err("missing token (Authorization: Bearer …)"),
        Some(t) if !hub.token.is_empty() && constant_eq(t, &hub.token) => Ok(Caller::Service(named)),
        Some(t) => hub.user_of(t).map(Caller::User).ok_or("unknown token"),
    }
}

/// `host[:port]` → the host (IPv6 in brackets, as written) and the port, if well formed.
fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, rest) = authority.split_at(end.checked_add(1)?);
        match rest {
            "" => (host, None),
            _ => (host, Some(rest.strip_prefix(':')?)),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let port = match port {
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => Some(p.parse().ok()?),
        Some(_) => return None,
        None => None,
    };
    (!host.is_empty()).then_some((host, port))
}

/// `localhost`, `127.x.y.z` or `[::1]`: names of this machine no DNS answer can change.
fn is_local_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<Ipv4Addr>().is_ok_and(|a| a.is_loopback())
        || host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).and_then(|h| h.parse::<Ipv6Addr>().ok()).is_some_and(|a| a.is_loopback())
}

/// `Host` names this machine on the server's port. Without a token, a page elsewhere could
/// otherwise point a name of its own at 127.0.0.1 (DNS rebinding) and read the desk as if it
/// were its own site.
fn host_is_local(hub: &Hub, head: &Head) -> bool {
    header(head, "host").and_then(split_host_port).is_some_and(|(host, port)| is_local_host(host) && port.unwrap_or(80) == hub.port)
}

/// What a request's `Origin` (a browser's page calling) is allowed.
enum Cors<'a> {
    /// No origin (not a browser, or the page's own site): nothing to allow.
    None,
    /// Named in `Access-Control-Allow-Origin`.
    Allow(&'a str),
    /// A page of another site calling a server without a token: refused.
    Refuse,
}

/// With a service token the API answers only to bearer tokens, which a browser never adds on
/// its own: any page may call it (the web app hosted elsewhere does). Without a token anyone
/// who reaches the API may use it, so only this machine's pages may (`http://127.0.0.1:*`,
/// `http://localhost:*`, `http://[::1]:*`: the server's own origin is one of them, since `Host`
/// must be local), and the origins of `--allow-origin`.
fn cors<'a>(hub: &Hub, head: &'a Head) -> Cors<'a> {
    let Some(origin) = header(head, "origin") else { return Cors::None };
    // Repeated in a header of the answer: printable ASCII only, of a sane length.
    let printable = !origin.is_empty() && origin.len() <= 200 && origin.bytes().all(|b| b.is_ascii_graphic());
    if !hub.token.is_empty() {
        return if printable { Cors::Allow(origin) } else { Cors::None };
    }
    let o = origin.to_ascii_lowercase();
    let local =
        o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).and_then(split_host_port).is_some_and(|(host, _)| is_local_host(host));
    if printable && (local || hub.allow_origins.contains(&o)) { Cors::Allow(origin) } else { Cors::Refuse }
}

/// A `Content-Type` of `application/json`: a page elsewhere can send a form or plain text to
/// any address without asking, but JSON only after the browser asked the server (CORS).
fn is_json(head: &Head) -> bool {
    header(head, "content-type").and_then(|c| c.split(';').next()).is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
}

/// A read-only REST route → the command and its parameters.
fn route(path: &str, query: &str) -> Option<(&'static str, Value)> {
    let parts: Vec<String> = path.trim_start_matches("/api/").split('/').filter(|p| !p.is_empty()).map(url_decode_path).collect();
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
        // The RustDesk check waits on the network (seconds when a firewall drops the
        // packets): it probes after the desk is released, so other requests go on.
        Ok(mut d) if command == "rustdesk.check" => {
            let check = d.prepare_rustdesk_check(params);
            drop(d);
            check.map(|c| c.run())
        }
        // The intranet too answers in seconds: asked with the desk released, then the
        // command runs with the answer.
        Ok(mut d) if matches!(command, "intranet.search" | "customer.import") => {
            let fetch = d.prepare_intranet(actor, command, params);
            drop(d);
            fetch.and_then(|f| {
                let answer = f.run();
                match hub.desk.lock() {
                    Ok(mut d) => d.execute_fetched(&f, answer),
                    Err(_) => Err(remotecraft_engine::Error::Core(remotecraft_engine::core::Error::Io("the desk is unavailable".into()))),
                }
            })
        }
        Ok(mut d) => d.execute_as(actor, command, params),
        Err(_) => return ("500 Internal Server Error", json!({"error": "the desk is unavailable"})),
    };
    match result {
        Ok(v) => ("200 OK", json!({"result": v})),
        Err(e) => (status_of(&e), json!({"error": e.to_string()})),
    }
}

fn handle(mut stream: TcpStream, hub: &Hub) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let Ok(socket) = stream.try_clone() else { return };
    let mut reader = BufReader::new(Timed { stream: socket, deadline: deadline_in(hub.limits.head) });
    let mut body_read = false;
    let length = match read_head(&mut reader) {
        Ok(head) => {
            answer(&mut stream, &mut reader, &head, hub, &mut body_read);
            head.length
        }
        Err(status) => {
            respond(&mut stream, status, &json!({"error": status.split_once(' ').map_or(status, |(_, why)| why)}), None);
            usize::MAX
        }
    };
    if length > 0 && !body_read {
        linger(&mut stream);
    }
}

/// Answers a request whose head was read; the body is read only for an authenticated
/// `POST /api/cmd` (`body_read` says when).
fn answer(stream: &mut TcpStream, reader: &mut BufReader<Timed>, head: &Head, hub: &Hub, body_read: &mut bool) {
    if hub.token.is_empty() && !host_is_local(hub, head) {
        respond(
            stream,
            "403 Forbidden",
            &json!({"error": "without a token the server answers only to this machine (Host: 127.0.0.1 or localhost)"}),
            None,
        );
        return;
    }
    let origin = match cors(hub, head) {
        Cors::None => None,
        Cors::Allow(o) => Some(o),
        Cors::Refuse => {
            respond(stream, "403 Forbidden", &json!({"error": "origin not allowed (start the server with --allow-origin ORIGIN)"}), None);
            return;
        }
    };
    if head.method == "OPTIONS" {
        respond_bytes(stream, "204 No Content", "text/plain", b"", origin);
        return;
    }
    if head.method == "GET" && head.path == "/api/health" {
        respond(stream, "200 OK", &json!({"ok": true, "name": "remotecraft", "version": env!("CARGO_PKG_VERSION")}), origin);
        return;
    }
    if !head.path.starts_with("/api/") {
        match (head.method.as_str(), &hub.static_dir) {
            ("GET", Some(dir)) => serve_static(stream, dir, &head.path, origin),
            ("GET", None) if head.path == "/" => respond_bytes(stream, "200 OK", "text/plain; charset=utf-8", WELCOME.as_bytes(), origin),
            ("GET", None) => {
                respond(stream, "404 Not Found", &json!({"error": "RemoteCraft API: see /api/health, POST /api/cmd, GET /api/commands"}), origin)
            }
            _ => respond(stream, "405 Method Not Allowed", &json!({"error": "method not allowed"}), origin),
        }
        return;
    }
    // Who is calling, before reading a byte of the body. A missing or wrong token is answered
    // at once: a fixed wait used to hold one of the connections served at once, so a few
    // hundred requests a second without a token turned every technician away with 503. Tokens
    // of at least 32 random characters cannot be guessed anyway.
    let caller = match authenticate(hub, head) {
        Ok(c) => c,
        Err(e) => {
            respond(stream, "401 Unauthorized", &json!({"error": e}), origin);
            return;
        }
    };
    if let Some(rest) = head.path.strip_prefix("/api/users") {
        if hub.token.is_empty() {
            respond(stream, "403 Forbidden", &json!({"error": "user tokens need a server with a service token (--token)"}), origin);
            return;
        }
        let Caller::Service(_) = caller else {
            respond(stream, "403 Forbidden", &json!({"error": "service token required"}), origin);
            return;
        };
        let parts: Vec<String> = rest.split('/').filter(|p| !p.is_empty()).map(|p| slug(&url_decode_path(p))).collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let (status, body) = match (head.method.as_str(), parts.as_slice()) {
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
        respond(stream, status, &body, origin);
        return;
    }
    let actor = match &caller {
        Caller::User(u) => u.clone(),
        Caller::Service(Some(u)) => u.clone(),
        Caller::Service(None) => String::new(),
    };
    let (status, body) = match (head.method.as_str(), head.path.as_str()) {
        ("GET", "/api/snapshot") => {
            let since: u64 = query_params(&head.query)["since"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            match hub.desk.lock() {
                Ok(mut d) => match d.snapshot(since, &actor) {
                    Ok(Some(s)) => ("200 OK", serde_json::to_value(s).unwrap_or(Value::Null)),
                    Ok(None) => ("200 OK", json!({"unchanged": true})),
                    Err(e) => (status_of(&e), json!({"error": e.to_string()})),
                },
                Err(_) => ("500 Internal Server Error", json!({"error": "the desk is unavailable"})),
            }
        }
        ("POST", "/api/cmd") if !is_json(head) => {
            ("415 Unsupported Media Type", json!({"error": "send the command as Content-Type: application/json"}))
        }
        ("POST", "/api/cmd") if head.length > MAX_BODY => {
            ("413 Payload Too Large", json!({"error": format!("the request is larger than {MAX_BODY} bytes")}))
        }
        ("POST", "/api/cmd") => {
            reader.get_mut().deadline = deadline_in(BODY_DEADLINE);
            match read_body(reader, head.length) {
                Some(bytes) => {
                    *body_read = true;
                    match serde_json::from_slice::<Value>(&bytes) {
                        Ok(body) => {
                            let command = body.get("command").and_then(Value::as_str).unwrap_or_default().to_string();
                            let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
                            run(hub, &actor, &command, &params)
                        }
                        Err(e) => (BAD_REQUEST, json!({"error": format!("invalid JSON: {e}")})),
                    }
                }
                None => (BAD_REQUEST, json!({"error": "the body is shorter than Content-Length, or too slow"})),
            }
        }
        ("GET", path) => match route(path, &head.query) {
            Some((command, params)) => run(hub, &actor, command, &params),
            None => ("404 Not Found", json!({"error": "unknown endpoint"})),
        },
        _ => ("405 Method Not Allowed", json!({"error": "method not allowed"})),
    };
    respond(stream, status, &body, origin);
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
    if !opts.token.is_empty() && opts.token.chars().count() < MIN_TOKEN {
        return Err(format!(
            "the service token is too short: use at least {MIN_TOKEN} random characters, for example REMOTECRAFT_TOKEN=$(openssl rand -hex 32)"
        ));
    }
    if !is_loopback(&opts.bind) && opts.token.is_empty() {
        return Err("binding beyond localhost needs a service token (REMOTECRAFT_TOKEN, and HTTPS in front: a reverse proxy or the framework)".into());
    }
    TcpListener::bind(&opts.bind).map_err(|e| format!("cannot listen on {}: {e}", opts.bind))
}

pub(crate) fn serve(listener: TcpListener, desk: Desk, opts: Options) {
    serve_with(listener, desk, opts, Limits::default());
}

/// One of the connections served at once, given back when dropped (the thread ended, or it
/// could not start).
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(live: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        let before = live.fetch_add(1, Ordering::SeqCst);
        let slot = Slot(live.clone());
        (before < max).then_some(slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The answer to a connection beyond the limit, without a thread.
fn busy(stream: &mut TcpStream) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let body = br#"{"error": "the server is busy, try again"}"#;
    let head = format!(
        "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
Retry-After: 1\r\nConnection: close\r\n{SAFETY_HEADERS}\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body));
}

pub(crate) fn serve_with(listener: TcpListener, desk: Desk, opts: Options, limits: Limits) {
    let hub = Arc::new(Hub {
        desk: Mutex::new(desk),
        token: opts.token,
        static_dir: opts.static_dir,
        tokens_path: opts.tokens_dir.map(|d| d.join("tokens.json")),
        tokens: Mutex::new(HashMap::new()),
        allow_origins: opts.allow_origins,
        port: listener.local_addr().map(|a| a.port()).unwrap_or(0),
        limits,
    });
    hub.load_tokens();
    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let Some(slot) = Slot::take(&live, limits.connections) else {
            busy(&mut stream);
            continue;
        };
        let h = hub.clone();
        let _ = std::thread::Builder::new().name("remotecraft-http".into()).spawn(move || {
            let _slot = slot;
            handle(stream, &h);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A service token long enough for [`bind`].
    pub(crate) const SVC: &str = "test-service-token-0123456789abcdef";

    fn options(token: &str) -> Options {
        Options { bind: "127.0.0.1:0".into(), token: token.into(), tokens_dir: None, static_dir: None, allow_origins: Vec::new() }
    }

    fn start_with(opts: Options, limits: Limits) -> String {
        let listener = bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut desk = Desk::demo();
        desk.set_actor("server");
        std::thread::spawn(move || serve_with(listener, desk, opts, limits));
        addr
    }

    fn start(token: &str) -> String {
        start_with(options(token), Limits::default())
    }

    /// Sends a request and gives the status, the head and the body. `Host` is the server's
    /// address and a POST is JSON, unless the headers say otherwise.
    fn raw(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n", body.len());
        let has = |name: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
        if !has("host") {
            req.push_str(&format!("Host: {addr}\r\n"));
        }
        if method == "POST" && !has("content-type") {
            req.push_str("Content-Type: application/json\r\n");
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(body);
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        let out = String::from_utf8_lossy(&out).into_owned();
        let status: u16 = out.split_whitespace().nth(1).unwrap_or_else(|| panic!("no answer: {out:?}")).parse().unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        (status, head.to_string(), body.to_string())
    }

    fn http(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
        let (status, _, body) = raw(addr, method, path, headers, body);
        (status, if body.is_empty() { Value::Null } else { serde_json::from_str(&body).unwrap() })
    }

    /// The value of a header in an answer's head.
    fn header_of<'a>(head: &'a str, name: &str) -> Option<&'a str> {
        head.lines().filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("remotecraft-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tokens_users_and_commands() {
        let addr = start(SVC);
        let bearer = format!("Bearer {SVC}");
        let auth = ("Authorization", bearer.as_str());
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
            "GET /api/status HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            "GET /api/status HTTP/1.1\r\nHost: \u{ff}\r\n\r\n",
        ] {
            let mut s = TcpStream::connect(&addr).unwrap();
            s.write_all(junk.as_bytes()).unwrap();
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
        }
        assert_eq!(http(&addr, "GET", "/api/health", &[], "").0, 200);
        assert!(bind(&Options { bind: "0.0.0.0:0".into(), ..options("") }).is_err());
    }

    #[test]
    fn an_open_server_answers_only_this_machine() {
        // A server without a token used to answer any site: the Origin was echoed, the Host
        // never checked, a text/plain POST (no preflight) ran commands, and anyone could
        // mint a user token, written into the desk's directory.
        let dir = temp_dir("open");
        let addr = start_with(Options { tokens_dir: Some(dir.clone()), ..options("") }, Limits::default());
        let port = addr.rsplit_once(':').unwrap().1.to_string();
        let cmd = r#"{"command": "customer.list"}"#;
        let evil = ("Origin", "https://evil.example");
        let (st, head, _) = raw(&addr, "POST", "/api/cmd", &[evil, ("Content-Type", "text/plain")], cmd);
        assert_eq!(st, 403);
        assert_eq!(header_of(&head, "access-control-allow-origin"), None, "{head}");
        let (st, head, _) = raw(&addr, "POST", "/api/cmd", &[evil], cmd);
        assert_eq!(st, 403);
        assert_eq!(header_of(&head, "access-control-allow-origin"), None, "{head}");
        let (st, head, _) = raw(&addr, "OPTIONS", "/api/cmd", &[evil, ("Access-Control-Request-Method", "POST")], "");
        assert_eq!(st, 403);
        assert_eq!(header_of(&head, "access-control-allow-origin"), None, "{head}");
        // Plain text needs no preflight: refused, origin or not.
        assert_eq!(http(&addr, "POST", "/api/cmd", &[("Content-Type", "text/plain")], cmd).0, 415);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[("Content-Type", "application/x-www-form-urlencoded")], cmd).0, 415);
        assert_eq!(http(&addr, "POST", "/api/cmd", &[("Content-Type", "Application/JSON; charset=utf-8")], cmd).0, 200);
        // DNS rebinding: a name of the attacker's pointed at 127.0.0.1.
        let named = format!("attacker.example:{port}");
        for host in ["attacker.example", named.as_str(), "127.0.0.1:1", "localhost.attacker.example", "127.0.0.1.nip.io"] {
            let (st, _, body) = raw(&addr, "GET", "/api/customers", &[("Host", host)], "");
            assert_eq!(st, 403, "{host}: {body}");
            assert!(!body.contains("Corvo"), "{host}: {body}");
        }
        let mut s = TcpStream::connect(&addr).unwrap();
        s.write_all(b"GET /api/customers HTTP/1.0\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 403"), "no Host: {out}");
        for host in [format!("localhost:{port}"), format!("LOCALHOST:{port}"), format!("[::1]:{port}"), format!("127.0.0.1:{port}")] {
            assert_eq!(raw(&addr, "GET", "/api/customers", &[("Host", host.as_str())], "").0, 200, "{host}");
        }
        // This machine's pages (the web app under `trunk serve`, the server's own) may call.
        let own = format!("http://127.0.0.1:{port}");
        for origin in ["http://127.0.0.1:8783", "http://localhost:8783", "http://[::1]:8783", own.as_str()] {
            let (st, head, _) = raw(&addr, "POST", "/api/cmd", &[("Origin", origin)], cmd);
            assert_eq!(st, 200, "{origin}");
            assert_eq!(header_of(&head, "access-control-allow-origin"), Some(origin), "{head}");
            let (st, head, _) = raw(&addr, "OPTIONS", "/api/cmd", &[("Origin", origin), ("Access-Control-Request-Method", "POST")], "");
            assert_eq!(st, 204);
            assert!(header_of(&head, "access-control-allow-headers").is_some_and(|h| h.contains("Content-Type")), "{head}");
        }
        // No user tokens on an open server, and nothing written.
        assert_eq!(http(&addr, "POST", "/api/users/evil/token", &[], "").0, 403);
        assert_eq!(http(&addr, "GET", "/api/users", &[], "").0, 403);
        assert!(!dir.join("tokens.json").exists());
        // Other sites only when named.
        let addr = start_with(Options { allow_origins: parse_origins(" https://App.Example.it/ , "), ..options("") }, Limits::default());
        let (st, head, _) = raw(&addr, "POST", "/api/cmd", &[("Origin", "https://app.example.it")], cmd);
        assert_eq!(st, 200);
        assert_eq!(header_of(&head, "access-control-allow-origin"), Some("https://app.example.it"));
        assert_eq!(raw(&addr, "POST", "/api/cmd", &[evil], cmd).0, 403);
        // With a service token the bearer header protects the API: any page may call with it
        // (the web app hosted elsewhere), still only as JSON.
        let addr = start(SVC);
        let bearer = format!("Bearer {SVC}");
        let (st, head, _) = raw(&addr, "POST", "/api/cmd", &[evil, ("Authorization", &bearer)], cmd);
        assert_eq!(st, 200);
        assert_eq!(header_of(&head, "access-control-allow-origin"), Some("https://evil.example"));
        assert_eq!(raw(&addr, "POST", "/api/cmd", &[evil], cmd).0, 401);
        assert_eq!(raw(&addr, "POST", "/api/cmd", &[("Authorization", &bearer), ("Content-Type", "text/plain")], cmd).0, 415);
        let (_, head, _) = raw(&addr, "GET", "/api/health", &[("Origin", "https://a.example x")], "");
        assert_eq!(header_of(&head, "access-control-allow-origin"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_body_is_read_only_for_a_known_caller() {
        // 8 MB bodies used to be read into memory before the token was looked at.
        let addr = start(SVC);
        let bearer = format!("Bearer {SVC}");
        let ask = |auth: Option<&str>, length: usize| {
            let mut s = TcpStream::connect(&addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let auth = auth.map(|a| format!("Authorization: {a}\r\n")).unwrap_or_default();
            let head = format!("POST /api/cmd HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n{auth}Content-Length: {length}\r\n\r\n");
            // A part of the body, then nothing: the answer must not wait for the rest.
            s.write_all(format!("{head}{}", " ".repeat(1000)).as_bytes()).unwrap();
            let mut out = Vec::new();
            let _ = s.read_to_end(&mut out);
            String::from_utf8_lossy(&out).into_owned()
        };
        let started = Instant::now();
        assert!(ask(None, 8 * 1024 * 1024).starts_with("HTTP/1.1 401"));
        assert!(ask(Some("Bearer wrong"), 8 * 1024 * 1024).starts_with("HTTP/1.1 401"));
        assert!(ask(Some(&bearer), MAX_BODY + 1).starts_with("HTTP/1.1 413"));
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(http(&addr, "GET", "/api/status", &[("Authorization", "Bearer wrong")], "").0, 401);
        assert_eq!(http(&addr, "GET", "/api/status", &[("Authorization", &bearer)], "").0, 200);
    }

    #[test]
    fn requests_without_a_good_token_do_not_turn_technicians_away() {
        // A missing or wrong token used to wait 250 ms holding one of the connections served at
        // once: a handful of such requests filled them all, and the technicians got 503.
        let addr = start_with(options(SVC), Limits { head: Duration::from_secs(5), connections: 4 });
        let bearer = format!("Bearer {SVC}");
        for round in 0..3 {
            let go = Arc::new(std::sync::Barrier::new(8));
            let flood: Vec<_> = (0..8)
                .map(|i| {
                    let (addr, go) = (addr.clone(), go.clone());
                    std::thread::spawn(move || {
                        let auth = if i % 2 == 0 { "" } else { "Authorization: Bearer wrong\r\n" };
                        go.wait();
                        let mut s = TcpStream::connect(&addr).unwrap();
                        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                        let _ = s.write_all(format!("GET /api/status HTTP/1.1\r\nHost: {addr}\r\n{auth}\r\n").as_bytes());
                        let mut out = String::new();
                        let _ = s.read_to_string(&mut out);
                    })
                })
                .collect();
            // The flood has been sent; a technician comes right after.
            std::thread::sleep(Duration::from_millis(100));
            let (st, v) = http(&addr, "GET", "/api/status", &[("Authorization", &bearer)], "");
            assert_eq!(st, 200, "round {round}: {v}");
            for t in flood {
                t.join().unwrap();
            }
        }
    }

    #[test]
    fn slow_and_too_many_connections_are_cut() {
        let limits = Limits { head: Duration::from_millis(800), connections: 4 };
        let patient = Limits { head: Duration::from_secs(5), ..limits };
        // One byte at a time used to restart the timeout: a thread held for ever.
        let addr = start_with(options(SVC), limits);
        let mut s = TcpStream::connect(&addr).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let started = Instant::now();
        let _ = s.write_all(b"GET /api/health HTTP/1.1\r\n");
        let mut out = Vec::new();
        for _ in 0..60 {
            let _ = s.write_all(b"X");
            let mut buf = [0u8; 512];
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => {}
            }
            if !out.is_empty() {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 408"), "{:?}", String::from_utf8_lossy(&out));
        assert!(started.elapsed() < Duration::from_secs(4));
        // The head as a whole has a size: 16 KiB per line, 100 lines used to pass.
        let line = format!("X-Pad: {}\r\n", "a".repeat(250));
        let head = format!("GET /api/health HTTP/1.1\r\nHost: x\r\n{}\r\n", line.repeat(90));
        let mut s = TcpStream::connect(&addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let _ = s.write_all(head.as_bytes());
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 431"), "{out}");
        // Beyond the limit, connections are turned away at once, until some end.
        let addr = start_with(options(SVC), patient);
        let idle: Vec<TcpStream> = (0..4).map(|_| TcpStream::connect(&addr).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(200));
        let mut s = TcpStream::connect(&addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 503"), "{out}");
        // Written by hand, without a thread, but with the headers of every answer.
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        assert_eq!(header_of(head, "x-content-type-options"), Some("nosniff"), "{head}");
        assert_eq!(header_of(head, "referrer-policy"), Some("no-referrer"), "{head}");
        assert_eq!(header_of(head, "retry-after"), Some("1"), "{head}");
        assert_eq!(header_of(head, "content-length"), Some(body.len().to_string().as_str()), "{head}");
        drop(idle);
        let back = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            let mut s = TcpStream::connect(&addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let _ = s.write_all(b"GET /api/health HTTP/1.1\r\nHost: x\r\n\r\n");
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out.starts_with("HTTP/1.1 200")
        });
        assert!(back, "the slots were not given back");
    }

    #[test]
    fn snapshots_and_static_files() {
        let dir = temp_dir("static");
        std::fs::write(dir.join("index.html"), "<html>app</html>").unwrap();
        std::fs::write(dir.join("app.wasm"), [0u8, 1, 2]).unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/app.js"), "export {}").unwrap();
        std::fs::write(dir.join("a+b.js"), "plus").unwrap();
        // A module of a few MB arrives whole, streamed from the disk.
        let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(dir.join("remotecraft-web-1f2e3d4c5b6a7980_bg.wasm"), &big).unwrap();
        let addr = start_with(Options { static_dir: Some(dir.clone()), ..options(SVC) }, Limits::default());
        let bearer = format!("Bearer {SVC}");
        let auth = ("Authorization", bearer.as_str());
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
        let get = |path: &str| {
            let mut s = TcpStream::connect(&addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut out = Vec::new();
            let _ = s.read_to_end(&mut out);
            out
        };
        let text = |path: &str| String::from_utf8_lossy(&get(path)).into_owned();
        let out = text("/");
        assert!(out.starts_with("HTTP/1.1 200") && out.contains("text/html") && out.ends_with("<html>app</html>"), "{out}");
        // The index names the current module: checked every time; the token in a link's
        // address is never sent on to another site.
        let (head, page) = out.split_once("\r\n\r\n").unwrap();
        assert_eq!(header_of(head, "cache-control"), Some("no-cache"));
        assert_eq!(header_of(head, "referrer-policy"), Some("no-referrer"));
        // The root page carries the server's mark (no `<head>`: in front), and its length.
        assert_eq!(page, format!("{SERVER_MARK}<html>app</html>"));
        assert_eq!(header_of(head, "content-length"), Some(page.len().to_string().as_str()));
        std::fs::write(dir.join("index.html"), "<!DOCTYPE html>\n<HTML><Head lang=\"it\">\n<title>RemoteCraft</title></head></html>").unwrap();
        let out = text("/index.html");
        let (head, page) = out.split_once("\r\n\r\n").unwrap();
        assert_eq!(page, format!("<!DOCTYPE html>\n<HTML><Head lang=\"it\">{SERVER_MARK}\n<title>RemoteCraft</title></head></html>"));
        assert_eq!(header_of(head, "content-length"), Some(page.len().to_string().as_str()));
        // Other pages are sent as they are.
        std::fs::write(dir.join("other.html"), "<head></head>").unwrap();
        assert!(text("/other.html").ends_with("\r\n\r\n<head></head>"));
        // A root page too large to read whole is refused, never cut.
        std::fs::write(dir.join("index.html"), vec![b' '; usize::try_from(MAX_INDEX).unwrap() + 1]).unwrap();
        assert!(text("/").starts_with("HTTP/1.1 500"));
        assert!(text("/app.wasm").contains("application/wasm"));
        assert!(text("/assets/app.js").starts_with("HTTP/1.1 200"));
        // `+` in a path is a plus, not a space.
        assert!(text("/a+b.js").ends_with("\r\n\r\nplus"));
        let out = get("/remotecraft-web-1f2e3d4c5b6a7980_bg.wasm");
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&out[..split]).into_owned();
        assert_eq!(header_of(&head, "content-length"), Some("3000000"));
        assert_eq!(header_of(&head, "cache-control"), Some("public, max-age=31536000, immutable"));
        assert!(out[split + 4..] == big[..], "the module arrived damaged");
        // Nothing outside the directory, however the path is spelled: an encoded leading slash
        // used to reach any absolute path (the token in /proc/self/environ), and /dev/zero
        // used to grow until the server aborted.
        let mut bad = vec![
            "/../Cargo.toml",
            "/%2e%2e/Cargo.toml",
            "/missing.js",
            "/a%20b.js",
            "/%2Fetc%2Fhostname",
            "/%2Fproc%2Fself%2Fenviron",
            "/%2Fdev%2Fzero",
            "//etc/hostname",
            "/assets/%2E%2E/%2E%2E/Cargo.toml",
            "/./index.html",
            "/C:%5CWindows%5Cwin.ini",
            "/assets",
            "/assets/",
        ];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", dir.join("leak.txt")).unwrap();
            std::os::unix::fs::symlink("/dev/zero", dir.join("zero.js")).unwrap();
            bad.extend(["/leak.txt", "/zero.js"]);
            // A FIFO would block the answer forever if it were opened.
            if std::process::Command::new("mkfifo").arg(dir.join("pipe.js")).status().is_ok_and(|s| s.success()) {
                bad.push("/pipe.js");
            }
        }
        for path in bad {
            let out = text(path);
            assert!(out.starts_with("HTTP/1.1 4"), "{path}: {out}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_and_commands_without_a_named_user_use_the_desk_actor() {
        for token in ["", SVC] {
            let addr = start(token);
            let bearer = format!("Bearer {token}");
            let auth: Vec<(&str, &str)> = if token.is_empty() { vec![] } else { vec![("Authorization", bearer.as_str())] };
            let (st, snap) = http(&addr, "GET", "/api/snapshot?since=0", &auth, "");
            assert_eq!(st, 200);
            assert_eq!(snap["actor"], "server", "the web app finds its own timers by this name");
            let body = r#"{"command": "intervention.start", "params": {"customer": 2, "kind": "phone"}}"#;
            let (_, v) = http(&addr, "POST", "/api/cmd", &auth, body);
            assert_eq!(v["result"]["intervention"]["technician"], snap["actor"]);
        }
    }

    #[test]
    fn rustdesk_check_does_not_hold_the_desk() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let mut desk = Desk::demo();
        desk.set_rustdesk_probe(
            "127.0.0.1",
            Box::new(move |_: &str, _: u16| {
                let released = wait.lock().map(|rx| rx.recv_timeout(Duration::from_secs(10)).is_ok()).unwrap_or(false);
                if released { Ok(1) } else { Err("not released".into()) }
            }),
        );
        let opts = options("");
        let listener = bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || serve(listener, desk, opts));
        let done = Arc::new(AtomicBool::new(false));
        let check = {
            let (addr, done) = (addr.clone(), done.clone());
            std::thread::spawn(move || {
                let r = http(&addr, "GET", "/api/rustdesk/check", &[], "");
                done.store(true, Ordering::SeqCst);
                r
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        // While the probes wait, the desk still answers.
        assert_eq!(http(&addr, "GET", "/api/status", &[], "").0, 200);
        assert!(!done.load(Ordering::SeqCst), "the status waited for the check");
        for _ in 0..3 {
            go.send(()).unwrap();
        }
        let (st, v) = check.join().unwrap();
        assert_eq!(st, 200, "{v}");
        assert_eq!(v["result"]["ports"][0]["ok"], true);
        assert_eq!(http(&addr, "GET", "/api/rustdesk/check?host=a%20b", &[], "").0, 400);
    }

    #[test]
    fn query_strings_decode() {
        assert_eq!(url_decode("a%20b+c%zz%"), "a b c%zz%");
        assert_eq!(url_decode_path("a%20b+c%zz%"), "a b+c%zz%");
        assert_eq!(query_params("query=Ros%C3%A0&token=x&flag"), json!({"query": "Rosà", "flag": "true"}));
        assert!(route("/api/customers/1/export", "").is_some_and(|(c, p)| c == "gdpr.export" && p["customer"] == "1"));
        assert!(route("/api/customers/Rossi+Figli", "").is_some_and(|(_, p)| p["customer"] == "Rossi+Figli"));
        assert!(route("/api/elsewhere", "").is_none());
    }

    #[test]
    fn service_tokens_are_long_and_compared_blindly() {
        let short = bind(&options("un-segreto-lungo")).unwrap_err();
        assert!(short.contains("openssl rand -hex 32"), "{short}");
        assert!(bind(&options(&"x".repeat(MIN_TOKEN))).is_ok());
        assert!(constant_eq(SVC, SVC));
        assert!(!constant_eq("", SVC));
        assert!(!constant_eq(&SVC[..10], SVC));
        assert!(!constant_eq(&format!("{SVC}x"), SVC));
        assert!(!constant_eq(&SVC.repeat(2), SVC));
        assert!(!constant_eq("x", ""));
        assert!(!constant_eq(&SVC.replace('0', "1"), SVC));
    }

    #[test]
    fn hosts_origins_and_cache() {
        assert_eq!(split_host_port("127.0.0.1:7979"), Some(("127.0.0.1", Some(7979))));
        assert_eq!(split_host_port("[::1]:80"), Some(("[::1]", Some(80))));
        assert_eq!(split_host_port("[::1]"), Some(("[::1]", None)));
        assert_eq!(split_host_port("localhost"), Some(("localhost", None)));
        for bad in ["", ":80", "a:", "a:b", "a:99999", "::1:80", "[::1", "[::1]x", "a:1:2"] {
            assert_eq!(split_host_port(bad), None, "{bad}");
        }
        assert!(is_local_host("127.0.0.2") && is_local_host("[::1]") && is_local_host("LocalHost"));
        assert!(!is_local_host("10.0.0.1") && !is_local_host("localhost.example") && !is_local_host("[::2]") && !is_local_host("::1"));
        assert_eq!(cache_control("index.html"), "no-cache");
        assert_eq!(cache_control("remotecraft-web-1f2e3d4c5b6a7980.js"), "public, max-age=31536000, immutable");
        assert_eq!(cache_control("remotecraft-web-1f2e3d4c5b6a7980_bg.wasm"), "public, max-age=31536000, immutable");
        assert_eq!(cache_control("app.wasm"), "no-cache");
        assert_eq!(cache_control("snippets/x-1f2e3d4c5b6a7980/inline0.js"), "no-cache");
        assert_eq!(cache_control("-1f2e3d4c5b6a7980.js"), "no-cache");
        assert_eq!(cache_control("logo-1f2e3d4c5b6a7980.png"), "no-cache");
        assert_eq!(parse_origins("https://a.example/, ,HTTP://B.example:8080"), vec!["https://a.example", "http://b.example:8080"]);
    }

    #[test]
    fn the_root_page_is_marked_after_its_head_tag() {
        let mark = |page: &str| String::from_utf8(with_server_mark(page.as_bytes())).unwrap();
        let m = SERVER_MARK;
        assert_eq!(mark("<html><head><title>x</title></head></html>"), format!("<html><head>{m}<title>x</title></head></html>"));
        assert_eq!(mark("<HEAD>"), format!("<HEAD>{m}"));
        assert_eq!(mark("<head\n  data-x=\"1\">a"), format!("<head\n  data-x=\"1\">{m}a"));
        // `<header>` is not the head; the real one later is.
        assert_eq!(mark("<header>h</header><head>"), format!("<header>h</header><head>{m}"));
        assert_eq!(mark("<header>h</header>"), format!("{m}<header>h</header>"));
        // An opening tag never closed, or nothing at all: in front.
        assert_eq!(mark("<head"), format!("{m}<head"));
        assert_eq!(mark("<head lang=it"), format!("{m}<head lang=it"));
        assert_eq!(mark(""), m);
        assert_eq!(with_server_mark(b"<head>\xff\xfe"), [b"<head>".as_slice(), m.as_bytes(), b"\xff\xfe"].concat());
    }

    #[test]
    fn heads_are_bounded() {
        let head = |text: &str| read_head(&mut std::io::Cursor::new(text.as_bytes().to_vec()));
        let h = head("POST /api/cmd?x=1 HTTP/1.1\r\nContent-Length: 5\r\nHost: a\r\n\r\nhello").unwrap();
        assert_eq!((h.method.as_str(), h.path.as_str(), h.query.as_str(), h.length), ("POST", "/api/cmd", "x=1", 5));
        assert_eq!(header(&h, "host"), Some("a"));
        assert_eq!(head("GET / HTTP/1.1\r\n").err(), Some(BAD_REQUEST));
        assert_eq!(head(&format!("GET / HTTP/1.1\r\n{}\r\n", "A: b\r\n".repeat(MAX_HEADERS + 1))).err(), Some(HEAD_TOO_LARGE));
        assert_eq!(head(&format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(20_000))).err(), Some(HEAD_TOO_LARGE));
        assert_eq!(head("GET / HTTP/1.1\r\nContent-Length: 18446744073709551616\r\n\r\n").err(), Some(BAD_REQUEST));
        assert_eq!(head("GET / HTTP/1.1\r\nX: \u{ff}\r\n\r\n").map(|h| h.headers.len()).ok(), Some(1));
        assert_eq!(read_head(&mut std::io::Cursor::new(b"GET / HTTP/1.1\r\nX: \xff\r\n\r\n".to_vec())).err(), Some(BAD_REQUEST));
        assert!(read_body(&mut std::io::Cursor::new(b"abc".to_vec()), 5).is_none());
        assert_eq!(read_body(&mut std::io::Cursor::new(b"abcdef".to_vec()), 3).as_deref(), Some(&b"abc"[..]));
    }
}
