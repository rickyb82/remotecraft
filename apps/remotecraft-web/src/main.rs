//! RemoteCraft in the browser. Build: `cd apps/remotecraft-web && trunk build --release` (trunk
//! generates the JS loader; no handwritten JS).
//!
//! The page mirrors a `remotecraft-cli serve` server: `server=` in the link, else the page's own
//! origin when the server serves the page, else the server saved in the browser. The
//! technician's token comes from the link, preferably in the fragment (`#token=…`, which never
//! reaches the server or a proxy's logs; `?token=…` still works), and leaves the address bar at
//! once. The browser keeps it paired with its server: it never goes to another one, nor over
//! plain `http://` to another machine. Parameters (`token`, `server`, `lang`) are read from the
//! fragment first, then the query. Only `rustdesk://connection/new/<ID>` links are opened (by
//! the RustDesk client installed on the technician's PC); exports are downloads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// What the page decides from its address and its storage, free of the browser so it is tested
/// natively.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod rules {
    use remotecraft_ui_egui::core::rustdesk;

    /// The server when nothing else names one: a `remotecraft-cli serve` on this computer.
    pub const DEFAULT_SERVER: &str = "http://127.0.0.1:7979";

    fn clean(server: &str) -> String {
        server.trim().trim_end_matches('/').to_string()
    }

    /// The server to mirror and the token to send it. `link_*` come from the address, `saved` is
    /// the (server, token) pair the browser kept, `own` the page's own origin when the server
    /// serves the page. A saved token goes only to the server it was saved with, so a link that
    /// names another server starts without one.
    pub fn connection(link_server: Option<&str>, link_token: Option<&str>, saved: Option<(&str, &str)>, own: Option<&str>) -> (String, String) {
        let server = link_server
            .map(clean)
            .filter(|s| !s.is_empty())
            .or_else(|| own.map(clean).filter(|s| !s.is_empty()))
            .or_else(|| saved.map(|(s, _)| clean(s)).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| DEFAULT_SERVER.to_string());
        let token = match link_token.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => t.to_string(),
            None => saved.filter(|(s, _)| clean(s) == server).map(|(_, t)| t.trim().to_string()).unwrap_or_default(),
        };
        (server, token)
    }

    /// The (server, token) pair to keep: the one in use when it has a token that may travel
    /// there, else the one kept before (a tokenless visit, or a link to another server, does not
    /// replace it).
    pub fn pair_to_keep(server: &str, token: &str, sendable: bool, saved: Option<(&str, &str)>) -> Option<(String, String)> {
        if !token.is_empty() && sendable {
            Some((clean(server), token.to_string()))
        } else {
            saved.filter(|(s, t)| !clean(s).is_empty() && !t.trim().is_empty()).map(|(s, t)| (clean(s), t.trim().to_string()))
        }
    }

    /// Whether `url` is a link the page opens: exactly `rustdesk://connection/new/<ID>` with an
    /// ID RustDesk accepts as it is (what the interface builds). Anything else, `http(s)`
    /// included, is not opened.
    pub fn is_rustdesk_link(url: &str) -> bool {
        url.strip_prefix("rustdesk://connection/new/").is_some_and(|id| rustdesk::normalize(id).is_ok_and(|n| !n.is_empty() && n == id))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const REAL: &str = "https://assistenza.example.it";
        const EVIL: &str = "https://evil.example";

        #[test]
        fn a_saved_token_goes_only_to_its_own_server() {
            let saved = Some((REAL, "tok"));
            // Nothing in the link: the saved pair.
            assert_eq!(connection(None, None, saved, None), (REAL.into(), "tok".into()));
            // A link naming another server: that server, without the saved token.
            assert_eq!(connection(Some(EVIL), None, saved, None), (EVIL.into(), String::new()));
            assert_eq!(connection(Some(EVIL), None, saved, Some(REAL)), (EVIL.into(), String::new()));
            // The same server (a trailing slash aside): its token.
            assert_eq!(connection(Some("https://assistenza.example.it/"), None, saved, None), (REAL.into(), "tok".into()));
            // A token in the link goes with the link.
            assert_eq!(connection(Some(EVIL), Some(" t2 "), saved, None), (EVIL.into(), "t2".into()));
            assert_eq!(connection(None, Some("t2"), saved, Some(REAL)), (REAL.into(), "t2".into()));
        }

        #[test]
        fn the_page_s_own_server_comes_before_a_saved_one() {
            // Served by the real server, with another server saved: the page's own origin, and
            // the token saved for the other server is not sent to it.
            assert_eq!(connection(None, None, Some((EVIL, "evil-tok")), Some(REAL)), (REAL.into(), String::new()));
            assert_eq!(connection(None, None, Some((REAL, "tok")), Some(REAL)), (REAL.into(), "tok".into()));
            // A static host (no own server): the saved one, else this computer.
            assert_eq!(connection(None, None, None, None), (DEFAULT_SERVER.into(), String::new()));
            assert_eq!(connection(Some("  "), Some(""), None, None), (DEFAULT_SERVER.into(), String::new()));
        }

        #[test]
        fn a_tokenless_visit_keeps_the_saved_pair() {
            let saved = Some((REAL, "tok"));
            // A link to another server without a token: the pair kept stays, so the next plain
            // visit goes back to the real server.
            assert_eq!(pair_to_keep(EVIL, "", true, saved), Some((REAL.into(), "tok".into())));
            assert_eq!(connection(None, None, saved, None), (REAL.into(), "tok".into()));
            // A token that may not travel there (plain http elsewhere) is not kept either.
            assert_eq!(pair_to_keep("http://192.168.1.10:7979", "t", false, saved), Some((REAL.into(), "tok".into())));
            // A new token for a server replaces the pair.
            assert_eq!(pair_to_keep("https://other.example/", "t2", true, saved), Some(("https://other.example".into(), "t2".into())));
            assert_eq!(pair_to_keep(EVIL, "", true, None), None);
            assert_eq!(pair_to_keep(EVIL, "", true, Some(("", "tok"))), None);
        }

        #[test]
        fn only_rustdesk_connection_links_are_opened() {
            assert!(is_rustdesk_link("rustdesk://connection/new/123456789"));
            assert!(is_rustdesk_link("rustdesk://connection/new/ufficio-pc_2"));
            for bad in [
                "rustdesk://connection/new/123456&calc.exe&rem",
                "rustdesk://connection/new/123456\"&calc",
                "rustdesk://connection/new/123 456 789",
                "rustdesk://connection/new/123-456-789",
                "rustdesk://connection/new/",
                "rustdesk://connection/new/12345",
                "rustdesk://connection/new/123456789/../x",
                "rustdesk://connection/new/123456789?x=1",
                "RUSTDESK://connection/new/123456789",
                "https://example.com",
                "http://example.com",
                "javascript:alert(1)",
                "",
            ] {
                assert!(!is_rustdesk_link(bad), "{bad}");
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::wasm_bindgen::closure::Closure;
    use eframe::wasm_bindgen::{JsCast, JsValue};
    use remotecraft_remote::RemoteDesk;
    use remotecraft_ui_egui::{DeskApp, Language, Request};
    use serde_json::{Value, json};

    use crate::rules;

    const STORAGE_KEY: &str = "remotecraft";
    /// How long a download's data stays reachable after the click (Safari reads it after the
    /// click returns).
    const REVOKE_AFTER_MS: i32 = 30_000;

    pub struct App {
        app: DeskApp,
        /// The (server, token) pair the browser keeps.
        keep: Option<(String, String)>,
    }

    /// The parameters of the address's fragment (`#token=…&lang=it`) and query (`?server=…`).
    struct Link {
        fragment: Option<web_sys::UrlSearchParams>,
        query: Option<web_sys::UrlSearchParams>,
    }

    impl Link {
        fn read() -> Self {
            let location = web_sys::window().map(|w| w.location());
            let params = |text: Option<String>, mark: char| {
                let text = text?;
                web_sys::UrlSearchParams::new_with_str(text.strip_prefix(mark).unwrap_or(&text)).ok()
            };
            Self {
                fragment: params(location.as_ref().and_then(|l| l.hash().ok()), '#'),
                query: params(location.as_ref().and_then(|l| l.search().ok()), '?'),
            }
        }

        /// The parameter from the fragment, else from the query.
        fn get(&self, name: &str) -> Option<String> {
            self.fragment.as_ref().and_then(|p| p.get(name)).or_else(|| self.query.as_ref().and_then(|p| p.get(name)))
        }

        /// Takes the token out of the address bar (history, bookmarks, screenshots), keeping
        /// every other parameter. Read it first: afterwards `get("token")` finds nothing.
        fn forget_token(&self) {
            let had = |p: &Option<web_sys::UrlSearchParams>| p.as_ref().is_some_and(|p| p.has("token"));
            if !had(&self.fragment) && !had(&self.query) {
                return;
            }
            let Some(window) = web_sys::window() else { return };
            let Ok(path) = window.location().pathname() else { return };
            let part = |p: &Option<web_sys::UrlSearchParams>, mark: char| {
                let Some(p) = p else { return String::new() };
                p.delete("token");
                let rest = String::from(p.to_string());
                if rest.is_empty() { rest } else { format!("{mark}{rest}") }
            };
            let clean = format!("{path}{}{}", part(&self.query, '?'), part(&self.fragment, '#'));
            if let Ok(history) = window.history() {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&clean));
            }
        }
    }

    /// The page's origin when it is served by remotecraft-cli itself (not a static host).
    fn own_server() -> Option<String> {
        let origin = web_sys::window()?.location().origin().ok()?;
        let host = web_sys::window()?.location().hostname().ok()?;
        (!host.ends_with("github.io") && !origin.starts_with("file:")).then_some(origin)
    }

    /// Hands `bytes` to the browser as a file download.
    fn download(name: &str, bytes: &[u8]) {
        let Some(window) = web_sys::window() else { return };
        let Some(document) = window.document() else { return };
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::new();
        parts.push(&array.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let Ok(blob) = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &options) else { return };
        let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
        if let Some(a) = document.create_element("a").ok().and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok()) {
            a.set_href(&url);
            a.set_download(name);
            // In the document while clicked (some browsers ignore a detached anchor).
            let attached = document.body().is_some_and(|b| b.append_child(&a).is_ok());
            a.click();
            if attached {
                a.remove();
            }
        }
        // The browser may still be reading the data after the click returns (Safari fails the
        // download if it is gone): release it later.
        let revoke = Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), REVOKE_AFTER_MS);
    }

    impl App {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let saved: Option<Value> = cc.storage.and_then(|s| s.get_string(STORAGE_KEY)).and_then(|t| serde_json::from_str(&t).ok());
            let saved_str = |key: &str| saved.as_ref().and_then(|v| v.get(key)?.as_str().map(str::to_string));
            let (saved_server, saved_token) = (saved_str("server"), saved_str("token"));
            let saved_pair = saved_server.as_deref().zip(saved_token.as_deref());
            let link = Link::read();
            let (link_server, link_token, link_lang) = (link.get("server"), link.get("token"), link.get("lang"));
            link.forget_token();
            let own = own_server();
            let (server, token) = rules::connection(link_server.as_deref(), link_token.as_deref(), saved_pair, own.as_deref());
            let sendable = remotecraft_remote::sends_token_to(&server);
            if !token.is_empty() && !sendable {
                eframe::web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "RemoteCraft: the token is not sent to {server}: plain http:// to another machine would show it to the network (use https://)"
                )));
            }
            let keep = rules::pair_to_keep(&server, &token, sendable, saved_pair);
            let mut app = DeskApp::new(Box::new(RemoteDesk::new(&server, &token)));
            let browser_language = web_sys::window().and_then(|w| w.navigator().language()).and_then(|l| Language::parse(&l));
            app.language = saved_str("language").and_then(|l| Language::parse(&l)).or(browser_language).unwrap_or_default();
            if let Some(l) = link_lang.and_then(|l| Language::parse(&l)) {
                app.language = l;
            }
            app.dark = saved.as_ref().and_then(|v| v.get("dark")?.as_bool()).unwrap_or_else(|| cc.egui_ctx.system_theme() == Some(egui::Theme::Dark));
            Self { app, keep }
        }

        fn serve_requests(&mut self, frame: &mut eframe::Frame) {
            for request in std::mem::take(&mut self.app.requests) {
                match request {
                    Request::OpenUrl(url) => {
                        let Some(w) = web_sys::window() else { continue };
                        if rules::is_rustdesk_link(&url) {
                            // A custom scheme: navigating to it hands the link to the client
                            // without leaving the page.
                            let _ = w.location().assign(&url);
                        }
                    }
                    Request::SaveFile { name, data } => download(&name, &data),
                    Request::ForgetToken => {
                        // Stored at once (eframe saves only now and then), then a fresh page:
                        // the address no longer holds the token, so it starts without one.
                        self.keep = None;
                        if let Some(storage) = frame.storage_mut() {
                            eframe::App::save(self, storage);
                            storage.flush();
                        }
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().reload();
                        }
                    }
                }
            }
        }
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.app.ui(ui);
            self.serve_requests(frame);
            // The remote session polls the server; keep frames coming while it does.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            let (language, dark) = (self.app.language.code(), self.app.dark);
            let state = match &self.keep {
                Some((server, token)) => json!({"language": language, "dark": dark, "server": server, "token": token}),
                None => json!({"language": language, "dark": dark}),
            };
            storage.set_string(STORAGE_KEY, state.to_string());
        }
    }

    pub fn start() {
        let options = eframe::WebOptions::default();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("remotecraft"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                return;
            };
            let started = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(App::new(cc))))).await;
            if let Err(e) = started {
                eframe::web_sys::console::error_1(&e);
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("remotecraft-web runs in the browser: cd apps/remotecraft-web && trunk serve --release (and remotecraft-cli serve)");
}
