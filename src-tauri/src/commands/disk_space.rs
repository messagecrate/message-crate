//! `low_disk_space`: what the Import form says about the disks a run writes
//! to, before the run starts (#2010).
//!
//! A run writes to the Staging Directory and the Scratch Directory. Once a
//! disk fills part-way, little can be said about what the run leaves behind,
//! so the form says how much is free on a disk below
//! [`staging::LOW_DISK_SPACE_BYTES`] while the person can still free
//! some. It never stops the import.

use crate::run_directories::RunDirectories;

/// One sentence for each disk under the Staging Directory or the Scratch
/// Directory with less than [`staging::LOW_DISK_SPACE_BYTES`] free,
/// and none for one with more ([`staging::low_disk_space`]).
///
/// # Errors
///
/// Returns an error when the Staging Directory's record cannot be read.
#[tauri::command(async)]
pub fn low_disk_space(
    app: tauri::AppHandle,
    directories: tauri::State<'_, RunDirectories>,
) -> Result<Vec<String>, String> {
    let staging = directories.root()?;
    let scratch = super::paths::scratch_dir(&app);
    Ok(staging::low_disk_space(&staging, &scratch))
}
