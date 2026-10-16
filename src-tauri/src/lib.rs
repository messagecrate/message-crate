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
mod app_log;
mod commands;
mod export_directories;
mod local_server;
mod run_directories;
mod run_logs;
mod state;
mod tool_downloads;

use app_directories::MessageCrateDirectory;
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
        // The Message Crate Directory and everything in it: the Tools
        // Directory and its downloads, the Staging Directory and the run
        // directories made under it, the Export Directory, and the start-up
        // sweep of the Scratch and Export Directories.
        .setup(|app| {
            let home = dirs::home_dir()
                .ok_or("The operating system reports no home directory for this user")?;
            let directory = MessageCrateDirectory::in_home(&home, tauri::is_dev());
            // A directory that cannot be made now is made again by whatever
            // writes into it first, which reports the failure where it
            // matters. With no Message Crate Directory there is no Logs
            // Directory to write this line in, so it goes to standard error.
            if let Err(error) = directory.make() {
                eprintln!("Could not make the Message Crate Directory: {error}");
            }
            // Every line the app writes outside an Import Run goes to its
            // own log in the Logs Directory from here on (#2185).
            app_log::init(&directory.logs());
            // ffmpeg and ffprobe are looked for on PATH, then in the Tools
            // Directory; wtsexporter only there (#1053). Every command that
            // runs them, and the server the app starts, gets them from the
            // `InstalledTools` kept here.
            // What is missing there is downloaded in the background, and
            // nothing waits for it (`tool_downloads`).
            let tools_dir = directory.tools();
            let installed = media::InstalledTools::new(Some(tools_dir.clone()));
            tool_downloads::start(
                tools_dir,
                installed.clone(),
                app.state::<ToolDownloads>().inner(),
            );
            app.manage(installed);
            // The record of run directories is app state, not data, so it
            // stays in the app-data directory.
            let record = app
                .path()
                .app_data_dir()?
                .join(run_directories::RECORD_FILE);
            app.manage(RunDirectories::at(
                record,
                directory.staging(),
                directory.logs(),
            ));
            app.manage(ExportDirectories::at(directory.exports()));
            app.manage(directory.clone());
            std::thread::spawn(move || app_directories::sweep_at_start_up(&directory));
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
            commands::local_server::open_message_crate_directory,
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
            commands::run_logs::list_logs,
            commands::run_logs::read_log_lines,
            commands::run_logs::read_log,
            commands::staging::summarize_staging,
            commands::staging::transcode_staging,
            commands::staging::delete_run_dir,
            commands::staging::list_account_run_dirs,
            commands::staging::delete_closed_run_dir,
            commands::staging::account_staging_bytes,
            commands::staging::all_staging_bytes,
            commands::staging::read_import_run_record,
            commands::staging::save_import_run_record,
            commands::disk_space::low_disk_space,
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
