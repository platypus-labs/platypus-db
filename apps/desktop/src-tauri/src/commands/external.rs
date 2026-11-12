//! Opens a link of the help pages in the user's browser (or mail client).

use std::process::{Command, Stdio};

use crate::error::{CommandError, CommandResult};

/// Whether `url` is a web or mail address that is safe to hand to the system: only the schemes
/// the guide links to, and no whitespace or control characters.
fn is_openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| lower.starts_with(s))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The program and arguments that open `url` on this system; the address is an argument, never
/// part of a shell command line.
fn opener(url: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            "rundll32",
            vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
        )
    } else if cfg!(target_os = "macos") {
        ("open", vec![url.to_owned()])
    } else {
        ("xdg-open", vec![url.to_owned()])
    }
}

#[tauri::command]
pub fn open_external_url(url: String) -> CommandResult<()> {
    if !is_openable(&url) {
        return Err(CommandError::new(
            "invalid",
            format!("Not a web address: {url}"),
        ));
    }
    let (program, args) = opener(&url);
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CommandError::new("io", format!("Could not open {url}: {e}")))?;
    // Waited for elsewhere, or it stays a defunct process on macOS and Linux until the app exits.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_openable;

    #[test]
    fn only_web_and_mail_addresses_open() {
        assert!(is_openable(
            "https://github.com/platypus-db/platypus-db/issues"
        ));
        assert!(is_openable("HTTP://example.org/a?b=1&c=2"));
        assert!(is_openable("mailto:someone@example.org"));
        assert!(!is_openable("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_openable("javascript:alert(1)"));
        assert!(!is_openable("calc.exe"));
        assert!(!is_openable("https://example.org/a b"));
        assert!(!is_openable("https://example.org/\nnext"));
    }
}
