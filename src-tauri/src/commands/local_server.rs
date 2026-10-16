//! Commands for the Message Crate this app starts for itself
//! (`crate::local_server`).

use crate::app_directories::MessageCrateDirectory;
use crate::local_server::{self, Launch, LocalServer, Status};
use tauri::{AppHandle, Manager, State};

/// How the app's own server is getting on while it is started again after
/// it stopped. Payload: [`local_server::Notice`]. The window shows it across
/// the top of the page (`web/src/components/LocalServerNotice.tsx`).
pub const NOTICE: &str = "local-server:notice";

/// Send each [`local_server::Notice`] to the window as [`NOTICE`].
pub fn send_notices(app: &AppHandle) {
    let handle = app.clone();
    app.state::<LocalServer>().on_notice(move |notice| {
        super::events::emit(&handle, NOTICE, notice);
    });
}

/// The directory inside the installed app that holds the built website. It
/// matches the `bundle.resources` target in `tauri.conf.json`.
const WEBSITE_RESOURCE: &str = "website";

/// The origin `cargo tauri dev` loads the screens from, which the server has
/// to be told to allow. An installed app loads them from its own origin,
/// which every server allows, and names nothing here.
fn dev_origins(app: &AppHandle) -> Vec<String> {
    if !tauri::is_dev() {
        return Vec::new();
    }
    app.config()
        .build
        .dev_url
        .iter()
        .map(|url| url.origin().ascii_serialization())
        .collect()
}

/// Everything the server is started with.
fn launch(app: &AppHandle, open_to_network: bool) -> Result<Launch, String> {
    let static_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Could not find the app's resources: {e}"))?
        .join(WEBSITE_RESOURCE);
    Ok(Launch {
        program: local_server::locate_server()?,
        data_dir: app.state::<MessageCrateDirectory>().data(),
        static_dir,
        address: local_server::OWN_ADDRESS
            .parse()
            .map_err(|e| format!("{}: {e}", local_server::OWN_ADDRESS))?,
        open_to_network,
        cors_origins: dev_origins(app),
        tools_dir: app.state::<MessageCrateDirectory>().tools(),
    })
}

/// Make sure the app's own Message Crate is running, and report its state.
/// The screens call this when the server address is the app's own; a start
/// already under way, or the app's own ready server, is left alone, and a
/// Message Crate the app found is asked again whether it still answers.
/// `open_to_network` is the person's setting; the app's own server is
/// restarted when it was started the other way.
///
/// # Errors
///
/// Returns an error when the app cannot work out what to start: its
/// resources or the server program is missing.
#[tauri::command]
pub fn start_local_server(
    app: AppHandle,
    server: State<'_, LocalServer>,
    open_to_network: bool,
) -> Result<Status, String> {
    server.ensure_started(launch(&app, open_to_network)?);
    Ok(server.status())
}

/// Take the person's "Let other devices on this network connect" setting.
/// It starts nothing: the app's own server is restarted to match it, and
/// while a desktop job runs only once the job has ended. A Message Crate the
/// app only found is not changed.
#[tauri::command]
pub fn set_open_to_network(server: State<'_, LocalServer>, open_to_network: bool) -> Status {
    server.set_open_to_network(open_to_network)
}

/// Report the state of the app's own Message Crate without starting it.
#[tauri::command]
pub fn local_server_status(server: State<'_, LocalServer>) -> Status {
    server.status()
}

/// Open the Message Crate Directory in the file manager, making it first, in
/// case it went since the app started, so the button always opens something.
///
/// # Errors
///
/// Returns an error when the directory cannot be made or opened.
#[tauri::command]
pub fn open_message_crate_directory(
    directory: State<'_, MessageCrateDirectory>,
) -> Result<(), String> {
    let dir = directory.root();
    file_io::create_dir_all_owner_only(dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    open::that_detached(dir).map_err(|e| format!("Could not open {}: {e}", dir.display()))
}
