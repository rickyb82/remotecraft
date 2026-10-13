//! RemoteCraft in the browser. Build: `cd apps/remotecraft-web && trunk build --release` (trunk
//! generates the JS loader; no handwritten JS).
//!
//! The page mirrors a `remotecraft-cli serve` server: the page's own origin when the server
//! serves the page, else `?server=http://host:7979`. The technician's token comes from
//! `?token=…` (minted by the framework) and is kept in the browser's storage; `?lang=it`.
//! `rustdesk://` links open the RustDesk client installed on the technician's PC; exports are
//! downloads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::wasm_bindgen::JsCast;
    use remotecraft_remote::RemoteDesk;
    use remotecraft_ui_egui::{DeskApp, Language, Request};
    use serde_json::{Value, json};

    const STORAGE_KEY: &str = "remotecraft";
    const DEFAULT_SERVER: &str = "http://127.0.0.1:7979";

    pub struct App {
        app: DeskApp,
        token: String,
    }

    fn query_param(name: &str) -> Option<String> {
        let search = web_sys::window()?.location().search().ok()?;
        web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
    }

    /// The page's origin when it is served by remotecraft-cli itself (not a static host).
    fn own_server() -> Option<String> {
        let origin = web_sys::window()?.location().origin().ok()?;
        let host = web_sys::window()?.location().hostname().ok()?;
        (!host.ends_with("github.io") && !origin.starts_with("file:")).then_some(origin)
    }

    /// Hands `bytes` to the browser as a file download.
    fn download(name: &str, bytes: &[u8]) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
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
            a.click();
        }
        let _ = web_sys::Url::revoke_object_url(&url);
    }

    impl App {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let saved: Option<Value> = cc.storage.and_then(|s| s.get_string(STORAGE_KEY)).and_then(|t| serde_json::from_str(&t).ok());
            let server = query_param("server")
                .or_else(|| saved.as_ref().and_then(|v| v.get("server")?.as_str().map(str::to_string)))
                .or_else(own_server)
                .unwrap_or_else(|| DEFAULT_SERVER.to_string());
            let token =
                query_param("token").or_else(|| saved.as_ref().and_then(|v| v.get("token")?.as_str().map(str::to_string))).unwrap_or_default();
            let mut app = DeskApp::new(Box::new(RemoteDesk::new(&server, &token)));
            let browser_language = web_sys::window().and_then(|w| w.navigator().language()).and_then(|l| Language::parse(&l));
            app.language =
                saved.as_ref().and_then(|v| v.get("language")?.as_str().and_then(Language::parse)).or(browser_language).unwrap_or_default();
            if let Some(l) = query_param("lang").and_then(|l| Language::parse(&l)) {
                app.language = l;
            }
            app.dark = saved.as_ref().and_then(|v| v.get("dark")?.as_bool()).unwrap_or_else(|| cc.egui_ctx.system_theme() == Some(egui::Theme::Dark));
            Self { app, token }
        }

        fn serve_requests(&mut self) {
            for request in std::mem::take(&mut self.app.requests) {
                match request {
                    Request::OpenUrl(url) => {
                        let Some(w) = web_sys::window() else { continue };
                        if url.starts_with("rustdesk://") {
                            // A custom scheme: navigating to it hands the link to the client
                            // without leaving the page.
                            let _ = w.location().assign(&url);
                        } else if url.starts_with("http://") || url.starts_with("https://") {
                            let _ = w.open_with_url_and_target(&url, "_blank");
                        }
                    }
                    Request::SaveFile { name, data } => download(&name, &data),
                }
            }
        }
    }

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.app.ui(ui);
            self.serve_requests();
            // The remote session polls the server; keep frames coming while it does.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }

        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            let state =
                json!({"language": self.app.language.code(), "dark": self.app.dark, "server": self.app.session.server(), "token": self.token});
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
