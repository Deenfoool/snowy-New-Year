#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

/// An experimental *ordinary* native window, NOT wallpaper embedding yet.
/// Its logical size tracks the capsule to avoid a huge invisible click-blocker.
#[tauri::command]
fn set_island_expanded(window: WebviewWindow, expanded: bool) -> Result<(), String> {
    let (mut width, mut height) = if expanded {
        (940.0_f64, 675.0_f64)
    } else {
        (360.0_f64, 75.0_f64)
    };

    let monitor = window.current_monitor().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;

    if let Some(display) = &monitor {
        width = width.min(display.size().width as f64 / scale - 20.0).max(290.0);
        height = height.min(display.size().height as f64 / scale - 36.0).max(68.0);
    }

    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|error| error.to_string())?;

    if let Some(display) = monitor {
        let physical_width = (width * scale).round() as i32;
        let position = PhysicalPosition::new(
            display.position().x + (display.size().width as i32 - physical_width) / 2,
            display.position().y + (20.0 * scale).round() as i32,
        );
        window
            .set_position(position)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![set_island_expanded])
        .run(tauri::generate_context!())
        .expect("Island Lab native host could not start");
}
