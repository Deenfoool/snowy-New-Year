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
- Clicking the capsule: Rust receives `set_island_expanded(true)` and resizes/recenters the window (up to 940 × 675 logical px, clamped to monitor).
- Closing: WebView remains an ordinary window. Clicking the collapse control restores compact size.
- Drag handle: uses Tauri's permission-gated native `startDragging` instead of only moving an HTML element.
- The web's demo snow is disabled in native mode, so no hidden full-screen canvas is rendered.
- The bottom wave canvas is outside the inner card and should preserve the down-facing silhouette.

## What this is *not* (yet)

1. **Not pinned to the desktop's WorkerW/Progman layer.** The native shell currently behaves like an ordinary borderless window. Proper desktop-only placement requires a separate, carefully tested Win32 adapter; attaching to Explorer windows too early is risky.
2. **Not integrated with Windows Media Sessions or system-wide audio.** The pIayer uses the local demo or user-selected audio files.
3. **No global Print Screen hook.** Capture remains the web permission-based flow, local image upload or clipboard paste. Do not promise native screenshots without implementing them.
4. **No tray/autostart/installer** in this milestone. The existing Snowy New Year executable and tray are unchanged.
5. **Not build-verified in this work environment.** The Rust toolchain and Windows SDK are unavailable here. This is a source implementation that must be compiled on Windows before being considered a release.

## Follow-up verification

On Windows run `npm run dev` and check: capsule click targets, drag without invisible mouse-blocking regions, expand/collapse sizes at 100/150/200% display scaling, transparent corners and wave outline, screenshot editing, closing and reopening, audio pause FPS, multimedia autoplay permissions, and manual weather/LRCLIB queries.

For a high-fidelity desktop-only experience, next implement the Explorer desktop layer adapter with window ownership detection, fallback when Explorer restarts, and multi-monitor support. It must not be enabled by default until verified.
