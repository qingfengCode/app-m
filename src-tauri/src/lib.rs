mod commands;
mod job_object;
mod models;
mod static_server;

use models::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let home = app
                .path()
                .home_dir()
                .expect("failed to resolve home dir");
            let data_dir = home.join(".app-m").to_string_lossy().to_string();

            std::fs::create_dir_all(&data_dir).ok();

            app.manage(AppState::new(data_dir));

            // 系统托盘：左键单击显示主窗口，菜单提供显示/退出
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new();
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            tray_builder
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            state.cleanup();
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_apps,
            commands::list_apps,
            commands::add_app,
            commands::update_app,
            commands::delete_app,
            commands::start_app,
            commands::stop_app,
            commands::restart_app,
            commands::stop_all_apps,
            commands::start_all_apps,
            commands::start_auto_start_apps,
            commands::update_sort_order,
            commands::get_groups,
            commands::export_config,
            commands::import_config,
            commands::refresh_all,
            commands::get_process_info,
            commands::get_app_logs,
            commands::clear_app_logs,
            commands::get_monitoring,
            commands::toggle_monitoring,
            commands::get_metrics,
            commands::get_system_info,
            commands::tool_find_process,
            commands::tool_find_port,
            commands::tool_kill_pid,
            commands::tool_read_hosts,
            commands::tool_write_hosts,
            commands::tool_flush_dns,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 关闭窗口时隐藏到系统托盘，应用继续在后台运行
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
