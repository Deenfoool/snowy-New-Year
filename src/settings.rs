use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    pub falling: bool,
    pub caps: bool,
    pub snowballs: bool,
    pub snowman_decorations: bool,
    pub icicle_drips: bool,
    pub wind_drifts: bool,
    pub bottom_snow: bool,
    pub shake_to_clear: bool,
    pub intensity: f32,
    pub size: f32,
    pub opacity: f32,
    pub speed: f32,
    pub particle_limit: usize,
    pub max_height: f32,
    pub roughness: f32,
    pub accumulation: f32,
    pub melt: f32,
    pub icicles: bool,
    pub real_weather: bool,
    pub city: String,
    pub manual_snow: f32,
    pub manual_wind: f32,
    pub manual_temp: f32,
    pub fullscreen_pause: bool,
    pub fps: u32,
    pub accent: usize,
    pub glass: f32,
    pub zoom: f32,
    pub animations: bool,
    pub close_to_tray: bool,
    pub monitor: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            falling: true,
            caps: true,
            snowballs: true,
            snowman_decorations: true,
            icicle_drips: true,
            wind_drifts: true,
            bottom_snow: true,
            shake_to_clear: true,
            intensity: 45.0,
            size: 3.0,
            opacity: 75.0,
            speed: 65.0,
            particle_limit: 600,
            max_height: 18.0,
            roughness: 40.0,
            accumulation: 1.0,
            melt: 1.0,
            icicles: false,
            real_weather: true,
            city: "Borisoglebsk".into(),
            manual_snow: 55.0,
            manual_wind: 20.0,
            manual_temp: -5.0,
            fullscreen_pause: true,
            fps: 30,
            accent: 0,
            glass: 35.0,
            zoom: 1.0,
            animations: true,
            close_to_tray: true,
            monitor: String::new(),
        }
    }
}

pub fn directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Snowy New Year")
}

pub fn load() -> Settings {
    let mut settings: Settings = fs::read(directory().join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    settings.fps = settings.fps.clamp(15, 120);
    settings.particle_limit = settings.particle_limit.clamp(50, 2000);
    settings.accent = settings.accent.min(3);
    settings.zoom = settings.zoom.clamp(0.85, 1.3);
    settings.max_height = settings.max_height.clamp(5.0, 50.0);
    settings.size = settings.size.clamp(1.0, 8.0);
    settings.intensity = settings.intensity.clamp(0.0, 100.0);
    settings.opacity = settings.opacity.clamp(10.0, 100.0);
    settings.speed = settings.speed.clamp(10.0, 170.0);
    settings.roughness = settings.roughness.clamp(0.0, 100.0);
    settings.accumulation = settings.accumulation.clamp(0.1, 3.0);
    settings.melt = settings.melt.clamp(0.0, 3.0);
    settings.manual_snow = settings.manual_snow.clamp(0.0, 100.0);
    settings.manual_wind = settings.manual_wind.clamp(-100.0, 100.0);
    settings.manual_temp = settings.manual_temp.clamp(-25.0, 25.0);
    settings.glass = settings.glass.clamp(0.0, 100.0);
    settings
}

pub fn save(settings: &Settings) -> io::Result<()> {
    write_json("settings", settings)
}

pub fn write_json(name: &str, data: &impl Serialize) -> io::Result<()> {
    let dir = directory();
    fs::create_dir_all(&dir)?;
    let temporary = dir.join(format!("{name}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(data)?)?;
    fs::rename(temporary, dir.join(format!("{name}.json")))
}

pub fn log(message: &str) {
    use std::io::Write;
    let dir = directory();
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("app.log"))
    {
        let _ = writeln!(file, "{:?} {message}", std::time::SystemTime::now());
    }
}
