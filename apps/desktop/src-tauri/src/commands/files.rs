//! Script files picked by the user in the native dialogs, and the clipboard.

use std::path::{Path, PathBuf};

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

/// Saves the clipboard's text (CSV or TSV, as a spreadsheet copies it) to a new temporary file
/// named `pasted.csv`, to import it like a file; returns its path. [`discard_pasted_file`]
/// deletes it again.
#[tauri::command]
pub fn clipboard_to_file() -> CommandResult<PathBuf> {
    let text = read_clipboard()?;
    if text.trim().is_empty() {
        return Err(CommandError::new(
            "clipboard",
            "the clipboard holds no text",
        ));
    }
    let io = |e: std::io::Error| CommandError::new("io", format!("cannot save the clipboard: {e}"));
    let dir = std::env::temp_dir().join(format!("{PASTE_PREFIX}{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(io)?;
    let path = dir.join("pasted.csv");
    std::fs::write(&path, text).map_err(io)?;
    Ok(path)
}

/// The folders of [`clipboard_to_file`] start with this.
const PASTE_PREFIX: &str = "platypus-paste-";

/// Deletes the folder of a file made by [`clipboard_to_file`]; any other path is left alone.
#[tauri::command]
pub fn discard_pasted_file(path: PathBuf) {
    if let Some(dir) = pasted_dir(&path, &std::env::temp_dir()) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The folder of `path` if [`clipboard_to_file`] made it in `temp`.
fn pasted_dir<'a>(path: &'a Path, temp: &Path) -> Option<&'a Path> {
    let dir = path.parent()?;
    let name = dir.file_name()?.to_str()?;
    (name.starts_with(PASTE_PREFIX) && dir.parent()? == temp && path.file_name()? == "pasted.csv")
        .then_some(dir)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::pasted_dir;

    #[test]
    fn only_pasted_files_are_discarded() {
        let temp = Path::new("/tmp");
        let pasted = Path::new("/tmp/platypus-paste-1/pasted.csv");
        assert_eq!(
            pasted_dir(pasted, temp),
            Some(Path::new("/tmp/platypus-paste-1"))
        );
        for other in [
            "/tmp/other/pasted.csv",
            "/home/platypus-paste-1/pasted.csv",
            "/tmp/platypus-paste-1/data.csv",
            "/tmp/platypus-paste-1/../x/pasted.csv",
        ] {
            assert_eq!(pasted_dir(Path::new(other), temp), None, "{other}");
        }
    }
}
