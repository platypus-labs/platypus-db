//! The main window's place and size, kept in the data folder when it closes and put back at
//! the next start. A screen that is gone (a disconnected monitor, another resolution) must not
//! leave the window out of reach: it opens centred on the primary screen instead.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{Monitor, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

const FILE: &str = "window-state.json";

/// The size of a window that has none saved (the one in `tauri.conf.json`), and the least
/// the fallback gives.
const DEFAULT_SIZE: (u32, u32) = (1280, 800);
const MIN_SIZE: (u32, u32) = (800, 500);

/// How much of the window must lie on a screen for it to count as there: enough to grab.
const MIN_VISIBLE: (i64, i64) = (100, 50);

/// A rectangle in physical pixels: the window's outer position and inner size, or a screen's
/// usable area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What is saved: the frame the window has when it is neither maximized nor minimized, and
/// whether it is maximized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(flatten)]
    pub frame: Frame,
    #[serde(default)]
    pub maximized: bool,
}

impl Frame {
    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    /// Whether enough of `self` is inside `screen` to reach the window.
    fn visible_on(&self, screen: &Self) -> bool {
        let across = self.right().min(screen.right()) - i64::from(self.x.max(screen.x));
        let down = self.bottom().min(screen.bottom()) - i64::from(self.y.max(screen.y));
        across >= MIN_VISIBLE.0.min(i64::from(self.width))
            && down >= MIN_VISIBLE.1.min(i64::from(self.height))
    }

    /// `self` no larger than `screen`, and moved inside it.
    fn fitted(&self, screen: &Self) -> Self {
        let width = self.width.min(screen.width);
        let height = self.height.min(screen.height);
        let room = |total: u32, used: u32| i32::try_from(total - used).unwrap_or(i32::MAX);
        Self {
            x: self
                .x
                .clamp(screen.x, screen.x.saturating_add(room(screen.width, width))),
            y: self.y.clamp(
                screen.y,
                screen.y.saturating_add(room(screen.height, height)),
            ),
            width,
            height,
        }
    }

    /// A frame of `size` (at least [`MIN_SIZE`], at most `self`) in the middle of `self`.
    fn centred(&self, size: (u32, u32)) -> Self {
        let width = size.0.max(MIN_SIZE.0).min(self.width);
        let height = size.1.max(MIN_SIZE.1).min(self.height);
        let middle = |from: i32, total: u32, used: u32| {
            from.saturating_add(i32::try_from((total - used) / 2).unwrap_or(0))
        };
        Self {
            x: middle(self.x, self.width, width),
            y: middle(self.y, self.height, height),
            width,
            height,
        }
    }
}

/// Where the window opens: the saved place when enough of it is on a screen that exists
/// (moved inside it, and no larger than it), else the middle of the `primary` screen, with the
/// saved size or the default one.
fn place(saved: Option<WindowState>, screens: &[Frame], primary: &Frame) -> WindowState {
    if let Some(saved) = saved
        && let Some(screen) = screens.iter().find(|s| saved.frame.visible_on(s))
    {
        return WindowState {
            frame: saved.frame.fitted(screen),
            ..saved
        };
    }
    let size = saved.map_or(DEFAULT_SIZE, |s| (s.frame.width, s.frame.height));
    WindowState {
        frame: primary.centred(size),
        maximized: saved.is_some_and(|s| s.maximized),
    }
}

fn frame_of(monitor: &Monitor) -> Frame {
    let area = monitor.work_area();
    Frame {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

/// The screens' usable areas and the primary one's; `None` when the system reports none.
fn screens(window: &WebviewWindow) -> Option<(Vec<Frame>, Frame)> {
    let screens: Vec<Frame> = window
        .available_monitors()
        .ok()?
        .iter()
        .map(frame_of)
        .collect();
    let primary = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| frame_of(&m))
        .or_else(|| screens.first().copied())?;
    Some((screens, primary))
}

/// The saved state in `path`; `None` when there is none or it cannot be read.
pub fn load(path: &Path) -> Option<WindowState> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn save(path: &Path, state: &WindowState) {
    // Losing the place of a window is not worth an error.
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(path, bytes);
    }
}

/// The file of the saved state, in the data folder `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// What `window` is now; `None` while it is minimized or fullscreen, when its frame is not
/// the one to keep. A maximized window keeps the frame it had, which is `previous`.
fn current(window: &WebviewWindow, previous: Option<WindowState>) -> Option<WindowState> {
    if window.is_minimized().ok()? || window.is_fullscreen().ok()? {
        return None;
    }
    let maximized = window.is_maximized().ok()?;
    let frame = match previous {
        Some(previous) if maximized => previous.frame,
        _ => {
            let (position, size) = (window.outer_position().ok()?, window.inner_size().ok()?);
            Frame {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            }
        }
    };
    Some(WindowState { frame, maximized })
}

/// Puts the hidden main `window` where it was left (or in the middle of the primary screen),
/// shows it, and keeps its state in `path` when it closes.
pub fn restore_and_track(window: &WebviewWindow, path: &Path, saved: Option<WindowState>) {
    let placed = screens(window).map(|(screens, primary)| place(saved, &screens, &primary));
    if let Some(state) = &placed {
        let Frame {
            x,
            y,
            width,
            height,
        } = state.frame;
        let _ = window.set_size(PhysicalSize::new(width, height));
        let _ = window.set_position(PhysicalPosition::new(x, y));
        if state.maximized {
            let _ = window.maximize();
        }
    }
    let _ = window.show();

    let kept = Arc::new(Mutex::new(placed.or_else(|| current(window, None))));
    let (watched, path) = (window.clone(), path.to_owned());
    window.on_window_event(move |event| {
        let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(now) = current(&watched, *kept) {
                    *kept = Some(now);
                }
            }
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
                if let Some(now) = current(&watched, *kept).or(*kept) {
                    save(&path, &now);
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn frame(x: i32, y: i32, width: u32, height: u32) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    const fn state(frame: Frame, maximized: bool) -> WindowState {
        WindowState { frame, maximized }
    }

    const PRIMARY: Frame = frame(0, 0, 1920, 1040);
    const RIGHT: Frame = frame(1920, 0, 2560, 1400);

    #[test]
    fn keeps_a_place_that_is_on_a_screen() {
        let saved = state(frame(2100, 100, 1200, 800), false);
        assert_eq!(place(Some(saved), &[PRIMARY, RIGHT], &PRIMARY), saved);
    }

    #[test]
    fn a_gone_screen_sends_the_window_to_the_middle_of_the_primary_one() {
        let saved = state(frame(2100, 100, 1200, 800), true);
        let placed = place(Some(saved), &[PRIMARY], &PRIMARY);
        assert_eq!(placed.frame, frame(360, 120, 1200, 800));
        assert!(placed.maximized);
    }

    #[test]
    fn a_window_with_only_its_edge_on_a_screen_counts_as_gone() {
        let saved = state(frame(1890, 30, 1200, 800), false);
        assert_eq!(
            place(Some(saved), &[PRIMARY], &PRIMARY).frame,
            frame(360, 120, 1200, 800)
        );
    }

    #[test]
    fn a_window_bigger_than_the_screen_is_cut_and_moved_inside() {
        let saved = state(frame(-300, -50, 3000, 2000), false);
        assert_eq!(
            place(Some(saved), &[PRIMARY], &PRIMARY).frame,
            frame(0, 0, 1920, 1040)
        );
        let far = state(frame(1500, 900, 1000, 600), false);
        assert_eq!(
            place(Some(far), &[PRIMARY], &PRIMARY).frame,
            frame(920, 440, 1000, 600)
        );
    }

    #[test]
    fn the_first_start_and_a_small_screen_get_a_sensible_size() {
        assert_eq!(
            place(None, &[PRIMARY], &PRIMARY),
            state(frame(320, 120, 1280, 800), false)
        );
        let small = frame(0, 0, 1024, 728);
        assert_eq!(place(None, &[small], &small).frame, frame(0, 0, 1024, 728));
        let tiny_saved = state(frame(5000, 0, 200, 100), false);
        assert_eq!(
            place(Some(tiny_saved), &[PRIMARY], &PRIMARY).frame,
            frame(560, 270, 800, 500)
        );
    }

    #[test]
    fn the_state_survives_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = path(dir.path());
        assert_eq!(load(&file), None);
        let saved = state(frame(-1200, 40, 1100, 700), true);
        save(&file, &saved);
        assert_eq!(load(&file), Some(saved));
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(load(&file), None);
    }
}
