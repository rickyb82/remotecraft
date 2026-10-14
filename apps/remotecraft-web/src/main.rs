//! RemoteCraft in the browser. Build: `cd apps/remotecraft-web && trunk build --release` (trunk
//! generates the JS loader; no handwritten JS).
//!
//! The page mirrors a `remotecraft-cli serve` server: `server=` in the link, else the page's own
//! origin when that server served the page (it marks the page with
//! `<meta name="remotecraft-server" content="same-origin">`), else the server saved in the
//! browser, else this computer. On a static host (GitHub Pages, any domain, `trunk serve`) the
//! page is not marked and goes to the saved server. The technician's token comes from the
//! link, preferably in the fragment (`#token=…`, which never reaches the server or a proxy's
//! logs; `?token=…` still works), and leaves the address bar at once. The browser keeps it
//! paired with its server: it never goes to another one, nor over plain `http://` to another
//! machine. A server on this computer without a token is kept too, so `trunk serve` finds
//! `remotecraft-cli serve --demo` again. Forgetting the token holds for every tab: the stored
//! state carries a number bumped by each forget, and a tab that sees a newer one writes nothing
//! more and reloads. Parameters (`token`, `server`, `lang`) are read from the fragment first,
//! then the query. Only `rustdesk://connection/new/<ID>` links are opened (by the RustDesk
//! client installed on the technician's PC); exports are downloads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// What the page decides from its address and its storage, free of the browser so it is tested
/// natively.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod rules {
    use remotecraft_ui_egui::core::rustdesk;
    use serde_json::{Map, Value};

    /// The server when nothing else names one: a `remotecraft-cli serve` on this computer.
    pub const DEFAULT_SERVER: &str = "http://127.0.0.1:7979";

    fn clean(server: &str) -> String {
        server.trim().trim_end_matches('/').to_string()
    }

    /// The page's own origin as its server, only when the server marked the page as its own
    /// (`marked`): a static host serves the same page unmarked, and has no `/api` behind it.
    pub fn own_server(origin: &str, marked: bool) -> Option<String> {
        let origin = clean(origin);
        let lower = origin.to_ascii_lowercase();
        (marked && (lower.starts_with("https://") || lower.starts_with("http://"))).then_some(origin)
    }

    /// The server to mirror and the token to send it. `link_*` come from the address, `saved` is
    /// the (server, token) pair the browser kept (an empty token for a server on this computer
    /// without one), `own` the page's own origin when its server marked it ([`own_server`]). A
    /// saved token goes only to the server it was saved with, so a link that names another
    /// server starts without one.
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
    /// there; else a server on this computer without a token (`trunk serve` with
    /// `remotecraft-cli serve --demo`), unless a token is kept; else the one kept before (a
    /// tokenless visit, or a link to another server, does not replace a token).
    pub fn pair_to_keep(server: &str, token: &str, sendable: bool, saved: Option<(&str, &str)>) -> Option<(String, String)> {
        let saved = saved
            .map(|(s, t)| (clean(s), t.trim().to_string()))
            .filter(|(s, t)| !s.is_empty() && (!t.is_empty() || remotecraft_remote::is_this_computer(s)));
        let server = clean(server);
        if !token.is_empty() && sendable {
            Some((server, token.to_string()))
        } else if token.is_empty() && remotecraft_remote::is_this_computer(&server) && saved.as_ref().is_none_or(|(_, t)| t.is_empty()) {
            Some((server, String::new()))
        } else {
            saved
        }
    }

    /// What the browser keeps for the page: one JSON object in its storage, shared by every
    /// tab of the page.
    #[derive(Debug, Default, Clone, PartialEq)]
    pub struct Stored {
        pub server: Option<String>,
        pub token: Option<String>,
        pub language: Option<String>,
        pub dark: Option<bool>,
        /// Bumped by every "forget the token": a tab that started with an older one must not
        /// write its pair back.
        pub epoch: u64,
    }

    impl Stored {
        /// The stored text, read leniently (missing or odd fields are absent).
        pub fn read(text: Option<&str>) -> Self {
            let v: Option<Value> = text.and_then(|t| serde_json::from_str(t).ok());
            let field = |key: &str| v.as_ref().and_then(|v| v.get(key));
            let text = |key: &str| field(key).and_then(Value::as_str).map(str::to_string);
            Self {
                server: text("server"),
                token: text("token"),
                language: text("language"),
                dark: field("dark").and_then(Value::as_bool),
                epoch: field("epoch").and_then(Value::as_u64).unwrap_or(0),
            }
        }

        pub fn pair(&self) -> Option<(&str, &str)> {
            self.server.as_deref().zip(self.token.as_deref())
        }

        /// Whether a tab that started at `epoch` must stop: the token was forgotten since, in
        /// another tab.
        pub fn forgotten_since(&self, epoch: u64) -> bool {
            self.epoch > epoch
        }
    }

    /// What this tab keeps, to merge into the storage.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Keeping {
        /// The [`Stored::epoch`] this tab started with (or set by forgetting).
        pub epoch: u64,
        pub pair: Option<(String, String)>,
        /// `pair` is this tab's news (a link's token, a forget) not written yet; otherwise the
        /// stored pair stays as it is, whatever another tab put there.
        pub fresh: bool,
        pub language: String,
        pub dark: bool,
    }

    impl Keeping {
        /// The text to store over `stored` (the storage as it is now), or `None` when this tab
        /// must write nothing: the token was forgotten in another tab since this one started.
        pub fn merge(&self, stored: &Stored) -> Option<String> {
            if stored.forgotten_since(self.epoch) {
                return None;
            }
            let pair = if self.fresh { self.pair.as_ref().map(|(s, t)| (s.as_str(), t.as_str())) } else { stored.pair() };
            let mut state = Map::new();
            state.insert("language".into(), Value::from(self.language.as_str()));
            state.insert("dark".into(), Value::from(self.dark));
            state.insert("epoch".into(), Value::from(self.epoch.max(stored.epoch)));
            if let Some((server, token)) = pair {
                state.insert("server".into(), Value::from(server));
                state.insert("token".into(), Value::from(token));
            }
            Some(Value::Object(state).to_string())
        }

        /// Forgets the pair in every tab: a newer epoch than any stored, no pair.
        pub fn forget(&mut self, stored: &Stored) {
            self.epoch = self.epoch.max(stored.epoch).saturating_add(1);
            self.pair = None;
            self.fresh = true;
        }
    }

    /// Whether `url` is a link the page opens: exactly `rustdesk://connection/new/<ID>` with an
    /// ID RustDesk accepts as it is (what the interface builds). Anything else, `http(s)`
    /// included, is not opened.
    pub fn is_rustdesk_link(url: &str) -> bool {
        url.strip_prefix("rustdesk://connection/new/").is_some_and(|id| rustdesk::normalize(id).is_ok_and(|n| !n.is_empty() && n == id))
    }

    /// Whether a key press is the app's "reload this screen's data" (Ctrl+R, Cmd+R on a Mac),
    /// which the browser would otherwise take as "reload the page" and lose what was typed.
    /// Shift (a hard reload) and Alt keep the browser's meaning; so does F5.
    pub fn is_app_reload(key: &str, ctrl: bool, meta: bool, shift: bool, alt: bool) -> bool {
        (ctrl || meta) && !shift && !alt && key.eq_ignore_ascii_case("r")
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
        fn the_own_origin_is_the_server_only_when_the_server_marked_the_page() {
            assert_eq!(own_server("https://assistenza.example.it", true).as_deref(), Some(REAL));
            assert_eq!(own_server("http://127.0.0.1:7979/", true).as_deref(), Some("http://127.0.0.1:7979"));
            // A static host serves the page unmarked: GitHub Pages under any domain, Netlify, a
            // company nginx, `trunk serve`.
            for origin in ["https://www.siriustec.it", "https://user.github.io", "https://app.example.it", "http://127.0.0.1:8783"] {
                assert_eq!(own_server(origin, false), None, "{origin}");
            }
            // A page opened from a file has no origin to talk to.
            assert_eq!(own_server("null", true), None);
            assert_eq!(own_server("file://", true), None);
        }

        #[test]
        fn a_static_host_goes_back_to_the_saved_server_on_a_plain_revisit() {
            // https://app.example.it/?server=https://assistenza.example.it#token=T once…
            let first = connection(Some(REAL), Some("T"), None, own_server("https://app.example.it", false).as_deref());
            assert_eq!(first, (REAL.into(), "T".into()));
            let kept = pair_to_keep(&first.0, &first.1, true, None);
            assert_eq!(kept, Some((REAL.into(), "T".into())));
            // …then https://app.example.it/ (a bookmark, a new tab): the saved pair, not the
            // static host's own origin (which has no /api behind it).
            let saved = kept.as_ref().map(|(s, t)| (s.as_str(), t.as_str()));
            assert_eq!(connection(None, None, saved, own_server("https://app.example.it", false).as_deref()), (REAL.into(), "T".into()));
            // Served by the server itself (marked): its own origin first.
            assert_eq!(connection(None, None, Some((EVIL, "x")), own_server(REAL, true).as_deref()), (REAL.into(), String::new()));
        }

        #[test]
        fn trunk_serve_remembers_a_server_on_this_computer_without_a_token() {
            const LOCAL: &str = "http://127.0.0.1:7979";
            let trunk = own_server("http://127.0.0.1:8783", false);
            // http://127.0.0.1:8783/?server=http://127.0.0.1:7979 once, no token…
            let first = connection(Some(LOCAL), None, None, trunk.as_deref());
            assert_eq!(first, (LOCAL.into(), String::new()));
            let kept = pair_to_keep(&first.0, &first.1, true, None);
            assert_eq!(kept, Some((LOCAL.into(), String::new())));
            // …then http://127.0.0.1:8783/: the same server, not trunk.
            let saved = kept.as_ref().map(|(s, t)| (s.as_str(), t.as_str()));
            assert_eq!(connection(None, None, saved, trunk.as_deref()), (LOCAL.into(), String::new()));
            assert_eq!(pair_to_keep(LOCAL, "", true, saved), kept);
            // A token saved for the local server goes with it.
            assert_eq!(connection(None, None, Some((LOCAL, "tok")), trunk.as_deref()), (LOCAL.into(), "tok".into()));
            // Another local server without a token replaces a tokenless one…
            assert_eq!(pair_to_keep("http://localhost:8000/", "", true, saved), Some(("http://localhost:8000".into(), String::new())));
            // …but never a token kept for a server.
            assert_eq!(pair_to_keep(LOCAL, "", true, Some((REAL, "tok"))), Some((REAL.into(), "tok".into())));
            // A server elsewhere without a token is not kept, nor a stored one.
            assert_eq!(pair_to_keep(EVIL, "", true, None), None);
            assert_eq!(pair_to_keep("http://192.168.1.10:7979", "", true, None), None);
            assert_eq!(pair_to_keep(EVIL, "", true, Some((EVIL, ""))), None);
            assert_eq!(pair_to_keep(EVIL, "", true, Some((LOCAL, " "))), Some((LOCAL.into(), String::new())));
        }

        fn tab(epoch: u64, pair: Option<(&str, &str)>, fresh: bool) -> Keeping {
            Keeping { epoch, pair: pair.map(|(s, t)| (s.into(), t.into())), fresh, language: "it".into(), dark: false }
        }

        fn stored(text: &str) -> Stored {
            Stored::read(Some(text))
        }

        #[test]
        fn forgetting_the_token_holds_for_every_tab() {
            let before = stored(r#"{"language":"it","dark":true,"server":"https://assistenza.example.it","token":"tok","epoch":3}"#);
            assert_eq!(before.pair(), Some((REAL, "tok")));
            // Two tabs on the same pair; one received it from the link and has not written it.
            let mut a = tab(3, Some((REAL, "tok")), false);
            let b = tab(3, Some((REAL, "tok")), true);
            // A forgets: a newer epoch, no pair.
            a.forget(&before);
            let after = stored(&a.merge(&before).unwrap());
            assert_eq!((after.pair(), after.epoch), (None, 4));
            // B (every 30 s, on losing focus, on closing) writes nothing back, and sees it must
            // reload.
            assert_eq!(b.merge(&after), None);
            assert!(after.forgotten_since(b.epoch));
            assert!(!after.forgotten_since(a.epoch));
            // Reloaded, the tabs start at the new epoch without the token.
            let reloaded = tab(after.epoch, pair_to_keep(REAL, "", true, after.pair()).as_ref().map(|(s, t)| (s.as_str(), t.as_str())), false);
            assert_eq!(reloaded.pair, None);
            assert_eq!(stored(&reloaded.merge(&after).unwrap()).pair(), None);
            // A link with a token opened afterwards is kept, and a tab that only holds what it
            // read does not erase it.
            let c = tab(after.epoch, Some((REAL, "new")), true);
            let with_new = stored(&c.merge(&after).unwrap());
            assert_eq!(with_new.pair(), Some((REAL, "new")));
            assert_eq!(stored(&reloaded.merge(&with_new).unwrap()).pair(), Some((REAL, "new")));
            // Forgetting again goes past any epoch stored; a tab already stale forgets too.
            let mut late = tab(1, None, false);
            late.forget(&with_new);
            assert_eq!(late.epoch, 5);
            assert_eq!(stored(&late.merge(&with_new).unwrap()).pair(), None);
        }

        #[test]
        fn the_stored_state_merges_preferences_and_survives_odd_text() {
            // Preferences are this tab's; the epoch never goes back.
            let t = Keeping { epoch: 2, pair: None, fresh: false, language: "en".into(), dark: true };
            let s = stored(&t.merge(&stored(r#"{"language":"it","dark":false,"epoch":1}"#)).unwrap());
            assert_eq!((s.language.as_deref(), s.dark, s.epoch), (Some("en"), Some(true), 2));
            // A cleared storage does not get the pair back from a tab that only read it.
            let t = tab(0, Some((REAL, "tok")), false);
            assert_eq!(stored(&t.merge(&Stored::default()).unwrap()).pair(), None);
            // State written before epochs existed, or nonsense, reads as epoch 0.
            assert_eq!(stored(r#"{"server":"https://assistenza.example.it","token":"tok"}"#).epoch, 0);
            for odd in ["", "[]", "{", "null", r#"{"epoch":-1,"server":5,"dark":"yes"}"#, r#"{"epoch":1e400}"#] {
                let s = stored(odd);
                assert_eq!((s.epoch, s.pair(), s.dark), (0, None, None), "{odd}");
            }
            assert_eq!(Stored::read(None), Stored::default());
            // The largest epoch does not overflow.
            let mut t = tab(u64::MAX, None, false);
            t.forget(&stored(&format!(r#"{{"epoch":{}}}"#, u64::MAX)));
            assert_eq!(t.epoch, u64::MAX);
        }

        #[test]
        fn ctrl_r_is_the_app_s_reload_and_the_browser_keeps_the_rest() {
            assert!(is_app_reload("r", true, false, false, false));
            assert!(is_app_reload("R", false, true, false, false), "Cmd+R on a Mac");
            assert!(!is_app_reload("R", true, false, true, false), "Ctrl+Shift+R stays a hard reload");
            assert!(!is_app_reload("r", true, false, false, true));
            assert!(!is_app_reload("r", false, false, false, false), "a plain r is typing");
            assert!(!is_app_reload("F5", false, false, false, false));
            assert!(!is_app_reload("t", true, false, false, false));
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

    use crate::rules::{self, Keeping, Stored};

    const STORAGE_KEY: &str = "remotecraft";
    /// How long a download's data stays reachable after the click (Safari reads it after the
    /// click returns).
    const REVOKE_AFTER_MS: i32 = 30_000;
    /// How often (seconds) the tab looks whether the token was forgotten in another tab.
    const FORGET_CHECK: f64 = 2.0;

    pub struct App {
        app: DeskApp,
        /// What this tab keeps in the browser's storage (merged with what other tabs wrote).
        keeping: Keeping,
        /// When the storage was last looked at (`egui` time, seconds).
        last_check: f64,
        /// The token was forgotten (in this tab or another): this tab writes nothing more and
        /// reloads.
        stale: bool,
        /// The reload was asked for.
        reloading: bool,
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

    /// The page's origin when remotecraft-cli served it and marked it as its own (a static host,
    /// `trunk serve` included, does not).
    fn own_server() -> Option<String> {
        let window = web_sys::window()?;
        let origin = window.location().origin().ok()?;
        let marked =
            window.document().and_then(|d| d.query_selector(r#"meta[name="remotecraft-server"][content="same-origin"]"#).ok().flatten()).is_some();
        rules::own_server(&origin, marked)
    }

    fn reload() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
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
            let stored = Stored::read(cc.storage.and_then(|s| s.get_string(STORAGE_KEY)).as_deref());
            let saved_pair = stored.pair();
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
            // A pair this tab brings (a link's token, a server on this computer) is written at
            // once; otherwise the stored one is left to whatever other tabs do with it.
            let fresh = keep.as_ref().map(|(s, t)| (s.as_str(), t.as_str())) != saved_pair;
            let mut app = DeskApp::new(Box::new(RemoteDesk::new(&server, &token)));
            let browser_language = web_sys::window().and_then(|w| w.navigator().language()).and_then(|l| Language::parse(&l));
            app.language = stored.language.as_deref().and_then(Language::parse).or(browser_language).unwrap_or_default();
            if let Some(l) = link_lang.and_then(|l| Language::parse(&l)) {
                app.language = l;
            }
            app.dark = stored.dark.unwrap_or_else(|| cc.egui_ctx.system_theme() == Some(egui::Theme::Dark));
            let keeping = Keeping { epoch: stored.epoch, pair: keep, fresh, language: app.language.code().to_string(), dark: app.dark };
            Self { app, keeping, last_check: 0.0, stale: false, reloading: false }
        }

        /// Merges this tab's state into the storage, unless the token was forgotten in another
        /// tab since this one started: then nothing is written and the tab goes stale.
        fn store(&mut self, storage: &mut dyn eframe::Storage) {
            if self.stale {
                return;
            }
            self.keeping.language = self.app.language.code().to_string();
            self.keeping.dark = self.app.dark;
            match self.keeping.merge(&Stored::read(storage.get_string(STORAGE_KEY).as_deref())) {
                Some(state) => {
                    storage.set_string(STORAGE_KEY, state);
                    storage.flush();
                    self.keeping.fresh = false;
                }
                None => self.stale = true,
            }
        }

        /// Writes a pair this tab brought, every [`FORGET_CHECK`] seconds looks whether another
        /// tab forgot the token since this one started, and reloads a stale tab (it then starts
        /// without the token: the address no longer holds it).
        fn sync_storage(&mut self, now: f64, frame: &mut eframe::Frame) {
            if !self.stale
                && self.keeping.fresh
                && let Some(storage) = frame.storage_mut()
            {
                self.store(storage);
            }
            if !self.stale && now - self.last_check >= FORGET_CHECK {
                self.last_check = now;
                self.stale = frame.storage().is_some_and(|s| Stored::read(s.get_string(STORAGE_KEY).as_deref()).forgotten_since(self.keeping.epoch));
            }
            if self.stale && !self.reloading {
                self.reloading = true;
                reload();
            }
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
                        // Stored at once (eframe saves only now and then) with a newer epoch, so
                        // every other tab stops writing its pair and reloads too; then a fresh
                        // page: the address no longer holds the token, so it starts without one.
                        if let Some(storage) = frame.storage_mut() {
                            let stored = Stored::read(storage.get_string(STORAGE_KEY).as_deref());
                            self.keeping.forget(&stored);
                            self.store(storage);
                        }
                        self.stale = true;
                    }
                }
            }
        }
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.app.ui(ui);
            self.serve_requests(frame);
            self.sync_storage(ui.ctx().input(|i| i.time), frame);
            // The remote session polls the server; keep frames coming while it does.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            self.store(storage);
        }
    }

    /// Ctrl+R (Cmd+R) is the app's "reload this screen's data", but eframe lets the browser
    /// reload the page on it, losing the timer card's text and unsent forms. A listener in the
    /// capture phase, which runs before eframe's own (and before it stops the event), keeps the
    /// browser from it; the key still reaches the app.
    fn keep_ctrl_r_for_the_app() {
        let Some(window) = web_sys::window() else { return };
        let listener = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
            if rules::is_app_reload(&e.key(), e.ctrl_key(), e.meta_key(), e.shift_key(), e.alt_key()) {
                e.prevent_default();
            }
        });
        if window.add_event_listener_with_callback_and_bool("keydown", listener.as_ref().unchecked_ref(), true).is_ok() {
            // The page's lifetime: the listener must outlive this function.
            listener.forget();
        }
    }

    pub fn start() {
        keep_ctrl_r_for_the_app();
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
