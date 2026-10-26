//! The app's log: `<data>/logs/platypus.log`, one line per event (start, errors of the web
//! view, panics), rotated at [`MAX_SIZE`] with [`KEPT`] older files. A panic also writes
//! `crash.txt`, which the next start offers to report: nothing leaves the computer unless the
//! user saves the diagnostic package and sends it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The log's size before it rotates.
const MAX_SIZE: u64 = 1 << 20;
/// The older logs kept: `platypus.1.log` (newest) to `platypus.<KEPT>.log`.
const KEPT: usize = 2;
const LOG: &str = "platypus.log";
const CRASH: &str = "crash.txt";

static DIR: OnceLock<PathBuf> = OnceLock::new();
/// One writer at a time, so lines and rotations do not interleave.
static WRITING: Mutex<()> = Mutex::new(());

/// The folder of the logs, once [`init`] has run.
pub fn dir() -> Option<&'static Path> {
    DIR.get().map(PathBuf::as_path)
}

/// Starts the log under `data_dir`, with the app's version and system.
pub fn init(data_dir: &Path) {
    let dir = data_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_ok() && DIR.set(dir).is_ok() {
        write(
            "INFO",
            &format!(
                "PlatypusDB {} started ({} {})",
                platypus_core::VERSION,
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
    }
}

/// Appends a line; before [`init`], or when the file cannot be written, it is lost.
pub fn write(level: &str, message: &str) {
    if let Some(dir) = dir() {
        let _writing = WRITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = append(dir, &line(level, message));
    }
}

fn line(level: &str, message: &str) -> String {
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // One event per line: a message's own line breaks are kept visible.
    format!(
        "{at} {level} {}\n",
        message.trim_end().replace('\n', "\n    ")
    )
}

fn append(dir: &Path, line: &str) -> std::io::Result<()> {
    let path = dir.join(LOG);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() + line.len() as u64 > MAX_SIZE) {
        rotate(dir);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

fn rotated(n: usize) -> String {
    format!("platypus.{n}.log")
}

/// `platypus.log` becomes `platypus.1.log`, each older one moves up, the oldest is dropped.
fn rotate(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(rotated(KEPT)));
    for n in (1..KEPT).rev() {
        let _ = std::fs::rename(dir.join(rotated(n)), dir.join(rotated(n + 1)));
    }
    let _ = std::fs::rename(dir.join(LOG), dir.join(rotated(1)));
}

/// Writes panics to the log and to `crash.txt` before the default hook runs (the release
/// build then aborts).
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = format!(
            "PlatypusDB {} ({} {}) panicked on thread {}: {info}\n\n{}",
            platypus_core::VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH,
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        write("PANIC", &report);
        if let Some(dir) = dir() {
            let _ = std::fs::write(dir.join(CRASH), &report);
        }
        default(info);
    }));
}

/// The report of the crash that ended the last run, until [`dismiss_crash`].
pub fn crash() -> Option<String> {
    std::fs::read_to_string(dir()?.join(CRASH)).ok()
}

pub fn dismiss_crash() {
    if let Some(dir) = dir() {
        let _ = std::fs::remove_file(dir.join(CRASH));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        // Three lines fit in a file.
        let big = "x".repeat(usize::try_from(MAX_SIZE).unwrap() / 4);
        for n in 0..8 {
            append(dir.path(), &format!("{n} {big}\n")).unwrap();
        }
        let first = |name: &str| {
            std::fs::read_to_string(dir.path().join(name))
                .unwrap()
                .chars()
                .next()
        };
        assert_eq!(first(LOG), Some('6'));
        assert_eq!(first(&rotated(1)), Some('3'));
        assert_eq!(first(&rotated(2)), Some('0'));
        assert!(!dir.path().join(rotated(3)).exists());
        assert!(std::fs::metadata(dir.path().join(LOG)).unwrap().len() <= MAX_SIZE);
    }

    #[test]
    fn one_event_per_line() {
        let text = line("ERROR", "first\nsecond\n");
        assert!(text.ends_with(" ERROR first\n    second\n"), "{text}");
        assert_eq!(text.lines().count(), 2);
    }
}
