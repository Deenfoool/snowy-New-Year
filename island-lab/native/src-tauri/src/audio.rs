//! Explicitly enabled Windows WASAPI loopback visualizer.
//!
//! Reads the shared-mode *render* endpoint (desktop playback mix, including
//! notification sounds). No microphone access, no disk writes or audio uploads.
//! COM objects are created, used and dropped on one dedicated worker thread.
//! The frontend gets only 16 normalized spectral amplitudes, not raw PCM.
//!
//! This is an experimental native module and must be built/smoke-tested on
//! Windows before release.

use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};

const BANDS: usize = 16;
const FFT_WINDOW: usize = 512;
const DECIMATED_RATE: f32 = 8_000.0;
const BAND_HZ: [f32; BANDS] = [
    47.0, 68.0, 96.0, 135.0, 190.0, 265.0, 375.0, 520.0,
    730.0, 1030.0, 1420.0, 1890.0, 2500.0, 3000.0, 3450.0, 3850.0,
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumFrame {
    pub bands: [f32; BANDS],
    pub rms: f32,
}

impl SpectrumFrame {
    fn silent() -> Self {
        Self {
            bands: [0.0; BANDS],
            rms: 0.0,
        }
    }
}

/// Pure DSP function for offline unit testing. Receives 512 mono PCM frames
/// decimated to 8 kHz; never retains or returns the original sound samples.
fn analyze(samples: &[f32; FFT_WINDOW]) -> SpectrumFrame {
    let mut values = [0.0_f32; FFT_WINDOW];
    let mut energy = 0.0;
    for (i, &sample) in samples.iter().enumerate() {
        let x = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        energy += x * x;
        let hann = 0.5 - 0.5
            * (std::f32::consts::TAU * i as f32 / (FFT_WINDOW - 1) as f32).cos();
        values[i] = x * hann;
    }

    let rms = (energy / FFT_WINDOW as f32).sqrt().clamp(0.0, 1.0);
    let mut out = SpectrumFrame::silent();
    out.rms = rms;
    for (index, &frequency) in BAND_HZ.iter().enumerate() {
        let w = std::f32::consts::TAU * frequency / DECIMATED_RATE;
        let coefficient = 2.0 * w.cos();
        let mut a = 0.0_f32;
        let mut b = 0.0_f32;
        for &sample in &values {
            let next = sample + coefficient * a - b;
            b = a;
            a = next;
        }
        let magnitude_squared = (a * a + b * b - coefficient * a * b).max(0.0);
        let magnitude = magnitude_squared.sqrt() * 4.0 / FFT_WINDOW as f32;
        // Perceptual compression lifts quieter details without clipping.
        out.bands[index] = (magnitude * 2.0).sqrt().clamp(0.0, 1.0);
    }
    out
}

/// Manage one capture thread only. No WinRT/COM pointers cross thread boundaries.
pub struct AudioCaptureState {
    enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for AudioCaptureState {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }
}

impl AudioCaptureState {
    fn stop_capture(&self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(worker) = guard.take() {
                let _ = worker.join();
            }
        }
        self.enabled.store(false, Ordering::Release);
    }
}

impl Drop for AudioCaptureState {
    fn drop(&mut self) {
        self.stop_capture();
    }
}

#[tauri::command]
pub fn audio_capture_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, AudioCaptureState>,
) -> Result<bool, String> {
    if state.enabled.swap(true, Ordering::AcqRel) {
        return Ok(true);
    }
    state.stop.store(false, Ordering::Release);

    // Only join a *finished* prior worker; a normal running worker is blocked
    // from recreation by the atomic enabled flag.
    let mut guard = state.worker.lock().map_err(|_| "Capture lock poisoned")?;
    if let Some(old) = guard.take() {
        let _ = old.join();
    }

    let stop = Arc::clone(&state.stop);
    let enabled = Arc::clone(&state.enabled);
    let worker = thread::Builder::new()
        .name("island-wasapi-loopback".to_owned())
        .spawn(move || {
            #[cfg(windows)]
            let result = platform::capture_loop(&app, &stop);

            #[cfg(not(windows))]
            let result: Result<(), String> =
                Err("WASAPI capture requires Windows".to_owned());

            if let Err(error) = result {
                use tauri::Emitter;
                let _ = app.emit("island:audio-error", error);
            }
            enabled.store(false, Ordering::Release);
        })
        .map_err(|error| {
            state.enabled.store(false, Ordering::Release);
            error.to_string()
        })?;

    *guard = Some(worker);
    Ok(true)
}

#[tauri::command]
pub fn audio_capture_stop(state: tauri::State<'_, AudioCaptureState>) -> bool {
    state.stop_capture();
    true
}

#[cfg(windows)]
mod platform {
    use super::{analyze, AudioCaptureState, SpectrumFrame, FFT_WINDOW};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Emitter};
    use wasapi::{
        initialize_mta, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
    };

    pub fn capture_loop(app: &AppHandle, stop: &AtomicBool) -> Result<(), String> {
        initialize_mta().map_err(|e| e.to_string())?;
        let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
        let device = enumerator
            .get_default_device(Direction::Render)
            .map_err(|e| e.to_string())?;
        let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;

        // Request an explicit float32/48kHz/stereo format so native output
        // devices with different mix formats use automatic conversion.
        // If unsupported, report an error and keep the effect disabled.
        let format = WaveFormat::new(32, 32, SampleType::Float, 48_000, 2, None);
        let (period, _) = client.get_device_period().map_err(|e| e.to_string())?;
        client
            .initialize_client(
                &format,
                Direction::Capture,
                StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: period,
                },
                None,
            )
            .map_err(|e| e.to_string())?;

        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
        client.start_stream().map_err(|e| e.to_string())?;

        let mut data = Vec::<u8>::new();
        let mut history = VecDeque::<f32>::with_capacity(FFT_WINDOW);
        let mut frame_number: usize = 0;
        let mut last_emit = Instant::now();

        let run = (|| -> Result<(), String> {
            while !stop.load(Ordering::Acquire) {
                // Event-driven buffering means no busy loop while silent.
                // Keep this timeout short so an explicit stop is responsive.
                let _ = event.wait_for_event(120);

                while !stop.load(Ordering::Acquire) {
                    let n = capture
                        .get_next_packet_size()
                        .map_err(|e| e.to_string())?
                        .unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    // Bound a damaged driver/endpoint's packet size.
                    if n > 131_072 {
                        return Err("WASAPI packet exceeded size limit".to_owned());
                    }
                    let bytes = n as usize * format.get_blockalign() as usize;
                    data.resize(bytes, 0);
                    let (frames, _) = capture
                        .read_from_device(&mut data)
                        .map_err(|e| e.to_string())?;

                    for sample in data[..frames as usize * 8].chunks_exact(8) {
                        // Stereo float32 LE; data returned by WASAPI is already
                        // converted to the format declared in Initialize.
                        let left = f32::from_le_bytes(sample[0..4].try_into().unwrap());
                        let right = f32::from_le_bytes(sample[4..8].try_into().unwrap());
                        if frame_number % 6 == 0 {
                            if history.len() == FFT_WINDOW {
                                history.pop_front();
                            }
                            history.push_back((left * 0.5 + right * 0.5).clamp(-1.0, 1.0));
                        }
                        frame_number = frame_number.wrapping_add(1);
                    }
                }

                if last_emit.elapsed() >= Duration::from_millis(55) {
                    let mut samples = [0.0; FFT_WINDOW];
                    if history.len() == FFT_WINDOW {
                        for (dst, src) in samples.iter_mut().zip(history.iter()) {
                            *dst = *src;
                        }
                    }
                    let level: SpectrumFrame = analyze(&samples);
                    let _ = app.emit("island:audio-spectrum", level);
                    last_emit = Instant::now();
                }
            }
            Ok(())
        })();

        // Ensure the audio endpoint is stopped on error and explicit disable.
        let stop_result = client.stop_stream().map_err(|e| e.to_string());
        run.and(stop_result)
    }
}

#[cfg(test)]
mod tests {
    use super::{analyze, SpectrumFrame, FFT_WINDOW, DECIMATED_RATE};

    #[test]
    fn silence_produces_zero_spectrum() {
        let result = analyze(&[0.0; FFT_WINDOW]);
        assert_eq!(result.rms, 0.0);
        assert!(result.bands.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn low_and_high_tones_peak_in_separate_ranges() {
        let tone = |hz: f32| -> [f32; FFT_WINDOW] {
            std::array::from_fn(|i| {
                0.5 * (std::f32::consts::TAU * hz * i as f32 / DECIMATED_RATE).sin()
            })
        };
        let low = analyze(&tone(100.0));
        let high = analyze(&tone(2_500.0));
        assert!(low.bands[0..6].iter().copied().fold(0.0, f32::max) > 0.25);
        assert!(high.bands[10..16].iter().copied().fold(0.0, f32::max) > 0.25);
        assert!(low.bands[0..6].iter().copied().fold(0.0, f32::max)
            > low.bands[10..16].iter().copied().fold(0.0, f32::max));
        assert!(high.bands[10..16].iter().copied().fold(0.0, f32::max)
            > high.bands[0..6].iter().copied().fold(0.0, f32::max));
    }

    #[test]
    fn invalid_samples_are_sanitized() {
        let mut samples = [f32::NAN; FFT_WINDOW];
        samples[0] = f32::INFINITY;
        samples[1] = f32::NEG_INFINITY;
        let result = analyze(&samples);
        assert!(result.rms.is_finite());
        assert!(result.bands.iter().all(|x| x.is_finite()));
    }
}
