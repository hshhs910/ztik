use tauri::{plugin::TauriPlugin, App, AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    PlayPause,
    NextVideo,
    Repost,
}

struct HotkeyBinding {
    action: HotkeyAction,
    label: &'static str,
    modifiers: Modifiers,
    code: Code,
}

const HOTKEY_MODIFIERS: Modifiers = Modifiers::CONTROL
    .union(Modifiers::ALT)
    .union(Modifiers::SHIFT);

const HOTKEY_BINDINGS: &[HotkeyBinding] = &[
    HotkeyBinding {
        action: HotkeyAction::PlayPause,
        label: "Ctrl+Alt+Shift+Space",
        modifiers: HOTKEY_MODIFIERS,
        code: Code::Space,
    },
    HotkeyBinding {
        action: HotkeyAction::NextVideo,
        label: "Ctrl+Alt+Shift+Right",
        modifiers: HOTKEY_MODIFIERS,
        code: Code::ArrowRight,
    },
    HotkeyBinding {
        action: HotkeyAction::Repost,
        label: "Ctrl+Alt+Shift+R",
        modifiers: HOTKEY_MODIFIERS,
        code: Code::KeyR,
    },
];

const PLAY_PAUSE_JS: &str = r#"(function () {
  try {
    const videos = document.querySelectorAll('video');
    let bestVideo = null;
    let bestScore = 0;

    for (let i = 0; i < videos.length; i += 1) {
      const video = videos[i];
      const rect = video.getBoundingClientRect();
      const xOverlap = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0));
      const yOverlap = Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
      const area = xOverlap * yOverlap;
      if (area <= 0) {
        continue;
      }
      const playingBonus = !video.paused && !video.ended ? 1.25 : 1;
      const score = area * playingBonus;
      if (score > bestScore) {
        bestScore = score;
        bestVideo = video;
      }
    }

    if (!bestVideo) {
      for (let i = 0; i < videos.length; i += 1) {
        if (!videos[i].paused && !videos[i].ended) {
          bestVideo = videos[i];
          break;
        }
      }
    }

    if (!bestVideo && videos.length > 0) {
      bestVideo = videos[0];
    }

    if (!bestVideo) {
      const audios = document.querySelectorAll('audio');
      for (let i = 0; i < audios.length; i += 1) {
        if (!audios[i].paused && !audios[i].ended) {
          audios[i].pause();
          return;
        }
      }
      if (audios.length > 0) {
        const playResult = audios[0].play();
        if (playResult && typeof playResult.catch === 'function') {
          playResult.catch(function () {});
        }
      }
      return;
    }

    if (!bestVideo.paused && !bestVideo.ended) {
      bestVideo.pause();
    } else {
      const playResult = bestVideo.play();
      if (playResult && typeof playResult.catch === 'function') {
        playResult.catch(function () {});
      }
    }
  } catch (_) {}
})();"#;

const NEXT_VIDEO_JS: &str = r#"(function () {
  try {
    function isVisible(el) {
      if (!el || !el.isConnected) {
        return false;
      }
      if (el.getAttribute('data-ztik-hidden') === 'true') {
        return false;
      }
      const style = window.getComputedStyle(el);
      if (!style || style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') {
        return false;
      }
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    }

    const selectors = [
      'button[data-e2e="arrow-right"]',
      'button[data-e2e="browse-next"]',
      'button[data-e2e="arrow-down"]',
      'button[data-e2e="feed-arrow-down"]',
      '[data-e2e="arrow-right"]',
      '[data-e2e="browse-next"]',
      'button[aria-label*="next video" i]',
      'button[aria-label="Go to next video"]',
    ];

    for (let i = 0; i < selectors.length; i += 1) {
      const btn = document.querySelector(selectors[i]);
      if (btn && isVisible(btn) && !btn.disabled && btn.getAttribute('aria-disabled') !== 'true') {
        btn.click();
        return;
      }
    }

    const videos = document.querySelectorAll('video');
    let bestVideo = null;
    let bestScore = 0;
    for (let i = 0; i < videos.length; i += 1) {
      const rect = videos[i].getBoundingClientRect();
      const x = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0));
      const y = Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
      const score = x * y * (!videos[i].paused && !videos[i].ended ? 1.25 : 1);
      if (score > bestScore) {
        bestScore = score;
        bestVideo = videos[i];
      }
    }

    const activeItem = bestVideo
      ? bestVideo.closest('article, [data-e2e="recommend-list-item-container"], [data-e2e="browse-video"], [data-e2e="detail-video"]')
      : null;
    const items = Array.from(document.querySelectorAll('article, [data-e2e="recommend-list-item-container"]'));
    if (items.length > 0) {
      const currentTop = activeItem ? activeItem.getBoundingClientRect().top : 0;
      for (let i = 0; i < items.length; i += 1) {
        if (items[i] !== activeItem && items[i].getBoundingClientRect().top > currentTop + 20) {
          items[i].scrollIntoView({ behavior: 'smooth', block: 'center' });
          return;
        }
      }
    }

    const step = Math.max(300, Math.round(window.innerHeight * 0.85));
    window.scrollBy({ top: step, behavior: 'smooth' });
  } catch (_) {}
})();"#;

const REPOST_JS: &str = r#"(function () {
  try {
    function isVisible(el) {
      if (!el || !el.isConnected) {
        return false;
      }
      if (el.getAttribute('data-ztik-hidden') === 'true') {
        return false;
      }
      const style = window.getComputedStyle(el);
      if (!style || style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') {
        return false;
      }
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    }

    function visibleArea(el) {
      const rect = el.getBoundingClientRect();
      const x = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0));
      const y = Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
      return x * y;
    }

    function isRepostTab(el) {
      if (!el) {
        return true;
      }
      if (el.getAttribute('role') === 'tab' || el.closest('[role="tab"], [role="tablist"]')) {
        return true;
      }
      return (el.getAttribute('data-e2e') || '').toLowerCase().includes('tab');
    }

    function findRepostControl(scope) {
      const root = scope || document;
      const candidates = root.querySelectorAll('[data-e2e*="repost" i]');
      let best = null;
      let bestArea = 0;
      for (let i = 0; i < candidates.length; i += 1) {
        const el = candidates[i];
        if (isRepostTab(el)) {
          continue;
        }
        const clickable = el.closest('button, [role="button"], [role="menuitem"], a') || el;
        if (isRepostTab(clickable) || !isVisible(clickable)) {
          continue;
        }
        const area = visibleArea(clickable);
        if (area > bestArea) {
          bestArea = area;
          best = clickable;
        }
      }
      return best;
    }

    const videos = document.querySelectorAll('video');
    let activeVideo = null;
    let bestScore = 0;
    for (let i = 0; i < videos.length; i += 1) {
      const score = visibleArea(videos[i]) * (!videos[i].paused && !videos[i].ended ? 1.25 : 1);
      if (score > bestScore) {
        bestScore = score;
        activeVideo = videos[i];
      }
    }

    let container = null;
    if (activeVideo) {
      container = activeVideo.closest(
        'article, [data-e2e="recommend-list-item-container"], [data-e2e="browse-video"], [data-e2e="detail-video"]'
      );
      if (!container) {
        let cur = activeVideo.parentElement;
        let depth = 0;
        while (cur && cur !== document.body && depth < 10) {
          if (cur.querySelector('[data-e2e*="repost" i], [data-e2e*="share" i], [data-e2e="like-icon"], [data-e2e="browse-like-icon"]')) {
            container = cur;
            break;
          }
          cur = cur.parentElement;
          depth += 1;
        }
      }
    }

    const direct = (container && findRepostControl(container)) || findRepostControl(document);
    if (direct) {
      direct.click();
      return;
    }

    if (!container) {
      return;
    }

    const shareTarget = container.querySelector(
      '[data-e2e="share-btn"], [data-e2e="video-share"], [data-e2e="share-icon"], [data-e2e="browse-share"]'
    );
    const shareBtn = (shareTarget && (shareTarget.closest('button, [role="button"]') || shareTarget)) || null;
    if (!shareBtn || !isVisible(shareBtn)) {
      return;
    }

    shareBtn.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    shareBtn.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    setTimeout(function () {
      const menuRepost = findRepostControl(document);
      if (menuRepost) {
        menuRepost.click();
      }
    }, 150);
  } catch (_) {}
})();"#;

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new().build()
}

pub fn register_all(app: &App) {
    let global_shortcut = app.global_shortcut();

    for binding in HOTKEY_BINDINGS {
        let shortcut = Shortcut::new(Some(binding.modifiers), binding.code);
        let action = binding.action;
        let label = binding.label;

        match global_shortcut.on_shortcut(shortcut, move |app_handle, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            dispatch_action(app_handle, action);
        }) {
            Ok(()) => {
                eprintln!("[ztik][hotkeys] registered {label} ({action:?})");
            }
            Err(err) => {
                eprintln!(
                    "[ztik][hotkeys] warning: failed to register {label} ({action:?}): {err}"
                );
            }
        }
    }
}

pub fn unregister_all(app: &AppHandle) {
    if let Err(err) = app.global_shortcut().unregister_all() {
        eprintln!("[ztik][hotkeys] warning: failed to unregister shortcuts on exit: {err}");
    }
}

pub(crate) fn script_for_action(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::PlayPause => PLAY_PAUSE_JS,
        HotkeyAction::NextVideo => NEXT_VIDEO_JS,
        HotkeyAction::Repost => REPOST_JS,
    }
}

fn dispatch_action(app: &AppHandle, action: HotkeyAction) {
    if window::is_hidden_to_tray(app) {
        eprintln!("[ztik][hotkeys] ignored {action:?} while hidden in tray");
        return;
    }

    let Some(main_window) = app.get_webview_window(window::MAIN_WINDOW_LABEL) else {
        return;
    };

    let script = script_for_action(action);
    if let Err(err) = main_window.eval(script) {
        eprintln!("[ztik][hotkeys] eval failed for {action:?}: {err}");
    } else {
        eprintln!("[ztik][hotkeys] triggered {action:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::{script_for_action, HotkeyAction, HOTKEY_BINDINGS};
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn hotkeys_have_three_distinct_bindings_and_one_way_scripts() {
        assert_eq!(HOTKEY_BINDINGS.len(), 3);

        let mut ids = Vec::new();
        for binding in HOTKEY_BINDINGS {
            let shortcut = Shortcut::new(Some(binding.modifiers), binding.code);
            assert!(
                !ids.contains(&shortcut.id()),
                "duplicate shortcut id for {}",
                binding.label
            );
            ids.push(shortcut.id());

            let script = script_for_action(binding.action);
            assert!(!script.trim().is_empty());
            assert!(!script.contains("__TAURI__"));
            assert!(!script.contains("invoke("));
            assert!(!script.contains(".css-"));
            assert!(!script.contains(".remove("));
        }

        assert!(script_for_action(HotkeyAction::PlayPause).contains(".pause()"));
        assert!(script_for_action(HotkeyAction::PlayPause).contains(".play()"));
        assert!(script_for_action(HotkeyAction::NextVideo).contains("scrollIntoView"));
        assert!(script_for_action(HotkeyAction::Repost).contains("repost"));
    }
}
