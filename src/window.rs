use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{
    webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent},
    App, AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Position, Size, Url,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::memory;

pub const MAIN_WINDOW_LABEL: &str = "main";

const MAIN_WINDOW_TITLE: &str = "ztik";
const TIKTOK_HOME_URL: &str = "https://www.tiktok.com/";
const MAIN_WINDOW_WIDTH: f64 = 1100.0;
const MAIN_WINDOW_HEIGHT: f64 = 800.0;
pub const MINI_PLAYER_WIDTH: f64 = 420.0;
pub const MINI_PLAYER_HEIGHT: f64 = 740.0;
const POPUP_DEFAULT_WIDTH: f64 = 520.0;
const POPUP_DEFAULT_HEIGHT: f64 = 680.0;
const SUSPEND_DELAY_MS: u64 = 180;
const ALLOWED_TIKTOK_DOMAINS: &[&str] = &[
    "tiktok.com",
    "tiktokv.com",
    "bytedance.com",
    "tiktokcdn.com",
];
const ALLOWED_OAUTH_HOSTS: &[&str] = &[
    "accounts.google.com",
    "www.facebook.com",
    "appleid.apple.com",
];

const INJECT_CSS: &str = include_str!("../assets/inject.css");
const INJECT_JS: &str = include_str!("../assets/inject.js");

const PAUSE_ALL_MEDIA_JS: &str = r#"(function () {
  try {
    const media = document.querySelectorAll('video, audio');
    for (let i = 0; i < media.length; i += 1) {
      try {
        media[i].pause();
      } catch (_) {}
    }
  } catch (_) {}
})();"#;

const ENTER_MINI_PLAYER_JS: &str = r#"(function () {
  window.__ZTIK_MINI = true;
  if (document.documentElement) {
    document.documentElement.classList.add('ztik-mini');
  }
  if (typeof window.__ztikRefreshMiniPlayer === 'function') {
    window.__ztikRefreshMiniPlayer();
  }
  window.setTimeout(function () {
    window.dispatchEvent(new Event('resize'));
  }, 100);
})();"#;

const EXIT_MINI_PLAYER_JS: &str = r#"(function () {
  window.__ZTIK_MINI = false;
  if (document.documentElement) {
    document.documentElement.classList.remove('ztik-mini');
  }
  if (typeof window.__ztikRefreshMiniPlayer === 'function') {
    window.__ztikRefreshMiniPlayer();
  }
  window.setTimeout(function () {
    window.dispatchEvent(new Event('resize'));
  }, 100);
})();"#;

static OAUTH_POPUP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedBounds {
    pub size: PhysicalSize<u32>,
    pub position: PhysicalPosition<i32>,
    pub was_maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RestoreBoundsPlan {
    pub size: Size,
    pub position: Option<Position>,
    pub maximize: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HideToTrayStep {
    PauseAllMedia,
    SetWebViewInvisible,
    WaitBeforeSuspendMs(u64),
    SuspendWebView,
    HideWindow,
}

pub(crate) const HIDE_TO_TRAY_SEQUENCE: [HideToTrayStep; 5] = [
    HideToTrayStep::PauseAllMedia,
    HideToTrayStep::SetWebViewInvisible,
    HideToTrayStep::WaitBeforeSuspendMs(SUSPEND_DELAY_MS),
    HideToTrayStep::SuspendWebView,
    HideToTrayStep::HideWindow,
];

pub(crate) fn split_hide_to_tray_sequence(
    steps: &[HideToTrayStep],
) -> (&[HideToTrayStep], u64, &[HideToTrayStep]) {
    for (idx, step) in steps.iter().enumerate() {
        if let HideToTrayStep::WaitBeforeSuspendMs(delay_ms) = *step {
            return (&steps[..idx], delay_ms, &steps[idx + 1..]);
        }
    }
    (steps, 0, &[])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowFromTrayStep {
    ResumeWebView,
    UnminimizeWindow,
    ShowWindow,
    FocusWindow,
}

pub(crate) const SHOW_FROM_TRAY_SEQUENCE: [ShowFromTrayStep; 4] = [
    ShowFromTrayStep::ResumeWebView,
    ShowFromTrayStep::UnminimizeWindow,
    ShowFromTrayStep::ShowWindow,
    ShowFromTrayStep::FocusWindow,
];

pub(crate) fn mini_player_target_size() -> Size {
    Size::Logical(LogicalSize::new(MINI_PLAYER_WIDTH, MINI_PLAYER_HEIGHT))
}

pub(crate) fn compute_restore_bounds(saved: SavedBounds) -> RestoreBoundsPlan {
    let size = if saved.size.width > 0 && saved.size.height > 0 {
        Size::Physical(saved.size)
    } else {
        Size::Logical(LogicalSize::new(MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT))
    };

    let position =
        is_valid_window_position(saved.position).then_some(Position::Physical(saved.position));

    RestoreBoundsPlan {
        size,
        position,
        maximize: saved.was_maximized,
    }
}

#[derive(Debug, Default)]
struct WindowInnerState {
    mini_player_saved: Option<SavedBounds>,
    hidden_to_tray: bool,
    hide_in_progress: bool,
    visibility_generation: u64,
}

#[derive(Debug, Default)]
pub struct WindowState {
    inner: Mutex<WindowInnerState>,
}

impl WindowState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_hide(&self) -> Option<u64> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.hide_in_progress || guard.hidden_to_tray {
            return None;
        }
        guard.hide_in_progress = true;
        guard.visibility_generation = guard.visibility_generation.wrapping_add(1);
        Some(guard.visibility_generation)
    }

    pub fn complete_hide(&self, generation: u64) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.hide_in_progress || guard.visibility_generation != generation {
            return false;
        }
        guard.hide_in_progress = false;
        guard.hidden_to_tray = true;
        true
    }

    pub fn mark_shown(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.hide_in_progress = false;
        guard.hidden_to_tray = false;
        guard.visibility_generation = guard.visibility_generation.wrapping_add(1);
    }

    pub fn is_hidden_to_tray(&self) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.hidden_to_tray || guard.hide_in_progress
    }

    pub fn enter_mini_player(&self, bounds: SavedBounds) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.mini_player_saved = Some(bounds);
    }

    pub fn exit_mini_player(&self) -> Option<SavedBounds> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.mini_player_saved.take()
    }

    pub fn is_mini_player_active(&self) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.mini_player_saved.is_some()
    }
}

pub fn is_hidden_to_tray(app: &AppHandle) -> bool {
    app.state::<WindowState>().is_hidden_to_tray()
}

pub fn hide_to_tray(app: &AppHandle) {
    let state = app.state::<WindowState>();
    let Some(generation) = state.begin_hide() else {
        return;
    };

    let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        state.mark_shown();
        return;
    };

    let (before_wait, wait_ms, after_wait) = split_hide_to_tray_sequence(&HIDE_TO_TRAY_SEQUENCE);
    for step in before_wait {
        match step {
            HideToTrayStep::PauseAllMedia => {
                eprintln!("[ztik][window] pause all <video>/<audio> via eval()");
                if let Err(err) = main_window.eval(PAUSE_ALL_MEDIA_JS) {
                    eprintln!("[ztik][window] pause eval failed: {err}");
                }
            }
            HideToTrayStep::SetWebViewInvisible => {
                memory::set_webview_visible(&main_window, false);
            }
            HideToTrayStep::WaitBeforeSuspendMs(_)
            | HideToTrayStep::SuspendWebView
            | HideToTrayStep::HideWindow => {}
        }
    }

    eprintln!("[ztik][window] waiting {wait_ms}ms before suspend");
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(wait_ms));
        let app_for_ui = app_handle.clone();
        if let Err(err) = app_handle.run_on_main_thread(move || {
            let state = app_for_ui.state::<WindowState>();
            if !state.complete_hide(generation) {
                eprintln!("[ztik][window] hide-to-tray cancelled before suspend");
                return;
            }
            if let Some(win) = app_for_ui.get_webview_window(MAIN_WINDOW_LABEL) {
                for step in after_wait {
                    match step {
                        HideToTrayStep::SuspendWebView => {
                            memory::suspend(&win);
                        }
                        HideToTrayStep::HideWindow => {
                            if let Err(err) = win.hide() {
                                eprintln!("[ztik][window] failed to hide main window: {err}");
                            } else {
                                eprintln!("[ztik][window] main window hidden to tray");
                            }
                        }
                        HideToTrayStep::PauseAllMedia
                        | HideToTrayStep::SetWebViewInvisible
                        | HideToTrayStep::WaitBeforeSuspendMs(_) => {}
                    }
                }
            }
        }) {
            eprintln!("[ztik][window] failed to dispatch suspend to UI thread: {err}");
        }
    });
}

pub fn show_main_window(app: &AppHandle) {
    let app_handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        show_main_window_on_ui_thread(&app_handle);
    }) {
        eprintln!("[ztik][window] failed to dispatch show to UI thread: {err}");
    }
}

pub fn prepare_exit(app: &AppHandle) {
    app.state::<WindowState>().mark_shown();
    for (label, win) in app.webview_windows() {
        if let Err(err) = win.destroy() {
            eprintln!("[ztik][window] failed to destroy window '{label}' during exit: {err}");
        }
    }
}

fn show_main_window_on_ui_thread(app: &AppHandle) {
    app.state::<WindowState>().mark_shown();

    let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    for step in SHOW_FROM_TRAY_SEQUENCE {
        match step {
            ShowFromTrayStep::ResumeWebView => {
                memory::resume(&win);
            }
            ShowFromTrayStep::UnminimizeWindow => {
                let _ = win.unminimize();
            }
            ShowFromTrayStep::ShowWindow => {
                if let Err(err) = win.show() {
                    eprintln!("[ztik][window] failed to show main window: {err}");
                }
            }
            ShowFromTrayStep::FocusWindow => {
                if let Err(err) = win.set_focus() {
                    eprintln!("[ztik][window] failed to focus main window: {err}");
                }
            }
        }
    }
    eprintln!("[ztik][window] main window shown");
}

fn is_valid_window_position(pos: PhysicalPosition<i32>) -> bool {
    pos.x > -10_000 && pos.y > -10_000
}

fn set_mini_player_dom_mode(window: &WebviewWindow, enabled: bool) {
    let (mode, script) = if enabled {
        ("enabled", ENTER_MINI_PLAYER_JS)
    } else {
        ("disabled", EXIT_MINI_PLAYER_JS)
    };

    if let Err(err) = window.eval(script) {
        eprintln!("[ztik][window] failed to set mini-player DOM mode {mode}: {err}");
    }
}

pub fn exit_mini_player_on_maximize(app: &AppHandle) -> bool {
    if app.state::<WindowState>().exit_mini_player().is_none() {
        return false;
    }

    if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(err) = win.set_always_on_top(false) {
            eprintln!("[ztik][window] failed to disable always_on_top after maximize: {err}");
        }
        set_mini_player_dom_mode(&win, false);
    }

    eprintln!("[ztik][window] maximized window exited mini-player mode");
    true
}

pub fn toggle_mini_player(app: &AppHandle) -> bool {
    let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return false;
    };

    let state = app.state::<WindowState>();
    if let Some(saved) = state.exit_mini_player() {
        if state.is_hidden_to_tray()
            || win.is_minimized().unwrap_or(false)
            || !win.is_visible().unwrap_or(true)
        {
            show_main_window_on_ui_thread(app);
        }

        set_mini_player_dom_mode(&win, false);

        if let Err(err) = win.set_always_on_top(false) {
            eprintln!("[ztik][window] failed to disable always_on_top: {err}");
        }

        let restore = compute_restore_bounds(saved);
        if let Err(err) = win.set_size(restore.size) {
            eprintln!("[ztik][window] failed to restore window size: {err}");
        }

        if let Some(position) = restore.position {
            if let Err(err) = win.set_position(position) {
                eprintln!("[ztik][window] failed to restore window position: {err}");
            }
        }

        if restore.maximize {
            let _ = win.maximize();
        }
        let _ = win.set_focus();

        eprintln!(
            "[ztik][window] exited mini-player, restored size={}x{} pos=({}, {}) maximized={}",
            saved.size.width,
            saved.size.height,
            saved.position.x,
            saved.position.y,
            saved.was_maximized
        );
        return false;
    }

    if state.is_hidden_to_tray()
        || win.is_minimized().unwrap_or(false)
        || !win.is_visible().unwrap_or(true)
    {
        show_main_window_on_ui_thread(app);
    }

    let was_maximized = win.is_maximized().unwrap_or(false);
    if was_maximized {
        let _ = win.unmaximize();
    }

    let size = win.inner_size().unwrap_or(PhysicalSize::new(
        MAIN_WINDOW_WIDTH as u32,
        MAIN_WINDOW_HEIGHT as u32,
    ));
    let position = win
        .outer_position()
        .unwrap_or(PhysicalPosition::new(100, 100));

    state.enter_mini_player(SavedBounds {
        size,
        position,
        was_maximized,
    });

    set_mini_player_dom_mode(&win, true);

    if let Err(err) = win.set_size(mini_player_target_size()) {
        eprintln!("[ztik][window] failed to set mini-player size: {err}");
    }
    if let Err(err) = win.set_always_on_top(true) {
        eprintln!("[ztik][window] failed to enable always_on_top: {err}");
    }
    let _ = win.set_focus();

    eprintln!(
        "[ztik][window] entered mini-player ({}x{}, always_on_top=true), saved size={}x{} pos=({}, {})",
        MINI_PLAYER_WIDTH as u32,
        MINI_PLAYER_HEIGHT as u32,
        size.width,
        size.height,
        position.x,
        position.y
    );
    true
}

pub fn create_main_window(app: &App) -> tauri::Result<WebviewWindow> {
    let data_dir = webview_data_dir(app)?;
    let tiktok_url = Url::parse(TIKTOK_HOME_URL).expect("valid TikTok home URL");

    let app_handle = app.handle().clone();
    let popup_data_dir = data_dir.clone();

    WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::External(tiktok_url))
        .title(MAIN_WINDOW_TITLE)
        .inner_size(MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT)
        .data_directory(data_dir)
        .initialization_script(build_initialization_script())
        .on_page_load(|window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }

            if window.state::<WindowState>().is_mini_player_active() {
                set_mini_player_dom_mode(&window, true);
            }
        })
        .on_navigation(is_main_navigation_allowed)
        .on_new_window(move |url, features| {
            handle_new_window(&app_handle, &popup_data_dir, &url, features)
        })
        .build()
}

fn encode_js_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '<' => out.push_str("\\u003c"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) fn build_initialization_script() -> String {
    format!(
        "window.__ztikCss = {};\n{}",
        encode_js_string(INJECT_CSS),
        INJECT_JS
    )
}

fn webview_data_dir(app: &App) -> tauri::Result<PathBuf> {
    let dir = app.path().app_data_dir()?.join("webview2");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub(crate) fn is_upload_blocked(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host_matches_domain(&host, "studio.tiktok.com") {
        return true;
    }

    let path = url.path().to_ascii_lowercase();
    let trimmed_path = path.trim_start_matches('/');
    if trimmed_path.starts_with("upload")
        || trimmed_path.starts_with("tiktokstudio")
        || trimmed_path.starts_with("creator-center/upload")
    {
        return true;
    }

    path.split('/').any(|segment| segment == "tiktokstudio")
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn is_allowed_https_navigation(url: &Url) -> bool {
    if url.scheme() != "https" || url.port_or_known_default() != Some(443) {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    let is_tiktok_or_bytedance_host = ALLOWED_TIKTOK_DOMAINS
        .iter()
        .any(|domain| host_matches_domain(&host, domain));
    let is_oauth_host = ALLOWED_OAUTH_HOSTS.contains(&host.as_str());

    (is_tiktok_or_bytedance_host || is_oauth_host) && !is_upload_blocked(url)
}

pub(crate) fn is_main_navigation_allowed(url: &Url) -> bool {
    is_allowed_https_navigation(url)
}

pub(crate) fn is_popup_navigation_allowed(url: &Url) -> bool {
    url.as_str() == "about:blank" || is_allowed_https_navigation(url)
}

pub(crate) fn is_oauth_popup_allowed(url: &Url) -> bool {
    is_allowed_https_navigation(url)
}

fn handle_new_window(
    app: &AppHandle,
    data_dir: &Path,
    url: &Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    if !is_oauth_popup_allowed(url) {
        return NewWindowResponse::Deny;
    }

    let label = format!(
        "oauth-popup-{}",
        OAUTH_POPUP_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let blank_url = Url::parse("about:blank").expect("valid about:blank URL");

    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(blank_url))
        .title(MAIN_WINDOW_TITLE)
        .inner_size(POPUP_DEFAULT_WIDTH, POPUP_DEFAULT_HEIGHT)
        .window_features(features)
        .data_directory(data_dir.to_path_buf())
        .on_document_title_changed(|window, title| {
            if !title.is_empty() {
                let _ = window.set_title(&title);
            }
        })
        .on_navigation(is_popup_navigation_allowed)
        .on_new_window(|_, _| NewWindowResponse::Deny);

    match builder.build() {
        Ok(window) => {
            attach_popup_auto_close(&window);
            NewWindowResponse::Create { window }
        }
        Err(_) => NewWindowResponse::Deny,
    }
}

#[cfg(target_os = "windows")]
fn attach_popup_auto_close(window: &WebviewWindow) {
    let popup = window.clone();
    let _ = window.with_webview(move |platform_webview| unsafe {
        if let Ok(core_webview) = platform_webview.controller().CoreWebView2() {
            let mut token: i64 = 0;
            let handler =
                webview2_com::WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                    let _ = popup.close();
                    Ok(())
                }));
            let _ = core_webview.add_WindowCloseRequested(&handler, &mut token);
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn attach_popup_auto_close(_window: &WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::{
        build_initialization_script, compute_restore_bounds, is_main_navigation_allowed,
        is_oauth_popup_allowed, is_popup_navigation_allowed, is_upload_blocked,
        is_valid_window_position, mini_player_target_size, split_hide_to_tray_sequence,
        HideToTrayStep, SavedBounds, ShowFromTrayStep, WindowState, ENTER_MINI_PLAYER_JS,
        EXIT_MINI_PLAYER_JS, HIDE_TO_TRAY_SEQUENCE, INJECT_CSS, INJECT_JS, MAIN_WINDOW_HEIGHT,
        MAIN_WINDOW_WIDTH, MINI_PLAYER_HEIGHT, MINI_PLAYER_WIDTH, PAUSE_ALL_MEDIA_JS,
        SHOW_FROM_TRAY_SEQUENCE,
    };
    use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, Position, Size, Url};

    fn parse(raw: &str) -> Url {
        Url::parse(raw).expect("valid test URL")
    }

    #[test]
    fn blocks_upload_and_studio_urls() {
        assert!(is_upload_blocked(&parse("https://www.tiktok.com/upload")));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/upload?lang=vi"
        )));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/UPLOAD/video"
        )));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/tiktokstudio"
        )));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/tiktokstudio/upload"
        )));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/en/tiktokstudio/content"
        )));
        assert!(is_upload_blocked(&parse("https://studio.tiktok.com/")));
        assert!(is_upload_blocked(&parse(
            "https://studio.tiktok.com/upload"
        )));
        assert!(is_upload_blocked(&parse(
            "https://www.tiktok.com/creator-center/upload"
        )));
    }

    #[test]
    fn allows_regular_tiktok_urls() {
        assert!(!is_upload_blocked(&parse("https://www.tiktok.com/")));
        assert!(!is_upload_blocked(&parse("https://www.tiktok.com/foryou")));
        assert!(!is_upload_blocked(&parse(
            "https://www.tiktok.com/@tiktokstudio"
        )));
        assert!(!is_upload_blocked(&parse("https://www.tiktok.com/@upload")));
        assert!(!is_upload_blocked(&parse(
            "https://www.tiktok.com/messages"
        )));
        assert!(!is_upload_blocked(&parse("https://www.tiktok.com/login")));
        assert!(!is_upload_blocked(&parse("about:blank")));
    }

    #[test]
    fn main_navigation_requires_https_and_a_trusted_domain() {
        assert!(is_main_navigation_allowed(&parse(
            "https://www.tiktok.com/foryou"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "https://sub.tiktokv.com/"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "https://www.bytedance.com/"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "https://v16.tiktokcdn.com/"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "https://accounts.google.com/o/oauth2/v2/auth"
        )));

        assert!(!is_main_navigation_allowed(&parse(
            "https://tiktok.com.evil.com/"
        )));
        assert!(!is_main_navigation_allowed(&parse("http://www.tiktok.com")));
        assert!(!is_main_navigation_allowed(&parse(
            "file:///C:/Windows/win.ini"
        )));
        assert!(!is_main_navigation_allowed(&parse("javascript:alert(1)")));
        assert!(!is_main_navigation_allowed(&parse("https://evil.example/")));
        assert!(!is_main_navigation_allowed(&parse(
            "https://www.tiktok.com:8443/"
        )));
    }

    #[test]
    fn popup_navigation_keeps_the_https_whitelist_after_opening() {
        assert!(is_popup_navigation_allowed(&parse("about:blank")));
        assert!(is_popup_navigation_allowed(&parse(
            "https://www.tiktok.com/passport/web/authorize/"
        )));
        assert!(is_popup_navigation_allowed(&parse(
            "https://accounts.google.com/o/oauth2/v2/auth"
        )));

        assert!(!is_popup_navigation_allowed(&parse(
            "https://tiktok.com.evil.com/"
        )));
        assert!(!is_popup_navigation_allowed(&parse(
            "http://www.tiktok.com"
        )));
        assert!(!is_popup_navigation_allowed(&parse(
            "file:///C:/Windows/win.ini"
        )));
        assert!(!is_popup_navigation_allowed(&parse("javascript:alert(1)")));
        assert!(!is_popup_navigation_allowed(&parse(
            "https://evil.example/"
        )));
    }

    #[test]
    fn allows_whitelisted_oauth_popups_only() {
        assert!(is_oauth_popup_allowed(&parse(
            "https://accounts.google.com/o/oauth2/v2/auth"
        )));
        assert!(is_oauth_popup_allowed(&parse(
            "https://www.facebook.com/v18.0/dialog/oauth"
        )));
        assert!(is_oauth_popup_allowed(&parse(
            "https://appleid.apple.com/auth/authorize"
        )));
        assert!(is_oauth_popup_allowed(&parse(
            "https://www.tiktok.com/passport/web/authorize/"
        )));
        assert!(is_oauth_popup_allowed(&parse("https://tiktok.com/login")));

        assert!(!is_oauth_popup_allowed(&parse("https://example.com/login")));
        assert!(!is_oauth_popup_allowed(&parse(
            "https://evil-tiktok.com/login"
        )));
        assert!(!is_oauth_popup_allowed(&parse(
            "http://accounts.google.com/o/oauth2/v2/auth"
        )));
        assert!(!is_oauth_popup_allowed(&parse(
            "https://www.tiktok.com/upload"
        )));
        assert!(!is_oauth_popup_allowed(&parse(
            "https://www.tiktok.com/tiktokstudio"
        )));
        assert!(!is_oauth_popup_allowed(&parse(
            "https://studio.tiktok.com/"
        )));
    }

    #[test]
    fn inject_assets_are_non_empty_and_contain_key_selectors() {
        assert!(!INJECT_CSS.trim().is_empty());
        assert!(!INJECT_JS.trim().is_empty());

        assert!(INJECT_CSS.contains("/upload"));
        assert!(INJECT_CSS.contains("/tiktokstudio"));
        assert!(INJECT_CSS.contains("upload-icon"));
        assert!(INJECT_CSS.contains("get-coins"));
        assert!(INJECT_CSS.contains("get-app"));
        assert!(INJECT_CSS.contains("promote"));
        assert!(!INJECT_CSS.contains(".css-"));
        assert!(INJECT_CSS.contains("html.ztik-mini"));
        assert!(INJECT_CSS.contains("[data-ztik-mini-hide=\"1\"]"));
        assert!(INJECT_CSS.contains("main"));
        assert!(INJECT_CSS.contains("margin-left: 0 !important"));
        assert!(INJECT_CSS.contains("max-width: 100% !important"));
        assert!(INJECT_CSS.contains("overflow-y: auto"));

        assert!(INJECT_JS.contains("__ztikInjected"));
        assert!(INJECT_JS.contains("ztik-style"));
        assert!(INJECT_JS.contains("MutationObserver"));
        assert!(INJECT_JS.contains("__ZTIK_MINI"));
        assert!(INJECT_JS.contains("data-ztik-mini-hide"));
        assert!(INJECT_JS.contains("nav-foryou"));
        assert!(INJECT_JS.contains("replaceState"));
        assert!(INJECT_JS.contains("visibilitychange"));
        assert!(INJECT_JS.contains("ArrowDown"));
        assert!(INJECT_JS.contains("repost"));

        let script = build_initialization_script();
        assert!(script.contains("window.__ztikCss = "));
        assert!(script.contains("/upload"));
        assert!(script.contains("/tiktokstudio"));
        assert!(script.contains("__ztikInjected"));
    }

    #[test]
    fn mini_player_scripts_toggle_class_and_request_viewport_resize() {
        assert!(ENTER_MINI_PLAYER_JS.contains("window.__ZTIK_MINI = true"));
        assert!(EXIT_MINI_PLAYER_JS.contains("window.__ZTIK_MINI = false"));
        assert!(ENTER_MINI_PLAYER_JS.contains("classList.add('ztik-mini')"));
        assert!(EXIT_MINI_PLAYER_JS.contains("classList.remove('ztik-mini')"));
        assert!(ENTER_MINI_PLAYER_JS.contains("window.dispatchEvent(new Event('resize'))"));
        assert!(EXIT_MINI_PLAYER_JS.contains("window.dispatchEvent(new Event('resize'))"));
        assert!(ENTER_MINI_PLAYER_JS.contains("}, 100)"));
        assert!(EXIT_MINI_PLAYER_JS.contains("}, 100)"));
    }

    #[test]
    fn window_state_mini_player_saves_and_restores_bounds() {
        assert_eq!(MINI_PLAYER_WIDTH, 420.0);
        assert_eq!(MINI_PLAYER_HEIGHT, 740.0);

        let state = WindowState::new();
        assert_eq!(state.exit_mini_player(), None);

        let original = SavedBounds {
            size: PhysicalSize::new(1100, 800),
            position: PhysicalPosition::new(120, 80),
            was_maximized: false,
        };
        state.enter_mini_player(original);
        assert_eq!(state.exit_mini_player(), Some(original));
        assert_eq!(state.exit_mini_player(), None);

        assert!(is_valid_window_position(PhysicalPosition::new(120, 80)));
        assert!(!is_valid_window_position(PhysicalPosition::new(
            -32000, -32000
        )));
    }

    #[test]
    fn window_state_hide_deduplicates_and_cancels_when_shown() {
        let state = WindowState::new();
        assert!(!state.is_hidden_to_tray());

        let gen1 = state.begin_hide().expect("first hide begins");
        assert!(state.is_hidden_to_tray());
        assert_eq!(state.begin_hide(), None);

        // User re-opens window during the 180ms delay -> pending hide must cancel.
        state.mark_shown();
        assert!(!state.is_hidden_to_tray());
        assert!(!state.complete_hide(gen1));

        // Normal hide completes cleanly.
        let gen2 = state.begin_hide().expect("second hide begins");
        assert!(state.complete_hide(gen2));
        assert!(state.is_hidden_to_tray());
        assert_eq!(state.begin_hide(), None);

        // Showing again resets hidden state.
        state.mark_shown();
        assert!(!state.is_hidden_to_tray());

        assert!(PAUSE_ALL_MEDIA_JS.contains("video, audio"));
        assert!(PAUSE_ALL_MEDIA_JS.contains(".pause()"));
    }

    #[test]
    fn hide_and_show_tray_sequences_follow_required_order() {
        let (before_wait, wait_ms, after_wait) =
            split_hide_to_tray_sequence(&HIDE_TO_TRAY_SEQUENCE);
        assert!((150..=200).contains(&wait_ms));
        assert_eq!(
            before_wait,
            &[
                HideToTrayStep::PauseAllMedia,
                HideToTrayStep::SetWebViewInvisible,
            ]
        );
        assert_eq!(
            after_wait,
            &[HideToTrayStep::SuspendWebView, HideToTrayStep::HideWindow]
        );
        assert_eq!(
            HIDE_TO_TRAY_SEQUENCE,
            [
                HideToTrayStep::PauseAllMedia,
                HideToTrayStep::SetWebViewInvisible,
                HideToTrayStep::WaitBeforeSuspendMs(wait_ms),
                HideToTrayStep::SuspendWebView,
                HideToTrayStep::HideWindow,
            ]
        );

        assert_eq!(
            SHOW_FROM_TRAY_SEQUENCE,
            [
                ShowFromTrayStep::ResumeWebView,
                ShowFromTrayStep::UnminimizeWindow,
                ShowFromTrayStep::ShowWindow,
                ShowFromTrayStep::FocusWindow,
            ]
        );
    }

    #[test]
    fn compute_restore_bounds_handles_normal_multimonitor_and_fallback_cases() {
        assert_eq!(
            mini_player_target_size(),
            Size::Logical(LogicalSize::new(420.0, 740.0))
        );

        // Normal saved physical size + position + maximized flag preserved.
        let normal = compute_restore_bounds(SavedBounds {
            size: PhysicalSize::new(1280, 720),
            position: PhysicalPosition::new(100, 60),
            was_maximized: true,
        });
        assert_eq!(normal.size, Size::Physical(PhysicalSize::new(1280, 720)));
        assert_eq!(
            normal.position,
            Some(Position::Physical(PhysicalPosition::new(100, 60)))
        );
        assert!(normal.maximize);

        // Multi-monitor negative coordinates are preserved.
        let left_monitor = compute_restore_bounds(SavedBounds {
            size: PhysicalSize::new(1100, 800),
            position: PhysicalPosition::new(-1920, 40),
            was_maximized: false,
        });
        assert_eq!(
            left_monitor.position,
            Some(Position::Physical(PhysicalPosition::new(-1920, 40)))
        );
        assert!(!left_monitor.maximize);

        // Zero size falls back to default logical size, and minimized sentinel position is skipped.
        let fallback = compute_restore_bounds(SavedBounds {
            size: PhysicalSize::new(0, 0),
            position: PhysicalPosition::new(-32000, -32000),
            was_maximized: false,
        });
        assert_eq!(
            fallback.size,
            Size::Logical(LogicalSize::new(MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT))
        );
        assert_eq!(fallback.position, None);
    }
}
