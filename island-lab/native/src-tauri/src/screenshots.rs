//! Local, native screenshot saving. User image pixels stay on disk.
//!
//! The frontend exports an annotated PNG and explicitly asks to save it.
//! No arbitrary file path reaches Rust: the destination is always a newly
//! created file inside the OS Pictures/Island Lab directory. No upload.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;
static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn screenshots_dir() -> Result<PathBuf, String> {
    dirs::picture_dir()
        .map(|path| path.join("Island Lab"))
        .ok_or_else(|| "Windows Pictures folder is unavailable".to_owned())
}

fn parse_png_payload(png_data: &str) -> Result<Vec<u8>, String> {
    let encoded = png_data
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "Expected PNG image data".to_owned())?;
    // Validate the encoded size before allocating a large buffer.
    if encoded.len() > MAX_PNG_BYTES.saturating_mul(4) / 3 + 8 {
        return Err("PNG screenshot exceeds 32 MiB".to_owned());
    }
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid PNG base64 data".to_owned())?;
    if decoded.len() < 24 || decoded.len() > MAX_PNG_BYTES {
        return Err("Invalid screenshot size".to_owned());
    }
    if !decoded.starts_with(PNG_MAGIC) || &decoded[12..16] != b"IHDR" {
        return Err("Invalid PNG signature or header".to_owned());
    }
    let width = u32::from_be_bytes(decoded[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(decoded[20..24].try_into().unwrap());
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err("Unsupported screenshot dimensions".to_owned());
    }
    Ok(decoded)
}

fn save_png_bytes(bytes: &[u8], folder: &std::path::Path) -> Result<PathBuf, String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();

    // File names are generated exclusively on the Rust side, not supplied by
    // the renderer, preventing path traversal or overwriting old screenshots.
    for _ in 0..20 {
        let counter = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = folder.join(format!("Island-Lab-{time}-{counter}.png"));
        let file = OpenOptions::new().create_new(true).write(true).open(&path);
        match file {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|_| file.flush()) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(format!("Could not write screenshot: {error}"));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Cannot create a unique screenshot filename".to_owned())
}

#[tauri::command(async)]
pub fn save_screenshot_native(png_data: String) -> Result<String, String> {
    #[cfg(windows)]
    {
        let bytes = parse_png_payload(&png_data)?;
        let path = save_png_bytes(&bytes, &screenshots_dir()?)?;
        Ok(path.to_string_lossy().into_owned())
    }
    #[cfg(not(windows))]
    {
        let _ = png_data;
        Err("Native screenshot saving requires Windows".to_owned())
    }
}

#[tauri::command]
pub fn open_screenshot_folder_native() -> Result<String, String> {
    #[cfg(windows)]
    {
        let path = screenshots_dir()?;
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        std::process::Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map_err(|error| format!("Could not open Explorer: {error}"))?;
        Ok(path.to_string_lossy().into_owned())
    }
    #[cfg(not(windows))]
    {
        Err("Windows Explorer is unavailable".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_png_payload, PNG_MAGIC, MAX_PNG_BYTES};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    fn make_data(width: u32, height: u32) -> String {
        let mut bytes = Vec::from(PNG_MAGIC.as_slice());
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        format!("data:image/png;base64,{}", STANDARD.encode(bytes))
    }

    #[test]
    fn accepts_png_header_with_valid_dimensions() {
        assert_eq!(&parse_png_payload(&make_data(1920, 1080)).unwrap()[..8], PNG_MAGIC);
    }

    #[test]
    fn rejects_invalid_types_and_dimensions() {
        assert!(parse_png_payload("data:image/jpeg;base64,AAAA").is_err());
        assert!(parse_png_payload(&make_data(0, 1080)).is_err());
        assert!(parse_png_payload(&make_data(8193, 1080)).is_err());
        assert!(parse_png_payload("data:image/png;base64,garbage").is_err());
    }

    #[test]
    fn limits_input_before_allocating() {
        let huge = "A".repeat(MAX_PNG_BYTES * 4 / 3 + 20);
        assert!(parse_png_payload(&format!("data:image/png;base64,{huge}")).is_err());
    }

    #[test]
    fn creates_files_without_overwriting() {
        let dir = std::env::temp_dir().join(format!(
            "island-lab-screenshot-test-{}-{}",
            std::process::id(),
            super::FILE_COUNTER.fetch_add(1, super::Ordering::Relaxed)
        ));
        let first = super::save_png_bytes(b"test1", &dir).unwrap();
        let second = super::save_png_bytes(b"test2", &dir).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first).unwrap(), b"test1");
        assert_eq!(std::fs::read(second).unwrap(), b"test2");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
