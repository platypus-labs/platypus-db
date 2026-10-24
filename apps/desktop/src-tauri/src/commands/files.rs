//! Script files picked by the user in the native dialogs, and the clipboard.

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

/// The clipboard's text, for pasting rows. Read here: the web view would ask the user for
/// permission first.
#[tauri::command]
pub fn read_clipboard() -> CommandResult<String> {
    let unreadable = |e: arboard::Error| {
        CommandError::new("clipboard", format!("cannot read the clipboard: {e}"))
    };
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(unreadable)
}
