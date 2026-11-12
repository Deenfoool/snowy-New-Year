#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{LogicalSize, PhysicalPosition, State, WebviewWindow};

#[derive(Default)]
struct WindowPositionState {
    initialized: AtomicBool,
}

fn clamp_inside_screen(value: i32, screen_start: i32, screen_size: i32, window_size: i32) -> i32 {
    // Never overflow on a very small monitor or high display scaling.
    let margin = 8;
    let first = screen_start.saturating_add(margin);
    let last = screen_start
        .saturating_add(screen_size)
        .saturating_sub(window_size)
        .saturating_sub(margin);
    value.clamp(first.min(last), first.max(last))
}

/// Experimental ordinary Windows window; desktop-only docking is not enabled.
///
/// On first startup place the capsule at the top center of the active monitor.
/// Later expansions keep the horizontal center and top edge of the *moved*
/// capsule instead of unexpectedly jumping back to the monitor center.
/// The native viewport remains small when the island is collapsed.
#[tauri::command]
fn set_island_expanded(
    window: WebviewWindow,
    position: State<'_, WindowPositionState>,
    expanded: bool,
) -> Result<(), String> {
    let (requested_width, requested_height) = if expanded {
        (940.0_f64, 675.0_f64)
    } else {
        (360.0_f64, 75.0_f64)
    };

    let monitor = window.current_monitor().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let old_position = window.outer_position().map_err(|e| e.to_string())?;
    let old_size = window.outer_size().map_err(|e| e.to_string())?;

    let (mut width, mut height) = (requested_width, requested_height);
    if let Some(m) = &monitor {
        width = width.min((m.size().width as f64 - 16.0) / scale).max(1.0);
        height = height.min((m.size().height as f64 - 36.0) / scale).max(1.0);
    }

    let new_width_px = (width * scale).round() as i32;
    let new_height_px = (height * scale).round() as i32;
    let first = !position.initialized.swap(true, Ordering::SeqCst);
    let (desired_x, desired_y) = if first {
        if let Some(m) = &monitor {
            (
                m.position().x
                    + (m.size().width as i32 - new_width_px) / 2,
                m.position().y + (20.0 * scale).round() as i32,
            )
        } else {
            (old_position.x, old_position.y)
        }
    } else {
        (
            old_position.x + (old_size.width as i32 - new_width_px) / 2,
            old_position.y,
        )
    };

    let (x, y) = if let Some(m) = &monitor {
        (
            clamp_inside_screen(
                desired_x,
                m.position().x,
                m.size().width as i32,
                new_width_px,
            ),
            clamp_inside_screen(
                desired_y,
                m.position().y,
                m.size().height as i32,
                new_height_px,
            ),
        )
    } else {
        (desired_x, desired_y)
    };

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit_island_lab(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    tauri::Builder::default()
        .manage(WindowPositionState::default())
        .invoke_handler(tauri::generate_handler![
            set_island_expanded,
            quit_island_lab
        ])
        .run(tauri::generate_context!())
        .expect("Island Lab native host could not start");
}

#[cfg(test)]
mod tests {
    use super::clamp_inside_screen;

    #[test]
    fn position_within_screen_edges() {
        assert_eq!(clamp_inside_screen(-400, 0, 1920, 360), 8);
        assert_eq!(clamp_inside_screen(4000, 0, 1920, 360), 1552);
        assert_eq!(clamp_inside_screen(580, 300, 1200, 460), 580);
    }

    #[test]
    fn negative_coordinates_are_supported_for_secondary_displays() {
        assert_eq!(clamp_inside_screen(-2000, -1920, 1920, 360), -1912);
        assert_eq!(clamp_inside_screen(-100, -1920, 1920, 360), -368);
    }
}
