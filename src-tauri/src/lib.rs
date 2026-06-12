mod commands;
mod models;
mod store;

use store::Store;
use tauri::Manager;

pub struct AppState {
    pub store: Store,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::terminal::TerminalSessions::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let store = Store::load(app.handle()).map_err(|e| e.to_string())?;
            app.manage(AppState { store });

            // macOS: replace the default menu so Cmd+W no longer closes the
            // window (which would quit this single-window app). Keep Edit
            // shortcuts (copy/paste/select-all) intact for the terminal.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{MenuBuilder, SubmenuBuilder};
                let h = app.handle();
                let app_menu = SubmenuBuilder::new(h, "sshub")
                    .about(None)
                    .separator()
                    .hide()
                    .quit()
                    .build()?;
                let edit_menu = SubmenuBuilder::new(h, "Edit")
                    .undo()
                    .redo()
                    .separator()
                    .cut()
                    .copy()
                    .paste()
                    .select_all()
                    .build()?;
                let window_menu = SubmenuBuilder::new(h, "Window")
                    .minimize()
                    .fullscreen()
                    .build()?;
                let menu = MenuBuilder::new(h)
                    .items(&[&app_menu, &edit_menu, &window_menu])
                    .build()?;
                app.set_menu(menu)?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Server commands
            commands::server::get_servers,
            commands::server::get_server,
            commands::server::create_server,
            commands::server::update_server,
            commands::server::delete_server,
            commands::server::toggle_favorite,
            // SSH Config commands
            commands::ssh_config::sync_servers_to_config,
            commands::ssh_config::sync_config_to_servers,
            // SSH Key commands
            commands::key::get_ssh_keys,
            commands::key::create_ssh_key,
            commands::key::import_ssh_key,
            commands::key::delete_ssh_key,
            commands::key::load_key_file,
            // Backup / sync commands
            commands::backup::export_data,
            commands::backup::import_data,
            // Terminal commands
            commands::terminal::start_terminal_session,
            commands::terminal::write_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::close_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
