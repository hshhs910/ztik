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

            if matches!(event, WindowEvent::Resized(_)) {
                let app_handle = win.app_handle();
                let is_maximized = win.is_maximized().unwrap_or(false);
                if is_maximized && window::exit_mini_player_on_maximize(app_handle) {
                    tray::set_mini_player_checked(app_handle, false);
                }
                if app_handle
                    .state::<window::WindowState>()
                    .update_maximized_state(is_maximized)
                {
                    if let Some(webview_window) =
                        app_handle.get_webview_window(window::MAIN_WINDOW_LABEL)
                    {
                        window::sync_window_frame_appearance(&webview_window);
                    }
                }
            }

            if !matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
                return;
            }

            let app_handle = win.app_handle();
            let is_minimized = match win.is_minimized() {
                Ok(minimized) => minimized,
                Err(err) => {
                    eprintln!("[ztik][memory] failed to query is_minimized: {err}");
                    false
                }
            };

            if let Some(action) = minimize_tracker.update(is_minimized) {
                match action {
                    memory::MemoryAction::SetLow => memory::notify_minimized(app_handle),
                    memory::MemoryAction::SetNormal => memory::notify_restored(app_handle),
                }
            }

            if let WindowEvent::Focused(focused) = event {
                if *focused {
                    if !is_minimized && !window::is_hidden_to_tray(app_handle) {
                        memory::notify_focused(app_handle);
                    }
                } else if !is_minimized && !window::is_hidden_to_tray(app_handle) {
                    memory::notify_unfocused(app_handle);
                }
            }
        })
        .setup(|app| {
            app.manage(memory::MemoryManager::start(app.handle().clone()));
            window::create_main_window(app)?;
            tray::create_tray(app)?;
            hotkeys::register_all(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
