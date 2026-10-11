//! Desktop process that hosts the Message Crate window.
//!
//! The app is the `message_crate_desktop_lib` library (`lib.rs`); this binary
//! only starts it.

// On Windows release builds, hide the extra console window so only the app
// window appears.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Start the desktop window and wait until the user quits.
fn main() {
    message_crate_desktop_lib::run();
}
