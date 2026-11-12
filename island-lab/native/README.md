# Island Lab — experimental native Windows host

This is the beginning of a **separate** executable, not an addition to Snowy New Year's Rust binary.

The host uses **Tauri 2 (Rust + system WebView2 on Windows)**, not Electron.
It reuses the canonical HTML/CSS/JS files in the parent `island-lab/` folder via a small preparation script. Changing those files updates both the web preview and Windows host.

## Prerequisites

- Windows 10/11 with Microsoft Edge WebView2 Runtime
- Rust toolchain 1.95+ with MSVC build tools (Visual Studio C++ workload)
- Node.js 20+ and npm (needed for the Tauri CLI and asset copy script)

## Launch for development

From the repository root, in PowerShell:

```powershell
cd island-lab/native
npm install
npm run dev
```

To only validate and stage frontend assets without Rust:

```powershell
npm run prepare:web
```

To compile an experimental executable without installer packaging:

```powershell
npm run build
```

`src-tauri/target/release/island-lab-native.exe` is the expected Rust binary path, depending on Tauri configuration and build target. `bundle.active` is **false**; packaging and signing come later.

## Native bridge v0

- On startup: a 360 × 75 logical pixel, **undecorated and transparent native window**.
- Clicking the capsule: Rust receives `set_island_expanded(true)` and resizes the window (up to 940 × 675 logical px, clamped to monitor). The initial capsule starts at the **top center**; after native dragging, expanding and collapsing preserve its horizontal anchor and top edge instead of jumping back to the center.
- Closing: WebView remains an ordinary window. Clicking the collapse control restores compact size.
- Drag handle: uses Tauri's permission-gated native `startDragging` instead of only moving an HTML element.
- The web's demo snow is disabled in native mode, so no hidden full-screen canvas is rendered.
- The bottom wave canvas is outside the inner card and should preserve the down-facing silhouette.
- **Experimental native player:** optional Windows Media Sessions adapter. In the expanded main tab, enable **«Плеер Windows»**, then play a track in Spotify, browser or another app exposing System Media Transport Controls. The adapter reads title/artist/position and handles play/pause/next/previous/seek commands. This is real Windows metadata integration, not a simulated music player. Actual Windows functionality still requires testing.
- The native player polls every 2 seconds only while the user has opted in; the WinRT session manager is cached and polling stops after disabling the switch. The browser does not poll Windows APIs.
- **System audio visualization is available as an experimental, separate opt-in feature**: after connecting «Плеер Windows», enable «Волны из звука Windows». Without this explicit toggle the wave stays flat for external music. LRC remains timed against the reported playback position.

## What this is *not* (yet)

1. **Not pinned to the desktop's WorkerW/Progman layer.** The native shell currently behaves like an ordinary borderless window. Proper desktop-only placement requires a separate, carefully tested Win32 adapter; attaching to Explorer windows too early is risky.
2. **Not integrated with Windows Media Sessions or system-wide audio.** The pIayer uses the local demo or user-selected audio files.
3. **No global Print Screen hook.** Capture remains the web permission-based flow, local image upload or clipboard paste. Do not promise native screenshots without implementing them.
4. **No tray/autostart/installer** in this milestone. The existing Snowy New Year executable and tray are unchanged.
   The native Media Sessions bridge is an untested Windows API integration. Compatible players must expose an active media session. Some players do not publish a reliable duration/position and may refuse seek or skip commands.
5. **Not build-verified in this work environment.** The Rust toolchain and Windows SDK are unavailable here. This is a source implementation that must be compiled on Windows before being considered a release.

## Follow-up verification

On Windows run `npm run dev` and check: capsule click targets, drag without invisible mouse-blocking regions, expand/collapse sizes at 100/150/200% display scaling, transparent corners and wave outline, screenshot editing, closing and reopening, audio pause FPS, multimedia autoplay permissions, and manual weather/LRCLIB queries.

For a high-fidelity desktop-only experience, next implement the Explorer desktop layer adapter with window ownership detection, fallback when Explorer restarts, and multi-monitor support. It must not be enabled by default until verified.

## Fast Windows check

With Node.js, Rust MSVC and C++ Build Tools installed, run from the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\island-lab\scripts\test-windows.ps1
```

After launching the compiled EXE, expand the capsule, open **Настройки → Проверка приложения** and select **«Проверить»**. Copy the report to share window/browser API availability and JavaScript error count. The report deliberately excludes private notes, images, music paths and city.

The quit button is intentionally exposed in settings because a frameless window has no standard close control. `Alt+F4` should remain a fallback.

The Tauri CSP explicitly allows `ipc:` and `http://ipc.localhost` for the native bridge while restricting remote requests to LRCLIB and Open-Meteo. The Windows workflow is at `.github/workflows/island-lab-windows.yml` and attempts an unsigned EXE build, if GitHub Actions is enabled for this repository.

## Windows audio-reactive wave (WASAPI, experimental)

`src-tauri/src/audio.rs` uses the safe Rust [wasapi-rs](https://github.com/HEnquist/wasapi-rs) library. It creates one COM-initialized capture thread **only when manually enabled** from the expanded music card, by selecting **«Плеер Windows»**, then **«Волны из звука Windows»**.

- The endpoint is the default **output** device, captured in *shared-mode loopback* (WASAPI). The microphone is not opened. This captures the **whole Windows output mix**, including game and notification sounds, not just the media session app.
- The native worker requests 48 kHz/stereo/float32 with automatic conversion, downsamples internally to 8 kHz and computes 16 normalized spectral bands with a Hann window and Goertzel detectors. Raw audio is never persisted, logged, sent to JavaScript, or uploaded. The UI receives only `bands[16]` and an RMS level.
- Stream updates are bounded to approximately 18 per second, event-driven, and cease when the toggle is disabled or the WebView becomes hidden. The native worker is joined, with a short 120 ms endpoint-wait timeout so shutdown can respond.
- The wave follows the 16 bands when a compatible media session reports playing. Silence gradually makes the lower edge flat; there is no fabricated beat.
- Unsupported device formats, missing default playback device, or device disconnection report an error and turn capture off. Re-enable after reconnecting a device.
- **Limitations:** not selective per-process audio capture yet; no FFT library is needed (the DSP is self-contained), no persistent recording, system-mix levels do not guarantee synchronization exclusively with the displayed track. Performance and Windows compatibility remain **unverified** until a local compile and smoke test.

Windows commands:
```powershell
cd island-lab/native
npm install
npm test
npm run dev
```

Manual scenario: play a track in Spotify or a browser, enable **Плеер Windows** → **Волны из звука Windows**, then vary the volume, pause the song, stop the toggle, switch the default audio device, and minimize the window. Check **Настройки → Проверка приложения**, and Task Manager CPU/GPU use.

Source references: Microsoft [WASAPI loopback](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) and [wasapi-rs](https://github.com/HEnquist/wasapi-rs).

## Experimental always-on-bottom trial

The Windows host now includes **Settings → Размещение на рабочем столе**: a 20-second reversible test of the documented Tauri `set_always_on_bottom` API. A separate Rust thread reverts it even if the frontend freezes. You may confirm the trial for the rest of the process lifetime or cancel it. **Nothing is persisted** and the window remains available on the taskbar. This does **not** attach to WorkerW, and may go behind the desktop icon host; it must be manually validated before being considered a real desktop widget.

See [the Windows desktop-layer test matrix](../DESKTOP_LAYER_TEST.md).
