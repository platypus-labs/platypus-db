//! Windows beside the main one: a tab moved to a window of its own (`tab-<n>`), and the quick
//! query window (`quick`) the tray icon opens. Each loads the same app, which shows a single
//! tab when its window is not `main`; a moved tab waits here until its window takes it.

// Tauri hands commands their arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WebviewWindowBuilder};

use crate::error::{CommandError, CommandResult};

/// The tabs waiting for their window, by its label, as the web view saved them (JSON).
#[derive(Default)]
pub struct Detached {
    tabs: Mutex<HashMap<String, String>>,
    next: AtomicU64,
}

const TRAY: &str = "main";
pub const QUICK: &str = "quick";

/// The main window's settings, for another window of the app: the same browser arguments and
/// data folder (WebView2 shares one environment), with `label`, `title` and `size`.
pub fn window_config(app: &AppHandle, label: &str) -> Option<tauri::utils::config::WindowConfig> {
    let mut config = app.config().app.windows.first().cloned()?;
    label.clone_into(&mut config.label);
    if let Some(port) = std::env::var("PLATYPUS_DEVTOOLS_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    {
        // Unauthenticated, so on the loopback only (Chromium's default, said explicitly): an
        // accepted risk for the end-to-end tests, see docs/DEVELOPMENT.md.
        let args = config.additional_browser_args.unwrap_or_default();
        config.additional_browser_args = Some(format!(
            "{args} --remote-debugging-address=127.0.0.1 --remote-debugging-port={port}"
        ));
    }
    Some(config)
}

/// Builds the window `config` describes, with the web view's data in the portable folder when
/// the app runs from one (and no `PLATYPUS_DATA_DIR` is set).
pub fn build(app: &AppHandle, config: &tauri::utils::config::WindowConfig) -> tauri::Result<()> {
    let mut window = WebviewWindowBuilder::from_config(app, config)?;
    if std::env::var_os("PLATYPUS_DATA_DIR").is_none()
        && let Some(dir) = platypus_mcp::profile::portable_dir()
    {
        window = window.data_directory(dir.join("webview"));
    }
    let window = window.build()?;
    #[cfg(windows)]
    shut_web_messages(&window)?;
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
}

/// Leaves a window one route to the app's commands, Tauri's `fetch` to `http://ipc.localhost`, which
/// refuses a sandboxed frame by its origin. WebView2 runs Tauri's scripts in every frame, the invoke
/// key included, so a plugin's page holds the key and `chrome.webview.postMessage`; what stood
/// between them and a command was WebView2 alone, which keeps a frame's message from the window's
/// handler. Nothing in the app uses this route: Tauri falls back to it only when its `fetch` fails.
#[cfg(windows)]
fn shut_web_messages(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| {
        #[allow(unsafe_code)]
        // SAFETY: on the main thread, where `with_webview` runs, on the controller it hands over.
        let shut = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.SetIsWebMessageEnabled(false))
        };
        if let Err(e) = shut {
            crate::applog::write("ERROR", &format!("the web message route stays open: {e}"));
        }
    })
}

/// Opens another window of the app, with the system's title bar.
fn open(
    app: &AppHandle,
    label: &str,
    title: &str,
    (width, height): (f64, f64),
    on_top: bool,
) -> CommandResult<()> {
    let Some(mut config) = window_config(app, label) else {
        return Err(CommandError::new("window", "no window is configured"));
    };
    title.clone_into(&mut config.title);
    config.width = width;
    config.height = height;
    config.min_width = Some(480.0);
    config.min_height = Some(320.0);
    config.decorations = true;
    config.always_on_top = on_top;
    build(app, &config).map_err(|e| CommandError::new("window", e.to_string()))
}

/// Opens a window for `tab` (saved by the web view as JSON), titled `title`; its label.
/// Asynchronous, as every command that makes a window: on Windows a synchronous one waits on
/// WebView2 forever.
#[tauri::command]
pub async fn open_tab_window(
    app: AppHandle,
    detached: State<'_, Detached>,
    tab: String,
    title: String,
) -> CommandResult<String> {
    let label = format!("tab-{}", detached.next.fetch_add(1, Ordering::Relaxed) + 1);
    lock(&detached.tabs).insert(label.clone(), tab);
    if let Err(e) = open(&app, &label, &title, (960.0, 680.0), false) {
        lock(&detached.tabs).remove(&label);
        return Err(e);
    }
    Ok(label)
}

/// The tab that waits for the window `label`, once.
#[tauri::command]
pub fn take_window_tab(detached: State<'_, Detached>, label: String) -> Option<String> {
    lock(&detached.tabs).remove(&label)
}

/// Shows the quick query window, opening it if it is not open.
pub fn open_quick(app: &AppHandle, title: &str) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window(QUICK) {
        reveal(&window);
        return Ok(());
    }
    open(app, QUICK, title, (760.0, 480.0), true)
}

#[tauri::command]
pub async fn open_quick_window(app: AppHandle, title: String) -> CommandResult<()> {
    open_quick(&app, &title)
}

/// Opens the quick query from an event handler, which must not make the window itself (it
/// would wait on WebView2 forever on Windows).
fn open_quick_later(app: &AppHandle, title: &str) {
    let (app, title) = (app.clone(), title.to_owned());
    tauri::async_runtime::spawn(async move {
        let _ = open_quick(&app, &title);
    });
}

/// Whether the system has a tray: on Linux, an `AppIndicator` library to load, without which
/// the tray crate panics (and a release build, which aborts on a panic, would end).
fn has_tray() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let names = ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("LD_LIBRARY_PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for dir in [
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/local/lib",
        "/app/lib",
    ] {
        dirs.push(dir.into());
    }
    let multiarch = format!("{}-linux-gnu", std::env::consts::ARCH);
    dirs.push(std::path::Path::new("/usr/lib").join(&multiarch));
    dirs.push(std::path::Path::new("/lib").join(multiarch));
    dirs.iter()
        .any(|dir| names.iter().any(|name| dir.join(name).exists()))
}

/// Shows the tray icon with the menu `labels` (quick query, show, quit, then the quick
/// window's title), or with `None` removes it. A system without a tray (Linux without
/// `AppIndicator`) is an error, not a crash.
#[tauri::command]
pub fn set_tray(app: AppHandle, labels: Option<[String; 4]>) -> CommandResult<()> {
    let _ = app.remove_tray_by_id(TRAY);
    let Some(labels) = labels else {
        return Ok(());
    };
    if !has_tray() {
        return Err(CommandError::new(
            "tray",
            "this system has no tray for the icon (install AppIndicator)",
        ));
    }
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tray(&app, &labels)));
    match built {
        Ok(result) => result
            .map(|_| ())
            .map_err(|e| CommandError::new("tray", e.to_string())),
        Err(_) => Err(CommandError::new(
            "tray",
            "this system has no tray for the icon",
        )),
    }
}

/// `labels`: the menu's quick query, show and quit, then the quick window's title.
fn tray(app: &AppHandle, labels: &[String; 4]) -> tauri::Result<TrayIcon> {
    let quick = MenuItem::with_id(app, "quick", &labels[0], true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", &labels[1], true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &labels[2], true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quick, &show, &quit])?;
    let title = labels[3].clone();
    let mut builder = TrayIconBuilder::with_id(TRAY)
        .tooltip("PlatypusDB")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(
            move |app: &AppHandle, event: MenuEvent| match event.id().as_ref() {
                "quick" => open_quick_later(app, &title),
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            },
        );
    let quick_title = labels[3].clone();
    builder = builder.on_tray_icon_event(move |icon, event| {
        // A click on the icon opens the quick query; the menu has the rest.
        if let TrayIconEvent::Click {
            button: tauri::tray::MouseButton::Left,
            button_state: tauri::tray::MouseButtonState::Up,
            ..
        } = event
        {
            open_quick_later(icon.app_handle(), &quick_title);
        }
    });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        reveal(&window);
    }
}

/// Brings `window` to the front, minimized or hidden as it may be.
fn reveal(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
