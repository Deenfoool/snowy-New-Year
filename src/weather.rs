use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const REFRESH: Duration = Duration::from_secs(3600);
pub const MAX_AGE: Duration = Duration::from_secs(7200);

#[derive(Clone, Copy, Default)]
pub struct Conditions {
    pub snow: f32,
    pub wind: f32,
    pub temperature: f32,
}

#[derive(Clone)]
pub struct Observation {
    pub city: String,
    pub current: Current,
    pub received: Instant,
    pub cached: bool,
    age_at_load: Duration,
}

impl Observation {
    pub fn age(&self) -> Duration {
        self.age_at_load + self.received.elapsed()
    }

    pub fn expires(&self) -> Instant {
        self.received + MAX_AGE.saturating_sub(self.age_at_load)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Current {
    pub last_updated_epoch: i64,
    pub temp_c: f32,
    pub precip_mm: f32,
    pub condition: Condition,
    pub wind_kph: f32,
    pub wind_degree: f32,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Condition {
    pub code: u16,
    pub text: String,
}

#[derive(Deserialize, Serialize)]
struct Cached {
    query: String,
    city: String,
    current: Current,
    saved: u64,
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load_cache(query: &str) -> Option<Observation> {
    let bytes = std::fs::read(crate::settings::directory().join("weatherapi.json")).ok()?;
    let cache: Cached = serde_json::from_slice(&bytes).ok()?;
    if cache.query != query {
        return None;
    }
    Some(Observation {
        city: cache.city,
        current: cache.current,
        received: Instant::now(),
        cached: true,
        age_at_load: Duration::from_secs(unix_time().saturating_sub(cache.saved)),
    })
}

pub fn save_cache(query: &str, observation: &Observation) -> std::io::Result<()> {
    crate::settings::write_json(
        "weatherapi",
        &Cached {
            query: query.to_owned(),
            city: observation.city.clone(),
            current: observation.current.clone(),
            saved: unix_time(),
        },
    )
}

pub fn conditions(
    settings: &Settings,
    observation: Option<&Observation>,
    now: Instant,
) -> Conditions {
    if !settings.real_weather {
        return Conditions {
            snow: settings.manual_snow / 100.0,
            wind: settings.manual_wind / 100.0,
            temperature: settings.manual_temp,
        };
    }
    let Some(observation) = observation.filter(|obs| now < obs.expires()) else {
        return Conditions::default();
    };
    let current = &observation.current;
    let snow = match current.condition.code {
        1204 | 1207 | 1210 | 1213 | 1255 => 0.3,
        1114 | 1216 | 1219 | 1279 => 0.55,
        1117 | 1222 | 1225 | 1258 | 1282 => 0.85,
        _ => 0.0,
    };
    Conditions {
        snow,
        wind: -current.wind_degree.to_radians().sin() * (current.wind_kph / 45.0).min(1.5),
        temperature: current.temp_c,
    }
}

fn key() -> Result<String, &'static str> {
    use std::{
        mem::zeroed,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let mut encrypted = std::fs::read(crate::settings::directory().join("weather-key.bin"))
        .map_err(|_| "Ключ WeatherAPI не настроен.")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted
            .len()
            .try_into()
            .map_err(|_| "Повреждён файл ключа WeatherAPI.")?,
        pbData: encrypted.as_mut_ptr(),
    };
    unsafe {
        let mut output: CRYPT_INTEGER_BLOB = zeroed();
        if CryptUnprotectData(
            &input,
            null_mut(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        ) == 0
        {
            return Err("Не удалось прочитать ключ WeatherAPI для этого пользователя Windows.");
        }
        let result = if output.pbData.is_null() || output.cbData == 0 {
            Err("Ключ WeatherAPI пуст.")
        } else {
            let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
            let result = std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| "Повреждён ключ WeatherAPI.");
            for byte in bytes {
                std::ptr::write_volatile(byte, 0);
            }
            result
        };
        LocalFree(output.pbData.cast());
        result
    }
}

pub fn fetch(city: &str) -> Result<Observation, String> {
    #[derive(Deserialize)]
    struct Location {
        name: String,
        country: String,
    }
    #[derive(Deserialize)]
    struct Response {
        location: Location,
        current: Current,
    }

    let key = key()?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .redirects(0)
        .build();
    let response: Response = agent
        .get("https://api.weatherapi.com/v1/current.json")
        .query("key", &key)
        .query("q", city)
        .query("lang", "ru")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(400, _) => "WeatherAPI не нашёл указанный город.",
            ureq::Error::Status(401 | 403, _) => {
                "WeatherAPI отклонил ключ или исчерпан лимит запросов."
            }
            _ => "Не удалось обновить погоду. Проверьте интернет-соединение.",
        })?
        .into_json()
        .map_err(|_| "Не удалось прочитать погодные данные.")?;
    Ok(Observation {
        city: format!("{}, {}", response.location.name, response.location.country),
        current: response.current,
        received: Instant::now(),
        cached: false,
        age_at_load: Duration::ZERO,
    })
}

pub fn age_minutes(observation: &Observation) -> u64 {
    unix_time().saturating_sub(observation.current.last_updated_epoch.max(0) as u64) / 60
}
