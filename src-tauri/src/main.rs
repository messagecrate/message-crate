//! Desktop process that hosts the Message Crate window.
//!
//! The app is the `message_crate_desktop_lib` library (`lib.rs`); this binary
//! only starts it.

// On Windows release builds, hide the extra console window so only the app
// window appears.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Calls [`message_crate_desktop_lib::run`].
fn main() {
    message_crate_desktop_lib::run();
}
