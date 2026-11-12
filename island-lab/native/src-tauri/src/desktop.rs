//! Reversible trial of Tauri's native always-on-bottom window level.
//!
//! This does NOT reparent into undocumented Explorer WorkerW windows. Windows
//! may put an always-on-bottom top-level window *behind* desktop icon hosts;
//! therefore the trial automatically reverts after 20 seconds. Its only goal
//! is a safe, testable first step toward desktop-only placement.

use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{State, WebviewWindow};

const NORMAL: u8 = 0;
const TRIAL: u8 = 1;
const KEPT: u8 = 2;
const TRIAL_DURATION_SECONDS: u64 = 20;

/// The app starts in Normal on every launch; the preview is never persisted.
#[derive(Default)]
pub struct DesktopModeState {
    generation: Arc<AtomicU64>,
    mode: Arc<AtomicU8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopModeStatus {
    pub mode: &'static str,
    pub trial_seconds: u64,
    pub genuine_desktop_host: bool,
}

impl DesktopModeState {
    fn status(&self) -> DesktopModeStatus {
        let mode = match self.mode.load(Ordering::Acquire) {
            TRIAL => "trial",
            KEPT => "bottom",
            _ => "normal",
        };
        DesktopModeStatus {
            mode,
            trial_seconds: TRIAL_DURATION_SECONDS,
            genuine_desktop_host: false,
        }
    }
}

#[tauri::command]
pub fn desktop_mode_status(state: State<'_, DesktopModeState>) -> DesktopModeStatus {
    state.status()
}

/// Test lower Z-order in a reversible, timed manner.
#[tauri::command]
pub fn desktop_mode_try(
    window: WebviewWindow,
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    // On error, don't claim a transition occurred.
    window
        .set_always_on_top(false)
        .map_err(|error| error.to_string())?;
    window
        .set_always_on_bottom(true)
        .map_err(|error| error.to_string())?;

    state.mode.store(TRIAL, Ordering::Release);
    let sequence = state.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let generation = Arc::clone(&state.generation);
    let mode = Arc::clone(&state.mode);

    // Clone before launching the worker, so a failed thread spawn can
    // still restore the window to a normal Z-order immediately.
    let worker_window = window.clone();
    if let Err(error) = thread::Builder::new()
        .name("island-bottom-trial".into())
        .spawn(move || {
            thread::sleep(Duration::from_secs(TRIAL_DURATION_SECONDS));
            if generation.load(Ordering::Acquire) != sequence {
                return;
            }
            if mode
                .compare_exchange(TRIAL, NORMAL, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                if worker_window.set_always_on_bottom(false).is_err() {
                    // The window may have closed; the next launch still
                    // starts in Normal, so there's no persisted danger.
                }
            }
        })
    {
        // A failed timer must not leave a permanently hidden window.
        state.generation.fetch_add(1, Ordering::AcqRel);
        state.mode.store(NORMAL, Ordering::Release);
        window
            .set_always_on_bottom(false)
            .map_err(|restore| format!("Timer failed: {error}; restore failed: {restore}"))?;
        return Err(format!("Unable to arm auto-revert timer: {error}"));
    }

    Ok(state.status())
}

#[tauri::command]
pub fn desktop_mode_keep(state: State<'_, DesktopModeState>) -> Result<DesktopModeStatus, String> {
    if state
        .mode
        .compare_exchange(TRIAL, KEPT, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("Start the 20-second trial first".into());
    }
    state.generation.fetch_add(1, Ordering::AcqRel);
    Ok(state.status())
}

#[tauri::command]
pub fn desktop_mode_cancel(
    window: WebviewWindow,
    state: State<'_, DesktopModeState>,
) -> Result<DesktopModeStatus, String> {
    state.generation.fetch_add(1, Ordering::AcqRel);
    state.mode.store(NORMAL, Ordering::Release);
    window
        .set_always_on_bottom(false)
        .map_err(|error| error.to_string())?;
    Ok(state.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starts_in_normal_and_reports_not_a_real_desktop_host() {
        let mode = DesktopModeState::default();
        assert_eq!(mode.status().mode, "normal");
        assert!(!mode.status().genuine_desktop_host);
        assert_eq!(mode.status().trial_seconds, 20);
    }
}
