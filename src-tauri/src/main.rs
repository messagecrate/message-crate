//! Desktop process that hosts the Message Crate window.
//!
//! The Vite UI in `web/` runs inside a WebView (a browser-like window). A web
//! page cannot read local phone backups, run the Rust exporters, open native
//! file dialogs, or start ffmpeg. This process is the native host: it owns
//! the window, talks to the operating system, and exposes those jobs as
//! commands the UI can call.

// On Windows release builds, hide the extra console window so only the app
// window appears.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_directories;
mod commands;
mod export_directories;
mod local_server;
mod staging_directories;
mod state;

use export_directories::ExportDirectories;
use local_server::LocalServer;
use staging_directories::StagingDirectories;
use state::AppState;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// Start the desktop window and wait until the user quits.
fn main() {
    // Import and export name this Build to the server on every request; the SPA
    // does the same for its own requests (`web/src/lib/api.ts`).
    message_crate_http::identify_desktop_app(env!("MESSAGE_CRATE_BUILD"));

    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Native open/save dialogs. A WebView page cannot show the OS file picker.
    let dialog_plugin = tauri_plugin_dialog::init();
    // Open files and links with the OS default handler.
    let shell_plugin = tauri_plugin_shell::init();

    let builder = tauri::Builder::default()
        .plugin(dialog_plugin)
        .plugin(shell_plugin)
        .manage(app_state)
        // The Message Crate this app starts for itself, when asked to.
        .manage(LocalServer::default())
        // The Staging Directory and the run directories made under it, kept
        // in the app-data directory; the Export Directory; and the start-up
        // sweep of the Scratch and Export Directories.
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let record = app_data_dir.join(staging_directories::RECORD_FILE);
            app.manage(StagingDirectories::at(record, dirs::home_dir()));
            app.manage(ExportDirectories::in_app_data(&app_data_dir));
            std::thread::spawn(move || app_directories::sweep_at_start_up(&app_data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::extract::extract,
            commands::extract::cancel,
            commands::ffmpeg::probe_ffmpeg_tools,
            commands::ffmpeg::set_ffmpeg_tools_dir,
            commands::format::format,
            commands::paths::home_dir,
            commands::paths::path_stat,
            commands::paths::ios_backup_encrypted,
            commands::paths::imessage_backup_identities,
            commands::paths::open_path,
            commands::paths::save_file,
            commands::local_server::start_local_server,
            commands::local_server::local_server_status,
            commands::local_server::set_open_to_network,
            commands::local_server::open_data_directory,
            commands::push::push,
            commands::pull::pull,
            commands::exports::export_directory,
            commands::exports::create_export_dir,
            commands::exports::finish_export_dir,
            commands::exports::discard_export_dir,
            commands::staging::staging_root,
            commands::staging::set_staging_root,
            commands::staging::create_staging_dir,
            commands::staging::import_run_log,
            commands::staging::summarize_staging,
            commands::staging::transcode_staging,
            commands::staging::delete_staging,
            commands::staging::read_import_run_record,
            commands::staging::save_import_run_record,
        ]);

    builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // The server the app started stops with the app. One it only
            // found (Docker on this computer) is not the app's to stop.
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<LocalServer>().stop();
            }
        });
}
