//! Safe and reversible test of Tauri's always-on-bottom window level.
//!
//! This is *not* attachment to undocumented Explorer WorkerW windows.
//! Some Windows configurations can place an always-on-bottom window behind
//! the desktop icons, rendering it inaccessible. Never persist the mode and
//! revert automatically after 20 seconds unless confirmed by the user.

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{State, WebviewWindow};

const TRIAL_SECONDS: u64 = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Normal,
    Trial,
    Bottom,
}

#[derive(Default)]
struct Inner {
    mode: Mode,
    revision: u64,
}

#[derive(Default)]
pub struct DesktopModeState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DesktopModeStatus {
    pub mode: &'static str,
    pub trial_seconds: u64,
    pub genuine_desktop_host: bool,
}

fn describe(mode: Mode) -> DesktopModeStatus {
    DesktopModeStatus {
        mode: match mode {
            Mode::Normal => "normal",
            Mode::Trial => "trial",
            Mode::Bottom => "bottom",
        },
        trial_seconds: TRIAL_SECONDS,
        genuine_desktop_host: false,
    }
}

fn lock(state: &DesktopModeState) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    state
        .inner
        .lock()
        .map_err(|_| "Desktop mode synchronization failed".to_owned())
}

#[tauri::command]
pub fn desktop_mode_status(
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    Ok(describe(lock(&state)?.mode))
}

#[tauri::command]
pub fn desktop_mode_try(
    window: WebviewWindow,
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    let mut inner = lock(&state)?;
    window
        .set_always_on_top(false)
        .map_err(|error| error.to_string())?;
    window
        .set_always_on_bottom(true)
        .map_err(|error| error.to_string())?;
    inner.revision = inner.revision.wrapping_add(1);
    inner.mode = Mode::Trial;
    let revision = inner.revision;
    let state_clone = Arc::clone(&state.inner);
    let worker_window = window.clone();

    if let Err(error) = thread::Builder::new()
        .name("island-bottom-revert".to_string())
        .spawn(move || {
            thread::sleep(Duration::from_secs(TRIAL_SECONDS));
            let Ok(mut current) = state_clone.lock() else {
                return;
            };
            // This lock makes "Keep", "Cancel" and timer rollback mutually
            // exclusive. No timer may revert a newer trial.
            if current.revision == revision && current.mode == Mode::Trial {
                if worker_window.set_always_on_bottom(false).is_ok() {
                    current.mode = Mode::Normal;
                    current.revision = current.revision.wrapping_add(1);
                }
            }
        })
    {
        // This branch still owns the GUI window handle and can undo the
        // potentially invisible lower-layer placement immediately.
        inner.revision = inner.revision.wrapping_add(1);
        inner.mode = Mode::Normal;
        window
            .set_always_on_bottom(false)
            .map_err(|restore| format!("Timer error: {error}; rollback error: {restore}"))?;
        return Err(format!("Unable to set rollback timer: {error}"));
    }
    Ok(describe(inner.mode))
}

#[tauri::command]
pub fn desktop_mode_keep(
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    let mut inner = lock(&state)?;
    if inner.mode != Mode::Trial {
        return Err("Start a 20-second trial before keeping this mode".to_owned());
    }
    inner.mode = Mode::Bottom;
    inner.revision = inner.revision.wrapping_add(1);
    Ok(describe(inner.mode))
}

#[tauri::command]
pub fn desktop_mode_cancel(
    window: WebviewWindow,
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    let mut inner = lock(&state)?;
    window
        .set_always_on_bottom(false)
        .map_err(|error| error.to_string())?;
    inner.mode = Mode::Normal;
    inner.revision = inner.revision.wrapping_add(1);
    Ok(describe(inner.mode))
}

#[cfg(test)]
mod tests {
    use super::{describe, DesktopModeState, Mode};

    #[test]
    fn native_mode_starts_safe_and_never_claims_workerw_ownership() {
        let state = DesktopModeState::default();
        let inner = state.inner.lock().unwrap();
        assert_eq!(inner.mode, Mode::Normal);
        assert_eq!(describe(inner.mode).mode, "normal");
        assert!(!describe(inner.mode).genuine_desktop_host);
        assert_eq!(describe(inner.mode).trial_seconds, 20);
    }

    #[test]
    fn status_names_are_stable_for_javascript() {
        assert_eq!(describe(Mode::Trial).mode, "trial");
        assert_eq!(describe(Mode::Bottom).mode, "bottom");
    }
}
