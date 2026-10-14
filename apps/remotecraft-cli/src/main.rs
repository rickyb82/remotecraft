//! RemoteCraft headless.
//!
//! ```text
//! remotecraft-cli serve [--bind 127.0.0.1:7979] [--token SECRET] [--static DIR]   # HTTP API (and the web app)
//! remotecraft-cli mcp                                                # MCP server on stdio (local desk)
//! remotecraft-cli mcp --server URL --token USER_TOKEN                # MCP for one technician of a server
//! remotecraft-cli customers [QUERY]                                  # find customers
//! remotecraft-cli customer CUSTOMER                                  # a customer's card
//! remotecraft-cli who RUSTDESK_ID                                    # whose PC is this?
//! remotecraft-cli start (--device D | --customer C) [--kind K] [--summary S]
//! remotecraft-cli stop [ID] [--notes N] [--minutes M]
//! remotecraft-cli running
//! remotecraft-cli report [--from DATE] [--to DATE] [--by customer|technician|month|device|kind]
//! remotecraft-cli review                                             # what the GDPR record needs
//! remotecraft-cli rustdesk [HOST]                                    # client settings and server check
//! remotecraft-cli export CUSTOMER [--out FILE]                       # GDPR export (art. 15/20)
//! remotecraft-cli run COMMAND [JSON]                                 # any engine command
//! remotecraft-cli commands                                           # every command and its parameters
//! ```
//! `--home DIR` (or `REMOTECRAFT_HOME`) selects the data directory, `--as NAME` (or
//! `REMOTECRAFT_USER`) the technician, `--demo` the built-in demo desk (in memory).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod serve;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use remotecraft_engine::{COMMANDS, Desk, Paths};
use remotecraft_mcp::{Runner, Server};
use serde_json::{Value, json};

const USAGE: &str = "Usage:
  remotecraft-cli serve [--bind 127.0.0.1:7979] [--token SECRET] [--static DIR]
  remotecraft-cli mcp [--as NAME]
  remotecraft-cli mcp --server URL --token USER_TOKEN
  remotecraft-cli customers [QUERY]
  remotecraft-cli customer CUSTOMER
  remotecraft-cli who RUSTDESK_ID
  remotecraft-cli start (--device D | --customer C) [--kind remote|onsite|phone|lab|other] [--summary S]
  remotecraft-cli stop [ID] [--notes N] [--minutes M]
  remotecraft-cli running
  remotecraft-cli report [--from DATE] [--to DATE] [--by customer|technician|month|device|kind]
  remotecraft-cli review
  remotecraft-cli rustdesk [HOST]
  remotecraft-cli export CUSTOMER [--out FILE]
  remotecraft-cli run COMMAND [JSON]
  remotecraft-cli commands
Options: --home DIR (data directory), --as NAME (technician), --demo (the built-in demo desk)
Environment: REMOTECRAFT_HOME, REMOTECRAFT_USER, REMOTECRAFT_TOKEN, REMOTECRAFT_BIND,\n  REMOTECRAFT_RUSTDESK_HOST, REMOTECRAFT_RUSTDESK_KEY_FILE, REMOTECRAFT_RUSTDESK_CHECK_HOST";

struct Opts {
    args: Vec<String>,
    demo: bool,
    home: Option<String>,
    actor: Option<String>,
}

fn parse(raw: &[String]) -> Opts {
    let mut o = Opts { args: Vec::new(), demo: false, home: None, actor: None };
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => o.demo = true,
            "--home" => o.home = it.next().cloned(),
            "--as" => o.actor = it.next().cloned(),
            _ => o.args.push(a.clone()),
        }
    }
    o
}

/// `--key value` among the arguments.
fn flag(args: &[String], key: &str) -> Option<String> {
    let i = args.iter().position(|a| a == key)?;
    args.get(i + 1).filter(|v| !v.starts_with("--")).cloned()
}

/// The arguments that are not options or option values.
fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn desk(o: &Opts) -> Result<Desk, String> {
    let mut d = if o.demo {
        Desk::demo()
    } else {
        let paths = match &o.home {
            Some(h) => Paths::under(std::path::Path::new(h)),
            None => Paths::default_dirs(),
        };
        Desk::open(paths).map_err(|e| e.to_string())?
    };
    if let Some(a) = &o.actor {
        d.set_actor(a);
    }
    // The RustDesk server next to the desk (the same container): its public key
    // (id_ed25519.pub, written on its first start), its public name, where to check it.
    if let Some(f) = env("REMOTECRAFT_RUSTDESK_KEY_FILE")
        && let Ok(key) = std::fs::read_to_string(&f)
    {
        d.set_default_rustdesk_key(key.trim());
    }
    if let Some(h) = env("REMOTECRAFT_RUSTDESK_HOST").or_else(|| env("RUSTDESK_HOST")) {
        d.set_default_rustdesk_host(&h);
    }
    d.set_rustdesk_probe(&env("REMOTECRAFT_RUSTDESK_CHECK_HOST").unwrap_or_default(), Box::new(probe));
    Ok(d)
}

/// Opens a TCP connection (3 s at most per address) and answers how long it took.
fn probe(host: &str, port: u16) -> Result<u64, String> {
    use std::net::ToSocketAddrs;
    let start = std::time::Instant::now();
    let addrs: Vec<_> = (host, port).to_socket_addrs().map_err(|e| format!("cannot resolve {host}: {e}"))?.take(4).collect();
    let mut last = format!("{host} has no address");
    for a in addrs {
        match std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
            Ok(_) => return Ok(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
            Err(e) => last = format!("{a}: {e}"),
        }
    }
    Err(last)
}

/// Runs engine commands on a RemoteCraft server (`mcp --server`).
struct RemoteRunner {
    url: String,
    token: String,
    agent: ureq::Agent,
}

impl RemoteRunner {
    fn new(server: &str, token: &str) -> Self {
        let config = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(60))).build();
        Self { url: format!("{}/api/cmd", server.trim_end_matches('/')), token: token.to_string(), agent: config.into() }
    }
}

impl Runner for RemoteRunner {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        let body = json!({"command": command, "params": params}).to_string();
        let mut resp = self
            .agent
            .post(&self.url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| format!("server unreachable: {e}"))?;
        let text = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| format!("bad answer from the server: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|_| format!("bad answer from the server ({})", resp.status()))?;
        match (v.get("result"), v.get("error").and_then(Value::as_str)) {
            (_, Some(e)) => Err(e.to_string()),
            (Some(r), None) => Ok(r.clone()),
            (None, None) => Err(format!("bad answer from the server ({})", resp.status())),
        }
    }
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn customer_lines(v: &Value) {
    let list = v["customers"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("Nessun cliente.");
    }
    for c in list {
        println!(
            "{:>5}  {:<40} {:<16} {:<28} PC {}{}",
            c["id"],
            c["name"].as_str().unwrap_or(""),
            c["tax_id"].as_str().unwrap_or(""),
            c["email"].as_str().unwrap_or(""),
            c["devices"],
            if c["archived"] == true { "  (archiviato)" } else { "" }
        );
    }
}

fn intervention_lines(list: &[Value]) {
    if list.is_empty() {
        println!("Nessun intervento.");
    }
    for i in list {
        let minutes = if i["running"] == true { format!("{} min (in corso)", i["elapsed_minutes"]) } else { format!("{} min", i["minutes"]) };
        println!(
            "{:>5}  {}  {:<10} {:<28} {:<20} {:<16} {}",
            i["id"],
            i["started_local"].as_str().unwrap_or(""),
            i["technician"].as_str().unwrap_or(""),
            i["customer_name"].as_str().unwrap_or(""),
            i["device_name"].as_str().unwrap_or(""),
            minutes,
            i["summary"].as_str().unwrap_or("")
        );
    }
}

fn run(raw: Vec<String>) -> Result<(), String> {
    let o = parse(&raw);
    let cmd = o.args.first().cloned().unwrap_or_default();
    let rest: Vec<String> = o.args.iter().skip(1).cloned().collect();
    let pos = positional(&rest);
    let arg = |i: usize| pos.get(i).cloned();
    match cmd.as_str() {
        "serve" => {
            let d = desk(&o)?;
            let opts = serve::Options {
                bind: flag(&rest, "--bind").or_else(|| env("REMOTECRAFT_BIND")).unwrap_or_else(|| "127.0.0.1:7979".into()),
                token: flag(&rest, "--token").or_else(|| env("REMOTECRAFT_TOKEN")).unwrap_or_default(),
                tokens_dir: d.paths().map(|p| p.dir.clone()),
                static_dir: flag(&rest, "--static").or_else(|| env("REMOTECRAFT_STATIC")).map(PathBuf::from),
            };
            serve::run_server(d, opts)
        }
        "mcp" => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut server = match flag(&rest, "--server").or_else(|| env("REMOTECRAFT_SERVER")) {
                Some(url) => {
                    let token = flag(&rest, "--token").or_else(|| env("REMOTECRAFT_TOKEN")).ok_or("--server needs --token USER_TOKEN")?;
                    Server::new(RemoteRunner::new(&url, &token))
                }
                None => Server::new(desk(&o)?),
            };
            server.serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
        }
        "commands" => {
            for c in COMMANDS {
                println!("{:<22} {}\n{:<22} {}", c.id, c.label, "", c.params);
            }
            Ok(())
        }
        "run" => {
            let command = arg(0).ok_or("run COMMAND [JSON]")?;
            let params: Value = match arg(1) {
                Some(j) => serde_json::from_str(&j).map_err(|e| format!("invalid JSON: {e}"))?,
                None => json!({}),
            };
            print(&desk(&o)?.execute(&command, &params).map_err(|e| e.to_string())?);
            Ok(())
        }
        "customers" => {
            let v = desk(&o)?.execute("customer.list", &json!({"query": pos.join(" ")})).map_err(|e| e.to_string())?;
            customer_lines(&v);
            Ok(())
        }
        "customer" => {
            print(&desk(&o)?.execute("customer.get", &json!({"customer": pos.join(" ")})).map_err(|e| e.to_string())?);
            Ok(())
        }
        "who" => {
            let v = desk(&o)?.execute("device.find", &json!({"rustdesk_id": pos.join(" ")})).map_err(|e| e.to_string())?;
            let list = v["devices"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("Nessun PC con questo ID.");
            }
            for d in list {
                println!(
                    "{} — {} (cliente {} {}) — {} {}",
                    d["rustdesk_display"].as_str().unwrap_or(""),
                    d["name"].as_str().unwrap_or(""),
                    d["customer"]["id"],
                    d["customer"]["name"].as_str().unwrap_or(""),
                    d["os"].as_str().unwrap_or(""),
                    if d["retired"] == true { "(dismesso)" } else { "" }
                );
            }
            Ok(())
        }
        "start" => {
            let mut p = json!({});
            for (k, f) in [
                ("device", "--device"),
                ("customer", "--customer"),
                ("kind", "--kind"),
                ("summary", "--summary"),
                ("consent_override", "--consent-override"),
            ] {
                if let Some(v) = flag(&rest, f) {
                    p[k] = json!(v);
                }
            }
            let v = desk(&o)?.execute("intervention.start", &p).map_err(|e| e.to_string())?;
            println!("Intervento {} avviato alle {}.", v["intervention"]["id"], v["intervention"]["started_local"].as_str().unwrap_or(""));
            if let Some(uri) = v["connect"]["uri"].as_str() {
                println!("RustDesk: {uri}");
            }
            for w in v["warnings"].as_array().cloned().unwrap_or_default() {
                println!("Attenzione: {}", w.as_str().unwrap_or(""));
            }
            Ok(())
        }
        "stop" => {
            let mut p = json!({});
            if let Some(id) = arg(0) {
                p["id"] = json!(id);
            }
            if let Some(n) = flag(&rest, "--notes") {
                p["notes"] = json!(n);
            }
            if let Some(m) = flag(&rest, "--minutes") {
                p["minutes"] = json!(m);
            }
            let v = desk(&o)?.execute("intervention.stop", &p).map_err(|e| e.to_string())?;
            println!("Intervento {} chiuso: {} minuti.", v["intervention"]["id"], v["intervention"]["minutes"]);
            Ok(())
        }
        "running" => {
            let v = desk(&o)?.execute("intervention.running", &json!({})).map_err(|e| e.to_string())?;
            intervention_lines(v["running"].as_array().map(Vec::as_slice).unwrap_or_default());
            Ok(())
        }
        "report" => {
            let mut p = json!({});
            for (k, f) in [("from", "--from"), ("to", "--to"), ("group_by", "--by"), ("customer", "--customer"), ("technician", "--technician")] {
                if let Some(v) = flag(&rest, f) {
                    p[k] = json!(v);
                }
            }
            let v = desk(&o)?.execute("report.time", &p).map_err(|e| e.to_string())?;
            for r in v["rows"].as_array().cloned().unwrap_or_default() {
                println!(
                    "{:<40} {:>4} interventi  {:>7} ore  {:>7} fatturabili",
                    r["label"].as_str().unwrap_or(""),
                    r["interventions"],
                    r["hours"].as_str().unwrap_or(""),
                    r["billable_hours"].as_str().unwrap_or("")
                );
            }
            println!(
                "{:<40} {:>4}             {:>7} ore  {:>7} fatturabili",
                "Totale",
                "",
                v["totals"]["hours"].as_str().unwrap_or(""),
                v["totals"]["billable_hours"].as_str().unwrap_or("")
            );
            Ok(())
        }
        "rustdesk" => {
            let mut d = desk(&o)?;
            let setup = d.execute("rustdesk.setup", &json!({})).map_err(|e| e.to_string())?;
            let check = d.execute("rustdesk.check", &json!({"host": arg(0)})).map_err(|e| e.to_string())?;
            println!("Server ID e relay: {}", setup["id_server"].as_str().filter(|s| !s.is_empty()).unwrap_or("(non impostato)"));
            println!("Chiave:            {}", setup["key"].as_str().filter(|s| !s.is_empty()).unwrap_or("(non trovata)"));
            if let Some(c) = setup["config_string"].as_str() {
                println!("Stringa da importare in RustDesk (Impostazioni › Rete › Importa configurazione server):\n  {c}");
            }
            if let Some(n) = setup["windows_installer_name"].as_str() {
                println!("Installer Windows già configurato: rinomina il file in\n  {n}");
            }
            println!("Verifica di {}:", check["host"].as_str().unwrap_or(""));
            for p in check["ports"].as_array().cloned().unwrap_or_default() {
                let state =
                    if p["ok"] == true { format!("ok ({} ms)", p["ms"]) } else { format!("NON risponde: {}", p["error"].as_str().unwrap_or("")) };
                println!("  {:>5}  {:<60} {state}", p["port"], p["service"].as_str().unwrap_or(""));
            }
            if check["ports"].as_array().is_some_and(|l| l.iter().all(|p| p["ok"] == true)) {
                Ok(())
            } else {
                Err("the RustDesk server does not answer on every port".into())
            }
        }
        "review" => {
            let v = desk(&o)?.execute("gdpr.review", &json!({})).map_err(|e| e.to_string())?;
            let list = v["findings"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("Niente da segnalare.");
            }
            for f in list {
                println!("{:<32} {:<36} {}", f["issue"].as_str().unwrap_or(""), f["name"].as_str().unwrap_or(""), f["text"].as_str().unwrap_or(""));
            }
            Ok(())
        }
        "export" => {
            let v =
                desk(&o)?.execute("gdpr.export", &json!({"customer": arg(0).ok_or("export CUSTOMER [--out FILE]")?})).map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
            match flag(&rest, "--out") {
                Some(f) => {
                    remotecraft_engine::core::storage::write_private(std::path::Path::new(&f), text.as_bytes()).map_err(|e| e.to_string())?;
                    println!("Esportato in {f} (file leggibile solo da te).");
                }
                None => println!("{text}"),
            }
            Ok(())
        }
        "" | "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match run(raw) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_positionals() {
        let raw: Vec<String> =
            ["start", "--device", "123 456 789", "--demo", "--as", "anna", "--summary", "x"].iter().map(|s| s.to_string()).collect();
        let o = parse(&raw);
        assert!(o.demo);
        assert_eq!(o.actor.as_deref(), Some("anna"));
        assert_eq!(flag(&o.args, "--device").as_deref(), Some("123 456 789"));
        assert!(positional(&o.args[1..]).is_empty());
        assert_eq!(positional(&["a".into(), "--x".into(), "1".into(), "b".into()]), vec!["a", "b"]);
    }

    #[test]
    fn remote_desk_mirrors_the_server() {
        use remotecraft_engine::{Completed, DeskSession};
        use remotecraft_remote::RemoteDesk;
        let opts = serve::Options { bind: "127.0.0.1:0".into(), token: "svc".into(), tokens_dir: None, static_dir: None };
        let listener = serve::bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap();
        let mut desk = Desk::demo();
        desk.set_actor("server");
        std::thread::spawn(move || serve::serve(listener, desk, opts));
        // Pumps until `until` holds (or 4 s pass), keeping every completion.
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
        let mut r = RemoteDesk::new(&format!("http://{addr}"), "svc");
        let mut done = Vec::new();
        assert!(pump_until(&mut r, &mut done, &|r, _| r.problem().is_none() && !r.data().customers.is_empty()), "{:?}", r.problem());
        assert_eq!(r.data().customers.len(), 3);
        let job = r.execute("customer.add", &json!({"name": "Dal browser"}));
        assert!(pump_until(&mut r, &mut done, &|r, d| d.iter().any(|c| c.job == job) && r.data().customers.len() == 4));
        let c = done.iter().find(|c| c.job == job).unwrap();
        assert_eq!(c.command, "customer.add");
        assert_eq!(c.result.as_ref().unwrap()["customer"]["name"], "Dal browser");
        let bad = r.execute("customer.get", &json!({"customer": 999}));
        assert!(pump_until(&mut r, &mut done, &|_, d| d.iter().any(|c| c.job == bad)));
        assert!(done.iter().find(|c| c.job == bad).unwrap().result.as_ref().unwrap_err().to_string().contains("no customer"));
        let mut wrong = RemoteDesk::new(&format!("http://{addr}"), "nope");
        let mut none = Vec::new();
        assert!(pump_until(&mut wrong, &mut none, &|r, _| r.problem().is_some_and(|p| p.contains("token"))), "{:?}", wrong.problem());
        assert!(wrong.problem().is_some_and(|p| p.starts_with(remotecraft_remote::UNAUTHORIZED)), "{:?}", wrong.problem());
        // The service token names nobody: the mirror works as the desk's technician, the one
        // commands run as, so the web app finds its own timers.
        assert_eq!(r.actor(), "server");
        let started = r.execute("intervention.start", &json!({"customer": 2, "kind": "phone"}));
        assert!(pump_until(&mut r, &mut done, &|r, d| d.iter().any(|c| c.job == started) && r.data().interventions.iter().any(|i| i.running())));
        let mine: Vec<_> = r.data().interventions.iter().filter(|i| i.running() && i.technician == r.actor()).collect();
        assert_eq!(mine.len(), 1);
    }

    #[test]
    fn mcp_over_http_reaches_the_server() {
        let opts = serve::Options { bind: "127.0.0.1:0".into(), token: "svc".into(), tokens_dir: None, static_dir: None };
        let listener = serve::bind(&opts).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || serve::serve(listener, Desk::demo(), opts));
        let mut r = RemoteRunner::new(&format!("http://{addr}"), "svc");
        let v = r.run("device.find", json!({"rustdesk_id": "200300400"})).unwrap();
        assert_eq!(v["devices"][0]["customer"]["name"], "Ferramenta Corvo");
        assert!(r.run("customer.get", json!({"customer": 999})).unwrap_err().contains("no customer"));
        let mut bad = RemoteRunner::new(&format!("http://{addr}"), "wrong");
        assert!(bad.run("app.status", json!({})).unwrap_err().contains("token"));
        let mut s = Server::new(RemoteRunner::new(&format!("http://{addr}"), "svc"));
        let out = s
            .handle(
                &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "find_customers", "arguments": {"query": "lunaria"}}}),
            )
            .unwrap();
        assert_eq!(out["result"]["structuredContent"]["total"], 1);
        assert!(probe("127.0.0.1", addr.port()).is_ok());
        assert!(probe("127.0.0.1", 1).is_err());
        assert!(probe("no such host.invalid", 1).is_err());
        let mut gone = RemoteRunner::new("http://127.0.0.1:1", "x");
        assert!(gone.run("app.status", json!({})).is_err());
    }
}
