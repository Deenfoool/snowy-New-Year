# Snowy New Year — UI/UX specification

**Status:** approved design direction, 2026-11-11; the Rust/egui window now follows this design. **Target:** Windows 10/11. Native changes have **not yet been verified by a Windows build or screenshot**.

## Product direction

- **Visual language:** Frosted Glass / dark / balanced layout.
- **Personality:** calm, cold, premium winter atmosphere, no excessive Christmas decoration.
- **Windows app:** quiet in the background; full settings panel opens only when requested.
- **Separation of concerns:** native UI, window observation, snow simulation, GPU renderer, weather provider must not depend on each other's frame loops.
- **Screen size:** design reference 960 × 640 logical pixels. Minimum recommended 800 × 560; scale responsively. Honor Windows DPI, 100–200% or greater if feasible.
- **Typography:** Segoe UI Variable, fallback Segoe UI; interface in Russian, architecture ready for EN.
- **Icons:** single-stroke Lucide-style set, consistently sized 16–20 logical px.
- **Corners:** 10–16 px. **Animation duration:** 120–200 ms; honor reduced-motion.
- **Transparency:** reserved for window chrome / limited high-impact surfaces; fallback to solid translucent dark panels where backdrop blur is costly or unsupported. A faux-glass appearance is acceptable.

## Design tokens

Canonical palette and metrics are in [tokens.json](../design/tokens.json).

| Role | Value |
| --- | --- |
| Window background | \`#0B1422\` |
| Sidebar / titlebar | \`#101C2C\` |
| Card surface | \`#172B40\` |
| Hover / selected navigation | \`#25465E\` |
| Border | \`#304358\` |
| Primary icy accent | \`#8DD9F6\` |
| Main text | \`#F1F7FD\` |
| Secondary text | \`#94ADC3\` |
| Success | \`#74D7A7\` |
| Warning | \`#E3B777\` |
| Danger | \`#E4A5A5\` |

Do not convey a condition using color alone. Buttons, toggles and text must remain legible over blurred backgrounds.

## Main window

- Titlebar: snowflake mark, "Snowy New Year", minimize, maximize/restore, close; draggable via native hit-testing.
- Left sidebar: fixed width ~190 px at desktop sizes, collapsible icons at narrow sizes.
- Main content scrolls independently. Cards have consistent padding and gaps.
- Status in sidebar footer: "Эффекты включены" / "Эффекты на паузе" / "Ошибка погоды".
- Closing window defaults to hiding UI into tray (with discoverable "Выход" action), not terminating the snow engine. User setting can change this behavior.

### Six screens

#### 1) Главная
Purpose: one-glance state and quick controls.
- Header "Зимняя атмосфера" and single Pause/Resume button.
- Weather card: city, condition, actual temperature, last update with explicit timezone, a small *demo* snowy-window visualization.
- Fast switches: "Снегопад", "Снежные шапки", "Синхронизация с погодой".
- Current mode label: "Реальная погода" / "Ручной режим".
- When real weather has no snowfall, **do not falsely depict active snow**. Preview may show a sample, but label it "Демонстрация".

#### 2) Эффекты
Two independent master toggles:
- Falling snow: intensity, size (e.g. 1–8 px), opacity, speed, horizontal wind, turbulence, draw depth, particle cap.
- Snow caps: cap height, irregularity, accumulation/melt speed, icicles, geometry adaptation.
- Three presets: "Лёгкий снег", "Обычная зима", "Метель"; allow manual customization.
- Preset choice changes *rendering multipliers*, not historical real-weather measurements.
- Visual preview uses a dedicated lightweight scene, never the actual desktop overlay.

#### 3) Погода
- Tabs: "Реальная" / "Ручная".
- Real: city search (Borisoglebsk is an initial **example**, not globally forced location), current provider, temperature, wind, precipitation, last successful update, refresh, connection/error indicator.
- Manual: independent control over snow intensity, wind, temperature and optional melting; manual state must not require network.
- Offline: display cached observation with timestamp and explicit stale-state messaging; offer manual mode.
- Provider abstraction: WeatherAPI is candidate, network layer independent of the renderer.
- API keys never committed. User-entered secret should be stored appropriately for Windows; warn that embedded keys in distributed binaries are extractable.

#### 4) Производительность
- Auto-pause visual effects over fullscreen apps (monitor-aware).
- Stop rendering at zero visible effects; no hidden transparent frames.
- Optional pause for locked screen/off display; weather refresh may operate separately.
- FPS cap: 15 / 30 / 60 / 120 / auto (implementation should not assume 120 is free).
- Diagnostic state: "Рендер остановлен: полноэкранное приложение", tracked windows, visible snow surfaces, particles, FPS, CPU/RAM measurements **only after real instrumentation**.
- Crucial: actual snow cap accumulation for *still existing windows* continues analytically across rendering pauses, not by simulating individual flake frames.

#### 5) Оформление
- Default theme Frosted Glass Dark.
- Ice blue accent; optional muted lavender, mint, silver-blue alternatives.
- Glass intensity, UI scaling, interface animations and reduced-motion support.
- Visual settings for snow are **not** placed here (see Эффекты).

#### 6) Настройки
- Autostart, close-to-tray, language, monitor selection, keyboard shortcuts, weather provider configuration, update/error logs, About.
- Explicit "Полностью выйти" in tray and settings.
- Display selection shows monitor position/resolution and per-display effects.
- Keyboard shortcuts must avoid stealing system shortcuts.

### Secondary surfaces

**Tray menu:** Open settings; Pause/Resume; Falling Snow on/off; Snow Caps on/off; weather mode; Exit. Icon states distinguish normal, paused and weather error. All labels include icons/text, not just color.

**First run:** 3 stages maximum — welcome + privacy, city/weather mode (can be skipped), effects + tray behavior. No forced account creation.

**Dialog states:** connection error, stale observation, no active windows, fullscreen pause, no internet, loading and successful save. Disable or explain unusable inputs.

## Window-snow lifecycle (approved architecture)

See root [README](../README.md). UI must clearly reflect these rules:
1. Snow belongs to a **specific window instance**, never a process/app name.
2. A newly opened or newly observed window starts accumulating from zero.
3. Minimized: hidden cap, time-based accumulation continues.
4. Restored: show recalculated height.
5. Moved/resized: follow/reflow profile.
6. Destroyed: delete snow state immediately.
7. Reopen browser: new window instance, new snow from zero.
8. Fullscreen or no visible snow: stop unnecessary painting; update cumulative snow on demand via elapsed time and weather history.
9. Snowfall on desktop is separate from cap visibility. No windows ≠ no need to render if desktop snowfall is enabled.

## Interaction states and UX details

- Switches have On/Off, disabled, hover, focus-visible and keyboard operation.
- Sliders show numeric current values and units. Small pointer-target devices can use numeric fields.
- Settings changes reflect in preview immediately; expensive GPU reinitializations are deferred and indicated.
- Space and Enter activate focused buttons; Tab traversal deterministic; Escape dismisses noncritical dialogs.
- Accessible text contrast, UI scaling, screen readers where supported; avoid animations when accessibility settings request it.
- Settings are persistent on restart. Weather city/locale are explicit user preferences, not inferred.

## Reference preview

- [Interactive six-screen HTML reference](../design/preview.html) — runs **locally in a web browser** as a UI-only mockup. It is not the Rust production UI, does not read windows, and does not query WeatherAPI.
- [Theme tokens](../design/tokens.json) — design token source for native implementation.

## Engineering handoff

Choose an actual Rust-native UI strategy before implementing the production window (e.g. \`iced\`, \`egui\`, or Win32/Direct2D). **Do not deploy HTML/Electron simply because the design preview is HTML.** Build overlay and particle renderer separately. Preserve event-driven idle mode, monitor-specific fullscreen detection and zero frame cost when painting is unnecessary.

### Acceptance checks for UI implementation
- Six screens and tray states present in RU.
- Titlebar/side navigation functional, window scalable and DPI-safe.
- Weather display distinguishes live/cached/demo and shows last-update time.
- Pausing UI does not destroy still-living window snow states.
- Fullscreen pauses rendering, **not** lifecycle cleanup.
- Closing the native settings UI hides it to tray without leaving a constantly rendering UI process.
- No secrets in source/repo or network access in visual preview.
- No performance numbers presented as measured unless collected.


## Native implementation notes (2026-11-11)

- `src/ui.rs` renders a 54-point borderless titlebar (drag, double-click maximize/restore, minimize, maximize, close), ~190-point left sidebar with selection highlight, six screens, home preview and weather card.
- `src/glass.rs` supplies rounded, softly graduated dark panels, icy highlights, a consistent palette and custom switches. It does **not require** Windows Composition for the UI to remain readable.
- `src/main.rs` creates an **opaque** window. The transparent snow overlay remains completely separate in `src/overlay.rs`.
- `src/glass_backdrop.rs` is retained in the repository for future experimental native blur, but the settings window no longer initializes it by default. This avoids transparent/empty screens when a host-backdrop brush fails.
- A custom borderless window must be manually verified for dragging, all resize edges, Windows 10/11 DPI and maximize/restore. No runtime performance claims are made without benchmarking.
