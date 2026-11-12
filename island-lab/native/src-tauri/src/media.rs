//! Optional Windows GlobalSystemMediaTransportControlsSessionManager adapter.
//! It is intentionally opt-in: the browser demo and local file player do not
//! require a running system media session.
//! The API is available on Windows 10 1809+ for compatible media apps.
//! No microphone recording and no audio capture occur in this module.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub source_app: String,
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
}

#[cfg(windows)]
mod platform {
    use super::MediaSnapshot;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };

    async fn active_session() -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|error| error.to_string())?
            .await
            .map_err(|error| error.to_string())?;
        // No active session is normal (e.g. music player is closed).
        Ok(manager.GetCurrentSession().ok())
    }

    pub async fn snapshot() -> Result<Option<MediaSnapshot>, String> {
        let Some(session) = active_session().await? else {
            return Ok(None);
        };
        let properties = session
            .TryGetMediaPropertiesAsync()
            .map_err(|error| error.to_string())?
            .await
            .map_err(|error| error.to_string())?;

        let title = properties.Title().map(|v| v.to_string()).unwrap_or_default();
        let artist = properties.Artist().map(|v| v.to_string()).unwrap_or_default();
        let source_app = session
            .SourceAppUserModelId()
            .map(|v| v.to_string())
            .unwrap_or_default();

        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .map(|status| status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
            .unwrap_or(false);

        // WinRT TimeSpan.Duration is measured in 100-nanosecond ticks.
        let (position_seconds, duration_seconds) = match session.GetTimelineProperties() {
            Ok(timeline) => {
                let start = timeline.StartTime().map(|v| v.Duration).unwrap_or(0);
                let end = timeline.EndTime().map(|v| v.Duration).unwrap_or(0);
                let position = timeline.Position().map(|v| v.Duration).unwrap_or(start);
                let to_seconds = |value: i64| (value.max(0) as f64) / 10_000_000.0;
                (
                    to_seconds(position.saturating_sub(start)),
                    to_seconds(end.saturating_sub(start)),
                )
            }
            Err(_) => (0.0, 0.0),
        };

        Ok(Some(MediaSnapshot {
            title,
            artist,
            source_app,
            playing,
            position_seconds,
            duration_seconds,
        }))
    }

    pub async fn action(action: &str, seconds: Option<f64>) -> Result<bool, String> {
        let Some(session) = active_session().await? else {
            return Ok(false);
        };
        let operation = match action {
            "toggle" => session.TryTogglePlayPauseAsync(),
            "next" => session.TrySkipNextAsync(),
            "previous" => session.TrySkipPreviousAsync(),
            "play" => session.TryPlayAsync(),
            "pause" => session.TryPauseAsync(),
            "seek" => {
                let Some(seconds) = seconds else {
                    return Err("Missing seek position".to_owned());
                };
                if !seconds.is_finite() || !(0.0..=86_400.0).contains(&seconds) {
                    return Err("Invalid seek position".to_owned());
                }
                session.TryChangePlaybackPositionAsync((seconds * 10_000_000.0) as i64)
            }
            _ => return Err("Unsupported media action".to_owned()),
        }
        .map_err(|error| error.to_string())?;
        operation.await.map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub async fn system_media_snapshot() -> Result<Option<MediaSnapshot>, String> {
    #[cfg(windows)]
    {
        platform::snapshot().await
    }
    #[cfg(not(windows))]
    {
        Err("Windows Media Sessions are only available on Windows".to_owned())
    }
}

#[tauri::command]
pub async fn system_media_action(action: String, seconds: Option<f64>) -> Result<bool, String> {
    #[cfg(windows)]
    {
        platform::action(&action, seconds).await
    }
    #[cfg(not(windows))]
    {
        let _ = (action, seconds);
        Err("Windows Media Sessions are only available on Windows".to_owned())
    }
}
