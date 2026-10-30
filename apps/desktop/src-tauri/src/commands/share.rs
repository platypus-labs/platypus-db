//! Shared queries reaching the app from outside: a `.platypus` file given as a command line
//! argument or opened from the file manager, and a `platypus://query?d=…` link opened from a
//! browser or a chat. This module only finds them in the arguments the OS passes and holds
//! them until the web UI asks; reading and checking their content is the UI's (`src/share`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

/// The event telling the web UI that [`take_pending_shares`] has something.
pub const SHARE_EVENT: &str = "share-open";

const LINK_PREFIX: &str = "platypus://";
const FILE_EXTENSION: &str = "platypus";

/// The shared queries waiting for the web UI, each a link or a file path. They wait here,
/// not in the event, because a second launch can come before the page has loaded.
#[derive(Default)]
pub struct PendingShares(Mutex<Vec<String>>);

impl PendingShares {
    fn push(&self, items: Vec<String>) {
        if let Ok(mut pending) = self.0.lock() {
            pending.extend(items);
        }
    }

    fn take(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }
}

/// The arguments that are a shared query: a `platypus://` link as it is, a `.platypus` file
/// as an absolute path (a relative one is taken from `cwd`, the folder the app was started
/// from, which is not this process's when a second launch hands its arguments over).
pub fn shared_args<I>(args: I, cwd: Option<&Path>) -> Vec<String>
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter()
        .filter_map(|arg| {
            let text = arg.to_string_lossy();
            if text
                .get(..LINK_PREFIX.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(LINK_PREFIX))
            {
                return Some(text.into_owned());
            }
            let path = PathBuf::from(&arg);
            let is_file = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(FILE_EXTENSION));
            if !is_file {
                return None;
            }
            let full = match cwd {
                Some(cwd) if path.is_relative() => cwd.join(path),
                _ => path,
            };
            Some(full.to_string_lossy().into_owned())
        })
        .collect()
}

/// Queues what `items` hold and wakes the web UI; nothing when `items` is empty.
pub fn offer(app: &AppHandle, items: Vec<String>) {
    if items.is_empty() {
        return;
    }
    if let Some(pending) = app.try_state::<PendingShares>() {
        pending.push(items);
    }
    // A failed emit leaves the items queued: the page takes them when it starts.
    let _ = app.emit(SHARE_EVENT, ());
}

/// Brings the main window forward, for a second launch that came with a shared query.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Queues the shared queries of this process's own arguments (the app was started with a
/// file or a link).
pub fn offer_launch(app: &AppHandle) {
    let cwd = std::env::current_dir().ok();
    offer(
        app,
        shared_args(std::env::args_os().skip(1), cwd.as_deref()),
    );
}

/// The links and file paths queued since the last call, oldest first.
#[tauri::command]
pub fn take_pending_shares(pending: State<PendingShares>) -> Vec<String> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn finds_links_and_files() {
        let found = shared_args(
            args(&[
                "--flag",
                "platypus://query?d=AAA",
                "/tmp/q.platypus",
                "x.sql",
            ]),
            None,
        );
        assert_eq!(found, ["platypus://query?d=AAA", "/tmp/q.platypus"]);
    }

    #[test]
    fn ignores_everything_else() {
        let other = args(&[
            "--mcp",
            "data.csv",
            "https://x.dev/a.platypus.html",
            "platypus",
        ]);
        assert_eq!(shared_args(other, None), Vec::<String>::new());
        assert_eq!(
            shared_args(args(&["platypus:/query", ".platypus"]), None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn matches_case_insensitively() {
        let found = shared_args(args(&["PLATYPUS://query?d=Z", "/tmp/Q.PLATYPUS"]), None);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn relative_paths_come_from_the_launch_folder() {
        let cwd = Path::new("/home/me/work");
        let found = shared_args(args(&["q.platypus", "/abs/r.platypus"]), Some(cwd));
        let joined = cwd.join("q.platypus").to_string_lossy().into_owned();
        assert_eq!(found, [joined.as_str(), "/abs/r.platypus"]);
        // Without a folder the path stays as given.
        assert_eq!(shared_args(args(&["q.platypus"]), None), ["q.platypus"]);
    }

    #[test]
    fn a_link_is_kept_whole() {
        let link = "platypus://query?d=AbC-_9&x=1#frag";
        assert_eq!(shared_args(args(&[link]), Some(Path::new("/x"))), [link]);
    }

    #[test]
    fn pending_shares_are_taken_once_in_order() {
        let pending = PendingShares::default();
        pending.push(vec!["a".into()]);
        pending.push(vec!["b".into(), "c".into()]);
        assert_eq!(pending.take(), ["a", "b", "c"]);
        assert_eq!(pending.take(), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_non_unicode_argument_does_not_stop_the_scan() {
        use std::os::unix::ffi::OsStringExt;
        let mut name = vec![0xff, 0xfe];
        name.extend_from_slice(b".platypus");
        assert_eq!(shared_args(vec![OsString::from_vec(name)], None).len(), 1);
    }
}
