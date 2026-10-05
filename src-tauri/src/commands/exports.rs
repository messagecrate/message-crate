//! The Export Directory: where each Export and Convert gets a directory of
//! its own ([`crate::export_directories`]).
//!
//! The window makes the directory before the run starts, writes into it, and
//! then either finishes it, leaving only the result, or discards it.

use crate::export_directories::{ExportDir, ExportDirectories, ExportKind, started_now};

/// The Export Directory, for Settings, made when it does not exist yet so
/// Settings can open it.
///
/// # Errors
///
/// Returns an error when the directory cannot be made.
#[tauri::command]
pub fn export_directory(exports: tauri::State<'_, ExportDirectories>) -> Result<String, String> {
    let root = exports.root();
    std::fs::create_dir_all(root)
        .map_err(|error| format!("Could not make {}: {error}", root.display()))?;
    Ok(root.display().to_string())
}

/// Make the directory of a new Export or Convert in the Export Directory.
/// `format` is the output format's id; `chosen` is the destination the person
/// chose, if any.
///
/// # Errors
///
/// Returns an error when `format` cannot name the directory, `chosen` holds
/// the Export Directory, or the directory cannot be made.
#[tauri::command]
pub fn create_export_dir(
    exports: tauri::State<'_, ExportDirectories>,
    kind: ExportKind,
    format: String,
    chosen: Option<String>,
) -> Result<ExportDir, String> {
    exports.create(kind, &format, &started_now(), chosen.as_deref())
}

/// Finish an Export's or Convert's directory after the run succeeded, leaving
/// only the result. Returns the directory, or `None` when the result went to
/// another destination and the directory was deleted.
///
/// # Errors
///
/// Returns an error when `dir` is not an Export's or Convert's directory, or
/// its in-between files cannot be deleted.
#[tauri::command]
pub fn finish_export_dir(
    exports: tauri::State<'_, ExportDirectories>,
    dir: String,
) -> Result<Option<String>, String> {
    Ok(exports
        .finish(&dir)?
        .map(|finished| finished.display().to_string()))
}

/// Delete an Export's or Convert's directory after the run failed or was
/// cancelled.
///
/// # Errors
///
/// Returns an error when `dir` is not an Export's or Convert's directory, or
/// cannot be deleted.
#[tauri::command]
pub fn discard_export_dir(
    exports: tauri::State<'_, ExportDirectories>,
    dir: String,
) -> Result<(), String> {
    exports.discard(&dir)
}
