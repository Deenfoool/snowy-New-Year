#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod glass;
mod glass_backdrop;
mod icons;
mod overlay;
mod settings;
mod shake;
mod snowcap;
mod snowball;
mod ui;
mod weather;

use std::ptr::{null, null_mut};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR},
    },
};

fn main() {
    unsafe {
        let instance = CreateMutexW(null(), 0, w!("Local\\SnowyNewYear.Rust"));
        let already_open = GetLastError() == ERROR_ALREADY_EXISTS;
        if already_open {
            overlay::open_existing();
            CloseHandle(instance);
            return;
        }
        let startup = std::env::args().any(|arg| arg == "--tray");
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([800.0, 560.0])
                .with_transparent(true)
                .with_icon(icons::window_icon()),
            ..Default::default()
        };
        let result = eframe::run_native(
            "Snowy New Year",
            options,
            Box::new(move |cc| Ok(Box::new(ui::App::new(cc, startup)?))),
        );
        if let Err(error) = result {
            settings::log(&format!("Startup: {error}"));
            let message = overlay::wide(&format!("Не удалось запустить Snowy New Year.\n{error}"));
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                w!("Snowy New Year"),
                MB_ICONERROR,
            );
        }
        if !instance.is_null() {
            CloseHandle(instance);
        }
    }
}
