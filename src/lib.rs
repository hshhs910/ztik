mod hotkeys;
mod memory;
mod tray;
mod window;

use tauri::{Manager, WindowEvent};

pub fn run() {
    let minimize_tracker = memory::MinimizeStateTracker::new();

    tauri::Builder::default()
        .manage(window::WindowState::new())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main_window(app);
        }))
        .plugin(hotkeys::plugin())
        .on_window_event(move |win, event| {
            if win.label() != window::MAIN_WINDOW_LABEL {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window::hide_to_tray(win.app_handle());
                return;
            }

            if matches!(event, WindowEvent::Resized(_)) && win.is_maximized().unwrap_or(false) {
                let app_handle = win.app_handle();
                if window::exit_mini_player_on_maximize(app_handle) {
                    tray::set_mini_player_checked(app_handle, false);
                }
            }

            if !matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
                return;
            }

            let is_minimized = match win.is_minimized() {
                Ok(minimized) => minimized,
                Err(err) => {
                    eprintln!("[ztik][memory] failed to query is_minimized: {err}");
                    return;
                }
            };

            let Some(action) = minimize_tracker.update(is_minimized) else {
                return;
            };

            let Some(webview_window) = win
                .app_handle()
                .get_webview_window(window::MAIN_WINDOW_LABEL)
            else {
                return;
            };

            match action {
                memory::MemoryAction::SetLow => memory::set_low_memory(&webview_window),
                memory::MemoryAction::SetNormal => memory::set_normal_memory(&webview_window),
            }
        })
        .setup(|app| {
            window::create_main_window(app)?;
            tray::create_tray(app)?;
            hotkeys::register_all(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
