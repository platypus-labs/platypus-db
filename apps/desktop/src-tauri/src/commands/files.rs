//! Script files picked by the user in the native dialogs.

use std::path::PathBuf;

use crate::error::{CommandError, CommandResult};

/// Reads a script picked by the user in the open dialog.
#[tauri::command]
pub fn read_text_file(path: PathBuf) -> CommandResult<String> {
    std::fs::read_to_string(&path)
        .map_err(|e| CommandError::new("io", format!("cannot read {}: {e}", path.display())))
}

/// Writes a script to a path picked by the user in the save dialog.
#[tauri::command]
pub fn write_text_file(path: PathBuf, contents: String) -> CommandResult<()> {
    std::fs::write(&path, contents)
        .map_err(|e| CommandError::new("io", format!("cannot write {}: {e}", path.display())))
}
