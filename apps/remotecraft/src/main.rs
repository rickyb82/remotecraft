//! RemoteCraft desktop app.
//!
//! Usage: `remotecraft [--demo] [--home DIR] [--lang en|it] [--as NAME]`. The desk lives in the
//! platform data directory (or `REMOTECRAFT_HOME`); `--demo` shows the built-in demo desk.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use remotecraft_ui_egui::engine::{Desk, Paths, Session};
use remotecraft_ui_egui::{DeskApp, Language, Request};

struct App(DeskApp);

impl App {
    /// Links and files requested by the UI.
    fn serve_requests(&mut self) {
        for request in std::mem::take(&mut self.0.requests) {
            match request {
                Request::OpenUrl(url) => {
                    if is_rustdesk_link(&url) {
                        open_with_system(&url);
                    }
                }
                Request::SaveFile { name, data } => {
                    if let Some(path) = rfd::FileDialog::new().set_file_name(&name).save_file() {
                        match remotecraft_ui_egui::core::storage::write_private(&path, &data) {
                            Ok(()) => self.0.status = format!("{} {}", self.0.tr("Saved"), path.display()),
                            Err(e) => self.0.status = format!("{}: {e}", path.display()),
                        }
                    }
                }
                // A local desk keeps no token.
                Request::ForgetToken => {}
            }
        }
    }
}

/// Opens a TCP connection (3 s at most per address) and answers how long it took, for
/// `rustdesk.check` (the session runs it off the interface's thread, one port per thread).
fn probe(host: &str, port: u16) -> Result<u64, String> {
    use std::net::ToSocketAddrs;
    let start = std::time::Instant::now();
    let addrs: Vec<_> = (host, port).to_socket_addrs().map_err(|e| format!("cannot resolve {host}: {e}"))?.take(4).collect();
    let mut last = format!("{host} has no address");
    for a in addrs {
        match std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)) {
            Ok(_) => return Ok(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)),
            Err(e) => last = format!("{a}: {e}"),
        }
    }
    Err(last)
}

/// Whether `url` is a link the app opens: exactly `rustdesk://connection/new/<ID>` with an ID
/// RustDesk accepts as it is (what the interface builds). Anything else (`http(s)`, an ID read
/// from a tampered desk file with `&`, quotes or spaces) is not handed to the system.
fn is_rustdesk_link(url: &str) -> bool {
    url.strip_prefix("rustdesk://connection/new/")
        .is_some_and(|id| remotecraft_ui_egui::core::rustdesk::normalize(id).is_ok_and(|n| !n.is_empty() && n == id))
}

/// Opens a `rustdesk://` link with the system's handler (the RustDesk client), with the link as
/// one argument and no shell in between (on Windows `cmd /C start` would parse it again).
fn open_with_system(url: &str) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    if let Ok(mut child) = std::process::Command::new(program).arg(url).spawn() {
        // Reaped off the interface's thread, so no zombie is left behind (`Builder` does not
        // panic when the system refuses a thread).
        let _ = std::thread::Builder::new().name("open".into()).spawn(move || child.wait());
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let title = self.0.window_title();
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));
        self.0.ui(ui);
        self.serve_requests();
        // An idle window still pumps every 2 s, so what the MCP server or the CLI writes to the
        // desk file shows without a click.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
    }
}

const USAGE: &str = "Usage: remotecraft [--demo] [--home DIR] [--lang en|it] [--as NAME]";

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut demo = false;
    let mut home: Option<String> = None;
    let mut lang: Option<Language> = None;
    let mut actor: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => demo = true,
            "--home" => home = it.next().cloned(),
            "--lang" => lang = it.next().and_then(|l| Language::parse(l)),
            "--as" => actor = it.next().cloned(),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(USAGE.to_string()),
        }
    }
    let mut desk = if demo {
        Desk::demo()
    } else {
        let paths = match home {
            Some(h) => Paths::under(std::path::Path::new(&h)),
            None => Paths::default_dirs(),
        };
        Desk::open(paths).map_err(|e| e.to_string())?
    };
    if let Some(a) = &actor {
        desk.set_actor(a);
    }
    desk.set_rustdesk_probe("", Box::new(probe));
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.trim().is_empty()).unwrap_or_default();
    if let Some(call) =
        remotecraft_remote::intranet::caller(&env("REMOTECRAFT_INTRANET_URL"), &env("REMOTECRAFT_INTRANET_TOKEN"), &env("REMOTECRAFT_INTRANET_ROLE"))
    {
        desk.set_intranet(call);
    }
    let mut app = DeskApp::new(Box::new(Session::new(desk)));
    if let Some(l) = lang {
        app.language = l;
    }
    let theme = app.session.data().preferences.theme.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1380.0, 880.0]).with_min_inner_size([900.0, 600.0]).with_title("RemoteCraft"),
        ..Default::default()
    };
    eframe::run_native(
        "RemoteCraft",
        options,
        Box::new(move |cc| {
            app.dark = match theme.as_str() {
                "dark" => true,
                "light" => false,
                _ => cc.egui_ctx.system_theme() == Some(egui::Theme::Dark),
            };
            Ok(Box::new(App(app)))
        }),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rustdesk_connection_links_reach_the_system() {
        assert!(is_rustdesk_link("rustdesk://connection/new/123456789"));
        assert!(is_rustdesk_link("rustdesk://connection/new/ufficio-pc_2"));
        // An ID from a tampered desk file, shell syntax, anything but the exact link.
        for bad in [
            "rustdesk://connection/new/123456&calc.exe&rem",
            "rustdesk://connection/new/123456\"&calc.exe",
            "rustdesk://connection/new/123456|calc",
            "rustdesk://connection/new/123456^&calc",
            "rustdesk://connection/new/123456%26calc",
            "rustdesk://connection/new/123 456 789",
            "rustdesk://connection/new/123-456-789",
            "rustdesk://connection/new/",
            "rustdesk://connection/new/12345",
            "rustdesk://connection/new/12345678901234567",
            "rustdesk://connection/new/123456789,/select",
            "rustdesk://connection/new/123456789 /x",
            "RUSTDESK://connection/new/123456789",
            "https://example.com",
            "http://example.com",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!is_rustdesk_link(bad), "{bad}");
        }
    }
}
