//! Script files picked by the user in the native dialogs, and the clipboard.

use std::path::{Path, PathBuf};

use crate::error::{CommandError, CommandResult};

/// Reads a script picked by the user in the open dialog, or named by `@include`, whatever
/// editor saved it: see [`decode_text`].
#[tauri::command]
pub fn read_text_file(path: PathBuf) -> CommandResult<String> {
    std::fs::read(&path)
        .map(|bytes| decode_text(&bytes))
        .map_err(|e| CommandError::new("io", format!("cannot read {}: {e}", path.display())))
}

/// The text of a script file: UTF-8 or UTF-16 as its byte order mark says (the mark itself
/// dropped, the server would read it as text), UTF-8 without one, UTF-16 without one when
/// every other byte is zero (ASCII text), else Windows-1252 (an ANSI file of Windows).
fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |bytes: &[u8], little: bool| {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| {
                if little {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let zeros = |offset: usize| {
        bytes
            .iter()
            .skip(offset)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let half = bytes.len() / 2;
    if bytes.len().is_multiple_of(2) && half > 0 {
        if zeros(1) * 10 >= half * 9 {
            return utf16(bytes, true);
        }
        if zeros(0) * 10 >= half * 9 {
            return utf16(bytes, false);
        }
    }
    bytes.iter().map(|&b| windows_1252(b)).collect()
}

/// The character of a Windows-1252 byte: Latin-1, but for the printable characters at
/// 0x80..0xA0.
fn windows_1252(byte: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
        0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];
    match byte {
        0x80..=0x9F => {
            char::from_u32(u32::from(HIGH[usize::from(byte - 0x80)])).unwrap_or('\u{FFFD}')
        }
        other => char::from(other),
    }
}

/// Writes a script to a path picked by the user in the save dialog.
#[tauri::command]
pub fn write_text_file(path: PathBuf, contents: String) -> CommandResult<()> {
    std::fs::write(&path, contents)
        .map_err(|e| CommandError::new("io", format!("cannot write {}: {e}", path.display())))
}

/// The files `write_binary_file` writes: images, by their extension.
const BINARY_EXTENSIONS: [&str; 1] = ["png"];

/// Writes an image to a path picked by the user in the save dialog, e.g. a chart as PNG. Only
/// image files: the web view cannot write bytes over a program or a configuration file.
#[tauri::command]
pub fn write_binary_file(path: PathBuf, bytes: Vec<u8>) -> CommandResult<()> {
    let image = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| BINARY_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
    if !image {
        return Err(CommandError::new(
            "io",
            format!("cannot write {}: not a PNG file", path.display()),
        ));
    }
    std::fs::write(&path, bytes)
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

    #[test]
    fn reads_scripts_in_any_common_encoding() {
        use super::decode_text;
        assert_eq!(decode_text(b"\xEF\xBB\xBFSELECT 1"), "SELECT 1");
        let little: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("SELECT 'é'".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&little), "SELECT 'é'");
        let big: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("SELECT 2".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode_text(&big), "SELECT 2");
        let no_bom: Vec<u8> = "SELECT 3"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_text(&no_bom), "SELECT 3");
        assert_eq!(decode_text(b"SELECT 'caf\xE9 \x80'"), "SELECT 'café €'");
        assert_eq!(decode_text("SELECT 'café'".as_bytes()), "SELECT 'café'");
    }

    #[test]
    fn writes_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chart.png");
        super::write_binary_file(path.clone(), vec![0x89, b'P', b'N', b'G']).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), [0x89, b'P', b'N', b'G']);
        let upper = dir.path().join("CHART.PNG");
        assert!(super::write_binary_file(upper, vec![1]).is_ok());
    }

    #[test]
    fn writes_bytes_only_to_images() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["run.exe", "config.toml", "chart", "chart.png.bat"] {
            let path = dir.path().join(name);
            assert!(
                super::write_binary_file(path.clone(), vec![1]).is_err(),
                "{name}"
            );
            assert!(!path.exists(), "{name}");
        }
    }
}
