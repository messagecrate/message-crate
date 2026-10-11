//! Native desktop host for the Message Crate UI.
//!
//! The screens in `web/` are a Vite app. In the desktop build they run inside
//! a WebView, which is a browser-like window. A web page cannot read local
//! phone backups, run the Rust exporters, open native file dialogs, or start
//! ffmpeg. Those jobs need a program that talks to the operating system.
//!
//! This crate is that program. It owns the window, loads the UI, and exposes
//! commands the UI can call (`extract`, `format`, `upload`, `export`, and
//! `tools_status`, where ffmpeg, ffprobe and wtsexporter are). It also starts the Message Crate server it ships with,
//! when nothing answers at the app's own address (`local_server`). Progress and errors go back to the UI as Tauri events.
//!
//! Every module is declared here, so each is compiled once. `main.rs` only
//! calls [`run`], which is the layout the Tauri v2 template uses.

mod app_directories;
mod commands;
mod export_directories;
mod local_server;
mod run_directories;
mod run_logs;
mod state;
mod tool_downloads;

use export_directories::ExportDirectories;
use local_server::LocalServer;
use run_directories::RunDirectories;
use state::AppState;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tool_downloads::ToolDownloads;

/// Start the desktop window and wait until the user quits.
pub fn run() {
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
        // Where each download into the Tools Directory stands, which
        // Settings → System → Media shows.
        .manage(ToolDownloads::default())
        // The Staging Directory and the run directories made under it, kept
        // in the app-data directory; the Export Directory; and the start-up
        // sweep of the Scratch and Export Directories.
        .setup(|app| {
            // ffmpeg and ffprobe are looked for on PATH, then in the Tools
            // Directory; wtsexporter only there (#1053). Named before the
            // server is started, which is told it too.
            // What is missing there is downloaded in the background, and
            // nothing waits for it (`tool_downloads`).
            if let Some(home) = dirs::home_dir() {
                let tools = app_directories::use_tools_dir_in(&home);
                tool_downloads::start(tools, app.state::<ToolDownloads>().inner());
            }
            let app_data_dir = app.path().app_data_dir()?;
            let record = app_data_dir.join(run_directories::RECORD_FILE);
            app.manage(RunDirectories::at(
                record,
                dirs::home_dir(),
                app_directories::logs_dir_in(&app_data_dir),
            ));
            app.manage(ExportDirectories::in_app_data(&app_data_dir));
            std::thread::spawn(move || app_directories::sweep_at_start_up(&app_data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::extract::extract,
            commands::extract::cancel,
            commands::format::format,
            commands::paths::home_dir,
            commands::paths::path_stat,
            commands::paths::ios_backup_encrypted,
            commands::paths::imessage_backup_identities,
            commands::paths::open_path,
            commands::paths::save_file,
            commands::download::save_download,
            commands::local_server::start_local_server,
            commands::local_server::local_server_status,
            commands::local_server::set_open_to_network,
            commands::local_server::open_data_directory,
            commands::upload::upload,
            commands::export::export,
            commands::exports::export_directory,
            commands::exports::create_export_dir,
            commands::exports::finish_export_dir,
            commands::exports::discard_export_dir,
            commands::staging::staging_root,
            commands::staging::set_staging_root,
            commands::staging::create_run_dir,
            commands::staging::import_run_log,
            commands::run_logs::start_import_run_log,
            commands::run_logs::list_import_run_logs,
            commands::run_logs::read_import_run_log_lines,
            commands::run_logs::read_import_run_log,
            commands::staging::summarize_staging,
            commands::staging::transcode_staging,
            commands::staging::delete_run_dir,
            commands::staging::read_import_run_record,
            commands::staging::save_import_run_record,
            commands::tools::tools_status,
            commands::tools::retry_tool_downloads,
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
