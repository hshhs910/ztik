use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, Wry,
};

use crate::{hotkeys, window};

pub const TRAY_ID: &str = "ztik-tray";

const MENU_SHOW_ID: &str = "tray-show";
const MENU_MINI_PLAYER_ID: &str = "tray-mini-player";
const MENU_QUIT_ID: &str = "tray-quit";

const MENU_SHOW_LABEL: &str = "Mở ztik";
const MENU_MINI_PLAYER_LABEL: &str = "Mini-player";
const MENU_QUIT_LABEL: &str = "Thoát hẳn";

const FALLBACK_ICON_ICO: &[u8] = include_bytes!("../icons/icon.ico");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayMenuAction {
    ShowWindow,
    ToggleMiniPlayer,
    QuitApp,
}

pub(crate) fn resolve_menu_action(id: &str) -> Option<TrayMenuAction> {
    match id {
        MENU_SHOW_ID => Some(TrayMenuAction::ShowWindow),
        MENU_MINI_PLAYER_ID => Some(TrayMenuAction::ToggleMiniPlayer),
        MENU_QUIT_ID => Some(TrayMenuAction::QuitApp),
        _ => None,
    }
}

pub(crate) fn should_show_window_on_tray_click(
    button: MouseButton,
    button_state: MouseButtonState,
) -> bool {
    matches!(
        (button, button_state),
        (MouseButton::Left, MouseButtonState::Up)
    )
}

struct TrayMenuState {
    mini_player_item: CheckMenuItem<Wry>,
}

pub fn create_tray(app: &App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, MENU_SHOW_ID, MENU_SHOW_LABEL, true, None::<&str>)?;
    let mini_player_item = CheckMenuItem::with_id(
        app,
        MENU_MINI_PLAYER_ID,
        MENU_MINI_PLAYER_LABEL,
        true,
        false,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, MENU_QUIT_LABEL, true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &mini_player_item, &quit_item])?;

    let mini_player_item_for_event = mini_player_item.clone();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("ztik")
        .show_menu_on_left_click(false)
        .on_menu_event(
            move |app, event| match resolve_menu_action(event.id().as_ref()) {
                Some(TrayMenuAction::ShowWindow) => {
                    window::show_main_window(app);
                }
                Some(TrayMenuAction::ToggleMiniPlayer) => {
                    let app_handle = app.clone();
                    let item = mini_player_item_for_event.clone();
                    if let Err(err) = app.run_on_main_thread(move || {
                        let active = window::toggle_mini_player(&app_handle);
                        if let Err(err) = item.set_checked(active) {
                            eprintln!(
                                "[ztik][tray] failed to update Mini-player check state: {err}"
                            );
                        }
                    }) {
                        eprintln!("[ztik][tray] failed to dispatch Mini-player toggle: {err}");
                    }
                }
                Some(TrayMenuAction::QuitApp) => {
                    eprintln!("[ztik][tray] quitting application");
                    let app_handle = app.clone();
                    if let Err(err) = app.run_on_main_thread(move || {
                        hotkeys::unregister_all(&app_handle);
                        window::prepare_exit(&app_handle);
                        app_handle.exit(0);
                    }) {
                        eprintln!("[ztik][tray] failed to dispatch quit on UI thread: {err}");
                        app.exit(0);
                    }
                }
                None => {}
            },
        )
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if should_show_window_on_tray_click(button, button_state) {
                    window::show_main_window(tray.app_handle());
                }
            }
        });

    if let Some(icon) = resolve_tray_icon(app) {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    let _ = app.manage(TrayMenuState {
        mini_player_item: mini_player_item.clone(),
    });
    eprintln!("[ztik][tray] system tray initialized");
    Ok(())
}

pub fn set_mini_player_checked(app: &AppHandle, checked: bool) {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        eprintln!("[ztik][tray] Mini-player menu state is unavailable");
        return;
    };

    if let Err(err) = state.mini_player_item.set_checked(checked) {
        eprintln!("[ztik][tray] failed to update Mini-player check state: {err}");
    }
}

fn resolve_tray_icon(app: &App) -> Option<Image<'_>> {
    if let Some(icon) = app.default_window_icon() {
        return Some(icon.clone());
    }
    decode_ico_rgba(FALLBACK_ICON_ICO)
}

fn decode_ico_rgba(bytes: &[u8]) -> Option<Image<'static>> {
    // ICONDIR (6 bytes) + at least 1 ICONDIRENTRY (16 bytes)
    if bytes.len() < 22 {
        return None;
    }
    let ico_type = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]);
    if ico_type != 1 || count == 0 {
        return None;
    }

    let offset = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as usize;
    if bytes.len() < offset + 40 {
        return None;
    }

    let dib = &bytes[offset..];
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if header_size < 40 || dib.len() < header_size {
        return None;
    }

    let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    let raw_height = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let bpp = u16::from_le_bytes([dib[14], dib[15]]);
    let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);

    if width <= 0 || raw_height == 0 || bpp != 32 || compression != 0 {
        return None;
    }

    let width_u32 = width as u32;
    let height_u32 = (raw_height.unsigned_abs()) / 2;
    let pixel_bytes = (width_u32 as usize)
        .checked_mul(height_u32 as usize)?
        .checked_mul(4)?;

    let pixels = dib.get(header_size..header_size + pixel_bytes)?;
    let mut rgba = vec![0u8; pixel_bytes];
    let row_bytes = (width_u32 as usize) * 4;
    let bottom_up = raw_height > 0;

    for y in 0..(height_u32 as usize) {
        let src_y = if bottom_up {
            (height_u32 as usize) - 1 - y
        } else {
            y
        };
        let src_row = &pixels[src_y * row_bytes..(src_y + 1) * row_bytes];
        let dst_row = &mut rgba[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..(width_u32 as usize) {
            let idx = x * 4;
            dst_row[idx] = src_row[idx + 2]; // R
            dst_row[idx + 1] = src_row[idx + 1]; // G
            dst_row[idx + 2] = src_row[idx]; // B
            dst_row[idx + 3] = src_row[idx + 3]; // A
        }
    }

    Some(Image::new_owned(rgba, width_u32, height_u32))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_ico_rgba, resolve_menu_action, should_show_window_on_tray_click, TrayMenuAction,
        FALLBACK_ICON_ICO, MENU_MINI_PLAYER_ID, MENU_MINI_PLAYER_LABEL, MENU_QUIT_ID,
        MENU_QUIT_LABEL, MENU_SHOW_ID, MENU_SHOW_LABEL,
    };
    use tauri::tray::{MouseButton, MouseButtonState};

    #[test]
    fn fallback_ico_decodes_to_rgba_image() {
        let image = decode_ico_rgba(FALLBACK_ICON_ICO).expect("valid 32-bit ICO");
        assert_eq!(image.width(), 32);
        assert_eq!(image.height(), 32);
        assert_eq!(image.rgba().len(), 32 * 32 * 4);
    }

    #[test]
    fn tray_menu_labels_and_action_routing_match_spec() {
        assert_eq!(MENU_SHOW_LABEL, "Mở ztik");
        assert_eq!(MENU_MINI_PLAYER_LABEL, "Mini-player");
        assert_eq!(MENU_QUIT_LABEL, "Thoát hẳn");

        assert_eq!(
            resolve_menu_action(MENU_SHOW_ID),
            Some(TrayMenuAction::ShowWindow)
        );
        assert_eq!(
            resolve_menu_action(MENU_MINI_PLAYER_ID),
            Some(TrayMenuAction::ToggleMiniPlayer)
        );
        assert_eq!(
            resolve_menu_action(MENU_QUIT_ID),
            Some(TrayMenuAction::QuitApp)
        );
        assert_eq!(resolve_menu_action("unknown"), None);
    }

    #[test]
    fn tray_icon_click_shows_window_only_on_left_button_up() {
        assert!(should_show_window_on_tray_click(
            MouseButton::Left,
            MouseButtonState::Up
        ));
        assert!(!should_show_window_on_tray_click(
            MouseButton::Left,
            MouseButtonState::Down
        ));
        assert!(!should_show_window_on_tray_click(
            MouseButton::Right,
            MouseButtonState::Up
        ));
        assert!(!should_show_window_on_tray_click(
            MouseButton::Middle,
            MouseButtonState::Up
        ));
    }
}
