use std::sync::atomic::{AtomicBool, Ordering};

use tauri::WebviewWindow;

#[cfg(target_os = "windows")]
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_19, ICoreWebView2_3, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    },
    TrySuspendCompletedHandler,
};
#[cfg(target_os = "windows")]
use windows::core::Interface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAction {
    SetLow,
    SetNormal,
}

#[derive(Debug, Default)]
pub struct MinimizeStateTracker {
    was_minimized: AtomicBool,
}

impl MinimizeStateTracker {
    pub const fn new() -> Self {
        Self {
            was_minimized: AtomicBool::new(false),
        }
    }

    pub fn update(&self, is_minimized: bool) -> Option<MemoryAction> {
        let prev = self.was_minimized.swap(is_minimized, Ordering::AcqRel);
        if prev == is_minimized {
            return None;
        }
        if is_minimized {
            Some(MemoryAction::SetLow)
        } else {
            Some(MemoryAction::SetNormal)
        }
    }
}

pub fn set_low_memory(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    set_memory_level(window, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, "Low");
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn set_normal_memory(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    set_memory_level(
        window,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        "Normal",
    );
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn set_webview_visible(window: &WebviewWindow, visible: bool) {
    #[cfg(target_os = "windows")]
    set_webview_visible_windows(window, visible);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, visible);
}

pub fn suspend(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    suspend_windows(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn resume(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    resume_windows(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
fn apply_memory_target_level(
    core_webview: &ICoreWebView2,
    level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
    level_name: &'static str,
) {
    match core_webview.cast::<ICoreWebView2_19>() {
        Ok(webview19) => {
            if let Err(err) = unsafe { webview19.SetMemoryUsageTargetLevel(level) } {
                eprintln!("[ztik][memory] SetMemoryUsageTargetLevel({level_name}) failed: {err}");
            } else {
                eprintln!("[ztik][memory] MemoryUsageTargetLevel -> {level_name}");
            }
        }
        Err(err) => {
            eprintln!("[ztik][memory] ICoreWebView2_19 unavailable, skipping {level_name}: {err}");
        }
    }
}

#[cfg(target_os = "windows")]
fn set_memory_level(
    window: &WebviewWindow,
    level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
    level_name: &'static str,
) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        match unsafe { controller.CoreWebView2() } {
            Ok(core_webview) => apply_memory_target_level(&core_webview, level, level_name),
            Err(err) => eprintln!("[ztik][memory] CoreWebView2() failed: {err}"),
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for {level_name}: {err}");
    }
}

#[cfg(target_os = "windows")]
fn set_webview_visible_windows(window: &WebviewWindow, visible: bool) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        if let Err(err) = unsafe { controller.SetIsVisible(visible) } {
            eprintln!("[ztik][memory] SetIsVisible({visible}) failed: {err}");
        } else {
            eprintln!("[ztik][memory] SetIsVisible({visible})");
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for SetIsVisible({visible}): {err}");
    }
}

#[cfg(target_os = "windows")]
fn suspend_windows(window: &WebviewWindow) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        if let Err(err) = unsafe { controller.SetIsVisible(false) } {
            eprintln!("[ztik][memory] SetIsVisible(false) failed: {err}");
            return;
        }

        let core_webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => webview,
            Err(err) => {
                eprintln!("[ztik][memory] CoreWebView2() failed during suspend: {err}");
                return;
            }
        };

        match core_webview.cast::<ICoreWebView2_3>() {
            Ok(webview3) => {
                let handler =
                    TrySuspendCompletedHandler::create(Box::new(|result, is_successful| {
                        if let Err(err) = result {
                            eprintln!("[ztik][memory] TrySuspend callback error: {err}");
                        } else if !is_successful {
                            eprintln!(
                                "[ztik][memory] TrySuspend completed with is_successful=false"
                            );
                        } else {
                            eprintln!("[ztik][memory] WebView2 suspended");
                        }
                        Ok(())
                    }));
                if let Err(err) = unsafe { webview3.TrySuspend(&handler) } {
                    eprintln!("[ztik][memory] TrySuspend failed: {err}");
                } else {
                    eprintln!("[ztik][memory] TrySuspend requested");
                }
            }
            Err(err) => {
                eprintln!("[ztik][memory] ICoreWebView2_3 unavailable, skipping TrySuspend: {err}");
            }
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for suspend: {err}");
    }
}

#[cfg(target_os = "windows")]
fn resume_windows(window: &WebviewWindow) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        let core_webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => Some(webview),
            Err(err) => {
                eprintln!("[ztik][memory] CoreWebView2() failed during resume: {err}");
                None
            }
        };

        if let Some(ref webview) = core_webview {
            match webview.cast::<ICoreWebView2_3>() {
                Ok(webview3) => {
                    if let Err(err) = unsafe { webview3.Resume() } {
                        eprintln!("[ztik][memory] Resume failed: {err}");
                    } else {
                        eprintln!("[ztik][memory] WebView2 resumed");
                    }
                }
                Err(err) => {
                    eprintln!("[ztik][memory] ICoreWebView2_3 unavailable, skipping Resume: {err}");
                }
            }
        }

        if let Err(err) = unsafe { controller.SetIsVisible(true) } {
            eprintln!("[ztik][memory] SetIsVisible(true) failed: {err}");
        } else {
            eprintln!("[ztik][memory] SetIsVisible(true)");
        }

        if let Some(ref webview) = core_webview {
            apply_memory_target_level(
                webview,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                "Normal",
            );
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for resume: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryAction, MinimizeStateTracker};

    #[test]
    fn transitions_only_when_minimized_state_changes() {
        let tracker = MinimizeStateTracker::new();

        // Repeated non-minimized events on startup or normal resize/focus do nothing.
        assert_eq!(tracker.update(false), None);
        assert_eq!(tracker.update(false), None);

        // Minimize triggers SetLow exactly once.
        assert_eq!(tracker.update(true), Some(MemoryAction::SetLow));
        assert_eq!(tracker.update(true), None);
        assert_eq!(tracker.update(true), None);

        // Restore triggers SetNormal exactly once.
        assert_eq!(tracker.update(false), Some(MemoryAction::SetNormal));
        assert_eq!(tracker.update(false), None);

        // Second cycle works identically.
        assert_eq!(tracker.update(true), Some(MemoryAction::SetLow));
        assert_eq!(tracker.update(false), Some(MemoryAction::SetNormal));
    }
}
