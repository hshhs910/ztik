use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{
    webview::{DownloadEvent, NewWindowFeatures, NewWindowResponse, PageLoadEvent},
    window::Color,
    App, AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Position, Size, Url,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::memory;

pub const MAIN_WINDOW_LABEL: &str = "main";

const MAIN_WINDOW_TITLE: &str = "ztik";
const SPLASH_APP_PATH: &str = "index.html";
const WINDOW_BG_BLACK: Color = Color(0, 0, 0, 255);
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

#[cfg(target_os = "windows")]
pub(crate) const WEBVIEW2_BROWSER_ARGS: &str = concat!(
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,BackForwardCache,Translate,InterestFeedContentSuggestions",
    " --autoplay-policy=no-user-gesture-required",
    " --disable-background-networking",
    " --disable-component-update",
    " --disable-sync",
    " --media-cache-size=33554432",
    " --disk-cache-size=67108864",
);

const FREE_MEMORY_PREPARE_JS: &str = r#"(function () {
  try {
    if (typeof window.__ztikFreeMemory === 'function') {
      window.__ztikFreeMemory();
    } else {
      const media = document.querySelectorAll('video, audio');
      for (let i = 0; i < media.length; i += 1) {
        try {
          media[i].pause();
          media[i].preload = 'none';
        } catch (_) {}
      }
      if (window.performance && typeof window.performance.clearResourceTimings === 'function') {
        window.performance.clearResourceTimings();
      }
    }
  } catch (_) {}
})();"#;

const SHOW_RAM_CLEANED_TOAST_JS: &str = r#"(function () {
  try {
    if (typeof window.__ztikShowToast === 'function') {
      window.__ztikShowToast('Đã dọn RAM', 2600);
    } else {
      window.__ztikPendingToast = 'Đã dọn RAM';
    }
  } catch (_) {}
})();"#;

const SET_NATIVE_DECORATIONS_ON_JS: &str = r#"(function () {
  window.__ZTIK_NATIVE_DECORATIONS = true;
  if (document.documentElement) {
    document.documentElement.classList.add('ztik-native-decorations');
  }
  if (typeof window.__ztikSetNativeDecorations === 'function') {
    window.__ztikSetNativeDecorations(true);
  }
})();"#;

const SET_NATIVE_DECORATIONS_OFF_JS: &str = r#"(function () {
  window.__ZTIK_NATIVE_DECORATIONS = false;
  if (document.documentElement) {
    document.documentElement.classList.remove('ztik-native-decorations');
  }
  if (typeof window.__ztikSetNativeDecorations === 'function') {
    window.__ztikSetNativeDecorations(false);
  }
})();"#;

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

pub(crate) const DOWNLOAD_CURRENT_JS: &str = r#"(function () {
  if (typeof window.__ztikDownloadCurrent === 'function') {
    window.__ztikDownloadCurrent();
  }
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

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowCornerMode {
    Round,
    DoNotRound,
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn desired_window_corner_mode(is_maximized: bool) -> WindowCornerMode {
    if is_maximized {
        WindowCornerMode::DoNotRound
    } else {
        WindowCornerMode::Round
    }
}

#[derive(Debug, Default)]
struct WindowInnerState {
    mini_player_saved: Option<SavedBounds>,
    hidden_to_tray: bool,
    hide_in_progress: bool,
    visibility_generation: u64,
    last_maximized: Option<bool>,
    pending_ram_clean_toast: bool,
}

#[derive(Debug, Default)]
pub struct WindowState {
    inner: Mutex<WindowInnerState>,
}

impl WindowState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_maximized_state(&self, is_maximized: bool) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.last_maximized == Some(is_maximized) {
            return false;
        }
        guard.last_maximized = Some(is_maximized);
        true
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

    pub fn set_pending_ram_clean_toast(&self, pending: bool) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.pending_ram_clean_toast = pending;
    }

    pub fn take_pending_ram_clean_toast(&self) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut guard.pending_ram_clean_toast)
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
            memory::notify_hidden_to_tray(&app_for_ui);
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

pub fn trigger_current_download(app: &AppHandle) {
    let app_handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        show_main_window_on_ui_thread(&app_handle);
        if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
            if let Err(err) = window.eval(DOWNLOAD_CURRENT_JS) {
                eprintln!("[ztik][download] failed to trigger media download: {err}");
            }
        }
    }) {
        eprintln!("[ztik][download] failed to dispatch media download: {err}");
    }
}

pub fn trigger_free_memory(app: &AppHandle) {
    let app_handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
            return;
        };

        let state = app_handle.state::<WindowState>();
        if state.is_hidden_to_tray() {
            state.set_pending_ram_clean_toast(true);
            let app_after_clear = app_handle.clone();
            memory::clear_safe_temp_cache(&window, move || {
                memory::trim_working_set_now(&app_after_clear);
            });
            eprintln!("[ztik][memory] triggered free memory while hidden in tray");
            return;
        }

        if let Err(err) = window.eval(FREE_MEMORY_PREPARE_JS) {
            eprintln!("[ztik][memory] failed to clear JS caches before reload: {err}");
        }
        state.set_pending_ram_clean_toast(true);

        let app_after_clear = app_handle.clone();
        memory::clear_safe_temp_cache(&window, move || {
            let app_for_ui = app_after_clear.clone();
            if let Err(err) = app_after_clear.run_on_main_thread(move || {
                if let Some(win) = app_for_ui.get_webview_window(MAIN_WINDOW_LABEL) {
                    if let Err(err) = win.reload() {
                        eprintln!("[ztik][memory] webview reload failed, falling back to location.reload(): {err}");
                        let _ = win.eval("window.location.reload();");
                    }
                }
                memory::trim_working_set_now(&app_for_ui);
            }) {
                eprintln!("[ztik][memory] failed to dispatch reload after cache clear: {err}");
                memory::trim_working_set_now(&app_after_clear);
            }
        });
        eprintln!("[ztik][memory] triggered free memory (safe cache clear + reload + working set trim)");
    }) {
        eprintln!("[ztik][memory] failed to dispatch free memory on UI thread: {err}");
    }
}

pub fn prepare_exit(app: &AppHandle) {
    memory::shutdown(app);
    app.state::<WindowState>().mark_shown();
    for (label, win) in app.webview_windows() {
        if let Err(err) = win.destroy() {
            eprintln!("[ztik][window] failed to destroy window '{label}' during exit: {err}");
        }
    }
}

fn show_main_window_on_ui_thread(app: &AppHandle) {
    let state = app.state::<WindowState>();
    state.mark_shown();

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
    if state.take_pending_ram_clean_toast() {
        let _ = win.eval(SHOW_RAM_CLEANED_TOAST_JS);
    }
    sync_window_frame_appearance(&win);
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
        sync_window_frame_appearance(&win);

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
    sync_window_frame_appearance(&win);

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

    let app_handle = app.handle().clone();
    let popup_data_dir = data_dir.clone();
    let download_app = app.handle().clone();

    let builder = WebviewWindowBuilder::new(
        app,
        MAIN_WINDOW_LABEL,
        WebviewUrl::App(PathBuf::from(SPLASH_APP_PATH)),
    )
    .title(MAIN_WINDOW_TITLE)
    .inner_size(MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT)
    .background_color(WINDOW_BG_BLACK)
    .decorations(false)
    .shadow(true)
    .resizable(true)
    .data_directory(data_dir)
    .initialization_script(build_initialization_script())
    .on_download(move |_webview, event| handle_main_download(&download_app, event))
    .on_page_load(|window, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }

        let is_error_page = is_internal_error_page_url(payload.url());
        sync_window_decorations(&window, is_error_page);

        if is_tiktok_page_url(payload.url())
            && window.state::<WindowState>().is_mini_player_active()
        {
            set_mini_player_dom_mode(&window, true);
        }

        if window.state::<WindowState>().take_pending_ram_clean_toast() {
            memory::trim_working_set_now(window.app_handle());
            let _ = window.eval(SHOW_RAM_CLEANED_TOAST_JS);
        }
    })
    .on_navigation(is_main_navigation_allowed)
    .on_new_window(move |url, features| {
        handle_new_window(&app_handle, &popup_data_dir, &url, features)
    });

    #[cfg(target_os = "windows")]
    let builder = builder.additional_browser_args(WEBVIEW2_BROWSER_ARGS);

    let window = builder.build()?;
    memory::record_webview_browser_pid(&window);
    #[cfg(target_os = "windows")]
    {
        configure_windows_frame(&window);
        configure_windows_webview_background(&window);
        attach_windows_navigation_guard(&window);
        let deferred_window = window.clone();
        let _ = app.run_on_main_thread(move || {
            sync_window_frame_appearance(&deferred_window);
        });
    }
    Ok(window)
}

pub(crate) fn is_internal_error_page_url(url: &Url) -> bool {
    url.scheme() == "chrome-error"
        || (url.scheme() == "about" && url.path().starts_with("neterror"))
}

pub(crate) fn is_tiktok_page_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host_matches_domain(&host, "tiktok.com")
}

pub(crate) fn is_local_splash_url(url: &Url) -> bool {
    let is_local_tauri_origin = match url.scheme() {
        "tauri" => url.host_str() == Some("localhost") && url.port().is_none(),
        "http" | "https" => url.host_str() == Some("tauri.localhost") && url.port().is_none(),
        _ => false,
    };
    if !is_local_tauri_origin || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    matches!(url.path(), "/" | "/index.html")
}

fn sync_window_decorations(window: &WebviewWindow, decorated: bool) {
    let currently_decorated = window.is_decorated().unwrap_or(false);
    if currently_decorated != decorated {
        if let Err(err) = window.set_decorations(decorated) {
            eprintln!("[ztik][window] failed to set decorations({decorated}): {err}");
            return;
        }
        #[cfg(target_os = "windows")]
        if !decorated {
            configure_windows_frame(window);
        }
    }

    let script = if decorated {
        SET_NATIVE_DECORATIONS_ON_JS
    } else {
        SET_NATIVE_DECORATIONS_OFF_JS
    };
    let _ = window.eval(script);
}

fn handle_main_download(app: &AppHandle, event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            if !is_allowed_media_download_url(&url) {
                eprintln!(
                    "[ztik][download] blocked untrusted download source: {}",
                    url.host_str().unwrap_or_else(|| url.scheme())
                );
                return false;
            }

            let directory = match app
                .path()
                .download_dir()
                .or_else(|_| app.path().home_dir().map(|path| path.join("Downloads")))
            {
                Ok(path) => path.join("ztik"),
                Err(err) => {
                    eprintln!("[ztik][download] failed to resolve Downloads directory: {err}");
                    return false;
                }
            };
            if let Err(err) = std::fs::create_dir_all(&directory) {
                eprintln!("[ztik][download] failed to create download directory: {err}");
                return false;
            }

            let suggested_name = destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ztik_download");
            let safe_name = sanitize_download_filename(suggested_name);
            *destination = reserve_unique_download_path(&directory, &safe_name, &url);
            eprintln!("[ztik][download] streaming media into Downloads/ztik/{safe_name}");
            true
        }
        DownloadEvent::Finished { url, path, success } => {
            release_download_reservation(&url, path.as_deref());
            if success {
                eprintln!("[ztik][download] completed media download");
            } else {
                eprintln!("[ztik][download] media download failed");
            }
            true
        }
        _ => true,
    }
}

const MEDIA_DOWNLOAD_DOMAINS: &[&str] = &[
    "tiktok.com",
    "tiktokv.com",
    "bytedance.com",
    "tiktokcdn.com",
    "tiktokcdn-us.com",
    "tiktokcdn-eu.com",
    "tiktokcdn-in.com",
    "ibyteimg.com",
    "ibytedtos.com",
    "byteoversea.com",
];

pub(crate) fn is_allowed_blob_download_url(url: &Url) -> bool {
    if url.scheme() != "blob" {
        return false;
    }
    let Ok(inner) = Url::parse(url.path()) else {
        return false;
    };
    if inner.scheme() != "https"
        || inner.port_or_known_default() != Some(443)
        || !inner.username().is_empty()
        || inner.password().is_some()
    {
        return false;
    }
    let Some(host) = inner.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host_matches_domain(&host, "tiktok.com")
}

pub(crate) fn is_allowed_media_download_url(url: &Url) -> bool {
    if is_allowed_blob_download_url(url) {
        return true;
    }
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    MEDIA_DOWNLOAD_DOMAINS
        .iter()
        .any(|domain| host_matches_domain(&host, domain))
}

pub(crate) fn sanitize_download_filename(input: &str) -> String {
    let leaf = input.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut clean = String::with_capacity(leaf.len().min(180));
    let mut previous_underscore = false;
    for ch in leaf.chars().take(180) {
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        let output = if safe { ch } else { '_' };
        if output == '_' {
            if previous_underscore {
                continue;
            }
            previous_underscore = true;
        } else {
            previous_underscore = false;
        }
        clean.push(output);
    }
    let clean = clean.trim_matches(['.', '_', '-']);
    if clean.is_empty() || clean == "." || clean == ".." {
        return "ztik_download".to_string();
    }
    let stem = clean
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "LPT1"
    ) {
        return format!("_{clean}");
    }
    clean.to_string()
}

#[derive(Default)]
struct DownloadReservations {
    paths: std::collections::HashSet<PathBuf>,
    by_url: std::collections::HashMap<String, std::collections::VecDeque<PathBuf>>,
}

static DOWNLOAD_RESERVATIONS: std::sync::OnceLock<Mutex<DownloadReservations>> =
    std::sync::OnceLock::new();

fn reserve_unique_download_path(directory: &Path, filename: &str, url: &Url) -> PathBuf {
    let reservations =
        DOWNLOAD_RESERVATIONS.get_or_init(|| Mutex::new(DownloadReservations::default()));
    let mut reservations = reservations.lock().unwrap_or_else(|e| e.into_inner());
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("ztik_download");
    let extension = path.extension().and_then(|s| s.to_str());
    let mut index = 0u32;
    loop {
        let candidate_name = if index == 0 {
            filename.to_string()
        } else if let Some(extension) = extension {
            format!("{stem}_{index}.{extension}")
        } else {
            format!("{stem}_{index}")
        };
        let candidate = directory.join(candidate_name);
        if !candidate.exists() && !reservations.paths.contains(&candidate) {
            reservations.paths.insert(candidate.clone());
            reservations
                .by_url
                .entry(url.as_str().to_string())
                .or_default()
                .push_back(candidate.clone());
            return candidate;
        }
        index = index.saturating_add(1);
    }
}

fn release_download_reservation(url: &Url, path: Option<&Path>) {
    if let Some(reservations) = DOWNLOAD_RESERVATIONS.get() {
        let mut reservations = reservations.lock().unwrap_or_else(|e| e.into_inner());
        let key = url.as_str();
        let mut remove_path = None;
        let mut remove_entry = false;
        if let Some(paths) = reservations.by_url.get_mut(key) {
            if let Some(path) = path {
                if let Some(index) = paths.iter().position(|candidate| candidate == path) {
                    remove_path = paths.remove(index);
                }
            } else {
                remove_path = paths.pop_front();
            }
            remove_entry = paths.is_empty();
        }
        if let Some(path) = remove_path {
            reservations.paths.remove(&path);
        }
        if remove_entry {
            reservations.by_url.remove(key);
        }
    }
}

pub fn sync_window_frame_appearance(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    configure_windows_frame(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
fn configure_windows_frame(window: &WebviewWindow) {
    use windows::core::BOOL;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        DWMWCP_ROUND,
    };
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    const WINDOWS_BORDER_COLOR_REF: u32 = 0x0024_2020; // COLORREF (0x00BBGGRR) for #202024

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let is_decorated = window.is_decorated().unwrap_or(false);
    let is_maximized = window.is_maximized().unwrap_or(false);
    let corner_preference = match desired_window_corner_mode(is_maximized) {
        WindowCornerMode::Round => DWMWCP_ROUND,
        WindowCornerMode::DoNotRound => DWMWCP_DONOTROUND,
    };

    unsafe {
        if !is_decorated {
            // Keep WS_CAPTION + WS_THICKFRAME + sysmenu/min/max (and no WS_POPUP) so Windows 11
            // DWM draws rounded corners, drop shadow, and Aero Snap while tao's WM_NCCALCSIZE hides
            // the native titlebar.
            let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let desired_style = (current_style & !WS_POPUP.0)
                | WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_MAXIMIZEBOX.0
                | WS_MINIMIZEBOX.0
                | WS_SYSMENU.0;
            if desired_style != current_style {
                SetWindowLongPtrW(hwnd, GWL_STYLE, desired_style as isize);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }

            let margins = if is_maximized {
                MARGINS {
                    cxLeftWidth: 0,
                    cxRightWidth: 0,
                    cyTopHeight: 0,
                    cyBottomHeight: 0,
                }
            } else {
                MARGINS {
                    cxLeftWidth: 0,
                    cxRightWidth: 0,
                    cyTopHeight: 1,
                    cyBottomHeight: 0,
                }
            };
            if let Err(err) = DwmExtendFrameIntoClientArea(hwnd, &margins) {
                eprintln!("[ztik][window] failed to extend DWM frame margins: {err}");
            }
        }

        let dark_mode = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of_val(&dark_mode) as u32,
        );

        if let Err(err) = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_preference as *const _ as *const _,
            std::mem::size_of_val(&corner_preference) as u32,
        ) {
            eprintln!("[ztik][window] failed to set Windows 11 corner preference: {err}");
        }

        let border_color = if is_maximized {
            DWMWA_COLOR_NONE
        } else {
            WINDOWS_BORDER_COLOR_REF
        };
        if let Err(err) = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_color as *const _ as *const _,
            std::mem::size_of_val(&border_color) as u32,
        ) {
            eprintln!("[ztik][window] failed to set Windows border color: {err}");
        }
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_webview_background(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller2, COREWEBVIEW2_COLOR,
    };
    use windows::core::Interface;

    let _ = window.with_webview(|platform_webview| unsafe {
        if let Ok(controller2) = platform_webview
            .controller()
            .cast::<ICoreWebView2Controller2>()
        {
            let black = COREWEBVIEW2_COLOR {
                A: 255,
                R: 0,
                G: 0,
                B: 0,
            };
            if let Err(err) = controller2.SetDefaultBackgroundColor(black) {
                eprintln!("[ztik][window] failed to set WebView2 DefaultBackgroundColor: {err}");
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn attach_windows_navigation_guard(window: &WebviewWindow) {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
        },
        NavigationCompletedEventHandler,
    };
    use windows::core::BOOL;

    let guarded_window = window.clone();
    if let Err(err) = window.with_webview(move |platform_webview| unsafe {
        if let Ok(core_webview) = platform_webview.controller().CoreWebView2() {
            let mut token: i64 = 0;
            let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut is_success = BOOL(1);
                    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                    let _ = args.IsSuccess(&mut is_success);
                    let _ = args.WebErrorStatus(&mut status);
                    if !is_success.as_bool()
                        && status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
                    {
                        sync_window_decorations(&guarded_window, true);
                    }
                }
                Ok(())
            }));
            if let Err(err) = core_webview.add_NavigationCompleted(&handler, &mut token) {
                eprintln!("[ztik][window] failed to register NavigationCompleted guard: {err}");
            }
        }
    }) {
        eprintln!("[ztik][window] failed to attach Windows navigation guard: {err}");
    }
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
    is_local_splash_url(url) || is_allowed_https_navigation(url)
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

    #[cfg(target_os = "windows")]
    let builder = builder.additional_browser_args(WEBVIEW2_BROWSER_ARGS);

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
        build_initialization_script, compute_restore_bounds, desired_window_corner_mode,
        is_allowed_blob_download_url, is_allowed_media_download_url, is_internal_error_page_url,
        is_local_splash_url, is_main_navigation_allowed, is_oauth_popup_allowed,
        is_popup_navigation_allowed, is_tiktok_page_url, is_upload_blocked,
        is_valid_window_position, mini_player_target_size, release_download_reservation,
        reserve_unique_download_path, sanitize_download_filename, split_hide_to_tray_sequence,
        HideToTrayStep, SavedBounds, ShowFromTrayStep, WindowCornerMode, WindowState,
        ENTER_MINI_PLAYER_JS, EXIT_MINI_PLAYER_JS, FREE_MEMORY_PREPARE_JS, HIDE_TO_TRAY_SEQUENCE,
        INJECT_CSS, INJECT_JS, MAIN_WINDOW_HEIGHT, MAIN_WINDOW_WIDTH, MINI_PLAYER_HEIGHT,
        MINI_PLAYER_WIDTH, PAUSE_ALL_MEDIA_JS, SHOW_FROM_TRAY_SEQUENCE, SHOW_RAM_CLEANED_TOAST_JS,
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
        assert!(is_main_navigation_allowed(&parse(
            "http://tauri.localhost/index.html"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "https://tauri.localhost/"
        )));
        assert!(is_main_navigation_allowed(&parse(
            "tauri://localhost/index.html"
        )));

        assert!(!is_main_navigation_allowed(&parse(
            "https://tiktok.com.evil.com/"
        )));
        assert!(!is_main_navigation_allowed(&parse("http://www.tiktok.com")));
        assert!(!is_main_navigation_allowed(&parse(
            "http://tauri.localhost:8080/index.html"
        )));
        assert!(!is_main_navigation_allowed(&parse(
            "http://tauri.localhost/other.html"
        )));
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
    fn media_download_whitelist_is_https_and_limited_to_tiktok_bytedance_cdn_domains() {
        assert!(is_allowed_media_download_url(&parse(
            "https://v16.tiktokcdn.com/video.mp4"
        )));
        assert!(is_allowed_media_download_url(&parse(
            "https://p16-sign-va.tiktokcdn-us.com/photo.jpg"
        )));
        assert!(is_allowed_media_download_url(&parse(
            "https://p16-tiktok-va.ibyteimg.com/photo.jpg"
        )));
        assert!(is_allowed_media_download_url(&parse(
            "https://v16-webapp.byteoversea.com/video.mp4"
        )));
        assert!(is_allowed_media_download_url(&parse(
            "blob:https://www.tiktok.com/550e8400-e29b-41d4-a716-446655440000"
        )));
        assert!(is_allowed_blob_download_url(&parse(
            "blob:https://tiktok.com/1234-5678"
        )));

        assert!(!is_allowed_blob_download_url(&parse(
            "blob:http://www.tiktok.com/1234-5678"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "blob:https://evil.example/1234-5678"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "blob:https://tiktok.com.evil.example/1234-5678"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "blob:null/1234-5678"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "http://v16.tiktokcdn.com/video.mp4"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "https://tiktokcdn-us.com.evil.example/photo.jpg"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "https://media.example/video.mp4"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "https://p16-sign-va.tiktokcdn-us.com:8443/photo.jpg"
        )));
        assert!(!is_allowed_media_download_url(&parse(
            "https://account@p16-sign-va.tiktokcdn-us.com/photo.jpg"
        )));
        assert!(!is_main_navigation_allowed(&parse(
            "https://p16-tiktok-va.ibyteimg.com/photo.jpg"
        )));
    }

    #[test]
    fn download_filename_is_clean_and_never_keeps_a_path() {
        assert_eq!(
            sanitize_download_filename(r"C:\Users\me\ztik:alice/123?.mp4"),
            "123_.mp4"
        );
        assert_eq!(sanitize_download_filename("bad<>name.mp4"), "bad_name.mp4");
        assert_eq!(sanitize_download_filename("../"), "ztik_download");
        assert_eq!(sanitize_download_filename("CON.mp4"), "_CON.mp4");
    }

    #[test]
    fn download_destination_reserves_a_new_name_for_collisions() {
        let directory =
            std::env::temp_dir().join(format!("ztik-download-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create temporary download folder");
        let url = parse("https://v16.tiktokcdn.com/video.mp4");

        let first = reserve_unique_download_path(&directory, "ztik_creator_123.mp4", &url);
        let second = reserve_unique_download_path(&directory, "ztik_creator_123.mp4", &url);
        assert_ne!(first, second);
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("ztik_creator_123_1.mp4")
        );

        release_download_reservation(&url, Some(&first));
        release_download_reservation(&url, Some(&second));
        std::fs::remove_dir_all(&directory).expect("remove temporary download folder");
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
    fn native_decorations_only_fallback_on_internal_error_pages() {
        assert!(is_internal_error_page_url(&parse(
            "chrome-error://chromewebdata/"
        )));
        assert!(is_internal_error_page_url(&parse(
            "about:neterror?e=dnsNotFound"
        )));
        assert!(!is_internal_error_page_url(&parse(
            "https://www.tiktok.com/"
        )));
        assert!(!is_internal_error_page_url(&parse(
            "http://tauri.localhost/index.html"
        )));
        assert!(!is_internal_error_page_url(&parse(
            "tauri://localhost/index.html"
        )));

        assert!(is_tiktok_page_url(&parse("https://www.tiktok.com/foryou")));
        assert!(is_tiktok_page_url(&parse("https://tiktok.com/")));
        assert!(!is_tiktok_page_url(&parse("chrome-error://chromewebdata/")));
        assert!(!is_tiktok_page_url(&parse("http://www.tiktok.com/")));

        assert!(is_local_splash_url(&parse(
            "http://tauri.localhost/index.html"
        )));
        assert!(is_local_splash_url(&parse("tauri://localhost/")));
        assert!(!is_local_splash_url(&parse("https://www.tiktok.com/")));

        assert_eq!(desired_window_corner_mode(false), WindowCornerMode::Round);
        assert_eq!(
            desired_window_corner_mode(true),
            WindowCornerMode::DoNotRound
        );

        let state = WindowState::new();
        assert!(state.update_maximized_state(false));
        assert!(!state.update_maximized_state(false));
        assert!(state.update_maximized_state(true));
        assert!(!state.update_maximized_state(true));
        assert!(state.update_maximized_state(false));
    }

    #[test]
    fn inject_assets_are_non_empty_and_contain_key_selectors() {
        const SPLASH_HTML: &str = include_str!("../dist/index.html");

        assert!(!INJECT_CSS.trim().is_empty());
        assert!(!INJECT_JS.trim().is_empty());
        assert!(SPLASH_HTML.contains("#000000"));
        assert!(SPLASH_HTML.contains("https://www.tiktok.com/"));
        assert!(SPLASH_HTML.contains("#25F4EE"));
        assert!(SPLASH_HTML.contains("#FE2C55"));

        assert!(INJECT_CSS.contains("/upload"));
        assert!(INJECT_CSS.contains("/tiktokstudio"));
        assert!(INJECT_CSS.contains("upload-icon"));
        assert!(INJECT_CSS.contains("get-coins"));
        assert!(INJECT_CSS.contains("get-app"));
        assert!(INJECT_CSS.contains("promote"));
        assert!(!INJECT_CSS.contains(".css-"));
        assert!(INJECT_CSS.contains("overflow-x: hidden"));
        assert!(INJECT_CSS.contains("overflow-x: clip"));
        assert!(INJECT_CSS.contains("ztik-native-decorations"));
        assert!(INJECT_CSS.contains("--ztik-titlebar-h"));
        assert!(INJECT_CSS.contains("100dvh"));
        assert!(INJECT_CSS.contains("border-radius: 16px"));
        assert!(INJECT_CSS.contains("html.ztik-mini"));
        assert!(INJECT_CSS.contains("[data-ztik-mini-hide=\"1\"]"));
        assert!(INJECT_CSS.contains("main"));
        assert!(INJECT_CSS.contains("margin-left: 0 !important"));
        assert!(INJECT_CSS.contains("max-width: 100% !important"));
        assert!(INJECT_CSS.contains("overflow-y: auto"));
        assert!(INJECT_CSS.contains("overflow: clip"));
        assert!(INJECT_CSS.contains("content-visibility: auto"));
        assert!(INJECT_CSS.contains("contain-intrinsic-size"));
        assert!(!INJECT_CSS.contains("translateZ(0)"));
        assert!(!INJECT_CSS.contains("will-change"));
        assert!(INJECT_CSS.contains("#ztik-titlebar"));
        assert!(INJECT_CSS.contains("#fe2c55"));

        assert!(INJECT_JS.contains("__ztikInjected"));
        assert!(INJECT_JS.contains("ztik-style"));
        assert!(INJECT_JS.contains("MutationObserver"));
        assert!(INJECT_JS.contains("IntersectionObserver"));
        assert!(INJECT_JS.contains("contentvisibilityautostatechange"));
        assert!(!INJECT_JS.contains("setInterval"));
        assert!(!INJECT_JS.contains("will-change"));
        assert!(!INJECT_JS.contains("translateZ(0)"));
        assert!(INJECT_JS.contains("__ZTIK_MINI"));
        assert!(INJECT_JS.contains("data-ztik-mini-hide"));
        assert!(INJECT_JS.contains("nav-foryou"));
        assert!(INJECT_JS.contains("replaceState"));
        assert!(INJECT_JS.contains("visibilitychange"));
        assert!(INJECT_JS.contains("ArrowDown"));
        assert!(INJECT_JS.contains("repost"));
        assert!(INJECT_JS.contains("requestAnimationFrame"));
        assert!(INJECT_JS.contains("MEDIA_CACHE_LIMIT = 18"));
        assert!(INJECT_JS.contains("BLOB_REVOKE_DELAY_MS = 3500"));
        assert!(INJECT_JS.contains("__UNIVERSAL_DATA_FOR_REHYDRATION__"));
        assert!(INJECT_JS.contains("bitrateInfo"));
        assert!(INJECT_JS.contains("__reactFiber$"));
        assert!(INJECT_JS.contains("getEntriesByType"));
        assert!(INJECT_JS.contains("ztik_intro_done"));
        assert!(INJECT_JS.contains("URL.createObjectURL"));
        assert!(INJECT_JS.contains("URL.revokeObjectURL"));
        assert!(INJECT_JS.contains("strict-origin-when-cross-origin"));
        assert!(INJECT_JS.contains("__ztikDownloadCurrent"));
        assert!(INJECT_JS.contains("__ztikFreeMemory"));
        assert!(INJECT_JS.contains("__ztikShowToast"));
        assert!(INJECT_JS.contains("ztikDismissed"));
        assert!(INJECT_JS.contains("recharge and save"));
        assert!(INJECT_JS.contains("--ztik-titlebar-h"));
        assert!(INJECT_JS.contains("__TAURI_INTERNALS__"));
        assert!(INJECT_JS.contains("plugin:window|"));
        assert!(!INJECT_JS.contains("window.fetch ="));
        assert!(!INJECT_JS.contains("XMLHttpRequest"));
        assert!(!INJECT_JS.contains("__ztikWrapped"));
        assert!(!INJECT_JS.contains("window.__TAURI__."));
        assert!(!INJECT_JS.contains("window.__TAURI__ &&"));

        let script = build_initialization_script();
        assert!(script.contains("window.__ztikCss = "));
        assert!(script.contains("/upload"));
        assert!(script.contains("/tiktokstudio"));
        assert!(script.contains("__ztikInjected"));
    }

    #[test]
    fn free_memory_scripts_and_pending_toast_state_work_cleanly() {
        assert!(FREE_MEMORY_PREPARE_JS.contains("__ztikFreeMemory"));
        assert!(FREE_MEMORY_PREPARE_JS.contains("clearResourceTimings"));
        assert!(SHOW_RAM_CLEANED_TOAST_JS.contains("Đã dọn RAM"));
        assert!(SHOW_RAM_CLEANED_TOAST_JS.contains("__ztikShowToast"));
        assert!(SHOW_RAM_CLEANED_TOAST_JS.contains("__ztikPendingToast"));

        let state = WindowState::new();
        assert!(!state.take_pending_ram_clean_toast());
        state.set_pending_ram_clean_toast(true);
        assert!(state.take_pending_ram_clean_toast());
        assert!(!state.take_pending_ram_clean_toast());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn webview2_browser_args_are_safe_low_ram_and_waf_friendly() {
        use super::WEBVIEW2_BROWSER_ARGS;

        assert!(WEBVIEW2_BROWSER_ARGS.contains("BackForwardCache"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("Translate"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("InterestFeedContentSuggestions"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--disable-background-networking"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--disable-component-update"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--disable-sync"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--media-cache-size=33554432"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--disk-cache-size=67108864"));
        assert!(WEBVIEW2_BROWSER_ARGS.contains("--autoplay-policy=no-user-gesture-required"));

        // Never inflate GPU Process VRAM or trigger TikTok 403 WAF fingerprinting.
        assert!(!WEBVIEW2_BROWSER_ARGS.contains("--enable-gpu-rasterization"));
        assert!(!WEBVIEW2_BROWSER_ARGS.contains("user-agent"));
        assert!(!WEBVIEW2_BROWSER_ARGS.contains("AutomationControlled"));
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
